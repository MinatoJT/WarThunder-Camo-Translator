mod artwork;
mod defaults;
mod forms;
mod game_directory;
mod job;
mod locale;
mod theme;
mod vehicle_cache;
mod vehicle_picker;
mod visibility_review;

use anyhow::Result;
use defaults::Defaults;
use eframe::egui::{
    self, Align, CentralPanel, Color32, Context, Frame, Layout, Rect, ScrollArea, Sense, Stroke,
    TopBottomPanel, Ui, ViewportBuilder, ViewportCommand, pos2, vec2,
};
use eframe::{
    egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew},
    wgpu,
};
use forms::{ExportForm, PrepareForm};
use job::{Event, Tab};
use locale::{Language, Message};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};
use visibility_review::VisibilityReview;

const TITLE: &str = "Camo Translator";

pub fn launch() -> Result<()> {
    let language = defaults::file()
        .and_then(|file| Defaults::load(&file))
        .map(|paths| paths.language)
        .unwrap_or_default();
    let mut viewport = ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([1120.0, 820.0])
        .with_min_inner_size([805.0, 610.0]);
    let icon = image::load_from_memory(include_bytes!("../../assets/camo-translator.png"))
        .ok()
        .map(|image| {
            let image = image.into_rgba8();
            let (width, height) = image.dimensions();
            std::sync::Arc::new(egui::IconData {
                rgba: image.into_raw(),
                width,
                height,
            })
        });
    if let Some(icon) = &icon {
        viewport = viewport.with_icon(icon.clone());
    }
    let options = eframe::NativeOptions {
        viewport,
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
        Box::new(move |creation| {
            let ctx = &creation.egui_ctx;
            theme::apply(ctx);
            ctx.send_viewport_cmd(ViewportCommand::SetTheme(egui::SystemTheme::Dark));
            let icon = icon.map(|icon| {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                );
                ctx.load_texture("application-icon", image, egui::TextureOptions::LINEAR)
            });
            Ok(Box::new(Desktop::new(
                icon,
                artwork::load_title_contours(ctx),
            )))
        }),
    );
    if let Err(error) = started {
        let message = language.text(
            "无法启动 Camo Translator。",
            "Could not start Camo Translator.",
        );
        let detail = format!("{message}\n\n{error}");
        show_startup_error(&detail);
        return Err(anyhow::anyhow!(detail));
    }
    Ok(())
}

fn open_output(folder: &Path) -> Result<(), Message> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    if !folder.is_dir() {
        return Err(Message::new(
            "输出目录已移动或无法访问",
            "The output folder has moved or is inaccessible.",
        ));
    }
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
    if status <= 32 {
        return Err(Message::new(
            format!("Windows 无法打开目录（错误 {status}）"),
            format!("Windows could not open the folder (error {status})."),
        ));
    }
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
    steps: VecDeque<Message>,
    notes: Vec<Message>,
    review: Option<VisibilityReview>,
    started: Instant,
    closing: bool,
}

enum Notice {
    Complete {
        tab: Tab,
        folder: PathBuf,
        open_error: Option<Message>,
        notes: Vec<Message>,
    },
    Error {
        text: Message,
    },
    Cancelled,
}

struct Desktop {
    tab: Tab,
    language: Language,
    icon: Option<egui::TextureHandle>,
    title_contours: Option<egui::TextureHandle>,
    language_error: Option<String>,
    prepare: PrepareForm,
    export: ExportForm,
    running: Option<Running>,
    notice: Option<Notice>,
    path_status: Option<Result<(), String>>,
}

impl Desktop {
    fn new(icon: Option<egui::TextureHandle>, title_contours: Option<egui::TextureHandle>) -> Self {
        let home = tool_folder();
        let (paths, path_status) = match defaults::file().and_then(|file| Defaults::load(&file)) {
            Ok(paths) => (paths, None),
            Err(error) => (Defaults::default(), Some(Err(format!("{error:#}")))),
        };
        let prepare = PrepareForm::new(&home, paths.prepare);
        let export = ExportForm::new(&home, &prepare.paths.game, paths.export);
        Self {
            tab: Tab::Prepare,
            language: paths.language,
            icon,
            title_contours,
            language_error: None,
            prepare,
            export,
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
                Ok(Event::Note(note)) => running.notes.push(note),
                Ok(Event::ReviewHiddenFaces { report, reply }) => {
                    let review = VisibilityReview::new(report, reply);
                    if running.closing {
                        review.cancel();
                    } else {
                        running.review = Some(review);
                    }
                }
                Ok(Event::Finished(result)) => {
                    if running.tab == Tab::Prepare
                        && let Ok(Some(folder)) = &result
                    {
                        self.export.use_project(folder);
                    }
                    self.notice = Some(match result {
                        Ok(Some(folder)) => Notice::Complete {
                            tab: running.tab,
                            folder,
                            open_error: None,
                            notes: std::mem::take(&mut running.notes),
                        },
                        Ok(None) => Notice::Cancelled,
                        Err(text) => Notice::Error { text },
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
                        text: Message::new(
                            "后台任务意外结束。请检查输入后重试。",
                            "The background task stopped unexpectedly. Check the inputs and try again.",
                        ),
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
        match job.and_then(|job| {
            job.start(ctx.clone()).map_err(|error| {
                Message::new(
                    format!("无法启动后台任务：{error}"),
                    format!("Could not start the background task: {error}"),
                )
            })
        }) {
            Ok(events) => {
                self.notice = None;
                self.running = Some(Running {
                    tab: self.tab,
                    events,
                    steps: VecDeque::from([Message::new("正在启动任务", "Starting the task")]),
                    notes: Vec::new(),
                    review: None,
                    started: Instant::now(),
                    closing: false,
                });
            }
            Err(text) => self.notice = Some(Notice::Error { text }),
        }
    }

    fn nav(&mut self, ctx: &Context) {
        let lang = self.language;
        TopBottomPanel::top("toolbar")
            .frame(Frame::new().fill(theme::SURFACE))
            .show(ctx, |ui| {
                let (rule, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                ui.painter().rect_filled(rule, 0, theme::ACCENT);
                let rect = Rect::from_min_size(rule.left_bottom(), vec2(rule.width(), 62.0));
                artwork::grid(ui.painter(), rect, Color32::from_black_alpha(10));
                Frame::new()
                    .inner_margin(egui::Margin::symmetric(22, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(icon) = &self.icon {
                                let response = ui.image((icon.id(), vec2(40.0, 40.0)));
                                ui.painter().rect_stroke(
                                    response.rect,
                                    0,
                                    Stroke::new(1.0_f32, theme::BORDER),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            ui.label(theme::heading(TITLE, 23.0));
                            ui.label(theme::note(format!("v{}", env!("CARGO_PKG_VERSION"))));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                self.language_picker(ui);
                                tab_button(
                                    ui,
                                    &mut self.tab,
                                    Tab::Export,
                                    lang.text("导出涂装", "Export Skin"),
                                );
                                tab_button(
                                    ui,
                                    &mut self.tab,
                                    Tab::Prepare,
                                    lang.text("准备模型", "Prepare Model"),
                                );
                            });
                        });
                    });
                ui.painter().hline(
                    rule.x_range(),
                    ui.min_rect().bottom(),
                    Stroke::new(1.0_f32, theme::BORDER),
                );
            });
    }

    fn language_picker(&mut self, ui: &mut Ui) {
        let before = self.language;
        let prompt = before.text("界面语言", "Interface language");
        let response = egui::ComboBox::from_id_salt("interface-language")
            .width(102.0)
            .selected_text(before.text("简体中文", "English"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.language, Language::Chinese, "简体中文");
                ui.selectable_value(&mut self.language, Language::English, "English");
            })
            .response;
        if self.language != before {
            self.language_error = defaults::file()
                .and_then(|file| Defaults::save_language(&file, self.language))
                .err()
                .map(|error| format!("{error:#}"));
        }
        if let Some(error) = &self.language_error {
            let heading = self
                .language
                .text("界面语言保存失败", "Could not save the interface language");
            response.on_hover_text(format!("{heading}\n{error}"));
        } else {
            response.on_hover_text(prompt);
        }
    }

    fn save_paths(&mut self) {
        let paths = Defaults {
            prepare: self.prepare.paths.clone(),
            export: self.export.paths.clone(),
            language: self.language,
        };
        self.path_status = Some(
            defaults::file()
                .and_then(|file| paths.save(&file))
                .map_err(|error| format!("{error:#}")),
        );
        if matches!(self.path_status, Some(Ok(()))) {
            self.language_error = None;
        }
    }

    fn footer(&mut self, ctx: &Context) {
        let lang = self.language;
        TopBottomPanel::bottom("actions")
            .frame(
                Frame::new()
                    .fill(theme::BAR)
                    .inner_margin(egui::Margin::symmetric(22, 11)),
            )
            .show(ctx, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    theme::bar_controls(ui);
                    let title = match self.tab {
                        Tab::Prepare => lang.text("生成 Painter 模型", "Generate Painter Model"),
                        Tab::Export => lang.text("导出涂装", "Export Skin"),
                    };
                    if theme::primary(ui, title, self.running.is_none()).clicked() {
                        self.start(ctx);
                    }
                    ui.with_layout(
                        Layout::left_to_right(Align::Center).with_main_wrap(true),
                        |ui| {
                            self.secondary_actions(ui);
                            let gap = ui.available_rect_before_wrap();
                            if gap.width() > 72.0 {
                                let dots = Rect::from_min_size(
                                    pos2(gap.left() + 12.0, gap.center().y - 15.0),
                                    vec2((gap.width() - 23.0).min(150.0), 30.0),
                                );
                                artwork::dots(ui.painter(), dots, Color32::from_white_alpha(200));
                            }
                        },
                    );
                });
            });
    }

    fn secondary_actions(&mut self, ui: &mut Ui) {
        let lang = self.language;
        if let Some(running) = &self.running {
            ui.spinner();
            let seconds = running.started.elapsed().as_secs();
            let elapsed = match lang {
                Language::Chinese => format!("已用时 {seconds} 秒"),
                Language::English => format!("Elapsed {seconds} s"),
            };
            ui.label(theme::note(elapsed).color(theme::BAR_TEXT));
            if running.closing {
                ui.label(
                    theme::note(lang.text(
                        "处理完成后可以关闭窗口",
                        "The window can close when processing finishes.",
                    ))
                    .color(theme::BAR_TEXT),
                );
            }
            return;
        }

        if ui
            .button(lang.text("保存默认路径", "Save Default Paths"))
            .on_hover_text(lang.text(
                "保存准备和导出页面的路径",
                "Save the paths on the prepare and export pages.",
            ))
            .clicked()
        {
            self.save_paths();
        }
        match &self.path_status {
            Some(Ok(())) => {
                ui.label(theme::note(lang.text("已保存", "Saved")).color(theme::BAR_TEXT));
            }
            Some(Err(error)) => {
                ui.label(
                    theme::note(lang.text("路径设置失败", "Could not save paths"))
                        .color(Color32::from_rgb(255, 190, 180)),
                )
                .on_hover_text(error);
            }
            None => {}
        }
        if let Some(Notice::Complete {
            tab,
            folder,
            open_error,
            ..
        }) = &mut self.notice
        {
            if ui
                .button(lang.text("打开输出目录", "Open Output Folder"))
                .clicked()
            {
                *open_error = open_output(folder).err();
            }
            if *tab == Tab::Prepare && ui.button(lang.text("去导出涂装", "Go to Export")).clicked()
            {
                self.tab = Tab::Export;
            }
        }
    }

    fn hero(&self, ctx: &Context) {
        let lang = self.language;
        let (title, subtitle) = match self.tab {
            Tab::Prepare => (
                lang.text("准备 Painter 模型", "Prepare Painter Model"),
                None,
            ),
            Tab::Export => (
                lang.text("从 Painter 回写 DDS", "Write Painter Textures to DDS"),
                Some(lang.text(
                    "把绘制结果映射到原纹理和游戏迷彩图",
                    "Map your painting to the original textures and in-game camouflage.",
                )),
            ),
        };

        TopBottomPanel::top("page-heading")
            .frame(Frame::new().fill(theme::BACK))
            .show(ctx, |ui| {
                let origin = ui.cursor().min;
                let width = ui.available_width();
                let background = ui.painter().add(egui::Shape::Noop);
                Frame::new()
                    .inner_margin(egui::Margin::symmetric(22, 11))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            bracket(ui, false);
                            ui.label(theme::heading(title, 34.0));
                            bracket(ui, true);
                        });
                        if let Some(subtitle) = subtitle {
                            ui.label(theme::label(subtitle));
                        }
                    });
                let bottom = ui.min_rect().bottom();
                if let Some(texture) = &self.title_contours {
                    let height = bottom - origin.y;
                    let image = texture.size_vec2();
                    let art_width = (width * 0.54).min(height * image.x / image.y);
                    let area = Rect::from_min_size(
                        origin + vec2(width - art_width, 0.0),
                        vec2(art_width, height),
                    );
                    ui.painter().set(
                        background,
                        egui::Shape::image(
                            texture.id(),
                            area,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        ),
                    );
                }
                ui.painter().hline(
                    origin.x..=(origin.x + width),
                    bottom,
                    Stroke::new(1.0_f32, theme::BORDER),
                );
            });
    }

    fn work_area(&mut self, ctx: &Context, native: &eframe::Frame) {
        let lang = self.language;
        CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::BACK)
                    .inner_margin(egui::Margin::symmetric(22, 14)),
            )
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .id_salt(self.tab as u8)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        let info_width = (width * 0.33).clamp(250.0, 390.0);
                        let form_width = width - info_width - 30.0;
                        let columns = ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.allocate_ui_with_layout(
                                vec2(form_width, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(form_width);
                                    match self.tab {
                                        Tab::Prepare => {
                                            self.prepare.show(ui, native, lang);
                                            self.export.follow_game(&self.prepare.paths.game);
                                        }
                                        Tab::Export => self.export.show(
                                            ui,
                                            native,
                                            &self.prepare.paths.game,
                                            lang,
                                        ),
                                    }
                                    ui.add_space(4.0);
                                },
                            );
                            ui.add_space(30.0);
                            ui.allocate_ui_with_layout(
                                vec2(info_width, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(info_width);
                                    match self.tab {
                                        Tab::Prepare => {
                                            theme::card().show(ui, |ui| {
                                                ui.set_min_width(ui.available_width());
                                                let top = ui.cursor().min - vec2(15.0, 15.0);
                                                ui.painter().line_segment(
                                                    [
                                                        top + vec2(1.0, 1.0),
                                                        top + vec2(info_width - 2.0, 1.0),
                                                    ],
                                                    Stroke::new(3.0_f32, theme::BAR),
                                                );
                                                self.prepare.show_tutorial(ui, lang);
                                            });
                                        }
                                        Tab::Export => self.export.show_details(ui, lang),
                                    }
                                    self.status(ui);
                                },
                            );
                        });
                        let divider = columns.response.rect.left() + form_width + 15.0;
                        ui.painter().vline(
                            divider,
                            columns.response.rect.y_range(),
                            Stroke::new(1.0_f32, theme::BORDER),
                        );
                    });
            });
    }

    fn status(&self, ui: &mut Ui) {
        let lang = self.language;
        if self.running.is_none() && self.notice.is_none() {
            return;
        }
        ui.add_space(8.0);
        let heading = match (&self.running, &self.notice) {
            (Some(_), _) => lang.text("处理进度", "Progress"),
            (
                _,
                Some(Notice::Complete {
                    tab: Tab::Prepare, ..
                }),
            ) => lang.text("Painter 项目已生成", "Painter Project Created"),
            (_, Some(Notice::Complete { .. })) => lang.text("涂装已导出", "Skin Exported"),
            (_, Some(Notice::Error { .. })) => lang.text("无法完成", "Could Not Complete"),
            _ => lang.text(
                "已取消生成 Painter 模型",
                "Painter Model Generation Canceled",
            ),
        };
        theme::card().inner_margin(0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::section_bar(ui, heading, |_| {});
            Frame::new().inner_margin(12).show(ui, |ui| {
                if let Some(running) = &self.running {
                    for (i, step) in running.steps.iter().enumerate() {
                        let text = step.text(lang);
                        if i + 1 == running.steps.len() {
                            ui.label(theme::label(text).size(13.0));
                        } else {
                            ui.label(theme::note(text));
                        }
                    }
                } else if let Some(notice) = &self.notice {
                    match notice {
                        Notice::Complete {
                            folder,
                            open_error,
                            notes,
                            ..
                        } => {
                            ui.add(
                                egui::Label::new(theme::note(forms::path_text(folder)))
                                    .wrap()
                                    .selectable(true),
                            );
                            for note in notes {
                                ui.add(
                                    egui::Label::new(
                                        theme::note(note.text(lang)).color(theme::TEXT),
                                    )
                                    .wrap()
                                    .selectable(true),
                                );
                            }
                            if let Some(error) = open_error {
                                ui.label(theme::note(error.text(lang)).color(theme::ERROR));
                            }
                        }
                        Notice::Error { text } => {
                            ui.add(
                                egui::Label::new(theme::note(text.text(lang)).color(theme::ERROR))
                                    .wrap()
                                    .selectable(true),
                            );
                        }
                        Notice::Cancelled => {}
                    }
                }
            });
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
            if let Some(review) = running.review.take() {
                review.cancel();
            }
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
        self.nav(ctx);
        self.hero(ctx);
        self.footer(ctx);
        self.work_area(ctx, frame);
        if let Some(running) = &mut self.running
            && let Some(review) = &mut running.review
            && review.show(ctx, self.language)
        {
            running.review = None;
        }
    }
}

fn bracket(ui: &mut Ui, right: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 34.0), Sense::hover());
    let (top, bottom, inner_top, inner_bottom) = if right {
        (
            rect.right_top(),
            rect.right_bottom(),
            rect.left_top(),
            rect.left_bottom(),
        )
    } else {
        (
            rect.left_top(),
            rect.left_bottom(),
            rect.right_top(),
            rect.right_bottom(),
        )
    };
    ui.painter().add(egui::Shape::line(
        vec![inner_top, top, bottom, inner_bottom],
        Stroke::new(3.0_f32, theme::ACCENT),
    ));
}

fn tab_button(ui: &mut Ui, selected: &mut Tab, tab: Tab, title: &str) {
    let active = *selected == tab;
    let button = egui::Button::new(theme::label(title))
        .selected(active)
        .fill(if active { theme::ACCENT } else { theme::FIELD })
        .stroke(Stroke::new(1.0_f32, theme::TEXT))
        .min_size(vec2(111.0, 35.0));
    if ui.add(button).clicked() {
        *selected = tab;
    }
}
