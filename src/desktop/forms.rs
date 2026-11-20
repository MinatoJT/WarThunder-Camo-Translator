use super::{
    defaults::{ExportPaths, PreparePaths},
    game_directory,
    job::Job,
    theme,
};
use anyhow::{Context, Result, ensure};
use camo_translator::{
    bake::{Alpha, ExportOptions},
    scene::read_json,
    vehicle::Vehicle,
};
use eframe::egui::{self, ComboBox, TextEdit, Ui};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

const SIZES: [u32; 7] = [128, 256, 512, 1024, 2048, 4096, 8192];

pub struct PrepareForm {
    pub(super) paths: PreparePaths,
    name: String,
    detection: Option<bool>,
}

impl PrepareForm {
    pub fn new(home: &Path, mut paths: PreparePaths) -> Self {
        if paths.game.trim().is_empty()
            && let Some(folder) = game_directory::detect(paths.vehicle)
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
                let found = game_directory::detect(self.paths.vehicle);
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
                    ui.label(theme::note("未找到所选车型的安装目录，请手动浏览"));
                }
                None => {}
            }
        });
        ui.add_space(6.0);
        ui.label(theme::label("车型"));
        ComboBox::from_id_salt("vehicle")
            .width(ui.available_width())
            .selected_text(vehicle_name(self.paths.vehicle))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.paths.vehicle, Vehicle::Leopard2a8, "豹 2A8");
                ui.selectable_value(&mut self.paths.vehicle, Vehicle::Type90b, "90B");
            });

        ui.add_space(6.0);
        if path_row(
            ui,
            frame,
            "源模型",
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

    pub fn show_details(&self, ui: &mut Ui) {
        ui.label(theme::label("项目文件").size(18.0));
        ui.add_space(6.0);
        let output = if path(&self.paths.scene)
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("obj"))
        {
            "Painter.obj + Painter.mtl"
        } else if self.is_c4d() {
            "Painter.fbx"
        } else {
            "Painter.fbx 或 Painter.obj"
        };
        detail(ui, output, "导入 Painter 时启用 UV Tile 工作流");
        detail(
            ui,
            "project.json",
            "保存原 UV、Painter UV 与迷彩 UV2 的对应",
        );
        detail(
            ui,
            "prepare-report.json",
            "查看材质槽、UDIM 与 UV2 匹配情况",
        );
    }

    pub fn job(&self) -> Result<Job> {
        let game = directory(&self.paths.game, "请选择 War Thunder 安装目录")?;
        self.paths
            .vehicle
            .pack(&game)
            .context("在所选游戏目录找不到此车型的资源")?;
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
        let (project, out) = output(&self.paths.parent, &self.name)?;
        Ok(Job::Prepare {
            project,
            scene,
            game,
            vehicle: self.paths.vehicle,
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
}

impl ExportForm {
    pub fn new(home: &Path, mut paths: ExportPaths) -> Self {
        if paths.parent.trim().is_empty() {
            paths.parent = path_text(&home.join("preview"));
        }
        Self {
            paths,
            name: String::new(),
            size: 4096,
            camo_size: 0,
            material: false,
            alpha: Alpha::FixedWhite,
            inspected: PathBuf::new(),
            brief: None,
            slots: BTreeSet::new(),
        }
    }

    pub fn use_project(&mut self, project: &Path) {
        self.paths.project = path_text(project);
        self.inspect();
    }

    pub fn show(&mut self, ui: &mut Ui, frame: &eframe::Frame) {
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
        path_row(
            ui,
            frame,
            "输出保存位置",
            &mut self.paths.parent,
            "选择存放 DDS 的目录",
            Picker::Folder,
        );
        ui.add_space(6.0);
        text_row(ui, "输出文件夹名", &mut self.name, "例如 Leopard2A8-skin");

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
            if let Some(vehicle) = Vehicle::from_model(&brief.model) {
                ui.label(theme::note(vehicle_name(vehicle)));
            }
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(theme::label("导出材质槽"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("全选").clicked() {
                        self.slots = brief.slots.iter().map(|slot| slot.name.clone()).collect();
                    }
                });
            });
            for slot in &brief.slots {
                let mut selected = self.slots.contains(&slot.name);
                if ui.checkbox(&mut selected, &slot.name).changed() {
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
        detail(ui, "_c.dds + camo.dds", "颜色回写到原 UV 和游戏迷彩 UV2");
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
        let (_, out) = output(&self.paths.parent, &self.name)?;
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
        self.brief = if folder.as_os_str().is_empty() {
            None
        } else {
            read_json::<ProjectBrief>(&folder.join("prepare-report.json"))
                .ok()
                .map(|mut project| {
                    project.slots.retain(|slot| slot.game_texture.is_some());
                    project
                })
        };
        self.slots = self
            .brief
            .iter()
            .flat_map(|p| p.slots.iter().map(|s| s.name.clone()))
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

fn output(parent: &str, name: &str) -> Result<(String, PathBuf)> {
    let parent = path(parent);
    let name = name.trim();
    ensure!(!parent.as_os_str().is_empty(), "请选择输出保存位置");
    ensure!(!name.is_empty(), "请输入新文件夹名称");
    let mut components = Path::new(name).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
            && !name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
            && !name.ends_with('.'),
        "请填写一个有效的文件夹名称"
    );
    let target = parent.join(name);
    ensure!(!target.exists(), "输出目录已存在，请换一个名称");
    Ok((name.to_owned(), target))
}

fn vehicle_name(vehicle: Vehicle) -> &'static str {
    match vehicle {
        Vehicle::Leopard2a8 => "豹 2A8",
        Vehicle::Type90b => "90B",
    }
}

fn alpha_name(alpha: Alpha) -> &'static str {
    match alpha {
        Alpha::CamoWhite => "复用区域为白（仅 DDS）",
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
