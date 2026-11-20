use anyhow::{Context, Result};
use camo_translator::{
    bake::{self, ExportOptions},
    catalog::InstalledVehicle,
    prepare::{self, PrepareRequest},
    progress::Progress,
    visibility::{HiddenFaces, VisibilityReport},
};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Prepare,
    Export,
}

pub enum Job {
    Prepare {
        project: String,
        scene: PathBuf,
        game: PathBuf,
        vehicle: InstalledVehicle,
        c4dpy: Option<PathBuf>,
        out: PathBuf,
    },
    Export {
        project: PathBuf,
        textures: PathBuf,
        out: PathBuf,
        options: ExportOptions,
    },
}

pub enum Event {
    Step(String),
    Note(String),
    ReviewHiddenFaces {
        report: VisibilityReport,
        reply: Sender<Option<HiddenFaces>>,
    },
    Finished(Result<Option<PathBuf>>),
}

impl Job {
    fn run(
        self,
        progress: impl FnMut(Progress<'_>),
        review: impl FnOnce(&VisibilityReport) -> Result<Option<HiddenFaces>>,
    ) -> Result<Option<PathBuf>> {
        match self {
            Self::Prepare {
                project,
                scene,
                game,
                vehicle,
                c4dpy,
                out,
            } => prepare::prepare_with_review(
                PrepareRequest {
                    name: project,
                    source: &scene,
                    game_root: &game,
                    vehicle: &vehicle,
                    c4dpy: c4dpy.as_deref(),
                    out: &out,
                },
                progress,
                review,
            ),
            Self::Export {
                project,
                textures,
                out,
                options,
            } => {
                bake::export_with_progress(&project, &textures, &out, &options, progress).map(Some)
            }
        }
    }

    pub fn start(self, ctx: egui::Context) -> std::io::Result<Receiver<Event>> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("camo-translation".to_owned())
            .spawn(move || {
                let progress = |step: Progress<'_>| {
                    let text = chinese(step);
                    let event = if matches!(
                        step,
                        Progress::ReusedUvWarning { .. } | Progress::CamoUvConflict(_)
                    ) {
                        Event::Note(text)
                    } else {
                        Event::Step(text)
                    };
                    let _ = tx.send(event);
                    ctx.request_repaint();
                };
                let review = |report: &VisibilityReport| {
                    if report.default_hidden_nodes.is_empty() {
                        let _ = tx.send(Event::Note("游戏配置未声明默认隐藏节点。".into()));
                        return Ok(Some(HiddenFaces::Keep));
                    }
                    let (reply, answer) = mpsc::channel();
                    tx.send(Event::ReviewHiddenFaces {
                        report: report.clone(),
                        reply,
                    })
                    .map_err(|_| anyhow::anyhow!("隐藏面检查窗口已关闭"))?;
                    ctx.request_repaint();
                    let choice = answer.recv().context("隐藏面检查窗口已关闭")?;
                    let detail = match choice {
                        Some(HiddenFaces::Keep) => "已保留全部源模型面。".to_owned(),
                        Some(HiddenFaces::Remove) => format!(
                            "已从 Painter 模型移除 {} 个默认隐藏三角面。",
                            report.removable_triangles
                        ),
                        None => return Ok(None),
                    };
                    let _ = tx.send(Event::Note(detail));
                    Ok(choice)
                };
                let result = self.run(progress, review);
                let _ = tx.send(Event::Finished(result));
                ctx.request_repaint();
            })?;
        Ok(rx)
    }
}

fn chinese(progress: Progress<'_>) -> String {
    match progress {
        Progress::ReadMesh => "读取源模型与材质槽".into(),
        Progress::ScanDefaultHiddenFaces => "读取游戏默认隐藏节点".into(),
        Progress::DefaultHiddenFaces { nodes, removable } => {
            format!("游戏声明 {nodes} 个默认隐藏节点，找到 {removable} 个可移除三角面")
        }
        Progress::MatchCamouflage(_) => "匹配游戏模型、材质和迷彩 UV".into(),
        Progress::FaceMatches { matched, total } => format!("已匹配 {matched} / {total} 个三角面"),
        Progress::SeparateUvIslands => "安排 Reused 的独立 UV".into(),
        Progress::WritePainterMesh => "保存 Painter 模型和项目".into(),
        Progress::ReadProject => "读取项目与 UV 对照".into(),
        Progress::ReadTextures { slot, tiles } => format!("读取 {slot} 的 {tiles} 个 UDIM"),
        Progress::ReprojectTextures { slot, .. } => format!("正在回写 {slot} 贴图"),
        Progress::ReusedUvWarning {
            slot,
            mixed,
            missing,
        } => {
            format!("{slot} 的 Reused 像素中，{mixed} 个与普通对象共用原 UV，{missing} 个缺少 UV2")
        }
        Progress::CamoUvConflict(pixels) => {
            format!("{pixels} 个迷彩像素同时收到不同颜色，详见导出报告")
        }
        Progress::WriteCamouflage => "保存涂装与导出报告".into(),
    }
}
