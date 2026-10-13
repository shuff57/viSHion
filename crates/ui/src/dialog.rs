//! Modal dialogs of the review screen: Change student, the confirmations (remove a page, transcribe again,
//! review again) and a small Settings dialog. While one is open it is the only thing the twin exposes and
//! Escape closes it. Everything goes through `Ctl` like the rest of the screen.
use crate::state::Session;
use crate::theme::{self, Tokens};
use crate::twin::DIALOG;
use crate::widgets::{self, Look, TARGET};
use crate::{Ctl, C};
use egui::{vec2, Align2, Margin, Order, RichText, Sense, Stroke, Ui};

/// What a confirmation does when accepted.
#[derive(Clone, Debug, PartialEq)]
pub enum After {
    RemovePage(String),
    TranscribeAgain(String),
    ReviewAgain(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    Settings,
    ChangeStudent {
        /// A name from the other students, or `None` while nothing is picked.
        pick: Option<String>,
        new_student: bool,
        name: String,
        all: bool,
        error: bool,
    },
    Confirm {
        title: String,
        text: String,
        ok: String,
        danger: bool,
        then: After,
    },
}

impl Dialog {
    pub fn change_student() -> Dialog {
        Dialog::ChangeStudent { pick: None, new_student: false, name: String::new(), all: false, error: false }
    }
}

impl Dialog {
    /// The control that takes focus when the dialog opens (js modal: the first thing you can act on; the
    /// safe choice for a confirmation).
    pub fn first_focus(&self, s: &Session) -> String {
        match self {
            Dialog::Settings => "dialog-close".into(),
            Dialog::Confirm { .. } => "dialog-cancel".into(),
            Dialog::ChangeStudent { .. } => {
                if s.other_students().is_empty() {
                    "cs-new".into()
                } else {
                    "cs-student-0".into()
                }
            }
        }
    }
}

/// What the dialog asks the screen to do.
#[derive(Debug, PartialEq)]
pub enum Out {
    Close,
    Student { name: String, all: bool },
    Confirmed(After),
    Theme(String),
}

fn heading(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, text: &str) {
    let r = ui.label(RichText::new(text).font(theme::serif_semi(22.0)).color(t.ink));
    ctl.heading("dialog-title", 2, text, DIALOG, r.rect.min);
}

/// Draw the dialog over a scrim. Returns what the user decided this frame.
pub fn show(ctx: &egui::Context, ctl: &mut Ctl, t: &Tokens, s: &Session, d: &mut Dialog) -> Option<Out> {
    let mut out = None;
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("scrim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(r, 0.0, if t.dark { egui::Color32::from_black_alpha(158) } else { egui::Color32::from_rgba_unmultiplied(20, 27, 45, 128) });
    });
    let width = (screen.width() - 32.0).clamp(280.0, 480.0);
    egui::Area::new(egui::Id::new("dialog")).order(Order::Foreground).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        egui::Frame::none()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.line))
            .rounding(12.0)
            .inner_margin(Margin::same(20.0))
            .show(ui, |ui| {
                ui.set_width(width - 40.0);
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                match d {
                    Dialog::Settings => out = settings(ctl, ui, t, s),
                    Dialog::ChangeStudent { pick, new_student, name, all, error } => {
                        out = change_student(ctl, ui, t, s, pick, new_student, name, all, error)
                    }
                    Dialog::Confirm { title, text, ok, danger, then } => {
                        heading(ctl, ui, t, title);
                        let r = ui.label(RichText::new(text.as_str()).font(theme::sans(14.0)).color(t.ink));
                        ctl.text("dialog-text", text, DIALOG, r.rect.min);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if widgets::button(ctl, ui, t, C::new("dialog-cancel", "Cancel", DIALOG), "Cancel", Look::Outline, 0.0) {
                                out = Some(Out::Close);
                            }
                            let look = Look::Primary;
                            if widgets::button(ctl, ui, t, C::new("dialog-ok", ok, DIALOG), ok, look, 0.0) {
                                out = Some(Out::Confirmed(then.clone()));
                            }
                            let _ = danger;
                        });
                    }
                }
            });
    });
    out
}

fn settings(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, s: &Session) -> Option<Out> {
    let mut out = None;
    heading(ctl, ui, t, "Settings");
    let r = ui.label(RichText::new("Theme").font(theme::sans_semi(14.0)).color(t.ink));
    ctl.text("settings-theme-label", "Theme", DIALOG, r.rect.min);
    let cur = s.settings.get("theme").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
    ui.horizontal_wrapped(|ui| {
        for (id, label, val) in [("theme-auto", "Match my device", "auto"), ("theme-light", "Light", "light"), ("theme-dark", "Dark", "dark")] {
            let on = cur == val || (val == "auto" && cur != "light" && cur != "dark");
            let look = if on { Look::Primary } else { Look::Outline };
            if widgets::button(ctl, ui, t, C::new(id, label, DIALOG).pressed(on), label, look, 0.0) {
                out = Some(Out::Theme(val.to_string()));
            }
        }
    });
    let r = ui.label(
        RichText::new("The model, the course words and the export options move into this dialog with phase 6.")
            .font(theme::sans(13.0))
            .color(t.muted),
    );
    ctl.text("settings-note", "The model, the course words and the export options move into this dialog with phase 6.", DIALOG, r.rect.min);
    ui.add_space(4.0);
    if widgets::button(ctl, ui, t, C::new("dialog-close", "Close", DIALOG), "Close", Look::Outline, 0.0) {
        out = Some(Out::Close);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn change_student(
    ctl: &mut Ctl,
    ui: &mut Ui,
    t: &Tokens,
    s: &Session,
    pick: &mut Option<String>,
    new_student: &mut bool,
    name: &mut String,
    all: &mut bool,
    error: &mut bool,
) -> Option<Out> {
    let mut out = None;
    heading(ctl, ui, t, "Change student");
    let r = ui.label(RichText::new("This page belongs to").font(theme::sans_semi(14.0)).color(t.ink));
    ctl.text("cs-label", "This page belongs to", DIALOG, r.rect.min);
    let names = s.other_students();
    egui::ScrollArea::vertical().max_height(220.0).auto_shrink([true, true]).show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            for (i, n) in names.iter().enumerate() {
                let on = pick.as_deref() == Some(n.as_str()) && !*new_student;
                let look = if on { Look::Primary } else { Look::Outline };
                if widgets::button(ctl, ui, t, C::new(&format!("cs-student-{i}"), n, DIALOG).pressed(on).role("listitem"), n, look, 0.0) {
                    *pick = Some(n.clone());
                    *new_student = false;
                    *error = false;
                }
            }
            let look = if *new_student { Look::Primary } else { Look::Outline };
            if widgets::button(ctl, ui, t, C::new("cs-new", "A new student\u{2026}", DIALOG).pressed(*new_student).role("listitem"),"A new student\u{2026}", look, 0.0) {
                *new_student = true;
                *error = false;
            }
        });
    });
    if *new_student {
        let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), TARGET), Sense::hover());
        let edit = egui::TextEdit::singleline(name).hint_text("Student name").font(theme::sans(14.0)).margin(vec2(12.0, 13.0)).desired_width(slot.width() - 24.0);
        let mut r = ui.put(slot, edit);
        r.rect = slot;
        if let Some(v) = ctl.reg_text(ui, &r, C::new("cs-name", "New student name", DIALOG), name) {
            *name = v;
        }
    }
    let pages = s.group().map(|g| g.pages.len()).unwrap_or(1);
    if pages > 1 {
        let who = s.group().map(|g| g.name).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            let label_all = format!("All {pages} pages for {who}");
            for (id, label, is_all) in [("cs-scope-page", "Only this page".to_string(), false), ("cs-scope-all", label_all, true)] {
                let on = *all == is_all;
                let look = if on { Look::Primary } else { Look::Outline };
                if widgets::button(ctl, ui, t, C::new(id, &label, DIALOG).pressed(on), &label, look, 0.0) {
                    *all = is_all;
                }
            }
        });
    }
    if *error {
        let m = "Choose a student, or type a name.";
        let r = ui.label(RichText::new(m).font(theme::sans(13.0)).color(t.bad));
        ctl.status("cs-error", m, DIALOG, r.rect.min);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if widgets::button(ctl, ui, t, C::new("dialog-cancel", "Cancel", DIALOG), "Cancel", Look::Outline, 0.0) {
            out = Some(Out::Close);
        }
        if widgets::button(ctl, ui, t, C::new("dialog-ok", "Save", DIALOG), "Save", Look::Primary, 0.0) {
            let chosen = if *new_student { name.trim().to_string() } else { pick.clone().unwrap_or_default() };
            if chosen.trim().is_empty() {
                *error = true;
            } else {
                out = Some(Out::Student { name: chosen, all: *all });
            }
        }
    });
    out
}
