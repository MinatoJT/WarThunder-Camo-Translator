use super::{locale::Language, theme};
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

    pub fn show(&mut self, ctx: &Context, lang: Language) -> bool {
        let response = Modal::new(Id::new("hidden-faces-review"))
            .frame(theme::card())
            .show(ctx, |ui| self.contents(ui, lang));
        let choice = response
            .inner
            .or_else(|| response.should_close().then_some(None));
        if let Some(choice) = choice {
            let _ = self.reply.send(choice);
            return true;
        }
        false
    }

    fn contents(&mut self, ui: &mut Ui, lang: Language) -> Option<Option<HiddenFaces>> {
        let width = (ui.ctx().content_rect().width() - 70.0).min(700.0);
        ui.set_width(width);
        ui.label(theme::title(
            lang.text("游戏默认隐藏面", "Faces Hidden by Default In-Game"),
        ));
        let nodes = self.report.default_hidden_nodes.len();
        let removable = self.report.removable_triangles;
        ui.label(theme::note(match lang {
            Language::Chinese => format!(
                "游戏声明 {nodes} 个默认隐藏节点，本模型有 {removable} 个三角面可从 Painter 模型移除。"
            ),
            Language::English => format!(
                "The game declares {nodes} nodes hidden by default. You can remove {removable} triangles from the Painter model."
            ),
        }))
        .on_hover_text(self.report.default_hidden_nodes.join("\n"));

        let ambiguous = self.report.ambiguous_triangles;
        if ambiguous > 0 {
            ui.label(theme::note(match lang {
                Language::Chinese => format!("有 {ambiguous} 个三角面也匹配可见节点，将保留。"),
                Language::English => {
                    format!("{ambiguous} triangles also match visible nodes and will be kept.")
                }
            }));
        }
        let partial = self.report.partial_triangles;
        if partial > 0 {
            ui.label(theme::note(match lang {
                Language::Chinese => {
                    format!("有 {partial} 个三角面所在的多边形仅部分隐藏，将保留。")
                }
                Language::English => format!(
                    "{partial} triangles belong to partially hidden polygons and will be kept."
                ),
            }));
        }

        if self.report.groups.is_empty() {
            ui.label(lang.text(
                "源模型中未确认有符合条件的隐藏面。",
                "No eligible hidden faces were confirmed in the source model.",
            ));
        } else {
            ui.add_space(5.0);
            ui.horizontal_top(|ui| {
                let list_width = width - 272.0;
                ui.vertical(|ui| {
                    ui.set_width(list_width);
                    ui.label(theme::label(
                        lang.text("游戏节点与源对象", "Game Nodes and Source Objects"),
                    ));
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
                    ui.label(theme::label(lang.text("原 UV 预览", "Original UV Preview")));
                    preview_uv(ui, group, lang);
                    ui.label(theme::note(match lang {
                        Language::Chinese => format!(
                            "{} · {} 个三角面 · 可移除 {} 个",
                            group.texture_set, group.triangles, group.removable_triangles
                        ),
                        Language::English => format!(
                            "{} · {} triangles · {} removable",
                            group.texture_set, group.triangles, group.removable_triangles
                        ),
                    }));
                    let to = lang.text("至", "to");
                    ui.label(theme::note(format!(
                        "U {:.3} {to} {:.3}   V {:.3} {to} {:.3}",
                        group.uv_min[0], group.uv_max[0], group.uv_min[1], group.uv_max[1]
                    )));
                });
            });
        }

        if self.report.removable_triangles == self.report.total_triangles {
            ui.label(theme::note(lang.text(
                "当前模型的所有面均匹配隐藏节点。请选择保留全部面。",
                "Every face in this model matches a hidden node. Choose Keep All Faces.",
            )));
        }
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add(
                    Button::new(lang.text("保留全部面", "Keep All Faces"))
                        .fill(theme::ACCENT)
                        .min_size(egui::vec2(152.0, 37.0)),
                )
                .clicked()
            {
                return Some(Some(HiddenFaces::Keep));
            }
            if ui
                .add_enabled(
                    self.report.can_remove(),
                    Button::new(lang.text("移除默认隐藏面", "Remove Default Hidden Faces"))
                        .min_size(egui::vec2(152.0, 37.0)),
                )
                .clicked()
            {
                return Some(Some(HiddenFaces::Remove));
            }
            if ui.button(lang.text("取消", "Cancel")).clicked() {
                return Some(None);
            }
            None
        })
        .inner
    }
}

fn preview_uv(ui: &mut Ui, group: &HiddenUvGroup, lang: Language) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(244.0, 208.0), Sense::hover());
    response.on_hover_text(lang.text(
        "浅框表示 0–1 区间。高亮表示该节点在源模型上的 UV。",
        "The light outline marks 0–1. The highlighted area shows this node's UVs on the source model.",
    ));
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
    painter.rect_filled(rect, 0, theme::BAR);
    painter.rect_stroke(
        Rect::from_two_pos(position([0.0, 0.0]), position([1.0, 1.0])),
        0,
        Stroke::new(1.0_f32, theme::BAR_TEXT),
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
