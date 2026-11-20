use super::theme;
use camo_translator::visibility::{HiddenFaces, HiddenUvGroup, VisibilityReport};
use eframe::egui::{self, Button, Context, Id, Mesh, Modal, Rect, ScrollArea, Sense, Stroke, Ui};
use std::sync::mpsc::Sender;

pub struct VisibilityReview {
    report: VisibilityReport,
    reply: Sender<Option<HiddenFaces>>,
    selected: usize,
}

impl VisibilityReview {
    pub fn new(report: VisibilityReport, reply: Sender<Option<HiddenFaces>>) -> Self {
        Self {
            report,
            reply,
            selected: 0,
        }
    }

    pub fn cancel(self) {
        let _ = self.reply.send(None);
    }

    pub fn show(&mut self, ctx: &Context) -> bool {
        let response = Modal::new(Id::new("hidden-faces-review"))
            .frame(theme::card())
            .show(ctx, |ui| self.contents(ui));
        let choice = response
            .inner
            .or_else(|| response.should_close().then_some(None));
        if let Some(choice) = choice {
            let _ = self.reply.send(choice);
            return true;
        }
        false
    }

    fn contents(&mut self, ui: &mut Ui) -> Option<Option<HiddenFaces>> {
        let width = (ui.ctx().content_rect().width() - 70.0).min(700.0);
        ui.set_width(width);
        ui.label(theme::title("游戏默认隐藏面"));
        ui.label(theme::note(format!(
            "游戏声明 {} 个默认隐藏节点，本模型有 {} 个三角面可从 Painter 模型移除。",
            self.report.default_hidden_nodes.len(),
            self.report.removable_triangles
        )))
        .on_hover_text(self.report.default_hidden_nodes.join("\n"));

        if self.report.ambiguous_triangles > 0 {
            ui.label(theme::note(format!(
                "有 {} 个三角面也匹配可见节点，将保留。",
                self.report.ambiguous_triangles
            )));
        }
        if self.report.partial_triangles > 0 {
            ui.label(theme::note(format!(
                "有 {} 个三角面所在的多边形仅部分隐藏，将保留。",
                self.report.partial_triangles
            )));
        }

        if self.report.groups.is_empty() {
            ui.label("源模型中未确认有符合条件的隐藏面。");
        } else {
            ui.add_space(5.0);
            ui.horizontal_top(|ui| {
                let list_width = width - 272.0;
                ui.vertical(|ui| {
                    ui.set_width(list_width);
                    ui.label(theme::label("游戏节点与源对象"));
                    ScrollArea::vertical()
                        .max_height(262.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (index, group) in self.report.groups.iter().enumerate() {
                                let object = group.object.join(" / ");
                                let text = format!("{} · {}", group.game_node, object);
                                let button = Button::selectable(index == self.selected, &text)
                                    .min_size(egui::vec2(ui.available_width(), 30.0))
                                    .truncate();
                                if ui.add(button).on_hover_text(&text).clicked() {
                                    self.selected = index;
                                }
                            }
                        });
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_width(244.0);
                    let group = &self.report.groups[self.selected];
                    ui.label(theme::label("原 UV 预览"));
                    preview_uv(ui, group);
                    ui.label(theme::note(format!(
                        "{} · {} 个三角面 · 可移除 {} 个",
                        group.texture_set, group.triangles, group.removable_triangles
                    )));
                    ui.label(theme::note(format!(
                        "U {:.3} 至 {:.3}   V {:.3} 至 {:.3}",
                        group.uv_min[0], group.uv_max[0], group.uv_min[1], group.uv_max[1]
                    )));
                });
            });
        }

        if self.report.removable_triangles == self.report.total_triangles {
            ui.label(theme::note(
                "当前模型的所有面均匹配隐藏节点。请选择保留全部面。",
            ));
        }
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add(theme::primary("保留全部面")).clicked() {
                return Some(Some(HiddenFaces::Keep));
            }
            if ui
                .add_enabled(
                    self.report.can_remove(),
                    Button::new("移除默认隐藏面").min_size(egui::vec2(152.0, 37.0)),
                )
                .clicked()
            {
                return Some(Some(HiddenFaces::Remove));
            }
            if ui.button("取消").clicked() {
                return Some(None);
            }
            None
        })
        .inner
    }
}

fn preview_uv(ui: &mut Ui, group: &HiddenUvGroup) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(244.0, 208.0), Sense::hover());
    response.on_hover_text("浅框表示 0–1 区间。高亮表示该节点在源模型上的 UV。");
    let canvas = rect.shrink(12.0);
    let min = egui::vec2(
        group.uv_min[0].floor().min(0.0),
        group.uv_min[1].floor().min(0.0),
    );
    let max = egui::vec2(
        group.uv_max[0].ceil().max(1.0),
        group.uv_max[1].ceil().max(1.0),
    );
    let range = max - min;
    let scale = (canvas.width() / range.x).min(canvas.height() / range.y);
    let center = min + range * 0.5;
    let position = |[u, v]: [f32; 2]| {
        canvas.center() + egui::vec2((u - center.x) * scale, (center.y - v) * scale)
    };
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 5, theme::FIELD);
    painter.rect_stroke(
        Rect::from_two_pos(position([0.0, 0.0]), position([1.0, 1.0])),
        0,
        Stroke::new(1.0_f32, theme::MUTED),
        egui::StrokeKind::Inside,
    );
    let mut mesh = Mesh::default();
    for triangle in &group.uv_triangles {
        let start = mesh.vertices.len() as u32;
        for &uv in triangle {
            mesh.colored_vertex(position(uv), theme::ACCENT);
        }
        mesh.add_triangle(start, start + 1, start + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}
