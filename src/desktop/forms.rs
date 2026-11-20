use super::{
    defaults::{ExportPaths, PreparePaths},
    game_directory,
    job::Job,
    theme,
    vehicle_picker::VehiclePicker,
};
use anyhow::{Result, ensure};
use camo_translator::{
    bake::{Alpha, ExportOptions},
    catalog::{is_game_root, user_skins},
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

    pub fn show(&mut self, ui: &mut Ui, frame: &eframe::Frame) {
        if path_row(
            ui,
            frame,
            "War Thunder 安装目录",
            &mut self.paths.game,
            "选择包含 content 文件夹的目录",
            Picker::Folder,
        ) {
            self.detection = None;
        }
        ui.horizontal(|ui| {
            if ui.small_button("自动检测").clicked() {
                let found = game_directory::detect();
                self.detection = Some(found.is_some());
                if let Some(folder) = found {
                    self.paths.game = path_text(&folder);
                }
            }
            match self.detection {
                Some(true) => {
                    ui.label(theme::note("已找到游戏目录"));
                }
                Some(false) => {
                    ui.label(theme::note("未找到 War Thunder 安装目录，请手动浏览"));
                }
                None => {}
            }
        });
        ui.add_space(6.0);
        let game = path(&self.paths.game);
        self.vehicles.show(ui, &game, &mut self.paths.vehicle);

        ui.add_space(6.0);
        if path_row(
            ui,
            frame,
            "你已经修完的模型（把想要用camo覆盖的部分单独放进一个Reused对象）",
            &mut self.paths.scene,
            "选择保存好的 C4D 工程或 OBJ",
            Picker::Scene,
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
                "Cinema 4D 脚本程序",
                &mut self.paths.c4dpy,
                "选择 Cinema 4D 目录里的 c4dpy.exe",
                Picker::Cinema,
            );
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        text_row(ui, "项目名称", &mut self.name, "例如 Leopard2A8-v2");
        ui.add_space(6.0);
        path_row(
            ui,
            frame,
            "项目保存位置",
            &mut self.paths.parent,
            "选择存放新项目的目录",
            Picker::Folder,
        );
    }

    pub fn show_tutorial(&mut self, ui: &mut Ui) {
        ui.label(theme::label("1. 把想去除 UV 复用的车体零件用「连接对象 + 删除」合成一个独立对象，命名为 Reused，再交给工具处理。"));
        ui.add_space(5.0);
        ui.label(theme::label(
            "2. 把生成的 Painter 模型导入 Substance Painter，按图设置项目。",
        ));
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
            if self.show_screenshot {
                egui::Window::new("Substance Painter 项目设置")
                    .open(&mut self.show_screenshot)
                    .resizable(false)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| {
                        let available = ui.ctx().content_rect().size() - egui::vec2(50.0, 92.0);
                        let scale = (available.x / texture.size_vec2().x)
                            .min(available.y / texture.size_vec2().y)
                            .clamp(0.3, 1.0);
                        ui.image((texture.id(), texture.size_vec2() * scale));
                    });
            }
        }
        ui.add_space(5.0);
        ui.label(theme::label(
            "3. 画完涂装后把 PNG 导出到项目的 Painter PNG 文件夹，再用工具生成涂装 DDS 和 BLK。",
        ));
    }

    pub fn job(&self) -> Result<Job> {
        let game = directory(&self.paths.game, "请选择 War Thunder 安装目录")?;
        let vehicle = self.vehicles.selected(&self.paths.vehicle)?.clone();
        let scene = file(&self.paths.scene, "请选择源模型文件")?;
        let extension = scene.extension().and_then(|v| v.to_str()).unwrap_or("");
        ensure!(
            extension.eq_ignore_ascii_case("c4d") || extension.eq_ignore_ascii_case("obj"),
            "源模型需要是 .c4d 或 .obj 文件"
        );
        let c4dpy = if self.is_c4d() {
            Some(file(
                &self.paths.c4dpy,
                "请选择 Cinema 4D 目录里的 c4dpy.exe",
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
    material: bool,
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
            material: false,
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

    pub fn show(&mut self, ui: &mut Ui, frame: &eframe::Frame, game: &str) {
        path_row(
            ui,
            frame,
            "已准备的项目",
            &mut self.paths.project,
            "选择包含 project.json 的目录",
            Picker::Folder,
        );
        self.inspect();
        ui.add_space(6.0);
        path_row(
            ui,
            frame,
            "Painter PNG 目录",
            &mut self.paths.textures,
            "选择 Painter 导出的 PNG 所在目录",
            Picker::Folder,
        );
        ui.add_space(6.0);
        if path_row(
            ui,
            frame,
            "输出保存位置",
            &mut self.paths.parent,
            "自动选用游戏的 UserSkins",
            Picker::Folder,
        ) {
            self.paths.auto_parent = Some(false);
        }
        if ui.small_button("使用游戏 UserSkins").clicked() {
            self.paths.auto_parent = Some(true);
            self.follow_game(game);
            if !is_game_root(&path(game))
                && let Some(root) = game_directory::detect()
            {
                self.paths.parent = path_text(&user_skins(&root));
            }
        }
        ui.add_space(6.0);
        text_row(ui, "输出文件夹名", &mut self.name, "例如 Leopard2A8-skin");
        if !self.name.trim().is_empty() && path(&self.paths.parent).join(self.name.trim()).is_dir()
        {
            ui.label(theme::note("本次生成的同名文件将覆盖现有文件"));
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);

        ui.columns(2, |cols| {
            size_field(&mut cols[0], "原纹理尺寸", &mut self.size, false);
            size_field(&mut cols[1], "Camo 尺寸", &mut self.camo_size, true);
        });
        ui.add_space(5.0);
        ui.label(theme::label("原颜色 Alpha"));
        ComboBox::from_id_salt("mask_mode")
            .width(ui.available_width())
            .selected_text(alpha_name(self.alpha))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.alpha,
                    Alpha::CamoWhite,
                    alpha_name(Alpha::CamoWhite),
                );
                ui.selectable_value(
                    &mut self.alpha,
                    Alpha::FixedWhite,
                    alpha_name(Alpha::FixedWhite),
                );
            });
        ui.checkbox(&mut self.material, "同时打包材质通道  _n.dds");
    }

    pub fn show_details(&mut self, ui: &mut Ui) {
        if let Some(brief) = &self.brief {
            ui.label(theme::label(brief.project.as_str()).size(18.0));
            let vehicle = if brief.vehicle_name.is_empty() {
                &brief.model
            } else {
                &brief.vehicle_name
            };
            ui.label(theme::note(vehicle));
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(theme::label("导出材质槽"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("全选").clicked() {
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
                });
            });
            for slot in &brief.slots {
                let mut target = self
                    .bindings
                    .get(&slot.name)
                    .cloned()
                    .or_else(|| slot.game_texture.clone())
                    .unwrap_or_default();
                if !brief.texture_candidates.is_empty() {
                    let previous = target.clone();
                    ComboBox::from_id_salt(("texture_binding", &slot.name))
                        .width(ui.available_width())
                        .selected_text(if target.is_empty() {
                            "选择游戏贴图"
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
                    if target != previous {
                        self.bindings.insert(slot.name.clone(), target.clone());
                        self.slots.insert(slot.name.clone());
                    }
                }
                let mut selected = self.slots.contains(&slot.name);
                if ui
                    .add_enabled(
                        !target.is_empty(),
                        egui::Checkbox::new(&mut selected, &slot.name),
                    )
                    .changed()
                {
                    if selected {
                        self.slots.insert(slot.name.clone());
                    } else {
                        self.slots.remove(&slot.name);
                    }
                }
                if !slot.tiles.is_empty() {
                    let tiles = slot
                        .tiles
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(" · ");
                    ui.label(theme::note(tiles));
                }
            }
        } else {
            ui.label(theme::label("导出内容").size(18.0));
        }

        ui.add_space(11.0);
        detail(ui, "_c.dds", "颜色回写到原 UV");
        detail(ui, "Reused 对象", "标记区域使用游戏 UV2 的迷彩图和单独底图");
        if self.alpha == Alpha::FixedWhite {
            detail(ui, ".blk", "与 DDS 一起放入游戏的 UserSkins 子目录");
        }
        if self.material {
            detail(ui, "_n.dds", "读取 Metallic、Roughness 和 Normal 的 PNG");
        }
    }

    pub fn job(&self) -> Result<Job> {
        let project = directory(&self.paths.project, "请选择包含 project.json 的项目目录")?;
        ensure!(
            project.join("project.json").is_file(),
            "所选项目目录缺少 project.json"
        );
        let textures = directory(&self.paths.textures, "请选择 Painter PNG 目录")?;
        let (_, out) = output(&self.paths.parent, &self.name, true)?;
        ensure!(
            self.brief.is_none() || !self.slots.is_empty(),
            "请至少选择一个材质槽"
        );
        let options = ExportOptions {
            size: self.size,
            camo_size: if self.camo_size == 0 {
                self.size
            } else {
                self.camo_size
            },
            material: self.material,
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

fn size_field(ui: &mut Ui, label: &str, value: &mut u32, allow_same: bool) {
    ui.label(theme::label(label));
    let selected = if *value == 0 {
        "同原纹理".to_owned()
    } else {
        format!("{value} × {value}")
    };
    ComboBox::from_id_salt(label)
        .width(ui.available_width())
        .selected_text(selected)
        .show_ui(ui, |ui| {
            if allow_same {
                ui.selectable_value(value, 0, "同原纹理");
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

fn directory(value: &str, message: &str) -> Result<PathBuf> {
    let p = path(value);
    ensure!(p.is_dir(), "{message}");
    Ok(p)
}

fn file(value: &str, message: &str) -> Result<PathBuf> {
    let p = path(value);
    ensure!(p.is_file(), "{message}");
    Ok(p)
}

fn output(parent: &str, name: &str, allow_existing: bool) -> Result<(String, PathBuf)> {
    let parent = path(parent);
    let name = name.trim();
    ensure!(!parent.as_os_str().is_empty(), "请选择输出保存位置");
    ensure!(!name.is_empty(), "请输入文件夹名称");
    let mut components = Path::new(name).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
            && !name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
            && !name.ends_with('.'),
        "请填写一个有效的文件夹名称"
    );
    let target = parent.join(name);
    ensure!(
        !target.exists() || (allow_existing && target.is_dir()),
        "输出位置不可用，请换一个名称"
    );
    Ok((name.to_owned(), target))
}

fn alpha_name(alpha: Alpha) -> &'static str {
    match alpha {
        Alpha::CamoWhite => "Reused 遮罩为白（仅 DDS）",
        Alpha::FixedWhite => "固定层为白（DDS 和 BLK）",
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
        TextEdit::singleline(value).hint_text(hint),
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
) -> bool {
    let field = ui.label(theme::label(label));
    ui.horizontal(|ui| {
        const BUTTON: f32 = 77.0;
        let width = (ui.available_width() - ui.spacing().item_spacing.x - BUTTON).max(80.0);
        let edited = ui
            .add_sized([width, 33.0], TextEdit::singleline(value).hint_text(hint))
            .labelled_by(field.id)
            .changed();
        if !ui
            .add_sized([BUTTON, 33.0], egui::Button::new("浏览…"))
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
    })
    .inner
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
