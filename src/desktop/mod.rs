mod defaults;
mod forms;
mod game_directory;
mod job;
mod theme;

use anyhow::Result;
use defaults::Defaults;
use eframe::egui::{
    self, Align, CentralPanel, Context, Frame, Layout, RichText, ScrollArea, Sense, Stroke,
    TopBottomPanel, Ui, ViewportBuilder, ViewportCommand,
};
use eframe::{
    egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew},
    wgpu,
};
use forms::{ExportForm, PrepareForm};
use job::{Event, Tab};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

const TITLE: &str = "Camo Translator";

pub fn launch() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([1000.0, 755.0])
            .with_min_inner_size([805.0, 610.0]),
        centered: true,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: WgpuConfiguration {
            present_mode: wgpu::PresentMode::Fifo,
            wgpu_setup: WgpuSetup::CreateNew(WgpuSetupCreateNew {
                instance_descriptor: wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::DX12,
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let started = eframe::run_native(
        TITLE,
        options,
        Box::new(|creation| {
            theme::apply(&creation.egui_ctx);
            Ok(Box::new(Desktop::new()))
        }),
    );
    if let Err(error) = started {
        let detail = format!("无法启动 Camo Translator。\n\n{error}");
        show_startup_error(&detail);
        return Err(anyhow::anyhow!(detail));
    }
    Ok(())
}

fn open_output(folder: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    anyhow::ensure!(folder.is_dir(), "输出目录已移动或无法访问");
    let path: Vec<u16> = folder.as_os_str().encode_wide().chain([0]).collect();
    let status = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            windows_sys::w!("open"),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    anyhow::ensure!(status > 32, "Windows 无法打开目录（错误 {status}）");
    Ok(())
}

fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let title = wide(TITLE);
    let message = wide(message);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

fn tool_folder() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(folder) = exe.parent()
    {
        if let Some(target) = folder.parent().filter(|p| p.ends_with("target"))
            && let Some(source) = target.parent().filter(|p| p.join("Cargo.toml").is_file())
        {
            return source.to_owned();
        }
        return folder.to_owned();
    }
    PathBuf::from(".")
}

struct Running {
    tab: Tab,
    events: Receiver<Event>,
    steps: VecDeque<String>,
    started: Instant,
    closing: bool,
}

enum Notice {
    Complete {
        tab: Tab,
        folder: PathBuf,
        open_error: Option<String>,
    },
    Error {
        text: String,
    },
}

struct Desktop {
    tab: Tab,
    prepare: PrepareForm,
    export: ExportForm,
    running: Option<Running>,
    notice: Option<Notice>,
    path_status: Option<Result<(), String>>,
}

impl Desktop {
    fn new() -> Self {
        let home = tool_folder();
        let (paths, path_status) = match defaults::file().and_then(|file| Defaults::load(&file)) {
            Ok(paths) => (paths, None),
            Err(error) => (Defaults::default(), Some(Err(format!("{error:#}")))),
        };
        Self {
            tab: Tab::Prepare,
            prepare: PrepareForm::new(&home, paths.prepare),
            export: ExportForm::new(&home, paths.export),
            running: None,
            notice: None,
            path_status,
        }
    }

    fn poll(&mut self, ctx: &Context) {
        let Some(running) = &mut self.running else {
            return;
        };
        loop {
            match running.events.try_recv() {
                Ok(Event::Step(step)) => {
                    running.steps.push_back(step);
                    if running.steps.len() > 4 {
                        running.steps.pop_front();
                    }
                }
                Ok(Event::Finished(result)) => {
                    self.notice = Some(match result {
                        Ok(folder) => Notice::Complete {
                            tab: running.tab,
                            folder,
                            open_error: None,
                        },
                        Err(error) => Notice::Error {
                            text: format!("{error:#}"),
                        },
                    });
                    self.running = None;
                    return;
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(170));
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    self.notice = Some(Notice::Error {
                        text: "后台任务意外结束。请检查输入后重试。".into(),
                    });
                    self.running = None;
                    return;
                }
            }
        }
    }

    fn start(&mut self, ctx: &Context) {
        let job = match self.tab {
            Tab::Prepare => self.prepare.job(),
            Tab::Export => self.export.job(),
        };
        match job.and_then(|job| Ok(job.start(ctx.clone())?)) {
            Ok(events) => {
                self.notice = None;
                self.running = Some(Running {
                    tab: self.tab,
                    events,
                    steps: VecDeque::from(["正在启动任务".into()]),
                    started: Instant::now(),
                    closing: false,
                });
            }
            Err(error) => {
                self.notice = Some(Notice::Error {
                    text: format!("{error:#}"),
                })
            }
        }
    }

    fn nav(&mut self, ctx: &Context) {
        TopBottomPanel::top("toolbar")
            .frame(
                Frame::new()
                    .fill(theme::BACK)
                    .inner_margin(egui::Margin::symmetric(24, 15)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(egui::vec2(33.0, 33.0), Sense::hover());
                    ui.painter().rect_filled(mark, 7, theme::ACCENT);
                    ui.painter().text(
                        mark.center(),
                        egui::Align2::CENTER_CENTER,
                        "CT",
                        egui::FontId::proportional(13.0),
                        theme::INK,
                    );
                    ui.label(theme::label(TITLE).size(23.0));
                    ui.label(theme::note(format!("v{}", env!("CARGO_PKG_VERSION"))));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        tab_button(ui, &mut self.tab, Tab::Export, "导出涂装");
                        tab_button(ui, &mut self.tab, Tab::Prepare, "准备模型");
                    });
                });
            });
    }

    fn save_paths(&mut self) {
        let paths = Defaults {
            prepare: self.prepare.paths.clone(),
            export: self.export.paths.clone(),
        };
        self.path_status = Some(
            defaults::file()
                .and_then(|file| paths.save(&file))
                .map_err(|error| format!("{error:#}")),
        );
    }

    fn footer(&mut self, ctx: &Context) {
        TopBottomPanel::bottom("actions")
            .frame(
                Frame::new()
                    .fill(theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.running.is_none() {
                        if ui
                            .button("保存默认路径")
                            .on_hover_text("保存准备和导出页面的路径")
                            .clicked()
                        {
                            self.save_paths();
                        }
                        match &self.path_status {
                            Some(Ok(())) => {
                                ui.label(theme::note("已保存"));
                            }
                            Some(Err(error)) => {
                                ui.label(theme::note("路径设置失败").color(theme::ERROR))
                                    .on_hover_text(error);
                            }
                            None => {}
                        }
                    }
                    if let Some(running) = &self.running {
                        ui.spinner();
                        ui.label(theme::note(format!(
                            "已用时 {} 秒",
                            running.started.elapsed().as_secs()
                        )));
                        if running.closing {
                            ui.label(theme::note("处理完成后可以关闭窗口"));
                        }
                    } else if let Some(Notice::Complete {
                        tab,
                        folder,
                        open_error,
                    }) = &mut self.notice
                    {
                        if ui.button("打开输出目录").clicked() {
                            *open_error = open_output(folder).err().map(|error| error.to_string());
                        }
                        if *tab == Tab::Prepare && ui.button("去导出涂装").clicked() {
                            self.export.use_project(folder);
                            self.tab = Tab::Export;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let title = match self.tab {
                            Tab::Prepare => "生成 Painter 模型",
                            Tab::Export => "导出涂装",
                        };
                        if ui
                            .add_enabled(self.running.is_none(), theme::primary(title))
                            .clicked()
                        {
                            self.start(ctx);
                        }
                    });
                });
            });
    }

    fn work_area(&mut self, ctx: &Context, native: &eframe::Frame) {
        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::BACK)
                    .inner_margin(egui::Margin::symmetric(25, 12)),
            )
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .id_salt(self.tab as u8)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            let info_width = 278.0;
                            let form_width = (ui.available_width() - info_width - 23.0).max(360.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(form_width, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(form_width);
                                    match self.tab {
                                        Tab::Prepare => {
                                            ui.label(theme::title("准备 Painter 模型"));
                                            ui.label(theme::note(
                                                "分离复用 UV，保留对象名称和材质槽。",
                                            ));
                                            ui.add_space(10.0);
                                            self.prepare.show(ui, native);
                                        }
                                        Tab::Export => {
                                            ui.label(theme::title("从 Painter 回写 DDS"));
                                            ui.label(theme::note(
                                                "把绘制结果映射到原纹理和游戏迷彩图。",
                                            ));
                                            ui.add_space(10.0);
                                            self.export.show(ui, native);
                                        }
                                    }
                                    ui.add_space(10.0);
                                },
                            );
                            ui.add_space(5.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(info_width, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(info_width);
                                    theme::card().show(ui, |ui| {
                                        ui.set_min_width(info_width - 38.0);
                                        ui.spacing_mut().item_spacing.y = 5.0;
                                        match self.tab {
                                            Tab::Prepare => self.prepare.show_details(ui),
                                            Tab::Export => self.export.show_details(ui),
                                        }
                                    });
                                    self.status(ui, info_width);
                                },
                            );
                        });
                    });
            });
    }

    fn status(&self, ui: &mut Ui, width: f32) {
        if self.running.is_none() && self.notice.is_none() {
            return;
        }
        ui.add_space(8.0);
        theme::card().show(ui, |ui| {
            ui.set_min_width(width - 38.0);
            if let Some(running) = &self.running {
                ui.label(theme::label("处理进度"));
                for (i, step) in running.steps.iter().enumerate() {
                    let last = i + 1 == running.steps.len();
                    ui.label(RichText::new(step).size(13.0).color(if last {
                        theme::ACCENT
                    } else {
                        theme::MUTED
                    }));
                }
            } else if let Some(notice) = &self.notice {
                match notice {
                    Notice::Complete {
                        tab,
                        folder,
                        open_error,
                    } => {
                        let ready = match tab {
                            Tab::Prepare => "Painter 项目已生成",
                            Tab::Export => "涂装已导出",
                        };
                        ui.label(RichText::new(ready).strong().color(theme::ACCENT));
                        ui.add(
                            egui::Label::new(theme::note(forms::path_text(folder)))
                                .wrap()
                                .selectable(true),
                        );
                        if let Some(error) = open_error {
                            ui.label(theme::note(error).color(theme::ERROR));
                        }
                    }
                    Notice::Error { text } => {
                        ui.label(RichText::new("无法完成").strong().color(theme::ERROR));
                        ui.add(
                            egui::Label::new(theme::note(text.as_str()))
                                .wrap()
                                .selectable(true),
                        );
                    }
                }
            }
        });
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame) {
        self.poll(ctx);
        if let Some(running) = &mut self.running
            && ctx.input(|i| i.viewport().close_requested())
        {
            running.closing = true;
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
        self.nav(ctx);
        self.footer(ctx);
        self.work_area(ctx, frame);
    }
}

fn tab_button(ui: &mut Ui, selected: &mut Tab, tab: Tab, title: &str) {
    let active = *selected == tab;
    let text = RichText::new(title).color(if active { theme::ACCENT } else { theme::MUTED });
    let button = egui::Button::new(text)
        .fill(if active { theme::SURFACE } else { theme::BACK })
        .stroke(if active {
            Stroke::new(1.0_f32, theme::BORDER)
        } else {
            Stroke::NONE
        })
        .min_size(egui::vec2(105.0, 34.0));
    if ui.add(button).clicked() {
        *selected = tab;
    }
}
