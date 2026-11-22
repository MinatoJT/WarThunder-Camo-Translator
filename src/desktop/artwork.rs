use eframe::egui::{self, Color32, Rect};

pub(super) fn grid(painter: &egui::Painter, area: Rect, color: Color32) {
    const CELL: f32 = 28.0;

    let clip = area.intersect(painter.clip_rect());
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }

    let painter = painter.with_clip_rect(clip);
    let stroke = egui::Stroke::new(0.7_f32, color);
    let mut x = area.left() + ((clip.left() - area.left()) / CELL).ceil() * CELL;
    while x < clip.right() {
        painter.vline(x, clip.y_range(), stroke);
        x += CELL;
    }

    let mut y = area.top() + ((clip.top() - area.top()) / CELL).ceil() * CELL;
    while y < clip.bottom() {
        painter.hline(clip.x_range(), y, stroke);
        y += CELL;
    }
}

pub(super) fn dots(painter: &egui::Painter, area: Rect, color: Color32) {
    const SPACING: f32 = 7.0;
    const RADIUS: f32 = 0.85;

    let clip = area.intersect(painter.clip_rect());
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }

    let painter = painter.with_clip_rect(clip);
    let columns = (area.width() / SPACING).max(1.0) as usize;
    let rows = (area.height() / SPACING).max(1.0) as usize;
    let start =
        area.center() - egui::vec2((columns - 1) as f32, (rows - 1) as f32) * (SPACING * 0.5);

    for column in 0..columns {
        let progress = column as f32 / columns.saturating_sub(1).max(1) as f32;
        let shade = color.gamma_multiply(1.0 - 0.82 * progress * progress);
        let x = start.x + column as f32 * SPACING;
        for row in 0..rows {
            painter.circle_filled(egui::pos2(x, start.y + row as f32 * SPACING), RADIUS, shade);
        }
    }
}

pub(super) fn load_title_contours(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let image = image::load_from_memory(include_bytes!("../../assets/title-contours.png"))
        .ok()?
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(ctx.load_texture(
        "title-contours",
        egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
        egui::TextureOptions::LINEAR,
    ))
}
