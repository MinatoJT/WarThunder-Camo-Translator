use eframe::egui::{
    self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Layout,
    Response, RichText, Sense, Shape, Stroke, TextStyle, Ui, Visuals, pos2, vec2,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const BACK: Color32 = Color32::from_rgb(246, 245, 242);
pub const SURFACE: Color32 = Color32::from_rgb(252, 251, 249);
pub const FIELD: Color32 = Color32::from_rgb(255, 254, 252);
pub const BORDER: Color32 = Color32::from_rgb(185, 183, 177);
pub const TEXT: Color32 = Color32::from_rgb(20, 20, 20);
pub const MUTED: Color32 = Color32::from_rgb(81, 80, 76);
pub const ACCENT: Color32 = Color32::from_rgb(255, 240, 19);
pub const SELECTION: Color32 = Color32::from_rgb(255, 247, 153);
pub const BAR: Color32 = Color32::from_rgb(73, 73, 74);
pub const BAR_TEXT: Color32 = Color32::from_rgb(255, 255, 250);
pub const ERROR: Color32 = Color32::from_rgb(159, 31, 29);

const FIELD_BORDER: Color32 = Color32::from_rgb(92, 89, 83);
const DEMI: &str = "Avant Garde Demi";
const BOLD: &str = "Avant Garde Bold";

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    set_fonts(ctx);
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::new(15.0, family(DEMI)));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(13.0));
    style.spacing.item_spacing = vec2(9.0, 6.0);
    style.spacing.interact_size = vec2(38.0, 32.0);
    style.spacing.button_padding = vec2(11.0, 5.0);

    let mut visuals = Visuals::light();
    visuals.panel_fill = BACK;
    visuals.window_fill = SURFACE;
    visuals.window_corner_radius = CornerRadius::same(2);
    visuals.menu_corner_radius = CornerRadius::same(2);
    visuals.extreme_bg_color = FIELD;
    visuals.text_edit_bg_color = Some(FIELD);
    visuals.faint_bg_color = BACK;
    visuals.hyperlink_color = Color32::from_rgb(107, 90, 0);
    visuals.error_fg_color = ERROR;
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.noninteractive.fg_stroke.color = TEXT;
    visuals.widgets.noninteractive.bg_stroke.color = BORDER;
    for (widget, fill, line) in [
        (&mut visuals.widgets.inactive, FIELD, FIELD_BORDER),
        (
            &mut visuals.widgets.hovered,
            Color32::from_rgb(255, 252, 218),
            TEXT,
        ),
        (&mut visuals.widgets.active, SELECTION, TEXT),
        (&mut visuals.widgets.open, SELECTION, TEXT),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0_f32, line);
        widget.fg_stroke.color = TEXT;
        widget.corner_radius = CornerRadius::same(2);
        widget.expansion = 0.0;
    }
    style.visuals = visuals;
    ctx.set_style(style);
}

fn set_fonts(ctx: &egui::Context) {
    let windows_fonts = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    let user_fonts = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|dir| dir.join("Microsoft/Windows/Fonts"));
    let profile_fonts = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|dir| dir.join("AppData/Local/Microsoft/Windows/Fonts"));
    let directories: Vec<&Path> = [user_fonts.as_deref(), profile_fonts.as_deref()]
        .into_iter()
        .flatten()
        .chain([windows_fonts.as_path()])
        .collect();
    let mut fonts = FontDefinitions::default();
    let bundled_cjk = std::env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .map(|folder| folder.join("fonts/NotoSansCJKsc-Regular.otf"))
    });
    if let Some(bytes) = ["msyh.ttc", "Deng.ttf", "simsun.ttc", "simhei.ttf"]
        .into_iter()
        .map(|file| windows_fonts.join(file))
        .chain(bundled_cjk)
        .find_map(|file| std::fs::read(file).ok())
    {
        fonts
            .font_data
            .insert("Chinese UI".into(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("Chinese UI".into());
        }
    }

    let mut body = fonts.families[&FontFamily::Proportional].clone();
    if let Some(font) = installed_font(
        &directories,
        &[
            "ITC Avant Garde Gothic Pro-Medium.otf",
            "ITC Avant Garde Gothic Pro-Book.otf",
        ],
    ) {
        fonts.font_data.insert("Avant Garde Body".into(), font);
        body.insert(0, "Avant Garde Body".into());
    }
    fonts
        .families
        .insert(FontFamily::Proportional, body.clone());
    for (face, files) in [
        (
            DEMI,
            ["ITC Avant Garde Gothic Pro-Demi.otf", "AvantGarde-Demi.otf"],
        ),
        (
            BOLD,
            ["ITC Avant Garde Gothic Pro-Bold.otf", "AvantGarde-Bold.ttf"],
        ),
    ] {
        let mut fallback = body.clone();
        if let Some(font) = installed_font(&directories, &files) {
            fonts.font_data.insert(face.into(), font);
            fallback.insert(0, face.into());
        }
        fonts.families.insert(family(face), fallback);
    }
    ctx.set_fonts(fonts);
}

fn installed_font(directories: &[&Path], filenames: &[&str]) -> Option<Arc<FontData>> {
    filenames
        .iter()
        .flat_map(|name| directories.iter().map(move |folder| folder.join(name)))
        .find_map(|path| std::fs::read(path).ok())
        .map(|bytes| {
            let tweak = egui::epaint::text::FontTweak {
                y_offset_factor: 0.20,
                ..Default::default()
            };
            Arc::new(FontData::from_owned(bytes).tweak(tweak))
        })
}

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

pub fn note(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED).size(13.0)
}

pub fn label(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .color(TEXT)
        .family(family(DEMI))
        .strong()
}

pub fn heading(text: &str, size: f32) -> RichText {
    label(text).family(family(BOLD)).size(size)
}

pub fn title(text: &str) -> RichText {
    heading(text, 25.0)
}

pub fn compact_button(ui: &mut Ui, title: &str) -> Response {
    let text = egui::WidgetText::from(title).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    // Center the visible glyphs across font families and sizes.
    let ink = text.mesh_bounds;
    let width = text.size().x + 2.0 * ui.spacing().button_padding.x;
    let height = (ink.height() + 6.0).max(24.0);
    let response = ui.add(egui::Button::new(()).small().min_size(vec2(width, height)));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
    });
    let color = ui.style().interact(&response).text_color();
    ui.painter()
        .galley(response.rect.center() - ink.center().to_vec2(), text, color);
    response
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .inner_margin(15)
}

pub fn bar_controls(ui: &mut Ui) {
    let visuals = ui.visuals_mut();
    visuals.widgets.noninteractive.fg_stroke.color = BAR_TEXT;
    for (widget, fill, stroke) in [
        (
            &mut visuals.widgets.inactive,
            BAR,
            BAR_TEXT.gamma_multiply(0.75),
        ),
        (
            &mut visuals.widgets.hovered,
            Color32::from_rgb(99, 99, 99),
            BAR_TEXT,
        ),
        (
            &mut visuals.widgets.active,
            Color32::from_rgb(113, 113, 110),
            BAR_TEXT,
        ),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke.color = stroke;
        widget.fg_stroke.color = BAR_TEXT;
    }
}

pub fn section_bar(ui: &mut Ui, title: &str, action: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(BAR)
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                bar_controls(ui);
                action(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let (corner, _) = ui.allocate_exact_size(vec2(9.0, 19.0), Sense::hover());
                    ui.painter().add(Shape::line(
                        vec![corner.left_bottom(), corner.left_top(), corner.right_top()],
                        Stroke::new(2.5_f32, ACCENT),
                    ));
                    ui.add(egui::Label::new(heading(title, 18.0).color(BAR_TEXT)).wrap());
                });
            });
        });
}

pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let layer = ui.painter().add(Shape::Noop);
    let button = ui.add_enabled(
        enabled,
        egui::Button::new(heading(text, 16.0).color(TEXT))
            .frame(false)
            .min_size(vec2(252.0, 42.0)),
    );
    let rect = button.rect;
    let fill = if !enabled {
        Color32::from_rgb(174, 171, 146)
    } else if button.is_pointer_button_down_on() || button.hovered() {
        SELECTION
    } else {
        ACCENT
    };
    let outline = if button.has_focus() {
        Stroke::new(2.0_f32, TEXT)
    } else {
        Stroke::NONE
    };
    let cut = 10.0;
    ui.painter().set(
        layer,
        Shape::convex_polygon(
            vec![
                pos2(rect.min.x + cut, rect.min.y),
                rect.right_top(),
                pos2(rect.max.x, rect.max.y - cut),
                pos2(rect.max.x - cut, rect.max.y),
                rect.left_bottom(),
                pos2(rect.min.x, rect.min.y + cut),
            ],
            fill,
            outline,
        ),
    );
    let center = rect.right_center() - vec2(18.0, 0.0);
    let direction = Stroke::new(1.6_f32, TEXT);
    ui.painter().line_segment(
        [center - vec2(7.0, 0.0), center + vec2(7.0, 0.0)],
        direction,
    );
    ui.painter().add(Shape::line(
        vec![
            center + vec2(1.0, -6.0),
            center + vec2(7.0, 0.0),
            center + vec2(1.0, 6.0),
        ],
        direction,
    ));
    button
}
