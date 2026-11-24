use super::{
    defaults::{ExportPaths, PreparePaths},
    game_directory,
    job::Job,
    locale::{Language, Message},
    theme,
    vehicle_picker::VehiclePicker,
};
use camo_translator::{
    bake::{Alpha, CAMO_SCALE_PERCENTS, DEFAULT_CAMO_SCALE_PERCENT, ExportOptions},
    catalog::{is_game_root, user_skins},
    color_adjustment::ColorAdjustment,
    materials::TextureBinding,
    prepare::PAINTER_PNG_DIR,
    scene::read_json,
};
use eframe::egui::{self, ComboBox, TextEdit, Ui};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

const SIZES: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

pub struct PrepareForm {
    pub(super) paths: PreparePaths,
    name: String,
    detection: Option<bool>,
    vehicles: VehiclePicker,
    tutorial: Option<egui::TextureHandle>,
    show_screenshot: bool,
}

impl PrepareForm {
    pub fn new(home: &Path, mut paths: PreparePaths) -> Self {
        if paths.game.trim().is_empty()
            && let Some(folder) = game_directory::detect()
        {
            paths.game = path_text(&folder);
        }
        if paths.c4dpy.trim().is_empty()
            && let Some(program) = find_c4dpy()
        {
            paths.c4dpy = path_text(&program);
        }
        if paths.parent.trim().is_empty() {
            paths.parent = path_text(&home.join("projects"));
        }
        Self {
            paths,
            name: String::new(),
            detection: None,
            vehicles: VehiclePicker::default(),
            tutorial: None,
            show_screenshot: false,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, frame: &eframe::Frame, lang: Language) {
        if path_row(
            ui,
            frame,
            lang.text("War Thunder 安装目录", "War Thunder Installation Folder"),
            &mut self.paths.game,
            lang.text(
                "选择包含 content 文件夹的目录",
                "Choose the folder that contains the content folder.",
            ),
            Picker::Folder,
            lang,
        ) {
            self.detection = None;
        }
        ui.horizontal(|ui| {
            if theme::compact_button(ui, lang.text("自动检测", "Detect Automatically")).clicked()
            {
                let found = game_directory::detect();
                self.detection = Some(found.is_some());
                if let Some(folder) = found {
                    self.paths.game = path_text(&folder);
                }
            }
            match self.detection {
                Some(true) => {
                    ui.label(theme::note(
                        lang.text("已找到游戏目录", "Game folder found"),
                    ));
                }
                Some(false) => {
                    ui.label(theme::note(lang.text(
                        "未找到 War Thunder 安装目录，请手动浏览",
                        "War Thunder was not found. Browse to its installation folder.",
                    )));
                }
                None => {}
            }
        });
        ui.add_space(6.0);
        let game = path(&self.paths.game);
        self.vehicles.show(ui, &game, &mut self.paths.vehicle, lang);

        ui.add_space(6.0);
        if path_row(
            ui,
            frame,
            lang.text(
                "你已经修完的模型（把想要用camo覆盖的部分单独放进一个Reused对象）",
                "Your repaired model (put the parts you want to cover with camo in a separate Reused object)",
            ),
            &mut self.paths.scene,
            lang.text("选择保存好的 C4D 工程或 OBJ", "Choose the saved C4D project or OBJ."),
            Picker::Scene,
            lang,
        ) && self.name.trim().is_empty()
            && path(&self.paths.scene).is_file()
            && let Some(stem) = path(&self.paths.scene).file_stem().and_then(|s| s.to_str())
        {
            self.name = stem.to_owned();
        }
        if self.is_c4d() {
            ui.add_space(6.0);
            path_row(
                ui,
                frame,
                lang.text("Cinema 4D 脚本程序", "Cinema 4D Scripting Executable"),
                &mut self.paths.c4dpy,
                lang.text(
                    "选择 Cinema 4D 目录里的 c4dpy.exe",
                    "Choose c4dpy.exe in the Cinema 4D folder.",
                ),
                Picker::Cinema,
                lang,
            );
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        text_row(
            ui,
            lang.text("项目名称", "Project Name"),
            &mut self.name,
            lang.text("例如 Leopard2A8-v2", "e.g. Leopard2A8-v2"),
        );
        ui.add_space(6.0);
        path_row(
            ui,
            frame,
            lang.text("项目保存位置", "Project Save Location"),
            &mut self.paths.parent,
            lang.text(
                "选择存放新项目的目录",
                "Choose where to save the new project.",
            ),
            Picker::Folder,
            lang,
        );
    }

    pub fn show_tutorial(&mut self, ui: &mut Ui, lang: Language) {
        tutorial_step(ui, "1.", lang.text(
            "把想去除 UV 复用的车体零件用「连接对象 + 删除」合成一个独立对象，命名为 Reused，再交给工具处理。",
            "Use Connect Objects + Delete to combine the vehicle parts you want to paint independently into one object. Name it Reused and let the tool handle the UVs.",
        ));
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        tutorial_step(ui, "2.", lang.text(
            "把生成的 Painter 模型导入 Substance Painter，按图设置项目。",
            "Import the generated Painter model into Substance Painter and use the project settings below.",
        ));
        match lang {
            Language::Chinese => self.painter_screenshot(ui),
            Language::English => {
                painter_settings(ui, false);
                if ui.small_button("View Painter Settings").clicked() {
                    self.show_screenshot = true;
                }
            }
        }
        if self.show_screenshot {
            egui::Window::new(lang.text(
                "Substance Painter 项目设置",
                "Substance Painter Project Settings",
            ))
            .id(egui::Id::new("painter-settings-window"))
            .open(&mut self.show_screenshot)
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |ui| match lang {
                Language::Chinese => {
                    if let Some(texture) = &self.tutorial {
                        let available = ui.ctx().content_rect().size() - egui::vec2(50.0, 92.0);
                        let scale = (available.x / texture.size_vec2().x)
                            .min(available.y / texture.size_vec2().y)
                            .clamp(0.3, 1.0);
                        ui.image((texture.id(), texture.size_vec2() * scale));
                    }
                }
                Language::English => painter_settings(ui, true),
            });
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        tutorial_step(ui, "3.", lang.text(
            "画完涂装后把 PNG 导出到项目的 Painter PNG 文件夹，再用工具生成涂装 DDS 和 BLK。",
            "When you finish painting, export the PNGs to the project's Painter PNG folder. Use the tool to generate the skin DDS and BLK files.",
        ));
    }

    fn painter_screenshot(&mut self, ui: &mut Ui) {
        const SETTINGS: &[u8] = include_bytes!("../../assets/painter-settings.png");
        if self.tutorial.is_none() {
            self.tutorial = image::load_from_memory(SETTINGS).ok().map(|image| {
                let image = image.into_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                ui.ctx().load_texture(
                    "painter-settings",
                    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                    egui::TextureOptions::LINEAR,
                )
            });
        }
        if let Some(texture) = &self.tutorial {
            let width = ui.available_width().min(texture.size_vec2().x);
            let size = texture.size_vec2() * (width / texture.size_vec2().x);
            if ui
                .add(egui::Image::new((texture.id(), size)).sense(egui::Sense::click()))
                .on_hover_text("点击放大 Painter 设置")
                .clicked()
            {
                self.show_screenshot = true;
            }
        }
    }

    pub fn job(&self) -> Result<Job, Message> {
        let game = required_path(
            &self.paths.game,
            Path::is_dir,
            (
                "请选择 War Thunder 安装目录",
                "Choose the War Thunder installation folder.",
            ),
        )?;
        let vehicle = self.vehicles.selected(&self.paths.vehicle)?.clone();
        let scene = required_path(
            &self.paths.scene,
            Path::is_file,
            ("请选择源模型文件", "Choose a source model file."),
        )?;
        let extension = scene.extension().and_then(|v| v.to_str()).unwrap_or("");
        if !extension.eq_ignore_ascii_case("c4d") && !extension.eq_ignore_ascii_case("obj") {
            return Err(Message::new(
                "源模型需要是 .c4d 或 .obj 文件",
                "The source model must be a .c4d or .obj file.",
            ));
        }
        let c4dpy = if self.is_c4d() {
            Some(required_path(
                &self.paths.c4dpy,
                Path::is_file,
                (
                    "请选择 Cinema 4D 目录里的 c4dpy.exe",
                    "Choose c4dpy.exe in the Cinema 4D folder.",
                ),
            )?)
        } else {
            None
        };
        let (project, out) = output(&self.paths.parent, &self.name, false)?;
        Ok(Job::Prepare {
            project,
            scene,
            game,
            vehicle,
            c4dpy,
            out,
        })
    }

    fn is_c4d(&self) -> bool {
        path(&self.paths.scene)
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("c4d"))
    }
}

#[derive(Deserialize)]
struct ProjectBrief {
    project: String,
    model: String,
    #[serde(default)]
    vehicle_name: String,
    #[serde(default)]
    texture_candidates: Vec<TextureBinding>,
    slots: Vec<Slot>,
}

#[derive(Deserialize)]
struct Slot {
    name: String,
    game_texture: Option<String>,
    tiles: Vec<u16>,
}

pub struct ExportForm {
    pub(super) paths: ExportPaths,
    name: String,
    size: u32,
    camo_size: u32, // Zero uses the base map size.
    camo_scale_percent: u16,
    material: bool,
    remove_vehicle_decals: bool,
    adjust_colors: bool,
    color_preset: ColorAdjustment,
    alpha: Alpha,
    inspected: PathBuf,
    brief: Option<ProjectBrief>,
    slots: BTreeSet<String>,
    bindings: BTreeMap<String, String>,
}

impl ExportForm {
    pub fn new(home: &Path, game: &str, mut paths: ExportPaths) -> Self {
        let initial_parent = path(&paths.parent);
        let auto = paths.auto_parent.unwrap_or_else(|| {
            initial_parent.as_os_str().is_empty()
                || initial_parent == home.join("preview")
                || initial_parent == user_skins(&path(game))
        });
        paths.auto_parent = Some(auto);
        let mut form = Self {
            paths,
            name: String::new(),
            size: 4096,
            camo_size: 0,
            camo_scale_percent: DEFAULT_CAMO_SCALE_PERCENT,
            material: false,
            remove_vehicle_decals: false,
            adjust_colors: false,
            color_preset: ColorAdjustment::default(),
            alpha: Alpha::FixedWhite,
            inspected: PathBuf::new(),
            brief: None,
            slots: BTreeSet::new(),
            bindings: BTreeMap::new(),
        };
        form.follow_game(game);
        let saved_textures = form.paths.textures.clone();
        form.inspect();
        if !saved_textures.trim().is_empty() {
            form.paths.textures = saved_textures;
        }
        form
    }

    pub fn follow_game(&mut self, game: &str) {
        if self.paths.auto_parent == Some(true) {
            let root = path(game);
            if is_game_root(&root) {
                self.paths.parent = path_text(&user_skins(&root));
            }
        }
    }

    pub fn use_project(&mut self, project: &Path) {
        self.paths.project = path_text(project);
        let textures = project.join(PAINTER_PNG_DIR);
        if textures.is_dir() {
            self.paths.textures = path_text(&textures);
        }
        self.inspect();
    }

    pub fn show(&mut self, ui: &mut Ui, frame: &eframe::Frame, game: &str, lang: Language) {
        path_row(
            ui,
            frame,
            lang.text("已准备的项目", "Prepared Project"),
            &mut self.paths.project,
            lang.text(
                "选择包含 project.json 的目录",
                "Choose the folder that contains project.json.",
            ),
            Picker::Folder,
            lang,
        );
        self.inspect();
        ui.add_space(6.0);
        path_row(
            ui,
            frame,
            lang.text("Painter PNG 目录", "Painter PNG Folder"),
            &mut self.paths.textures,
            lang.text(
                "选择 Painter 导出的 PNG 所在目录",
                "Choose the folder containing Painter's exported PNGs.",
            ),
            Picker::Folder,
            lang,
        );
        ui.add_space(6.0);
        if path_row(
            ui,
            frame,
            lang.text("输出保存位置", "Output Location"),
            &mut self.paths.parent,
            lang.text(
                "自动选用游戏的 UserSkins",
                "Defaults to the game's UserSkins folder.",
            ),
            Picker::Folder,
            lang,
        ) {
            self.paths.auto_parent = Some(false);
        }
        if theme::compact_button(ui, lang.text("使用游戏 UserSkins", "Use Game's UserSkins"))
            .clicked()
        {
            self.paths.auto_parent = Some(true);
            self.follow_game(game);
            if !is_game_root(&path(game))
                && let Some(root) = game_directory::detect()
            {
                self.paths.parent = path_text(&user_skins(&root));
            }
        }
        ui.add_space(6.0);
        text_row(
            ui,
            lang.text("输出文件夹名", "Output Folder Name"),
            &mut self.name,
            lang.text("例如 Leopard2A8-skin", "e.g. Leopard2A8-skin"),
        );
        if !self.name.trim().is_empty() && path(&self.paths.parent).join(self.name.trim()).is_dir()
        {
            ui.label(theme::note(lang.text(
                "本次生成的同名文件将覆盖现有文件",
                "Export will overwrite existing files with the same names.",
            )));
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);

        ui.columns(2, |cols| {
            size_field(
                &mut cols[0],
                "original_size",
                lang.text("原纹理尺寸", "Original Texture Size"),
                &mut self.size,
                None,
            );
            size_field(
                &mut cols[1],
                "camo_size",
                lang.text("Camo 贴图分辨率", "Camo Texture Resolution"),
                &mut self.camo_size,
                Some(lang.text("同原纹理", "Same as Original")),
            );
        });
        ui.add_space(5.0);
        ui.label(theme::label(
            lang.text("游戏内 Camo 缩放", "In-Game Camo Scale"),
        ));
        ComboBox::from_id_salt("camo_scale")
            .width(ui.available_width())
            .height(270.0)
            .selected_text(format!("{}%", self.camo_scale_percent))
            .show_ui(ui, |ui| {
                for percent in CAMO_SCALE_PERCENTS.into_iter().rev() {
                    ui.selectable_value(
                        &mut self.camo_scale_percent,
                        percent,
                        format!("{percent}%"),
                    );
                }
            });
        if self.alpha == Alpha::FixedWhite {
            ui.label(theme::note(lang.text(
                "含 Camo 的涂装会在游戏中固定为所选比例",
                "Skins with Camo use the selected scale in-game.",
            )));
        }
        ui.add_space(5.0);
        ui.label(theme::label(
            lang.text("原颜色 Alpha", "Original Color Alpha"),
        ));
        ComboBox::from_id_salt("mask_mode")
            .width(ui.available_width())
            .selected_text(alpha_name(self.alpha, self.remove_vehicle_decals, lang))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.alpha,
                    Alpha::CamoWhite,
                    alpha_name(Alpha::CamoWhite, self.remove_vehicle_decals, lang),
                );
                ui.selectable_value(
                    &mut self.alpha,
                    Alpha::FixedWhite,
                    alpha_name(Alpha::FixedWhite, self.remove_vehicle_decals, lang),
                );
            });
        ui.checkbox(
            &mut self.material,
            lang.text(
                "同时打包材质通道  _n.dds",
                "Also pack material channels into _n.dds",
            ),
        );
        if self.alpha == Alpha::FixedWhite {
            ui.checkbox(
                &mut self.remove_vehicle_decals,
                lang.text("去除载具贴纸", "Remove Vehicle Decals"),
            )
            .on_hover_text(lang.text(
                "替换使用 Camo 遮罩的材质底图，并一起导出对应的 _n.dds。",
                "Replace the base layer of masked camouflage materials and export their _n.dds maps.",
            ));
        }
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.adjust_colors,
                lang.text("导出时调整色彩", "Adjust Colors on Export"),
            );
            if self.adjust_colors {
                ui.add_space(8.0);
                ComboBox::from_id_salt("color_adjustment_preset")
                    .width(ui.available_width())
                    .selected_text(color_preset_name(self.color_preset, lang))
                    .show_ui(ui, |ui| {
                        for preset in [ColorAdjustment::FullV2, ColorAdjustment::A3Shadows] {
                            ui.selectable_value(
                                &mut self.color_preset,
                                preset,
                                color_preset_name(preset, lang),
                            );
                        }
                    });
            }
        });
    }

    pub fn show_details(&mut self, ui: &mut Ui, lang: Language) {
        theme::card().inner_margin(0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some(brief) = &self.brief {
                theme::section_bar(
                    ui,
                    lang.text("导出材质槽", "Export Material Slots"),
                    |ui| {
                        if ui.small_button(lang.text("全选", "Select All")).clicked() {
                            self.slots = brief
                                .slots
                                .iter()
                                .filter(|slot| {
                                    slot.game_texture.is_some()
                                        || self.bindings.contains_key(&slot.name)
                                })
                                .map(|slot| slot.name.clone())
                                .collect();
                        }
                    },
                );
            } else {
                theme::section_bar(ui, lang.text("导出内容", "Export Contents"), |_| {});
            }

            egui::Frame::new().inner_margin(12).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.spacing_mut().interact_size.y = 26.0;
                if let Some(brief) = &self.brief {
                    ui.label(theme::label(brief.project.as_str()).size(18.0));
                    let vehicle = if lang == Language::English || brief.vehicle_name.is_empty() {
                        &brief.model
                    } else {
                        &brief.vehicle_name
                    };
                    ui.label(theme::note(vehicle));
                    for slot in &brief.slots {
                        ui.separator();
                        let mut target = self
                            .bindings
                            .get(&slot.name)
                            .cloned()
                            .or_else(|| slot.game_texture.clone())
                            .unwrap_or_default();
                        let mut selected = self.slots.contains(&slot.name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !slot.tiles.is_empty() {
                                let tiles = slot
                                    .tiles
                                    .iter()
                                    .map(u16::to_string)
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                ui.label(theme::note(tiles));
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                    if ui
                                        .add_enabled(
                                            !target.is_empty(),
                                            egui::Checkbox::new(
                                                &mut selected,
                                                theme::label(slot.name.as_str()),
                                            ),
                                        )
                                        .on_hover_text(&slot.name)
                                        .changed()
                                    {
                                        if selected {
                                            self.slots.insert(slot.name.clone());
                                        } else {
                                            self.slots.remove(&slot.name);
                                        }
                                    }
                                },
                            );
                        });
                        if !brief.texture_candidates.is_empty() {
                            let previous = target.clone();
                            ui.indent(("texture_binding_indent", &slot.name), |ui| {
                                ComboBox::from_id_salt(("texture_binding", &slot.name))
                                    .width(ui.available_width())
                                    .wrap_mode(egui::TextWrapMode::Truncate)
                                    .selected_text(if target.is_empty() {
                                        lang.text("选择游戏贴图", "Choose Game Texture")
                                    } else {
                                        &target
                                    })
                                    .show_ui(ui, |ui| {
                                        for binding in &brief.texture_candidates {
                                            ui.selectable_value(
                                                &mut target,
                                                binding.color.clone(),
                                                &binding.color,
                                            );
                                        }
                                    });
                            });
                            if target != previous {
                                self.bindings.insert(slot.name.clone(), target);
                                self.slots.insert(slot.name.clone());
                            }
                        }
                    }
                    ui.add_space(5.0);
                    ui.separator();
                }
                detail(
                    ui,
                    "_c.dds",
                    lang.text("颜色回写到原 UV", "Write color to the original UVs."),
                );
                detail(
                    ui,
                    lang.text("Reused 对象", "Reused Object"),
                    lang.text(
                        "标记区域使用游戏 UV2 的迷彩图和单独底图",
                        "Marked areas use the game's UV2 camouflage and a separate base texture.",
                    ),
                );
                if self.alpha == Alpha::FixedWhite {
                    detail(
                        ui,
                        ".blk",
                        lang.text(
                            "与 DDS 一起放入游戏的 UserSkins 子目录",
                            "Place it with the DDS files in a game UserSkins subfolder.",
                        ),
                    );
                }
                if self.material || (self.remove_vehicle_decals && self.alpha == Alpha::FixedWhite)
                {
                    detail(
                        ui,
                        "_n.dds",
                        lang.text(
                            "读取 Metallic、Roughness 和 Normal 的 PNG",
                            "Read the Metallic, Roughness, and Normal PNGs.",
                        ),
                    );
                }
            });
        });
    }

    pub fn job(&self) -> Result<Job, Message> {
        let project = required_path(
            &self.paths.project,
            Path::is_dir,
            (
                "请选择包含 project.json 的项目目录",
                "Choose the project folder that contains project.json.",
            ),
        )?;
        if !project.join("project.json").is_file() {
            return Err(Message::new(
                "所选项目目录缺少 project.json",
                "The selected project folder is missing project.json.",
            ));
        }
        let textures = required_path(
            &self.paths.textures,
            Path::is_dir,
            ("请选择 Painter PNG 目录", "Choose the Painter PNG folder."),
        )?;
        let (_, out) = output(&self.paths.parent, &self.name, true)?;
        if self.brief.is_some() && self.slots.is_empty() {
            return Err(Message::new(
                "请至少选择一个材质槽",
                "Select at least one material slot.",
            ));
        }
        let options = ExportOptions {
            size: self.size,
            camo_size: if self.camo_size == 0 {
                self.size
            } else {
                self.camo_size
            },
            camo_scale_percent: self.camo_scale_percent,
            material: self.material,
            remove_vehicle_decals: self.remove_vehicle_decals && self.alpha == Alpha::FixedWhite,
            color_adjustment: self.adjust_colors.then_some(self.color_preset),
            alpha: self.alpha,
            slots: self.slots.iter().cloned().collect(),
            bindings: self.bindings.clone(),
        };
        Ok(Job::Export {
            project,
            textures,
            out,
            options,
        })
    }

    fn inspect(&mut self) {
        let folder = path(&self.paths.project);
        if folder == self.inspected {
            return;
        }
        self.inspected = folder.clone();
        self.bindings.clear();
        let textures = folder.join(PAINTER_PNG_DIR);
        if !folder.as_os_str().is_empty() && textures.is_dir() {
            self.paths.textures = path_text(&textures);
        }
        self.brief = if folder.as_os_str().is_empty() {
            None
        } else {
            read_json::<ProjectBrief>(&folder.join("prepare-report.json")).ok()
        };
        self.slots = self
            .brief
            .iter()
            .flat_map(|p| {
                p.slots
                    .iter()
                    .filter(|s| s.game_texture.is_some())
                    .map(|s| s.name.clone())
            })
            .collect();
        if self.name.trim().is_empty()
            && let Some(brief) = &self.brief
        {
            self.name = format!("{}-skin", brief.project);
        }
    }
}

fn tutorial_step(ui: &mut Ui, number: &str, text: &str) {
    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(theme::ACCENT)
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| ui.label(theme::label(number)));
        ui.add(egui::Label::new(theme::label(text)).wrap());
    });
}

fn painter_settings(ui: &mut Ui, expanded: bool) {
    theme::card()
        .fill(theme::BACK)
        .inner_margin(9)
        .show(ui, |ui| {
            if expanded {
                ui.set_min_width(335.0);
            } else {
                ui.set_min_width(ui.available_width());
            }
            setting(ui, "Document Resolution", "4096");
            setting(ui, "Normal Map Format", "DirectX");
            if expanded {
                setting(ui, "Compute Tangent Space Per Fragment", "Off");
            }
            setting(ui, "UV Tile Workflow", "On");
            ui.label(theme::note(
                "Preserve UV Tile layout per material and enable painting across tiles.",
            ));
            if expanded {
                setting(ui, "Import Cameras", "On");
                setting(ui, "Auto-Unwrap", "Off");
            }
        });
}

fn setting(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::note(label));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(theme::label(value).size(13.0));
        });
    });
}

fn size_field(ui: &mut Ui, id: &str, label: &str, value: &mut u32, same_size: Option<&str>) {
    ui.label(theme::label(label));
    let selected = if *value == 0 {
        same_size.unwrap_or_default().to_owned()
    } else {
        format!("{value} × {value}")
    };
    ComboBox::from_id_salt(id)
        .width(ui.available_width() - 2.0)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            if let Some(same_size) = same_size {
                ui.selectable_value(value, 0, same_size);
            }
            for size in SIZES {
                ui.selectable_value(value, size, format!("{size} × {size}"));
            }
        });
}

pub fn path_text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn path(value: &str) -> PathBuf {
    PathBuf::from(value.trim().trim_matches('"'))
}

fn required_path(
    value: &str,
    exists: impl FnOnce(&Path) -> bool,
    (chinese, english): (&str, &str),
) -> Result<PathBuf, Message> {
    let p = path(value);
    if exists(&p) {
        Ok(p)
    } else {
        Err(Message::new(chinese, english))
    }
}

fn output(parent: &str, name: &str, allow_existing: bool) -> Result<(String, PathBuf), Message> {
    let parent = path(parent);
    let name = name.trim();
    if parent.as_os_str().is_empty() {
        return Err(Message::new(
            "请选择输出保存位置",
            "Choose an output location.",
        ));
    }
    if name.is_empty() {
        return Err(Message::new("请输入文件夹名称", "Enter a folder name."));
    }
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
        || name.ends_with('.')
    {
        return Err(Message::new(
            "请填写一个有效的文件夹名称",
            "Enter a valid folder name.",
        ));
    }
    let target = parent.join(name);
    if target.exists() && !(allow_existing && target.is_dir()) {
        return Err(Message::new(
            "输出位置不可用，请换一个名称",
            "The output location is unavailable. Choose a different name.",
        ));
    }
    Ok((name.to_owned(), target))
}

fn alpha_name(alpha: Alpha, remove_decals: bool, lang: Language) -> &'static str {
    match alpha {
        Alpha::CamoWhite => lang.text("Reused 遮罩为白（仅 DDS）", "White Reused Mask (DDS Only)"),
        Alpha::FixedWhite if remove_decals => lang.text(
            "替换底图（DDS 和 BLK）",
            "Base Texture Replacement (DDS and BLK)",
        ),
        Alpha::FixedWhite => lang.text(
            "固定层为白（DDS 和 BLK）",
            "White Fixed Layer (DDS and BLK)",
        ),
    }
}

fn color_preset_name(preset: ColorAdjustment, lang: Language) -> &'static str {
    match preset {
        ColorAdjustment::FullV2 => lang.text("完整补偿 · v2", "Full Compensation · v2"),
        ColorAdjustment::A3Shadows => lang.text("A3 + 暗部补偿", "A3 + Shadow Recovery"),
    }
}

fn find_c4dpy() -> Option<PathBuf> {
    let direct = std::env::var_os("C4DPY").map(PathBuf::from);
    direct
        .into_iter()
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths)
                .map(|p| p.join("c4dpy.exe")),
        )
        .find(|p| p.is_file())
}

fn detail(ui: &mut Ui, label: &str, info: &str) {
    ui.label(theme::label(label));
    ui.label(theme::note(info));
    ui.add_space(9.0);
}

enum Picker {
    Folder,
    Scene,
    Cinema,
}

fn text_row(ui: &mut Ui, label: &str, value: &mut String, hint: &str) {
    let label = ui.label(theme::label(label));
    ui.add_sized(
        [ui.available_width(), 33.0],
        TextEdit::singleline(value)
            .hint_text(hint)
            .vertical_align(egui::Align::Center),
    )
    .labelled_by(label.id);
}

fn path_row(
    ui: &mut Ui,
    frame: &eframe::Frame,
    label: &str,
    value: &mut String,
    hint: &str,
    picker: Picker,
    lang: Language,
) -> bool {
    let field = ui.add(egui::Label::new(theme::label(label)).wrap());
    let (row, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 33.0), egui::Sense::hover());
    let browse = egui::Rect::from_min_size(
        row.right_top() - egui::vec2(90.0, 0.0),
        egui::vec2(90.0, 33.0),
    );
    let entry =
        egui::Rect::from_min_max(row.left_top(), browse.left_bottom() - egui::vec2(10.0, 0.0));
    let edited = ui
        .place(
            entry,
            TextEdit::singleline(value)
                .id_salt(field.id)
                .hint_text(hint)
                .vertical_align(egui::Align::Center),
        )
        .labelled_by(field.id)
        .changed();
    if !ui
        .place(browse, egui::Button::new(lang.text("浏览…", "Browse…")))
        .clicked()
    {
        return edited;
    }
    let mut dialog = rfd::FileDialog::new().set_parent(frame).set_title(label);
    let current = path(value);
    if let Some(folder) = current.ancestors().find(|p| p.is_dir()) {
        dialog = dialog.set_directory(folder);
    }
    let selected = match picker {
        Picker::Folder => dialog.pick_folder(),
        Picker::Scene => dialog.add_filter("C4D / OBJ", &["c4d", "obj"]).pick_file(),
        Picker::Cinema => dialog.add_filter("c4dpy.exe", &["exe"]).pick_file(),
    };
    if let Some(chosen) = selected {
        *value = path_text(&chosen);
        true
    } else {
        edited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_detected_user_skins_and_the_prepared_png_folder() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path();
        let game = root.join("War Thunder");
        std::fs::create_dir_all(game.join("content/base/res")).unwrap();
        std::fs::write(game.join("aces.vromfs.bin"), []).unwrap();
        let project = root.join("new project");
        let png = project.join(PAINTER_PNG_DIR);
        std::fs::create_dir_all(&png).unwrap();

        let mut form = ExportForm::new(root, &path_text(&game), ExportPaths::default());
        assert_eq!(path(&form.paths.parent), game.join("UserSkins"));
        form.use_project(&project);
        assert_eq!(path(&form.paths.textures), png);

        form.paths.parent = path_text(&root.join("custom"));
        form.paths.auto_parent = Some(false);
        form.follow_game(&path_text(&game));
        assert_eq!(path(&form.paths.parent), root.join("custom"));
    }
}
