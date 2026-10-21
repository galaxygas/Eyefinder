//! Fonts, colors and widget styling.

use std::sync::Arc;

use ef_core::SupportLevel;
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};

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
    pub accent: Color32,
    pub accent_soft: Color32,
    pub verified: Color32,
    pub spec: Color32,
    pub unsupported: Color32,
    pub danger: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x0E, 0x10, 0x14),
    panel: Color32::from_rgb(0x14, 0x17, 0x1D),
    raised: Color32::from_rgb(0x1C, 0x20, 0x28),
    border: Color32::from_rgb(0x2A, 0x2F, 0x3A),
    text: Color32::from_rgb(0xE6, 0xE9, 0xEF),
    muted: Color32::from_rgb(0x8B, 0x93, 0xA3),
    accent: Color32::from_rgb(0x4C, 0xC9, 0xB0),
    accent_soft: Color32::from_rgb(0x17, 0x3A, 0x35),
    verified: Color32::from_rgb(0x46, 0xC2, 0x7A),
    spec: Color32::from_rgb(0xE8, 0xB0, 0x4A),
    unsupported: Color32::from_rgb(0x7A, 0x82, 0x92),
    danger: Color32::from_rgb(0xE5, 0x5B, 0x5B),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF4, 0xF5, 0xF7),
    panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    raised: Color32::from_rgb(0xEC, 0xEE, 0xF2),
    border: Color32::from_rgb(0xD8, 0xDC, 0xE3),
    text: Color32::from_rgb(0x1A, 0x1D, 0x24),
    muted: Color32::from_rgb(0x5E, 0x66, 0x75),
    accent: Color32::from_rgb(0x12, 0x8C, 0x78),
    accent_soft: Color32::from_rgb(0xD5, 0xF0, 0xEA),
    verified: Color32::from_rgb(0x1F, 0x8F, 0x4E),
    spec: Color32::from_rgb(0xB0, 0x74, 0x0B),
    unsupported: Color32::from_rgb(0x7A, 0x82, 0x92),
    danger: Color32::from_rgb(0xC6, 0x34, 0x34),
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

fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.raised;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    v.override_text_color = Some(p.text);
    let r = CornerRadius::same(6);
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
    v.widgets.hovered.weak_bg_fill = p.border;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    v
}

/// Configures both the dark and the light style; `ctx.set_theme` then picks one.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT, false));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
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
