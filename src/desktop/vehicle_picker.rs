use super::{
    locale::{Language, Message},
    theme, vehicle_cache,
};
use camo_translator::catalog::{Catalog, Family, InstalledVehicle, is_game_root};
use eframe::egui::{self, Align, Context, Layout, Ui};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Default)]
pub struct VehiclePicker {
    root: PathBuf,
    catalog: Option<Catalog>,
    pending: Option<Receiver<Result<vehicle_cache::LoadedCatalog, Message>>>,
    notice: Option<Message>,
    search: String,
    results_open: bool,
    family: Option<Family>,
}

impl VehiclePicker {
    fn start_scan(&mut self, ctx: &Context, refresh: bool) {
        self.notice = None;
        let (sender, receiver) = mpsc::channel();
        let root = self.root.clone();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("vehicle-catalog".into())
            .spawn(move || {
                let result = if is_game_root(&root) {
                    vehicle_cache::load(&root, refresh)
                } else {
                    Err(invalid_game_folder())
                };
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => self.pending = Some(receiver),
            Err(error) => self.notice = Some(Message::diagnostic(error.to_string())),
        }
    }

    fn poll(&mut self, ctx: &Context, root: &Path) {
        if root != self.root {
            self.root = root.to_owned();
            self.pending = None;
            self.catalog = None;
            self.notice = None;
            self.results_open = false;
            if !root.as_os_str().is_empty() {
                self.start_scan(ctx, false);
            }
        }
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(Ok(loaded)) => {
                    self.catalog = Some(loaded.catalog);
                    self.notice = loaded.notice;
                }
                Ok(Err(error)) => self.notice = Some(error),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.notice = Some(Message::new(
                        "车型目录读取中断",
                        "Reading the vehicle catalog was interrupted.",
                    ))
                }
            }
            self.pending = None;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, root: &Path, selection: &mut String, lang: Language) {
        self.poll(ui.ctx(), root);
        ui.horizontal(|ui| {
            ui.label(theme::label(lang.text("车型", "Vehicle")));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_refresh = self.pending.is_none() && !root.as_os_str().is_empty();
                let rescan = match lang {
                    Language::Chinese => {
                        ui.add_enabled_ui(can_refresh, |ui| theme::compact_button(ui, "重新扫描"))
                            .inner
                    }
                    Language::English => {
                        ui.add_enabled(can_refresh, egui::Button::new("Rescan").small())
                    }
                };
                if rescan.clicked() {
                    self.start_scan(ui.ctx(), true);
                }
                if self.pending.is_some() {
                    ui.spinner();
                    ui.label(theme::note(
                        lang.text("正在读取车型目录…", "Reading the vehicle catalog…"),
                    ));
                }
            });
        });
        let field = ui.add_enabled(
            self.catalog.is_some(),
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(lang.text(
                    "搜索车型（中文、英文或游戏编号）",
                    "Search by Chinese or English name, or game ID",
                ))
                .desired_width(f32::INFINITY)
                .min_size(egui::vec2(0.0, 32.0))
                .vertical_align(Align::Center),
        );
        let Some(catalog) = &self.catalog else {
            if self.pending.is_none() {
                let fallback = lang.text("请选择有效的游戏目录", "Choose a valid game folder.");
                let notice = self
                    .notice
                    .as_ref()
                    .map_or(fallback, |notice| notice.text(lang));
                ui.label(theme::note(notice));
            }
            return;
        };
        if let Some(notice) = &self.notice {
            ui.label(theme::note(notice.text(lang)));
        }
        let selected = catalog.vehicle(selection).ok();
        if let Some(vehicle) = selected {
            let prefix = lang.text("已选", "Selected");
            let name = vehicle_name(vehicle, lang);
            ui.label(theme::note(format!("{prefix} {name}  ·  {}", vehicle.id)));
        } else if !selection.is_empty() {
            ui.label(theme::note(lang.text(
                "当前目录没有上次选择的车型，请重新选择",
                "The previous vehicle is absent from this catalog. Select a vehicle again.",
            )));
        }
        if field.gained_focus() || field.clicked() || field.changed() {
            self.results_open = true;
        }
        if !self.results_open {
            return;
        }

        let results = theme::card().inner_margin(6).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let previous_family = self.family;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.family, None, lang.text("全部", "All"));
                for family in [Family::Ground, Family::Air, Family::Naval] {
                    let english = match family {
                        Family::Ground => "Ground",
                        Family::Air => "Air",
                        Family::Naval => "Naval",
                    };
                    ui.selectable_value(
                        &mut self.family,
                        Some(family),
                        lang.text(family.label(), english),
                    );
                }
            });
            let items: Vec<_> = catalog
                .entries()
                .iter()
                .filter(|item| {
                    self.family.is_none_or(|family| family == item.family)
                        && item.matches(&self.search)
                })
                .collect();
            if items.is_empty() {
                ui.label(theme::note(
                    lang.text("没有匹配的车型", "No matching vehicles"),
                ));
                return None;
            }

            let mut result = None;
            let row_height = 29.0;
            let mut list = egui::ScrollArea::vertical()
                .id_salt("vehicle-results")
                .max_height(108.0)
                .auto_shrink([false, true]);
            if field.changed() || previous_family != self.family {
                list = list.vertical_scroll_offset(0.0);
            }
            list.show_rows(ui, row_height, items.len(), |ui, rows| {
                for item in &items[rows] {
                    let name = vehicle_name(item, lang);
                    let label = format!("{name}  ·  {}", item.id);
                    let active = selected.is_some_and(|vehicle| vehicle.id == item.id);
                    let button = egui::Button::selectable(active, label).truncate();
                    let response = ui.add_sized([ui.available_width(), row_height], button);
                    if active {
                        let rect = response.rect;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                            0,
                            theme::TEXT,
                        );
                    }
                    if response.clicked() {
                        result = Some(item.id.clone());
                    }
                }
            });
            result
        });
        if let Some(id) = results.inner {
            *selection = id;
            self.search.clear();
            self.results_open = false;
        } else if (field.clicked_elsewhere() && results.response.clicked_elsewhere())
            || ui.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.results_open = false;
        }
    }

    pub fn selected(&self, selection: &str) -> Result<&InstalledVehicle, Message> {
        let catalog = self.catalog.as_ref().ok_or_else(|| {
            Message::new(
                "请先读取游戏车型目录",
                "Load the game vehicle catalog first.",
            )
        })?;
        if selection.trim().is_empty() {
            return Err(Message::new("请选择车型", "Select a vehicle."));
        }
        catalog
            .vehicle(selection)
            .map_err(|error| Message::diagnostic(error.to_string()))
    }
}

fn invalid_game_folder() -> Message {
    Message::new("请选择有效的游戏目录", "Choose a valid game folder.")
}

fn vehicle_name(vehicle: &InstalledVehicle, lang: Language) -> &str {
    if vehicle.english.is_empty() {
        &vehicle.name
    } else {
        lang.text(&vehicle.name, &vehicle.english)
    }
}
