//! Small design-system widgets drawn by hand (rounded segmented controls, icon buttons, pills). Each one
//! is at least 44 x 44 points and goes through `Ctl::reg`.
use crate::theme::{self, Tokens};
use crate::{Ctl, C};
use egui::{pos2, vec2, Color32, Rect, Response, Sense, Shape, Stroke, Ui};

pub const TARGET: f32 = 44.0;
/// `.btn { padding: 0 16px; border: 1px solid }`: what a text button adds to its label's width.
pub const BTN_PAD: f32 = 34.0;

/// Where a text of `size` goes to be centred in `rect`, on a whole pixel. A 14px label centred in a 44 high
/// control lands exactly on a half pixel (22 - 17 / 2), where floating point noise in the layout above decides
/// between two rows of pixels from one frame to the next; this decides it the same way every time.
pub fn centered(rect: Rect, size: egui::Vec2) -> egui::Pos2 {
    let p = rect.center() - size / 2.0;
    pos2((p.x + 0.02).round(), (p.y - 0.02).round())
}

/// A CSS `box-shadow` with a blur radius. egui's own shadow is one linear ramp as wide as `blur`, which is
/// harder at the edge and ends sooner than a Gaussian (sigma = blur / 2): the page sheet and the dialog looked
/// hard-edged next to the browser's. Three ramps of different widths, weighted, give the soft tail; their
/// alphas are chosen so that the solid middle composes to exactly `shadow.color`.
pub fn soft_shadow(shadow: egui::epaint::Shadow, rect: Rect, rounding: f32) -> Vec<egui::epaint::RectShape> {
    if shadow.blur < 4.0 {
        return vec![shadow.as_shape(rect, rounding)];
    }
    let total = shadow.color.a() as f32 / 255.0;
    [(0.5_f32, 0.75_f32), (0.3, 1.5), (0.2, 2.5)]
        .iter()
        .map(|(weight, width)| {
            let a = 1.0 - (1.0 - total).powf(*weight);
            let color = Color32::from_rgba_unmultiplied(shadow.color.r(), shadow.color.g(), shadow.color.b(), (a * 255.0).round() as u8);
            egui::epaint::Shadow { blur: shadow.blur * width, color, ..shadow }.as_shape(rect, rounding)
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    /// Filled with ink (the primary action).
    Primary,
    /// White with a border.
    Outline,
    /// No chrome until hovered.
    Ghost,
}

/// What a control senses. A dimmed control with a reason stays a Tab stop natively (as `aria-disabled` does
/// on the web) so a keyboard user can land on it and hear why; one without a reason is skipped.
fn sense_of(c: &C) -> Sense {
    if !c.disabled {
        Sense::click()
    } else if crate::a11y::disabled_reason(c.id).is_some() {
        Sense::focusable_noninteractive()
    } else {
        Sense::hover()
    }
}

fn paint_look(ui: &Ui, r: &Response, t: &Tokens, look: Look, on: bool, disabled: bool) {
    let p = ui.painter();
    let hover = r.hovered() && !disabled;
    match look {
        Look::Primary => {
            // `.btn.primary:hover { background: var(--ink); filter: brightness(1.15) }`: still ink, a shade brighter
            let b = |c: u8| (c as f32 * 1.15).round().min(255.0) as u8;
            let fill = if disabled { t.line2 } else if hover { Color32::from_rgb(b(t.ink.r()), b(t.ink.g()), b(t.ink.b())) } else { t.ink };
            p.rect_filled(snap(r.rect), 8.0, fill);
        }
        Look::Outline => {
            // (a CSS box is painted on whole pixels with its 1px border inside it)
            let rr = snap(r.rect);
            p.rect_filled(rr, 8.0, if hover { t.sunken } else { t.surface });
            p.rect_stroke(rr.shrink(0.5), 7.5, Stroke::new(1.0, t.line2));
        }
        Look::Ghost => {
            if hover || on {
                p.rect_filled(snap(r.rect), 8.0, t.hover);
            }
        }
    }
}

/// `rect` on whole pixels, as a browser paints a box.
pub fn snap(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min.round(), rect.max.round())
}

/// A text button. `min_w` keeps related buttons aligned; the height is always 44.
pub fn button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, text: &str, look: Look, min_w: f32) -> bool {
    button_pad(ctl, ui, t, c, text, look, min_w, BTN_PAD)
}

/// `button` with its own horizontal padding in total (the zoom label has 6 on each side, buttons 16 + border).
#[allow(clippy::too_many_arguments)]
pub fn button_pad(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, text: &str, look: Look, min_w: f32, pad: f32) -> bool {
    let fg = match look {
        Look::Primary => t.on_ink,
        _ => if c.disabled { t.faint } else { t.ink },
    };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    let w = (g.size().x + pad).max(min_w);
    let sense = sense_of(&c);
    let r = ui.allocate_response(vec2(w, TARGET), sense);
    paint_button(ui, &r, t, &c, g, look);
    ctl.reg(ui, &r, c)
}

/// A text button whose label is `size` px (buttons inherit the font size of what holds them: `.tx-foot` is 13).
pub fn button_sized(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, text: &str, look: Look, size: f32) -> bool {
    let fg = match look {
        Look::Primary => t.on_ink,
        _ => if c.disabled { t.faint } else { t.ink },
    };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(size), fg);
    let sense = sense_of(&c);
    let r = ui.allocate_response(vec2(g.size().x + BTN_PAD, TARGET), sense);
    paint_button(ui, &r, t, &c, g, look);
    ctl.reg(ui, &r, c)
}

fn paint_button(ui: &Ui, r: &Response, t: &Tokens, c: &C, g: std::sync::Arc<egui::Galley>, look: Look) {
    let fg = match look {
        Look::Primary => t.on_ink,
        _ => if c.disabled { t.faint } else { t.ink },
    };
    paint_look(ui, r, t, look, false, c.disabled);
    let pos = centered(r.rect, g.size());
    ui.painter().galley(pos, g, fg);
}

/// A text button drawn at `at` (its top left) without moving the layout cursor: for the design's negative
/// margins (`margin: 4px 0 0 -16px`). The caller reserves the space it wants itself.
pub fn button_at(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, text: &str, look: Look, at: egui::Pos2) -> bool {
    let fg = match look {
        Look::Primary => t.on_ink,
        _ => if c.disabled { t.faint } else { t.ink },
    };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    let rect = Rect::from_min_size(at, vec2(g.size().x + BTN_PAD, TARGET));
    let sense = sense_of(&c);
    let r = ui.interact(rect, ui.id().with(("button-at", c.id)), sense);
    paint_button(ui, &r, t, &c, g, look);
    let disabled = c.disabled;
    ctl.reg(ui, &r, c) && !disabled
}

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Left,
    Right,
    Close,
    Boxes,
    Gear,
    ZoomIn,
    ZoomOut,
    Rotate,
    Level,
    Swap,
    Trash,
    Plus,
    Page,
    Warn,
    Check,
}

/// The 24 x 24 stroke paths of js/ui/icons.js, verbatim (circles and the dashed rect written as paths).
fn icon_path(icon: Icon) -> &'static str {
    match icon {
        Icon::Gear => "M9 12a3 3 0 1 0 6 0a3 3 0 1 0-6 0M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z",
        Icon::Left => "M15 18l-6-6 6-6",
        Icon::Right => "M9 18l6-6-6-6",
        Icon::Check => "M5 12.5l4.5 4.5L19 7.5",
        Icon::Close => "M6 6l12 12M18 6L6 18",
        Icon::Plus => "M12 5v14M5 12h14",
        Icon::Trash => "M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 002 2h6a2 2 0 002-2l1-12M9 7V4h6v3",
        Icon::Warn => "M12 3l10 18H2L12 3zM12 10v5M12 18v.01",
        Icon::Rotate => "M20 12a8 8 0 11-2.6-5.9M20 4v5h-5",
        Icon::Swap => "M4 8h13l-3-3M20 16H7l3 3",
        Icon::Boxes => "M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z",
        Icon::Page => "M7 3h7l5 5v13H7zM14 3v5h5",
        Icon::Level => "M4 9l16-3M4 14l16-3M4 19l16-3",
        Icon::ZoomIn => "M4 11a7 7 0 1 0 14 0a7 7 0 1 0-14 0M20 20l-4-4M11 8v6M8 11h6",
        Icon::ZoomOut => "M4 11a7 7 0 1 0 14 0a7 7 0 1 0-14 0M20 20l-4-4M8 11h6",
    }
}

thread_local! {
    static ICONS: std::cell::RefCell<std::collections::HashMap<&'static str, std::rc::Rc<Vec<crate::svgpath::Sub>>>> = Default::default();
}

fn icon_subs(icon: Icon) -> std::rc::Rc<Vec<crate::svgpath::Sub>> {
    let d = icon_path(icon);
    ICONS.with(|m| m.borrow_mut().entry(d).or_insert_with(|| std::rc::Rc::new(crate::svgpath::parse(d))).clone())
}

/// The line icons of js/ui/icons.js: the same paths, `stroke-width: 2` on the 24-unit grid, scaled to
/// `rect` (the glyph box, 20 to 40).
pub fn paint_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let k = rect.width().min(rect.height()) / 24.0;
    let c = rect.center();
    let s = Stroke::new((2.0 * k).max(1.5), color);
    let p = ui.painter();
    let map = |q: &[f32; 2]| pos2(c.x + (q[0] - 12.0) * k, c.y + (q[1] - 12.0) * k);
    for sub in icon_subs(icon).iter() {
        let mut pts: Vec<egui::Pos2> = sub.pts.iter().map(map).collect();
        if sub.closed && pts.len() > 1 {
            pts.push(pts[0]);
        }
        if pts.len() == 2 && pts[0] == pts[1] {
            p.circle_filled(pts[0], s.width / 2.0, color);
        } else if icon == Icon::Boxes {
            p.add(Shape::dashed_line(&pts, s, 3.0 * k, 3.0 * k));
        } else {
            p.add(Shape::line(pts, s));
            // round joins and caps: a dot at every vertex of a thick stroke
            if s.width > 1.6 {
                let r = s.width / 2.0;
                let first = map(&sub.pts[0]);
                let last = map(sub.pts.last().unwrap());
                p.circle_filled(first, r, color);
                p.circle_filled(last, r, color);
            }
        }
    }
}

/// The `spin` icon (`M12 3a9 9 0 109 9`: three quarters of a circle) turning once in 0.9 s, as
/// `.save-state[data-s="saving"] .ico` does. `time` is the egui clock in seconds.
pub fn paint_spinner(ui: &Ui, rect: Rect, color: Color32, time: f64) {
    let k = rect.width().min(rect.height()) / 24.0;
    let c = rect.center();
    let turn = spinner_turn(time, reduced_motion(ui.ctx()));
    let (sin, cos) = turn.sin_cos();
    let pts: Vec<egui::Pos2> = (0..=24)
        .map(|i| {
            // from the top, counter-clockwise through the left and the bottom to the right: 270 degrees
            let a = -std::f32::consts::FRAC_PI_2 - i as f32 / 24.0 * 1.5 * std::f32::consts::PI;
            let (x, y) = (9.0 * k * a.cos(), 9.0 * k * a.sin());
            pos2(c.x + x * cos - y * sin, c.y + x * sin + y * cos)
        })
        .collect();
    ui.painter().add(Shape::line(pts, Stroke::new((2.0 * k).max(1.5), color)));
}

/// Where the spinner is, in radians: it turns once in 0.9 s, and stands still when the person asked for
/// reduced motion (the CSS `animation: none`).
pub fn spinner_turn(time: f64, reduced: bool) -> f32 {
    if reduced {
        0.0
    } else {
        (time / 0.9).fract() as f32 * std::f32::consts::TAU
    }
}

const REDUCED_MOTION: &str = "reduced-motion";

/// The display preference, kept in the context so drawing code can read it.
pub fn set_reduced_motion(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(REDUCED_MOTION), on));
}

pub fn reduced_motion(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(REDUCED_MOTION))).unwrap_or(false)
}

/// A text button with a leading icon ("Add page").
pub fn icon_text_button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, icon: Icon, text: &str, look: Look) -> bool {
    let fg = if c.disabled { t.faint } else { t.ink };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    // `.btn`: 1px border and 16px padding each side, a 20px icon and a gap of 8
    let w = g.size().x + BTN_PAD + 20.0 + 8.0;
    let sense = sense_of(&c);
    let r = ui.allocate_response(vec2(w, TARGET), sense);
    paint_look(ui, &r, t, look, false, c.disabled);
    paint_icon(ui, Rect::from_center_size(pos2(r.rect.left() + 17.0 + 10.0, r.rect.center().y), vec2(20.0, 20.0)), icon, fg);
    ui.painter().galley(pos2(r.rect.left() + 17.0 + 28.0, (r.rect.center().y - g.size().y / 2.0 - 1.0).round()), g, fg);
    let disabled = c.disabled;
    ctl.reg(ui, &r, c) && !disabled
}

/// The crumb: back arrow and the assignment name (long names are cut with an ellipsis at `max_w`).
pub fn crumb(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, name: &str, max_w: f32) -> bool {
    let font = theme::sans_medium(14.0);
    let mut chars: Vec<char> = name.chars().collect();
    let mut shown = name.to_string();
    let mut g = ui.painter().layout_no_wrap(shown.clone(), font.clone(), t.ink);
    while g.size().x > max_w && !chars.is_empty() {
        chars.pop();
        shown = chars.iter().collect::<String>() + "\u{2026}";
        g = ui.painter().layout_no_wrap(shown.clone(), font.clone(), t.ink);
    }
    // `.crumb { gap: 6px; padding: 0 }`: a 20px arrow, 6, then the name
    let w = (g.size().x + 20.0 + 6.0).max(TARGET);
    let r = ui.allocate_response(vec2(w, TARGET), Sense::click());
    let col = if r.hovered() { t.accent } else { t.ink };
    paint_icon(ui, Rect::from_center_size(pos2(r.rect.left() + 10.0, r.rect.center().y), vec2(20.0, 20.0)), Icon::Left, col);
    ui.painter().galley(pos2(r.rect.left() + 26.0, (r.rect.center().y - g.size().y / 2.0 - 1.0).round()), g, col);
    ctl.reg(ui, &r, c)
}

/// The wordmark at rest (header.js `logo`): "vi" "SH" "ion" in 600 weight monospace, the SH in the accent,
/// in an svg 32px high (viewBox 312 x 84), so 119px wide with the text stretched to 114px.
pub const BRAND_W: f32 = 119.0;
/// How far the second pass of the wordmark's glyphs sits to the right of the first (see `brand`).
pub const BRAND_EMBOLDEN: f32 = 0.45;

/// How long the scan-in lasts (`lg-hand` / `lg-clean` / `lg-bar`: 1.6 s ease-in-out).
const SCAN_SECS: f64 = 1.6;

/// `ease-in-out` of the CSS keyframes (the cubic-bezier is close to this smooth-step).
fn ease(p: f64) -> f32 {
    let p = p.clamp(0.0, 1.0);
    (p * p * (3.0 - 2.0 * p)) as f32
}

/// Progress of the wordmark's scan-in, `None` at rest. It plays once per page load and again whenever the
/// pointer or the keyboard arrives (header.js `logo`, css `.brand:hover`). Reduced motion keeps it at rest.
fn scan_progress(ui: &Ui, r: &egui::Response) -> Option<f64> {
    if reduced_motion(ui.ctx()) {
        return None;
    }
    let now = ui.input(|i| i.time);
    let id = egui::Id::new("brand-scan");
    let (mut start, mut played, mut was_over): (Option<f64>, bool, bool) = ui.ctx().data(|d| d.get_temp(id)).unwrap_or((None, false, false));
    let over = r.hovered() || r.has_focus();
    if !played {
        start = Some(now);
        played = true;
    } else if over && !was_over {
        start = Some(now);
    }
    was_over = over;
    let p = start.map(|s| (now - s) / SCAN_SECS);
    if p.is_some_and(|p| p >= 1.0) {
        start = None;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, (start, played, was_over)));
    match p {
        Some(p) if p < 1.0 => {
            ui.ctx().request_repaint();
            Some(p)
        }
        _ => None,
    }
}

pub fn brand(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C) -> bool {
    // js/ui/header.js `logo()`: SVG text `600 64px mono`, `textLength=300 spacingAndGlyphs` in a 312 x 84 view box
    // drawn 32px high (x 0.381): glyphs are stretched to 114.3px wide, the baseline sits 24.4px under the top of
    // the 32px box, which is centred in the 44px button. The stretch is applied to the glyph meshes.
    let font = theme::mono_bold(21.0);
    let parts = [("vi", t.ink), ("SH", t.accent), ("ion", t.ink)];
    let r = ui.allocate_response(vec2(BRAND_W, TARGET), Sense::click());
    let hover = if r.hovered() { Some(t.accent) } else { None };
    let scan = scan_progress(ui, &r).map(ease);
    let gs: Vec<_> = parts.iter().map(|(s, col)| ui.painter().layout_no_wrap((*s).to_string(), font.clone(), hover.unwrap_or(*col))).collect();
    let natural: f32 = gs.iter().map(|g| g.size().x).sum();
    let x0 = r.rect.left() + 1.0;
    let k = 117.4 / natural.max(1.0);
    let baseline = r.rect.center().y + 8.38;
    // the lowest vertex of the first row is the glyph quads bottom, a fixed distance under the baseline
    let bottom = gs
        .iter()
        .flat_map(|g| g.rows.iter().take(1))
        .flat_map(|row| row.visuals.mesh.vertices.iter().map(|v| v.pos.y))
        .fold(f32::MIN, f32::max);
    // (the glyph quads reach about 3px under the baseline at this size: measured against the browser)
    let dy = baseline - bottom + 0.3;
    // the row meshes use texel UVs; the tessellator normalises them for text shapes, here it is done by hand
    let [tw, th] = ui.fonts(|f| f.font_image_size());
    // during the scan-in the typed word is revealed from the left (`clip-path: inset(0 100% 0 0)` to `inset(0)`)
    // and the handwriting is wiped away from the left (`inset(0)` to `inset(0 0 0 100%)`)
    let text_w = 117.4;
    let edge = x0 + scan.unwrap_or(1.0) * text_w;
    let clean_clip = if scan.is_some() { Rect::from_min_max(r.rect.min, pos2(edge, r.rect.bottom())) } else { r.rect.expand(4.0) };
    let p = ui.painter().with_clip_rect(clean_clip);
    let mut x = 0.0;
    for g in &gs {
        for row in &g.rows {
            let mut m = row.visuals.mesh.clone();
            for v in &mut m.vertices {
                v.pos = pos2(x0 + (x + v.pos.x) * k, v.pos.y + dy);
                v.uv = pos2(v.uv.x / tw as f32, v.uv.y / th as f32);
            }
            // JS draws Consolas at weight 600 (a real bold, about a fifth more ink than Hack Bold at this size):
            // the same glyphs once more, a fraction of a pixel to the right, make the strokes as heavy
            let mut m2 = m.clone();
            for v in &mut m2.vertices {
                v.pos.x += BRAND_EMBOLDEN;
            }
            p.add(egui::Shape::mesh(m));
            p.add(egui::Shape::mesh(m2));
        }
        x += g.size().x;
    }
    if let Some(e) = scan {
        // the handwriting: Caveat 600 at 68 units, turned 3 degrees about the middle of the view box, the part
        // to the right of the scan line
        let unit = 32.0 / 84.0;
        let hand = ui.painter().layout_no_wrap("viSHion".to_string(), theme::hand(68.0 * unit), t.accent);
        let hw = hand.size().x.max(1.0);
        let kh = (300.0 * unit) / hw;
        let top = r.rect.center().y - 16.0;
        let hbottom = hand.rows.first().map(|row| row.visuals.mesh.vertices.iter().map(|v| v.pos.y).fold(f32::MIN, f32::max)).unwrap_or(0.0);
        let hdy = (top + (82.0 - 18.0) * unit) - hbottom + 0.3;
        let hx0 = r.rect.left() + (30.0 - 24.0) * unit;
        let pivot = pos2(r.rect.left() + (180.0 - 24.0) * unit, top + (60.0 - 18.0) * unit);
        let (sin, cos) = (-3.0f32).to_radians().sin_cos();
        let hand_clip = Rect::from_min_max(pos2(edge, r.rect.top()), r.rect.right_bottom() + vec2(8.0, 0.0));
        let hp = ui.painter().with_clip_rect(hand_clip);
        for row in &hand.rows {
            let mut m = row.visuals.mesh.clone();
            for v in &mut m.vertices {
                let q = pos2(hx0 + v.pos.x * kh, v.pos.y + hdy) - pivot;
                v.pos = pivot + vec2(q.x * cos - q.y * sin, q.x * sin + q.y * cos);
                v.uv = pos2(v.uv.x / tw as f32, v.uv.y / th as f32);
            }
            hp.add(egui::Shape::mesh(m));
        }
        // the scan bar: 4 x 76 units, sweeping 300 units to the right, gone in the last 8% of the time
        let fade = if e > 0.92 { 1.0 - (e - 0.92) / 0.08 } else { 1.0 };
        let bar = Rect::from_min_size(pos2(r.rect.left() + (28.0 - 24.0) * unit + e * 300.0 * unit, top + (22.0 - 18.0) * unit), vec2(4.0 * unit, 76.0 * unit));
        ui.painter().rect_filled(bar, 1.0, t.accent.gamma_multiply(fade));
    }
    ctl.reg(ui, &r, c)
}

/// A 44 x 44 icon-only button; `label` is its accessible name.
pub fn icon_button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, icon: Icon, on: bool) -> bool {
    let disabled = c.disabled;
    let sense = sense_of(&c);
    let r = ui.allocate_response(vec2(TARGET, TARGET), sense);
    paint_look(ui, &r, t, Look::Ghost, on, disabled);
    paint_icon(ui, Rect::from_center_size(r.rect.center(), vec2(22.0, 22.0)), icon, if disabled { t.faint } else { t.ink });
    ctl.reg(ui, &r, c) && !disabled
}

/// Two-option segmented control in a rounded tray. Returns the index clicked.
pub fn seg(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, region: u8, group: &str, items: &[(&str, &str, bool, bool)]) -> Option<usize> {
    // items: (id, label, on, disabled)
    let fonts: Vec<_> = items
        .iter()
        .map(|(_, label, on, dis)| {
            let col = if *dis { t.faint } else if *on { t.ink } else { t.muted };
            ui.painter().layout_no_wrap((*label).to_string(), theme::sans_semi(14.0), col)
        })
        .collect();
    let widths: Vec<f32> = fonts.iter().map(|g| g.size().x + 28.0).collect();
    let total = widths.iter().sum::<f32>() + 6.0 + 2.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, TARGET), Sense::hover());
    ui.painter().rect_filled(outer, 10.0, t.hover);
    let mut x = outer.min.x + 3.0;
    let mut clicked = None;
    for (i, ((id, label, on, dis), g)) in items.iter().copied().zip(fonts).enumerate() {
        let w = widths[i];
        let slot = Rect::from_min_size(pos2(x, outer.min.y), vec2(w, TARGET));
        let sense = if dis { Sense::hover() } else { Sense::click() };
        let r = ui.interact(slot, ui.id().with((group, id)), sense);
        if on {
            let pill = slot.shrink2(vec2(0.0, 3.0));
            ui.painter().rect_filled(pill.translate(vec2(0.0, 1.0)), 8.0, Color32::from_black_alpha(if t.dark { 70 } else { 28 }));
            ui.painter().rect_filled(pill, 8.0, t.surface);
        }
        // (`.seg button` has no hover rule: an unpressed segment keeps the tray's colour under the pointer)
        ui.painter().galley(centered(slot, g.size()), g, t.ink);
        if ctl.reg(ui, &r, C::new(id, label, region).pressed(on).disabled(dis)) {
            clicked = Some(i);
        }
        x += w + 2.0;
    }
    clicked
}

/// Height of `.pill`: 6 + 6 padding around one 14px line at 1.4.
pub const PILL_H: f32 = 14.0 * 1.4 + 12.0;

/// A rounded status pill (not interactive): `.pill` in css/base.css, drawn with its top left at `min`.
pub fn pill_at(ui: &Ui, t: &Tokens, kind: crate::state::PillKind, text: &str, min: egui::Pos2) -> Rect {
    use crate::state::PillKind::*;
    let (bg, fg, outline) = match kind {
        Warn => (t.warn, t.ink, false),
        Ok => (t.ok_bg, t.ok, false),
        Bad => (t.bad_bg, t.bad, false),
        Busy => (t.hover, t.ink, false),
        Queued => (Color32::TRANSPARENT, t.muted, true),
    };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    let rect = Rect::from_min_size(min, vec2(g.size().x + 28.0, PILL_H));
    ui.painter().rect_filled(rect, 999.0, bg);
    if outline {
        ui.painter().rect_stroke(rect.shrink(0.5), 999.0, Stroke::new(1.0, t.line2));
    }
    ui.painter().galley(centered(rect, g.size()), g, fg);
    rect
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn the_spinner_stands_still_when_motion_is_reduced() {
        assert!(spinner_turn(0.45, false) > 0.0);
        assert_eq!(spinner_turn(0.45, true), 0.0);
        assert_eq!(spinner_turn(7.3, true), 0.0);
        let ctx = egui::Context::default();
        assert!(!reduced_motion(&ctx));
        set_reduced_motion(&ctx, true);
        assert!(reduced_motion(&ctx));
    }
}

