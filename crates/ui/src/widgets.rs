//! Small design-system widgets drawn by hand (rounded segmented controls, icon buttons, pills). Each one
//! is at least 44 x 44 points and goes through `Ctl::reg`.
use crate::theme::{self, Tokens};
use crate::{Ctl, C};
use egui::{pos2, vec2, Color32, Rect, Response, Sense, Shape, Stroke, Ui};

pub const TARGET: f32 = 44.0;

/// Where a text of `size` goes to be centred in `rect`, on a whole pixel. A 14px label centred in a 44 high
/// control lands exactly on a half pixel (22 - 17 / 2), where floating point noise in the layout above decides
/// between two rows of pixels from one frame to the next; this decides it the same way every time.
pub fn centered(rect: Rect, size: egui::Vec2) -> egui::Pos2 {
    let p = rect.center() - size / 2.0;
    pos2((p.x + 0.02).round(), (p.y + 0.02).round())
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

fn paint_look(ui: &Ui, r: &Response, t: &Tokens, look: Look, on: bool, disabled: bool) {
    let p = ui.painter();
    let hover = r.hovered() && !disabled;
    match look {
        Look::Primary => {
            let fill = if disabled { t.line2 } else if hover { t.ink.gamma_multiply(0.88) } else { t.ink };
            p.rect_filled(r.rect, 8.0, fill);
        }
        Look::Outline => {
            p.rect_filled(r.rect, 8.0, if hover { t.sunken } else { t.surface });
            p.rect_stroke(r.rect, 8.0, Stroke::new(1.0, t.line2));
        }
        Look::Ghost => {
            if hover || on {
                p.rect_filled(r.rect, 8.0, t.hover);
            }
        }
    }
}

/// A text button. `min_w` keeps related buttons aligned; the height is always 44.
pub fn button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, text: &str, look: Look, min_w: f32) -> bool {
    button_pad(ctl, ui, t, c, text, look, min_w, 36.0)
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
    let sense = if c.disabled { Sense::hover() } else { Sense::click() };
    let r = ui.allocate_response(vec2(w, TARGET), sense);
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
    let rect = Rect::from_min_size(at, vec2(g.size().x + 36.0, TARGET));
    let sense = if c.disabled { Sense::hover() } else { Sense::click() };
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

/// A text button with a leading icon ("Add page").
pub fn icon_text_button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, icon: Icon, text: &str, look: Look) -> bool {
    let fg = if c.disabled { t.faint } else { t.ink };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    let w = g.size().x + 20.0 + 8.0 + 28.0;
    let sense = if c.disabled { Sense::hover() } else { Sense::click() };
    let r = ui.allocate_response(vec2(w, TARGET), sense);
    paint_look(ui, &r, t, look, false, c.disabled);
    paint_icon(ui, Rect::from_center_size(pos2(r.rect.left() + 14.0 + 10.0, r.rect.center().y), vec2(20.0, 20.0)), icon, fg);
    ui.painter().galley(pos2(r.rect.left() + 14.0 + 28.0, r.rect.center().y - g.size().y / 2.0), g, fg);
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
    let w = (g.size().x + 20.0 + 6.0 + 24.0).max(TARGET);
    let r = ui.allocate_response(vec2(w, TARGET), Sense::click());
    let col = if r.hovered() { t.accent } else { t.ink };
    paint_icon(ui, Rect::from_center_size(pos2(r.rect.left() + 12.0 + 10.0, r.rect.center().y), vec2(20.0, 20.0)), Icon::Left, col);
    ui.painter().galley(pos2(r.rect.left() + 12.0 + 26.0, r.rect.center().y - g.size().y / 2.0), g, col);
    ctl.reg(ui, &r, c)
}

/// The wordmark at rest (header.js `logo`): "vi" "SH" "ion" in 600 weight monospace, the SH in the accent,
/// in an svg 32px high (viewBox 312 x 84), so 119px wide with the text stretched to 114px.
pub const BRAND_W: f32 = 119.0;

pub fn brand(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C) -> bool {
    let font = theme::mono_bold(26.5);
    let parts = [("vi", t.ink), ("SH", t.accent), ("ion", t.ink)];
    let r = ui.allocate_response(vec2(BRAND_W, TARGET), Sense::click());
    let hover = if r.hovered() { Some(t.accent) } else { None };
    let gs: Vec<_> = parts.iter().map(|(s, col)| ui.painter().layout_no_wrap((*s).to_string(), font.clone(), hover.unwrap_or(*col))).collect();
    let total: f32 = gs.iter().map(|g| g.size().x).sum();
    let mut x = r.rect.left() + 2.0 + (114.0 - total) / 2.0;
    for g in gs {
        let sz = g.size();
        ui.painter().galley(pos2(x, r.rect.center().y - sz.y / 2.0 - 1.0), g, Color32::PLACEHOLDER);
        x += sz.x;
    }
    ctl.reg(ui, &r, c)
}

/// A 44 x 44 icon-only button; `label` is its accessible name.
pub fn icon_button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, icon: Icon, on: bool) -> bool {
    let disabled = c.disabled;
    let sense = if disabled { Sense::hover() } else { Sense::click() };
    let r = ui.allocate_response(vec2(TARGET, TARGET), sense);
    paint_look(ui, &r, t, Look::Ghost, on, disabled);
    paint_icon(ui, Rect::from_center_size(r.rect.center(), vec2(22.0, 22.0)), icon, if disabled { t.faint } else { t.ink });
    ctl.reg(ui, &r, c) && !disabled
}

/// Two-option segmented control in a rounded tray. Returns the index clicked.
pub fn seg(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, region: u8, group: &str, items: [(&str, &str, bool, bool); 2]) -> Option<usize> {
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
    for (i, ((id, label, on, dis), g)) in items.into_iter().zip(fonts).enumerate() {
        let w = widths[i];
        let slot = Rect::from_min_size(pos2(x, outer.min.y), vec2(w, TARGET));
        let sense = if dis { Sense::hover() } else { Sense::click() };
        let r = ui.interact(slot, ui.id().with((group, id)), sense);
        if on {
            let pill = slot.shrink2(vec2(0.0, 3.0));
            ui.painter().rect_filled(pill.translate(vec2(0.0, 1.0)), 8.0, Color32::from_black_alpha(if t.dark { 70 } else { 28 }));
            ui.painter().rect_filled(pill, 8.0, t.surface);
        } else if r.hovered() && !dis {
            ui.painter().rect_filled(slot.shrink2(vec2(0.0, 3.0)), 8.0, t.line);
        }
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
