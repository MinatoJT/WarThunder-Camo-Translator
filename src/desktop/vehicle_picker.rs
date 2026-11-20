use camo_translator::catalog::{Catalog, Family, InstalledVehicle, is_game_root};
use eframe::egui::{self, Align, Context, Layout, Popup, PopupCloseBehavior, Ui};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::{theme, vehicle_cache};

#[derive(Default)]
pub struct VehiclePicker {
    root: PathBuf,
    catalog: Option<Catalog>,
    pending: Option<Receiver<Result<vehicle_cache::LoadedCatalog, String>>>,
    notice: Option<String>,
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
                    vehicle_cache::load(&root, refresh).map_err(|error| format!("{error:#}"))
                } else {
                    Err("请选择有效的游戏目录".into())
                };
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => self.pending = Some(receiver),
            Err(error) => self.notice = Some(error.to_string()),
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
                Err(TryRecvError::Disconnected) => self.notice = Some("车型目录读取中断".into()),
            }
            self.pending = None;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, root: &Path, selection: &mut String) {
        self.poll(ui.ctx(), root);
        ui.horizontal(|ui| {
            ui.label(theme::label("车型"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can_refresh = self.pending.is_none() && !root.as_os_str().is_empty();
                if ui
                    .add_enabled(can_refresh, egui::Button::new("重新扫描").small())
                    .clicked()
                {
                    self.start_scan(ui.ctx(), true);
                }
                if self.pending.is_some() {
                    ui.spinner();
                    ui.label(theme::note("正在读取车型目录…"));
                }
            });
        });
        let field = ui.add_enabled(
            self.catalog.is_some(),
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("搜索车型（中文、英文或游戏编号）")
                .desired_width(f32::INFINITY),
        );
        let Some(catalog) = &self.catalog else {
            if self.pending.is_none() {
                ui.label(theme::note(
                    self.notice.as_deref().unwrap_or("请选择有效的游戏目录"),
                ));
            }
            return;
        };
        if let Some(notice) = &self.notice {
            ui.label(theme::note(notice));
        }
        let selected = catalog.vehicle(selection).ok();
        if let Some(vehicle) = selected {
            ui.label(theme::note(format!(
                "已选 {}  ·  {}",
                vehicle.name, vehicle.id
            )));
        } else if !selection.is_empty() {
            ui.label(theme::note("当前目录没有上次选择的车型，请重新选择"));
        }
        if field.gained_focus() || field.clicked() || field.changed() {
            self.results_open = true;
        }

        let popup = Popup::from_response(&field)
            .open_bool(&mut self.results_open)
            .close_behavior(PopupCloseBehavior::IgnoreClicks)
            .width(field.rect.width())
            .show(|ui| {
                ui.set_min_width(field.rect.width() - 20.0);
                let previous_family = self.family;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.family, None, "全部");
                    for family in [Family::Ground, Family::Air, Family::Naval] {
                        ui.selectable_value(&mut self.family, Some(family), family.label());
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
                    ui.label(theme::note("没有匹配的车型"));
                }
                let mut result = None;
                let row_height = ui.spacing().interact_size.y;
                let mut list = egui::ScrollArea::vertical().max_height(245.0);
                if field.changed() || previous_family != self.family {
                    list = list.vertical_scroll_offset(0.0);
                }
                list.show_rows(ui, row_height, items.len(), |ui, rows| {
                    for item in &items[rows] {
                        let label = format!("{}  ·  {}", item.name, item.id);
                        let button = egui::Button::selectable(
                            selected.is_some_and(|vehicle| vehicle.id == item.id),
                            label,
                        );
                        if ui
                            .add_sized([ui.available_width(), row_height], button)
                            .clicked()
                        {
                            result = Some(item.id.clone());
                            ui.close();
                        }
                    }
                });
                result
            });
        if let Some(popup) = popup {
            if let Some(id) = popup.inner {
                *selection = id;
                self.search.clear();
            }
            if field.clicked_elsewhere() && popup.response.clicked_elsewhere() {
                self.results_open = false;
            }
        }
    }

    pub fn selected(&self, selection: &str) -> anyhow::Result<&InstalledVehicle> {
        let catalog = self
            .catalog
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("请先读取游戏车型目录"))?;
        anyhow::ensure!(!selection.trim().is_empty(), "请选择车型");
        catalog.vehicle(selection)
    }
}
