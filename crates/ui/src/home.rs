//! The Home screen of js/ui/home.js: where work is saved and the list of assignments. It works without a
//! session (the app starts here when nothing is stored) and is drawn over one when the person goes back.
use crate::host::Card;
use crate::theme::{self, Tokens};
use crate::twin::TEXT;
use crate::widgets::{self, Look, TARGET};
use crate::{Ctl, C};
use egui::{pos2, vec2, Margin, RichText, Sense, Stroke, Ui};

#[derive(Default)]
pub struct Home {
    /// The home screen is the one shown (a session may sit behind it).
    pub active: bool,
    /// `fs.listProjects`; `None` until the host has answered.
    pub cards: Option<Vec<Card>>,
    /// The list was asked for and not answered yet.
    pub asked: bool,
    /// The new-assignment box is open, with what is typed in it.
    pub creating: Option<String>,
    /// (slug, new name) while a card is being renamed.
    pub renaming: Option<(String, String)>,
    /// The slug a delete is waiting to be confirmed for.
    pub deleting: Option<String>,
    /// Where work is saved (web); `None` on native, which shows the storage line only.
    pub folder: Option<crate::host::FolderInfo>,
    /// Old single-file sessions can be imported.
    pub legacy: bool,
}

pub(crate) enum HomeAct {
    Open(String),
    Create(String),
    Rename(String, String),
    Duplicate(String),
    Delete(String),
    ImportZip,
    ImportFolder,
    ImportLegacy,
    DownloadZip(String),
    Folder(&'static str),
}

/// `OCR.fmtAgo` for a time in ms; older than a day is the date.
pub fn ago(now_ms: i64, then_ms: f64) -> String {
    if then_ms <= 0.0 {
        return String::new();
    }
    let s = (now_ms as f64 - then_ms) / 1000.0;
    if s < 60.0 {
        "just now".into()
    } else if s < 3600.0 {
        format!("{} min ago", (s / 60.0).floor() as i64)
    } else if s < 86400.0 {
        format!("{} h ago", (s / 3600.0).floor() as i64)
    } else {
        // (the JS shows "Oct 14" in the viewer's locale; a stable form is enough here)
        let days = (then_ms / 86_400_000.0).floor() as i64;
        let (_, m, d) = civil(days);
        const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        format!("{} {}", MON[(m - 1) as usize], d)
    }
}

/// `new Date(ms).toISOString()` (UTC, milliseconds).
pub fn iso(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

/// Days since 1970-01-01 to (year, month, day) (Hinnant's civil_from_days).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn text_box(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &str, name: &str, hint: &str, buf: &mut String, w: f32) -> bool {
    let (slot, _) = ui.allocate_exact_size(vec2(w, TARGET), Sense::hover());
    let edit = egui::TextEdit::singleline(buf).hint_text(RichText::new(hint).font(theme::sans(14.0)).color(t.placeholder())).font(theme::sans(14.0)).margin(vec2(12.0, 13.0)).desired_width(slot.width() - 24.0);
    let mut r = ui.put(slot, edit);
    r.rect = slot;
    if let Some(v) = ctl.reg_text(ui, &r, C::new(id, name, TEXT), buf) {
        *buf = v;
    }
    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

pub(crate) fn show(ctx: &egui::Context, ctl: &mut Ctl, t: &Tokens, h: &mut Home, storage: &str, narrow: bool, now: i64) -> Vec<HomeAct> {
    let mut acts = vec![];
    egui::CentralPanel::default().frame(egui::Frame::none().fill(t.bg)).show(ctx, |ui| {
        ctl.scrolls[TEXT as usize] = true;
        egui::ScrollArea::vertical().auto_shrink([false, false]).animated(false).id_source("home-scroll").show(ui, |ui| {
            let avail = ui.available_width();
            let pad = if narrow { 16.0 } else { 24.0 };
            let side = (avail - avail.min(1120.0)) / 2.0 + pad;
            let margin = Margin { left: side, right: side, top: if narrow { 28.0 } else { 40.0 }, bottom: if narrow { 48.0 } else { 56.0 } };
            egui::Frame::none().inner_margin(margin).show(ui, |ui| {
                ui.set_width(ui.available_width());
                body(ui, ctl, t, h, storage, narrow, now, &mut acts);
            });
            ctl.flush_scroll(ui, &[crate::twin::HEADER, TEXT]);
        });
    });
    acts
}

#[allow(clippy::too_many_arguments)]
fn body(ui: &mut Ui, ctl: &mut Ctl, t: &Tokens, h: &mut Home, storage: &str, narrow: bool, now: i64, acts: &mut Vec<HomeAct>) {
    let w = ui.available_width();
    let size = if narrow { 30.0 } else { 36.0 };
    let r = ui.label(RichText::new("viSHion").font(theme::serif_semi(size)).color(t.ink));
    ctl.heading("home-title", 1, "viSHion", TEXT, r.rect);
    ui.add_space(8.0);
    let sub = "Turn stacks of handwritten pages into typed text.";
    let r = ui.label(RichText::new(sub).font(theme::sans(14.0)).color(t.muted));
    ctl.text("home-sub", sub, TEXT, r.rect);
    ui.add_space(20.0);

    // `.save-card`: where the work lives. In the browser it says which of the four states it is in.
    if let Some(f) = h.folder.clone() {
        let (title, text, buttons): (String, String, Vec<(&'static str, &'static str, &'static str, Look)>) = match (f.mode.as_str(), f.permission.as_str()) {
            ("none", _) => (
                "Choose where assignments are saved".into(),
                "Pick a folder on this computer. Each assignment gets its own subfolder with pages, images and exports.".into(),
                vec![("choose", "Choose a folder", "choose-folder", Look::Primary), ("browser", "Use browser storage instead", "use-browser", Look::Outline)],
            ),
            ("folder", "prompt") => (
                format!("Reconnect \u{201C}{}\u{201D}", f.name),
                "The browser needs your permission again before it can open this folder. Nothing was lost.".into(),
                vec![("reconnect", "Reconnect folder", "reconnect-folder", Look::Primary), ("choose", "Choose a different folder", "choose-folder", Look::Outline)],
            ),
            ("idb", _) => (
                "Saving in this browser".into(),
                if f.supported {
                    "Work lives in this browser only. Use Export to keep a copy, or choose a folder to save to disk.".into()
                } else {
                    "This browser cannot pick folders, so work lives in this browser only. Use Export to keep a copy. Chrome and Edge can save to a folder.".into()
                },
                if f.supported { vec![("choose", "Choose a folder instead", "choose-folder", Look::Outline)] } else { vec![] },
            ),
            _ => (
                format!("Saving to {}", f.name),
                "Changes are saved automatically a moment after you make them.".into(),
                vec![("choose", "Change folder", "choose-folder", Look::Outline)],
            ),
        };
        egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, if f.permission == "prompt" { t.warn_line } else { t.line })).rounding(10.0).inner_margin(Margin::symmetric(20.0, 14.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let r = ui.label(RichText::new(&title).font(theme::sans_semi(14.0)).color(t.ink));
            ctl.heading("home-storage-title", 2, &title, TEXT, r.rect);
            let r = ui.label(RichText::new(&text).font(theme::sans(13.0)).color(t.muted));
            ctl.text("home-storage-text", &text, TEXT, r.rect);
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (what, label, id, look) in &buttons {
                    if widgets::button(ctl, ui, t, C::new(id, label, TEXT), label, *look, 0.0) {
                        acts.push(HomeAct::Folder(what));
                    }
                }
            });
        });
        ui.add_space(28.0);
    } else {
    let line = if storage.is_empty() { "Changes are saved automatically a moment after you make them.".to_string() } else { storage.to_string() };
    egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::symmetric(20.0, 14.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let r = ui.label(RichText::new(&line).font(theme::sans_semi(14.0)).color(t.ink));
        ctl.status("home-storage", &line, TEXT, r.rect);
    });
    ui.add_space(28.0);
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let r = ui.label(RichText::new("Assignments").font(theme::serif_semi(24.0)).color(t.ink));
        ctl.heading("home-assignments", 2, "Assignments", TEXT, r.rect);
        if h.legacy && widgets::button(ctl, ui, t, C::new("import-legacy", "Import old sessions", TEXT), "Import old sessions", Look::Outline, 0.0) {
            acts.push(HomeAct::ImportLegacy);
        }
        if widgets::button(ctl, ui, t, C::new("import-zip", "Import zip", TEXT), "Import zip", Look::Outline, 0.0) {
            acts.push(HomeAct::ImportZip);
        }
        if widgets::button(ctl, ui, t, C::new("import-folder", "Import folder", TEXT), "Import folder", Look::Outline, 0.0) {
            acts.push(HomeAct::ImportFolder);
        }
        if h.creating.is_none() && widgets::button(ctl, ui, t, C::new("new-assignment", "New assignment", TEXT), "New assignment", Look::Primary, 0.0) {
            h.creating = Some(String::new());
        }
    });
    ui.add_space(12.0);

    if let Some(name) = h.creating.as_mut() {
        let mut go = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let bw = 160.0;
            let fw = (ui.available_width() - bw - 8.0 - 100.0 - 8.0).clamp(160.0, 420.0);
            let entered = text_box(ctl, ui, t, "new-name", "Assignment name", "Assignment name", name, fw);
            let ok = widgets::button(ctl, ui, t, C::new("create-assignment", "Create assignment", TEXT), "Create assignment", Look::Primary, bw);
            cancel = widgets::button(ctl, ui, t, C::new("cancel-new", "Cancel", TEXT), "Cancel", Look::Outline, 0.0);
            go = ok || entered;
        });
        if go && !name.trim().is_empty() {
            acts.push(HomeAct::Create(name.trim().to_string()));
            h.creating = None;
        } else if cancel {
            h.creating = None;
        }
        ui.add_space(12.0);
    }

    let Some(cards) = h.cards.clone() else {
        let m = "Looking for your assignments.";
        let r = ui.label(RichText::new(m).font(theme::sans(14.0)).color(t.muted));
        ctl.status("home-loading", m, TEXT, r.rect);
        return;
    };
    if cards.is_empty() {
        egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::symmetric(24.0, 40.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                widgets::paint_icon(ui, ir, crate::widgets::Icon::Page, t.muted);
                ui.add_space(10.0);
                let r = ui.label(RichText::new("No assignments yet").font(theme::sans_semi(18.0)).color(t.ink));
                ctl.heading("home-empty", 3, "No assignments yet", TEXT, r.rect);
                let m = "An assignment holds one stack of student pages. Create one, then drop in your scans.";
                let r = ui.label(RichText::new(m).font(theme::sans(14.0)).color(t.muted));
                ctl.text("home-empty-note", m, TEXT, r.rect);
            });
        });
        return;
    }
    let _ = w;
    for (i, c) in cards.iter().enumerate() {
        card(ui, ctl, t, h, c, i, now, acts);
        ui.add_space(12.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn card(ui: &mut Ui, ctl: &mut Ctl, t: &Tokens, h: &mut Home, c: &Card, i: usize, now: i64, acts: &mut Vec<HomeAct>) {
    egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::symmetric(20.0, 16.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let id = |k: &str| format!("{k}-{i}");
        let meta = format!("{}, {} transcribed", crate::state::plural(c.pages, "page", "pages"), c.done);
        let when = format!("Updated {}", ago(now, c.updated));
        if h.renaming.as_ref().is_some_and(|(s, _)| *s == c.slug) {
            let (mut go, mut cancel) = (false, false);
            let (slug, name) = h.renaming.as_mut().map(|(s, n)| (s.clone(), n)).unwrap();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let entered = text_box(ctl, ui, t, &id("rename-name"), "Assignment name", "Assignment name", name, 320.0);
                let ok = widgets::button(ctl, ui, t, C::new(&id("rename-ok"), "Rename", TEXT), "Rename", Look::Primary, 0.0);
                cancel = widgets::button(ctl, ui, t, C::new(&id("rename-cancel"), "Cancel", TEXT), "Cancel", Look::Outline, 0.0);
                go = ok || entered;
            });
            if go && !name.trim().is_empty() {
                acts.push(HomeAct::Rename(slug, name.trim().to_string()));
                h.renaming = None;
            } else if cancel {
                h.renaming = None;
            }
            return;
        }
        // the whole name is the way in, as the card's button is in the JS
        let label = format!("{}. {}. {}", c.name, meta, when);
        let r = ui.allocate_response(vec2(ui.available_width(), TARGET), Sense::click());
        let col = if r.hovered() { t.accent } else { t.ink };
        ui.painter().text(pos2(r.rect.left(), r.rect.center().y), egui::Align2::LEFT_CENTER, &c.name, theme::sans_semi(18.0), col);
        if ctl.reg(ui, &r, C::new(&id("open"), &label, TEXT)) {
            acts.push(HomeAct::Open(c.slug.clone()));
        }
        let r = ui.label(RichText::new(format!("{meta}. {when}")).font(theme::sans(13.0)).color(t.muted));
        ctl.text(&id("meta"), &format!("{meta}. {when}"), TEXT, r.rect);
        // the bar of how many pages have a transcript
        ui.add_space(8.0);
        let pct = if c.pages > 0 { c.done as f32 / c.pages as f32 } else { 0.0 };
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        ui.painter().rect_filled(bar, 3.0, t.track);
        ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * pct.clamp(0.0, 1.0), bar.height())), 3.0, t.bar_ok);
        ui.add_space(10.0);

        if h.deleting.as_deref() == Some(c.slug.as_str()) {
            let m = format!("Delete \u{201C}{}\u{201D} and all of its pages? This cannot be undone.", c.name);
            let r = ui.label(RichText::new(&m).font(theme::sans(14.0)).color(t.bad));
            ctl.status(&id("delete-ask"), &m, TEXT, r.rect);
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ok = widgets::button(ctl, ui, t, C::new(&id("delete-ok"), "Delete assignment", TEXT), "Delete assignment", Look::Primary, 0.0);
                let no = widgets::button(ctl, ui, t, C::new(&id("delete-cancel"), "Cancel", TEXT), "Cancel", Look::Outline, 0.0);
                if ok {
                    acts.push(HomeAct::Delete(c.slug.clone()));
                    h.deleting = None;
                } else if no {
                    h.deleting = None;
                }
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let b = |ctl: &mut Ctl, ui: &mut Ui, k: &str, text: &str, said: &str| {
                let name = format!("{said} {}", c.name);
                widgets::button(ctl, ui, t, C::new(&id(k), &name, TEXT), text, Look::Ghost, 0.0)
            };
            if b(ctl, ui, "rename", "Rename", "Rename") {
                h.renaming = Some((c.slug.clone(), c.name.clone()));
            }
            if b(ctl, ui, "duplicate", "Duplicate", "Duplicate") {
                acts.push(HomeAct::Duplicate(c.slug.clone()));
            }
            if b(ctl, ui, "zip", "Download zip", "Download zip of") {
                acts.push(HomeAct::DownloadZip(c.slug.clone()));
            }
            if b(ctl, ui, "delete", "Delete", "Delete") {
                h.deleting = Some(c.slug.clone());
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_follows_fmt_ago() {
        let now = 1_700_000_000_000i64;
        assert_eq!(ago(now, now as f64 - 5_000.0), "just now");
        assert_eq!(ago(now, now as f64 - 5.0 * 60_000.0), "5 min ago");
        assert_eq!(ago(now, now as f64 - 3.0 * 3_600_000.0), "3 h ago");
        assert_eq!(ago(now, 1_696_000_000_000.0), "Sep 29");
        assert_eq!(ago(now, 0.0), "");
        assert_eq!(iso(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
    }
}
