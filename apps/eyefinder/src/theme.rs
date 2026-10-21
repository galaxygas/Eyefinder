//! Fonts, colours and widget styling.
//!
//! The look: near-black neutral greys (no blue cast), one yellow accent, sharp 4-6 px corners,
//! accent bars and glows on whatever is selected. Every text colour meets WCAG 2.x contrast 4.5:1
//! against every surface it is drawn on (checked in the tests below).

use std::sync::Arc;

use ef_core::SupportLevel;
use egui::epaint::{Mesh, Shape};
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Rect, RichText, Stroke, TextStyle,
    Ui, Visuals, pos2,
};

pub const FONT_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const FONT_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub raised: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    /// Accent fill (buttons, bars, progress).
    pub accent: Color32,
    /// Accent used as text or icon colour on surfaces.
    pub accent_text: Color32,
    /// Text on an accent fill.
    pub on_accent: Color32,
    /// Accent tint for selected rows and cards.
    pub accent_soft: Color32,
    pub verified: Color32,
    pub spec: Color32,
    pub unsupported: Color32,
    pub danger: Color32,
    pub dark: bool,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0B, 0x0B, 0x0D),
    panel: Color32::from_rgb(0x12, 0x12, 0x14),
    raised: Color32::from_rgb(0x1B, 0x1B, 0x1F),
    border: Color32::from_rgb(0x2B, 0x2B, 0x31),
    text: Color32::from_rgb(0xED, 0xED, 0xEF),
    muted: Color32::from_rgb(0x9C, 0x9C, 0xA4),
    accent: Color32::from_rgb(0xFF, 0xC2, 0x1A),
    accent_text: Color32::from_rgb(0xFF, 0xC2, 0x1A),
    on_accent: Color32::from_rgb(0x17, 0x14, 0x0A),
    accent_soft: Color32::from_rgb(0x2E, 0x26, 0x0E),
    verified: Color32::from_rgb(0x4D, 0xD3, 0x8F),
    spec: Color32::from_rgb(0x5C, 0xB8, 0xFF),
    unsupported: Color32::from_rgb(0x94, 0x94, 0x9D),
    danger: Color32::from_rgb(0xFF, 0x5C, 0x5C),
    dark: true,
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF2, 0xF1, 0xEE),
    panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    raised: Color32::from_rgb(0xEB, 0xE9, 0xE4),
    border: Color32::from_rgb(0xD8, 0xD5, 0xCE),
    text: Color32::from_rgb(0x19, 0x19, 0x1B),
    muted: Color32::from_rgb(0x5C, 0x5A, 0x56),
    accent: Color32::from_rgb(0xF5, 0xB7, 0x00),
    accent_text: Color32::from_rgb(0x85, 0x59, 0x00),
    on_accent: Color32::from_rgb(0x17, 0x14, 0x0A),
    accent_soft: Color32::from_rgb(0xFB, 0xEE, 0xC4),
    verified: Color32::from_rgb(0x16, 0x6C, 0x3C),
    spec: Color32::from_rgb(0x1B, 0x5E, 0xA8),
    unsupported: Color32::from_rgb(0x5E, 0x5C, 0x58),
    danger: Color32::from_rgb(0xB5, 0x24, 0x24),
    dark: false,
};

impl Palette {
    pub fn support(&self, s: SupportLevel) -> Color32 {
        match s {
            SupportLevel::Verified => self.verified,
            SupportLevel::FromSpec => self.spec,
            SupportLevel::DetectedOnly => self.unsupported,
        }
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter".into(), Arc::new(FontData::from_static(FONT_REGULAR)));
    fonts.font_data.insert("inter-semibold".into(), Arc::new(FontData::from_static(FONT_SEMIBOLD)));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    fonts.families.insert(semibold(), vec!["inter-semibold".into(), "inter".into()]);
    // Phosphor icons use the Private Use Area. The bundled Inter files have their PUA glyphs removed
    // (see assets/fonts/README.md), so text and icons never compete for a code point.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    if let Some(ph) =
        fonts.families.get(&FontFamily::Proportional).and_then(|f| f.iter().find(|n| n.contains("phosphor")).cloned())
    {
        fonts.families.get_mut(&semibold()).unwrap().push(ph);
    }
    ctx.set_fonts(fonts);
}

fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.raised;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.selection.bg_fill = p.accent_soft;
    // also the text colour of selected rows, so it must be readable on accent_soft
    v.selection.stroke = Stroke::new(1.0, p.accent_text);
    v.hyperlink_color = p.accent_text;
    v.override_text_color = Some(p.text);
    v.slider_trailing_fill = true;
    let r = CornerRadius::same(5);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.inactive.weak_bg_fill = p.raised;
    v.widgets.inactive.bg_fill = p.raised;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.hovered.weak_bg_fill = p.raised;
    v.widgets.hovered.bg_fill = p.raised;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_fill = p.accent;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.open.weak_bg_fill = p.accent_soft;
    v
}

/// Configures both the dark and the light style; `ctx.set_theme` then picks one.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.animation_time = 0.12;
        // labels are not text-selectable, so clicks reach rows, cards and list items
        s.interaction.selectable_labels = false;
        s.text_styles = [
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
            (TextStyle::Heading, FontId::new(19.0, semibold())),
        ]
        .into();
    });
}

pub fn palette(ctx: &egui::Context) -> Palette {
    match ctx.theme() {
        egui::Theme::Dark => DARK,
        egui::Theme::Light => LIGHT,
    }
}

/// A small uppercase heading with letter spacing, e.g. "LIBRARY".
pub fn section_label(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(10.5).family(semibold()).color(p.muted).extra_letter_spacing(1.2));
}

/// A horizontal accent gradient, fading out to the right (used under the top bar).
pub fn accent_line(painter: &Painter, rect: Rect, c: Color32) {
    let mut m = Mesh::default();
    let clear = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 0);
    m.colored_vertex(rect.left_top(), c);
    m.colored_vertex(rect.right_top(), clear);
    m.colored_vertex(rect.right_bottom(), clear);
    m.colored_vertex(rect.left_bottom(), c);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(m));
}

/// A soft glow around `rect` (selected cards): a few widening, fading outlines.
pub fn glow(painter: &Painter, rect: Rect, radius: u8, c: Color32, strength: f32) {
    for i in 1..=4 {
        let a = (strength * 0.22 / i as f32).clamp(0.0, 1.0);
        painter.rect_stroke(
            rect.expand(i as f32 * 1.5),
            CornerRadius::same(radius + i * 2),
            Stroke::new(1.5, c.gamma_multiply(a)),
            egui::StrokeKind::Outside,
        );
    }
}

/// The checkerboard drawn behind transparent texture previews.
pub fn checkerboard(painter: &Painter, rect: Rect, p: &Palette, cell: f32) {
    let (a, b) = if p.dark {
        (Color32::from_gray(0x26), Color32::from_gray(0x1C))
    } else {
        (Color32::from_gray(0xF4), Color32::from_gray(0xDE))
    };
    painter.rect_filled(rect, 0.0, a);
    let mut m = Mesh::default();
    let (nx, ny) = ((rect.width() / cell).ceil() as i32, (rect.height() / cell).ceil() as i32);
    for y in 0..ny {
        for x in 0..nx {
            if (x + y) % 2 == 1 {
                let r = Rect::from_min_max(
                    pos2(rect.left() + x as f32 * cell, rect.top() + y as f32 * cell),
                    pos2(
                        (rect.left() + (x + 1) as f32 * cell).min(rect.right()),
                        (rect.top() + (y + 1) as f32 * cell).min(rect.bottom()),
                    ),
                );
                m.add_colored_rect(r, b);
            }
        }
    }
    painter.add(Shape::mesh(m));
}

/// A single line of text, cut with "…" when wider than `max_w`.
pub fn elided(
    painter: &Painter,
    text: impl Into<String>,
    font: egui::FontId,
    color: Color32,
    max_w: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(8.0));
    painter.layout_job(job)
}

/// The app icon colours, shared with the window icon.
pub const ICON_DARK: [u8; 3] = [0x0B, 0x0B, 0x0D];
pub const ICON_ACCENT: [u8; 3] = [0xFF, 0xC2, 0x1A];
pub const ICON_LIGHT: [u8; 3] = [0xED, 0xED, 0xEF];

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.x relative luminance and contrast ratio.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let l = |c: Color32| {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (l(a), l(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_colours_meet_wcag_aa() {
        for p in [DARK, LIGHT] {
            for surface in [p.bg, p.panel, p.raised, p.accent_soft] {
                for (name, fg) in [
                    ("text", p.text),
                    ("muted", p.muted),
                    ("accent_text", p.accent_text),
                    ("verified", p.verified),
                    ("spec", p.spec),
                    ("unsupported", p.unsupported),
                    ("danger", p.danger),
                ] {
                    let c = contrast(fg, surface);
                    assert!(c >= 4.5, "{name} {fg:?} on {surface:?}: {c:.2} (dark={})", p.dark);
                }
            }
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "on_accent (dark={})", p.dark);
        }
    }
}
