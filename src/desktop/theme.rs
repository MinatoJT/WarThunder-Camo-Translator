use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke, TextStyle,
    Visuals,
};
use std::path::PathBuf;
use std::sync::Arc;

pub const BACK: Color32 = Color32::from_rgb(16, 23, 34);
pub const SURFACE: Color32 = Color32::from_rgb(24, 34, 48);
pub const FIELD: Color32 = Color32::from_rgb(13, 21, 32);
pub const BORDER: Color32 = Color32::from_rgb(53, 70, 90);
pub const TEXT: Color32 = Color32::from_rgb(234, 241, 250);
pub const MUTED: Color32 = Color32::from_rgb(169, 187, 208);
pub const ACCENT: Color32 = Color32::from_rgb(110, 232, 210);
pub const INK: Color32 = Color32::from_rgb(13, 35, 37);
pub const ERROR: Color32 = Color32::from_rgb(255, 160, 158);

pub fn apply(ctx: &egui::Context) {
    set_fonts(ctx);
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(13.0));
    style.spacing.item_spacing = egui::vec2(9.0, 7.0);
    style.spacing.interact_size = egui::vec2(38.0, 32.0);
    style.spacing.button_padding = egui::vec2(11.0, 5.0);

    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BACK;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = FIELD;
    visuals.faint_bg_color = SURFACE;
    visuals.hyperlink_color = ACCENT;
    visuals.error_fg_color = ERROR;
    visuals.selection.bg_fill = Color32::from_rgb(32, 96, 101);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(36, 53, 70);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(45, 78, 88);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals = visuals;
    ctx.set_style(style);
}

fn set_fonts(ctx: &egui::Context) {
    let fonts_dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    for file in ["msyh.ttc", "Deng.ttf", "simsun.ttc", "simhei.ttf"] {
        if let Ok(bytes) = std::fs::read(fonts_dir.join(file)) {
            let mut fonts = FontDefinitions::default();
            let name = "Chinese UI".to_owned();
            fonts
                .font_data
                .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push(name.clone());
            }
            ctx.set_fonts(fonts);
            break;
        }
    }
}

pub fn note(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED).size(13.0)
}

pub fn label(text: impl Into<String>) -> RichText {
    RichText::new(text).color(TEXT).strong()
}

pub fn title(text: &str) -> RichText {
    label(text).size(25.0)
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(10)
        .inner_margin(18)
}

pub fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong().color(INK))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(6)
        .min_size(egui::vec2(152.0, 37.0))
}
