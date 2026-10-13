//! Small design-system widgets drawn by hand (rounded segmented controls, icon buttons, pills). Each one
//! is at least 44 x 44 points and goes through `Ctl::reg`.
use crate::theme::{self, Tokens};
use crate::{Ctl, C};
use egui::{pos2, vec2, Color32, FontFamily, FontId, Rect, Response, Sense, Shape, Stroke, Ui};

pub const TARGET: f32 = 44.0;

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
    let fg = match look {
        Look::Primary => t.on_ink,
        _ => if c.disabled { t.faint } else { t.ink },
    };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    let w = (g.size().x + 36.0).max(min_w);
    let sense = if c.disabled { Sense::hover() } else { Sense::click() };
    let r = ui.allocate_response(vec2(w, TARGET), sense);
    paint_look(ui, &r, t, look, false, c.disabled);
    let pos = r.rect.center() - g.size() / 2.0;
    ui.painter().galley(pos, g, fg);
    let disabled = c.disabled;
    let on = ctl.reg(ui, &r, c);
    on && !disabled
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

/// Line icons in the style of js/ui/icons.js (2px strokes, round caps). `size` is the glyph box, 20 to 40.
pub fn paint_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let k = rect.width().min(rect.height()) / 24.0; // 24-unit design grid
    let s = Stroke::new((1.8 * k).max(1.5), color);
    let p = ui.painter();
    let at = |x: f32, y: f32| pos2(c.x + x * k, c.y + y * k);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|(x, y)| at(*x, *y)).collect(), s));
    };
    match icon {
        Icon::Left => line(&[(3.0, -7.0), (-3.5, 0.0), (3.0, 7.0)]),
        Icon::Right => line(&[(-3.0, -7.0), (3.5, 0.0), (-3.0, 7.0)]),
        Icon::Close => {
            line(&[(-6.0, -6.0), (6.0, 6.0)]);
            line(&[(6.0, -6.0), (-6.0, 6.0)]);
        }
        Icon::Boxes => {
            for dy in [-6.0, 0.0, 6.0] {
                line(&[(-8.0, dy), (8.0, dy)]);
            }
            for x in [-10.0f32, 10.0] {
                p.add(Shape::dashed_line(&[at(x, -10.0), at(x, 10.0)], Stroke::new(1.5 * k, color), 3.0 * k, 3.0 * k));
            }
        }
        Icon::Gear => {
            p.circle_stroke(c, 3.0 * k, s);
            p.circle_stroke(c, 7.5 * k, s);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                p.line_segment([at(cs * 7.5, sn * 7.5), at(cs * 10.0, sn * 10.0)], Stroke::new(2.6 * k, color));
            }
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            p.circle_stroke(at(-2.0, -2.0), 7.0 * k, s);
            line(&[(3.0, 3.0), (9.0, 9.0)]);
            line(&[(-5.5, -2.0), (1.5, -2.0)]);
            if icon == Icon::ZoomIn {
                line(&[(-2.0, -5.5), (-2.0, 1.5)]);
            }
        }
        Icon::Rotate => {
            // three quarters of a circle with an arrow head
            let pts: Vec<(f32, f32)> = (0..=18)
                .map(|i| {
                    let a = -0.3 + i as f32 * (std::f32::consts::PI * 1.5) / 18.0;
                    (a.cos() * 7.5, a.sin() * 7.5)
                })
                .collect();
            line(&pts);
            let (ex, ey) = pts[pts.len() - 1];
            line(&[(ex - 1.0, ey - 4.5), (ex, ey), (ex + 4.5, ey - 1.5)]);
        }
        Icon::Level => {
            line(&[(-9.0, 4.0), (9.0, -4.0)]);
            p.circle_stroke(at(0.0, 5.5), 2.2 * k, s);
            line(&[(-9.0, 9.0), (9.0, 9.0)]);
        }
        Icon::Swap => {
            line(&[(-8.0, -4.0), (8.0, -4.0)]);
            line(&[(4.0, -8.0), (8.0, -4.0), (4.0, 0.0)]);
            line(&[(8.0, 5.0), (-8.0, 5.0)]);
            line(&[(-4.0, 1.0), (-8.0, 5.0), (-4.0, 9.0)]);
        }
        Icon::Trash => {
            line(&[(-8.0, -5.0), (8.0, -5.0)]);
            line(&[(-3.0, -5.0), (-3.0, -8.0), (3.0, -8.0), (3.0, -5.0)]);
            line(&[(-6.0, -5.0), (-5.0, 9.0), (5.0, 9.0), (6.0, -5.0)]);
            line(&[(-2.0, -1.0), (-2.0, 5.0)]);
            line(&[(2.0, -1.0), (2.0, 5.0)]);
        }
        Icon::Plus => {
            line(&[(-7.0, 0.0), (7.0, 0.0)]);
            line(&[(0.0, -7.0), (0.0, 7.0)]);
        }
        Icon::Page => {
            line(&[(-7.0, -10.0), (2.0, -10.0), (7.0, -5.0), (7.0, 10.0), (-7.0, 10.0), (-7.0, -10.0)]);
            line(&[(2.0, -10.0), (2.0, -5.0), (7.0, -5.0)]);
            line(&[(-3.0, 1.0), (3.0, 1.0)]);
            line(&[(-3.0, 5.0), (3.0, 5.0)]);
        }
        Icon::Warn => {
            line(&[(0.0, -9.0), (9.0, 7.0), (-9.0, 7.0), (0.0, -9.0)]);
            line(&[(0.0, -3.0), (0.0, 2.0)]);
            line(&[(0.0, 5.0), (0.0, 5.2)]);
        }
        Icon::Check => line(&[(-6.0, 0.5), (-2.0, 5.0), (7.0, -5.0)]),
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

/// The wordmark: "vi" "SH" "ion", a button that returns to the assignment.
pub fn brand(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C) -> bool {
    let font = FontId::new(17.0, FontFamily::Monospace);
    let parts = [("vi", t.ink), ("SH", t.accent), ("ion", t.ink)];
    let gs: Vec<_> = parts.iter().map(|(s, col)| ui.painter().layout_no_wrap((*s).to_string(), font.clone(), *col)).collect();
    let w: f32 = gs.iter().map(|g| g.size().x).sum::<f32>() + 16.0;
    let r = ui.allocate_response(vec2(w, TARGET), Sense::click());
    let mut x = r.rect.left() + 8.0;
    for (g, (_, col)) in gs.into_iter().zip(parts) {
        let sz = g.size();
        ui.painter().galley(pos2(x, r.rect.center().y - sz.y / 2.0), g, col);
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
        ui.painter().galley(slot.center() - g.size() / 2.0, g, t.ink);
        if ctl.reg(ui, &r, C::new(id, label, region).pressed(on).disabled(dis)) {
            clicked = Some(i);
        }
        x += w + 2.0;
    }
    clicked
}

/// A rounded status pill (not interactive).
pub fn pill(ui: &mut Ui, t: &Tokens, kind: crate::state::Kind, text: &str) -> Rect {
    use crate::state::Kind::*;
    let (bg, fg) = match kind {
        Warn => (t.warn, t.ink),
        Ok => (t.ok_bg, t.ok),
        Bad => (t.bad_bg, t.bad),
    };
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(14.0), fg);
    let (rect, _) = ui.allocate_exact_size(g.size() + vec2(28.0, 12.0), Sense::hover());
    ui.painter().rect_filled(rect, 999.0, bg);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
    rect
}
