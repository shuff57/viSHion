//! Small design-system widgets drawn by hand (rounded segmented controls, icon buttons, pills). Each one
//! is at least 44 x 44 points and goes through `Ctl::reg`.
use crate::theme::{self, Tokens};
use crate::{Ctl, C};
use egui::{pos2, vec2, Color32, Rect, Response, Sense, Shape, Stroke, Ui};

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
}

pub fn paint_icon(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = Stroke::new(2.0, color);
    let p = ui.painter();
    match icon {
        Icon::Left => p.add(Shape::line(vec![pos2(c.x + 3.0, c.y - 7.0), pos2(c.x - 3.5, c.y), pos2(c.x + 3.0, c.y + 7.0)], s)),
        Icon::Right => p.add(Shape::line(vec![pos2(c.x - 3.0, c.y - 7.0), pos2(c.x + 3.5, c.y), pos2(c.x - 3.0, c.y + 7.0)], s)),
        Icon::Close => {
            p.line_segment([pos2(c.x - 6.0, c.y - 6.0), pos2(c.x + 6.0, c.y + 6.0)], s);
            p.line_segment([pos2(c.x + 6.0, c.y - 6.0), pos2(c.x - 6.0, c.y + 6.0)], s);
            return;
        }
        Icon::Boxes => {
            for dy in [-6.0, 0.0, 6.0] {
                p.line_segment([pos2(c.x - 8.0, c.y + dy), pos2(c.x + 8.0, c.y + dy)], s);
            }
            for x in [-10.0, 10.0] {
                p.add(Shape::dashed_line(&[pos2(c.x + x, c.y - 10.0), pos2(c.x + x, c.y + 10.0)], Stroke::new(1.5, color), 3.0, 3.0));
            }
            return;
        }
    };
}

/// A 44 x 44 icon-only button; `label` is its accessible name.
pub fn icon_button(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, c: C, icon: Icon, on: bool) -> bool {
    let disabled = c.disabled;
    let sense = if disabled { Sense::hover() } else { Sense::click() };
    let r = ui.allocate_response(vec2(TARGET, TARGET), sense);
    paint_look(ui, &r, t, Look::Ghost, on, disabled);
    paint_icon(ui, r.rect, icon, if disabled { t.faint } else { t.ink });
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
