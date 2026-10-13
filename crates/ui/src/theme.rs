//! Design tokens (css/tokens.css, light and dark) and the bundled fonts.
//!
//! Instrument Sans (400/500/600) is the UI face, Source Serif 4 (400/600/400 italic) is the transcript face.
//! egui has no font weights, so each weight is its own named family. The fonts egui ships stay behind them
//! as fallbacks for glyphs the Latin subsets do not have (CJK is still not covered, as in the legacy app).
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};
use std::sync::Arc;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// One theme's tokens. Names follow css/tokens.css.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub sunken: Color32,
    pub ink: Color32,
    pub on_ink: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub line: Color32,
    pub line2: Color32,
    pub hover: Color32,
    pub warn: Color32,
    pub warn_bg: Color32,
    pub bad: Color32,
    pub bad_bg: Color32,
    pub ok: Color32,
    pub ok_bg: Color32,
    pub accent: Color32,
    pub scanbg: Color32,
    pub scanhead: Color32,
    pub scanline: Color32,
    pub paper: Color32,
    /// Fixed in both themes (tokens "same in both").
    pub paper_ink: Color32,
    pub paper_rule: Color32,
    pub bar_warn: Color32,
    pub bar_ok: Color32,
}

pub const LIGHT: Tokens = Tokens {
    dark: false,
    bg: rgb(0xEEF1F5),
    surface: rgb(0xFFFFFF),
    sunken: rgb(0xF3F5F8),
    ink: rgb(0x141B2D),
    on_ink: rgb(0xFFFFFF),
    muted: rgb(0x5A6477),
    faint: rgb(0x8792A6),
    line: rgb(0xD3D9E2),
    line2: rgb(0xBCC5D2),
    hover: rgb(0xE4E9F0),
    warn: rgb(0xFFE27A),
    warn_bg: rgb(0xFFF6CF),
    bad: rgb(0xA5221B),
    bad_bg: rgb(0xFBDAD5),
    ok: rgb(0x176B45),
    ok_bg: rgb(0xDDF0E5),
    accent: rgb(0x26358F),
    scanbg: rgb(0xC6CEDA),
    scanhead: rgb(0xD3DAE4),
    scanline: rgb(0xB5BECC),
    paper: rgb(0xFCFCF8),
    paper_ink: rgb(0x26358F),
    paper_rule: rgb(0xC3D4EA),
    bar_warn: rgb(0xFFE27A),
    bar_ok: rgb(0x2F8F62),
};

pub const DARK: Tokens = Tokens {
    dark: true,
    bg: rgb(0x0F131C),
    surface: rgb(0x171C28),
    sunken: rgb(0x1D2331),
    ink: rgb(0xE8ECF3),
    on_ink: rgb(0x0F131C),
    muted: rgb(0x9AA5B8),
    faint: rgb(0x6F7A90),
    line: rgb(0x2B3345),
    line2: rgb(0x3E4A63),
    hover: rgb(0x232B3B),
    warn: rgb(0x6B5712),
    warn_bg: rgb(0x3A3110),
    bad: rgb(0xFF9C92),
    bad_bg: rgb(0x4A1F1B),
    ok: rgb(0x5FD39A),
    ok_bg: rgb(0x14382A),
    accent: rgb(0x9DB0FF),
    scanbg: rgb(0x0A0D14),
    scanhead: rgb(0x131824),
    scanline: rgb(0x252D40),
    paper: rgb(0xD8D7CB),
    paper_ink: rgb(0x26358F),
    paper_rule: rgb(0xC3D4EA),
    bar_warn: rgb(0xFFE27A),
    bar_ok: rgb(0x2F8F62),
};

/// `settings.theme`: "light", "dark" or anything else (auto: follow the system).
pub fn pick(setting: &str, system_dark: bool) -> &'static Tokens {
    match setting {
        "light" => &LIGHT,
        "dark" => &DARK,
        _ if system_dark => &DARK,
        _ => &LIGHT,
    }
}

// ---------------------------------------------------------------- fonts

pub const SANS_MEDIUM: &str = "sans-500";
pub const SANS_SEMI: &str = "sans-600";
pub const SERIF: &str = "serif-400";
pub const SERIF_SEMI: &str = "serif-600";
pub const SERIF_ITALIC: &str = "serif-400i";

fn named(n: &str) -> FontFamily {
    FontFamily::Name(Arc::from(n))
}

pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn sans_medium(size: f32) -> FontId {
    FontId::new(size, named(SANS_MEDIUM))
}
pub fn sans_semi(size: f32) -> FontId {
    FontId::new(size, named(SANS_SEMI))
}
pub fn serif(size: f32) -> FontId {
    FontId::new(size, named(SERIF))
}
pub fn serif_semi(size: f32) -> FontId {
    FontId::new(size, named(SERIF_SEMI))
}
pub fn serif_italic(size: f32) -> FontId {
    FontId::new(size, named(SERIF_ITALIC))
}

pub const MONO: &str = "mono-400";

/// Our faces only. egui's own fonts are not linked (see crates/vendor/PATCHES.md); Hack (Latin, Greek,
/// operators) is the monospace face and the last fallback for glyphs the two main families lack.
pub fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::empty();
    let files: [(&str, &'static [u8]); 7] = [
        ("Instrument Sans", include_bytes!("../assets/fonts/instrument-sans-latin-400-normal.ttf")),
        (SANS_MEDIUM, include_bytes!("../assets/fonts/instrument-sans-latin-500-normal.ttf")),
        (SANS_SEMI, include_bytes!("../assets/fonts/instrument-sans-latin-600-normal.ttf")),
        (SERIF, include_bytes!("../assets/fonts/source-serif-4-latin-400-normal.ttf")),
        (SERIF_SEMI, include_bytes!("../assets/fonts/source-serif-4-latin-600-normal.ttf")),
        (SERIF_ITALIC, include_bytes!("../assets/fonts/source-serif-4-latin-400-italic.ttf")),
        (MONO, include_bytes!("../assets/fonts/hack-latin-400-normal.ttf")),
    ];
    for (name, bytes) in files {
        f.font_data.insert(name.to_string(), FontData::from_static(bytes));
    }
    let with = |first: &str| -> Vec<String> {
        if first == MONO {
            vec![MONO.to_string()]
        } else {
            vec![first.to_string(), MONO.to_string()]
        }
    };
    f.families.insert(FontFamily::Proportional, with("Instrument Sans"));
    for n in [SANS_MEDIUM, SANS_SEMI, SERIF, SERIF_SEMI, SERIF_ITALIC] {
        f.families.insert(named(n), with(n));
    }
    f.families.insert(FontFamily::Monospace, with(MONO));
    f
}

/// Install fonts and the visuals of `t`. Cheap enough to call when the theme changes.
pub fn apply(ctx: &egui::Context, t: &Tokens) {
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.override_text_color = Some(t.ink);
    v.panel_fill = t.bg;
    v.window_fill = t.surface;
    v.extreme_bg_color = t.surface;
    v.faint_bg_color = t.sunken;
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = if t.dark { t.accent.gamma_multiply(0.45) } else { t.accent.gamma_multiply(0.30) };
    v.selection.stroke = Stroke::new(1.0, t.ink);
    let w = &mut v.widgets;
    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.fg_stroke.color = t.ink;
        s.rounding = egui::Rounding::same(8.0);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.line);
    w.inactive.bg_fill = t.surface;
    w.inactive.weak_bg_fill = t.surface;
    w.inactive.bg_stroke = Stroke::new(1.0, t.line2);
    w.hovered.bg_fill = t.hover;
    w.hovered.weak_bg_fill = t.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, t.line2);
    w.active.bg_fill = t.hover;
    w.active.weak_bg_fill = t.hover;
    ctx.set_visuals(v);
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    let mut style = (*ctx.style()).clone();
    style.spacing.interact_size.y = 44.0;
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.text_styles.insert(egui::TextStyle::Body, sans(14.0));
    style.text_styles.insert(egui::TextStyle::Button, sans_semi(14.0));
    style.text_styles.insert(egui::TextStyle::Small, sans(13.0));
    style.text_styles.insert(egui::TextStyle::Heading, sans_semi(16.0));
    ctx.set_style(style);
}
