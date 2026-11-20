use anyhow::Result;
use camo_translator::{
    bake::{self, ExportOptions},
    prepare,
    progress::Progress,
    vehicle::Vehicle,
};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

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
        vehicle: Vehicle,
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
    Warning(String),
    Finished(Result<PathBuf>),
}

impl Job {
    fn run(self, progress: impl FnMut(Progress<'_>)) -> Result<PathBuf> {
        match self {
            Self::Prepare {
                project,
                scene,
                game,
                vehicle,
                c4dpy,
                out,
            } => prepare::prepare_with_progress(
                project,
                &scene,
                &game,
                vehicle,
                c4dpy.as_deref(),
                &out,
                progress,
            ),
            Self::Export {
                project,
                textures,
                out,
                options,
            } => bake::export_with_progress(&project, &textures, &out, &options, progress),
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
                        Event::Warning(text)
                    } else {
                        Event::Step(text)
                    };
                    let _ = tx.send(event);
                    ctx.request_repaint();
                };
                let result = self.run(progress);
                let _ = tx.send(Event::Finished(result));
                ctx.request_repaint();
            })?;
        Ok(rx)
    }
}

fn chinese(progress: Progress<'_>) -> String {
    match progress {
        Progress::ReadMesh => "读取源模型与材质槽".into(),
        Progress::MatchCamouflage(_) => "匹配游戏原版迷彩 UV2".into(),
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
