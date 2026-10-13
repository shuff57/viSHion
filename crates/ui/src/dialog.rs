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
    ClearFixes,
    RemovePage(String),
    TranscribeAgain(String),
    ReviewAgain(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// `drafts`: text the person is typing in the settings boxes, by control id; they are saved when the
    /// dialog closes (as the JS inputs save on `change`).
    Settings { drafts: std::collections::BTreeMap<&'static str, String> },
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
    pub fn settings() -> Dialog {
        Dialog::Settings { drafts: Default::default() }
    }

    pub fn change_student() -> Dialog {
        Dialog::ChangeStudent { pick: None, new_student: false, name: String::new(), all: false, error: false }
    }
}

impl Dialog {
    /// The control that takes focus when the dialog opens (js modal: the first thing you can act on; the
    /// safe choice for a confirmation).
    pub fn first_focus(&self, s: &Session) -> String {
        match self {
            // the first control in reading order (the one for the theme in use), not the last: Tab goes on
            // through the rest and Shift+Tab does not leave the dialog at once
            Dialog::Settings { .. } => match s.settings.get("theme").and_then(|v| v.as_str()) {
                Some("light") => "theme-light".into(),
                Some("dark") => "theme-dark".into(),
                _ => "theme-auto".into(),
            },
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
    /// A setting changed in the open dialog (it stays open): key, value, what is said.
    Setting(&'static str, serde_json::Value, String),
    /// Ask before forgetting the remembered fixes.
    AskClearFixes,
}

/// Save what was typed in the settings boxes (the JS inputs save on `change`).
pub fn commit(s: &mut Session, d: &Dialog) {
    let Dialog::Settings { drafts } = d else { return };
    let mut changed = false;
    for (id, v) in drafts {
        let (key, val) = match *id {
            "st-ocr" => ("ocrUrl", serde_json::json!(v.trim())),
            "st-key" => ("apiKey", serde_json::json!(v.trim())),
            "st-model" => ("model", serde_json::json!(v.trim())),
            "st-timeout" => ("timeoutSec", serde_json::json!(v.trim().parse::<f64>().ok().filter(|n| *n > 0.0).map_or(180.0, |n| n.round().max(10.0)))),
            "st-words" => {
                let topic = s.a.meta.topic().to_string();
                let terms: Vec<String> = v.split([',', '\n', ';']).map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
                if terms == s.settings.terms(&topic) {
                    continue;
                }
                let mut topics = s.settings.get("topics").cloned().unwrap_or_else(|| serde_json::json!({}));
                if let Some(o) = topics.as_object_mut() {
                    let e = o.entry(topic).or_insert_with(|| serde_json::json!({ "description": "", "terms": [] }));
                    if let Some(m) = e.as_object_mut() {
                        m.insert("terms".into(), serde_json::json!(terms));
                    }
                }
                ("topics", topics)
            }
            _ => continue,
        };
        if s.settings.get(key) != Some(&val) {
            s.settings.set(key, val);
            changed = true;
        }
    }
    if changed {
        s.dirty.settings = true;
    }
}

fn heading(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, text: &str) {
    let r = ui.label(RichText::new(text).font(theme::serif_semi(22.0)).color(t.ink));
    ctl.heading("dialog-title", 2, text, DIALOG, r.rect.min);
}

/// Draw the dialog over a scrim. Returns what the user decided this frame.
pub fn show(ctx: &egui::Context, ctl: &mut Ctl, t: &Tokens, s: &Session, d: &mut Dialog) -> Option<Out> {
    let mut out = None;
    // only the student list scrolls; a stray scroll request here has no later scroll area to leak into
    ctl.scrolls[DIALOG as usize] = true;
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
                    Dialog::Settings { drafts } => out = settings(ctl, ui, t, s, drafts),
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

fn section(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &'static str, text: &str) {
    ui.add_space(6.0);
    let r = ui.label(RichText::new(text).font(theme::serif_semi(17.0)).color(t.ink));
    ctl.heading(id, 3, text, DIALOG, r.rect.min);
}

fn label(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &'static str, text: &str) {
    let r = ui.label(RichText::new(text).font(theme::sans_semi(14.0)).color(t.ink));
    ctl.text(id, text, DIALOG, r.rect.min);
}

fn hint(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &'static str, text: &str) {
    let r = ui.label(RichText::new(text).font(theme::sans(13.0)).color(t.muted));
    ctl.text(id, text, DIALOG, r.rect.min);
}

/// A row of choices where one is on (`aria-pressed`): returns the value picked.
fn choice(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, prefix: &str, cur: &str, items: &[(&str, &str)]) -> Option<String> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        for (val, text) in items {
            let on = cur == *val;
            let look = if on { Look::Primary } else { Look::Outline };
            if widgets::button(ctl, ui, t, C::new(&format!("{prefix}{val}"), text, DIALOG).pressed(on), text, look, 0.0) {
                out = Some(val.to_string());
            }
        }
    });
    out
}

fn check(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &str, text: &str, on: bool) -> bool {
    let look = if on { Look::Primary } else { Look::Outline };
    widgets::button(ctl, ui, t, C::new(id, text, DIALOG).pressed(on).role("checkbox"), text, look, 0.0)
}

fn text_box(ctl: &mut Ctl, ui: &mut Ui, id: &'static str, name: &str, hint: &str, cur: String, drafts: &mut std::collections::BTreeMap<&'static str, String>, secret: bool) {
    let buf = drafts.entry(id).or_insert(cur);
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), TARGET), Sense::hover());
    let edit = egui::TextEdit::singleline(buf).hint_text(hint).password(secret).font(theme::sans(14.0)).margin(vec2(12.0, 13.0)).desired_width(slot.width() - 24.0);
    let mut r = ui.put(slot, edit);
    r.rect = slot;
    if let Some(v) = ctl.reg_text(ui, &r, C::new(id, name, DIALOG), buf) {
        *buf = v;
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "off"
    } else {
        "on"
    }
}

fn settings(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, s: &Session, drafts: &mut std::collections::BTreeMap<&'static str, String>) -> Option<Out> {
    use serde_json::json;
    let mut out = None;
    heading(ctl, ui, t, "Settings");
    let get_s = |k: &str, d: &str| s.settings.get(k).and_then(|v| v.as_str()).unwrap_or(d).to_string();
    let get_b = |k: &str, d: bool| s.settings.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
    let max_h = (ui.ctx().screen_rect().height() - 190.0).clamp(150.0, 560.0);
    egui::ScrollArea::vertical().max_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            section(ctl, ui, t, "set-appearance", "Appearance");
            label(ctl, ui, t, "settings-theme-label", "Theme");
            let cur = get_s("theme", "auto");
            let cur = if cur == "light" || cur == "dark" { cur } else { "auto".to_string() };
            if let Some(v) = choice(ctl, ui, t, "theme-", &cur, &[("auto", "Match my device"), ("light", "Light"), ("dark", "Dark")]) {
                out = Some(Out::Theme(v));
            }

            section(ctl, ui, t, "set-model", "Model");
            hint(ctl, ui, t, "set-model-note", "One model reads the pages, finds the student\u{2019}s name and checks spelling.");
            label(ctl, ui, t, "st-ocr-label", "Server address");
            text_box(ctl, ui, "st-ocr", "Server address", "http://localhost:8188", get_s("ocrUrl", ""), drafts, false);
            label(ctl, ui, t, "st-key-label", "API key");
            text_box(ctl, ui, "st-key", "API key", "Not needed for a server on this computer", get_s("apiKey", ""), drafts, true);
            label(ctl, ui, t, "st-model-label", "Model name");
            text_box(ctl, ui, "st-model", "Model name", "Whatever the server has loaded", get_s("model", ""), drafts, false);
            label(ctl, ui, t, "st-timeout-label", "Give up on a page after (seconds)");
            let secs = s.settings.get("timeoutSec").and_then(|v| v.as_f64()).unwrap_or(180.0);
            text_box(ctl, ui, "st-timeout", "Give up on a page after (seconds)", "180", format!("{secs}"), drafts, false);
            let on = get_b("straighten", true);
            if check(ctl, ui, t, "st-straighten", "Straighten tilted pages", on) {
                out = Some(Out::Setting("straighten", json!(!on), format!("Straighten tilted pages: {}.", on_off(on))));
            }
            let on = get_b("demo", false);
            if check(ctl, ui, t, "st-demo", "Demo mode", on) {
                out = Some(Out::Setting("demo", json!(!on), format!("Demo mode: {}.", on_off(on))));
            }

            section(ctl, ui, t, "set-spelling", "Spelling check");
            label(ctl, ui, t, "st-jmode-label", "How to suggest");
            let cur = if get_s("judgeMode", "auto") == "heuristic" { "heuristic" } else { "auto" };
            if let Some(v) = choice(ctl, ui, t, "st-jmode-", cur, &[("auto", "Ask the model"), ("heuristic", "Built-in matching only")]) {
                let said = format!("How to suggest: {}.", if v == "auto" { "ask the model" } else { "built-in matching only" });
                out = Some(Out::Setting("judgeMode", json!(v), said));
            }
            let topic = s.a.meta.topic().to_string();
            let name = if topic.is_empty() { "Course words".to_string() } else { format!("Course words for {topic}") };
            label(ctl, ui, t, "st-words-label", &name);
            hint(ctl, ui, t, "st-words-hint", "Separate words with commas. Used to catch course words that get misread.");
            text_box(ctl, ui, "st-words", &name, "derivative, integral", s.settings.terms(&topic).join(", "), drafts, false);

            section(ctl, ui, t, "set-unreadable", "Unreadable words");
            let on = get_b("guessIllegible", true);
            if check(ctl, ui, t, "st-guess", "Suggest likely words", on) {
                out = Some(Out::Setting("guessIllegible", json!(!on), format!("Suggest likely words: {}.", on_off(on))));
            }
            let on = get_b("rememberFixes", true);
            if check(ctl, ui, t, "st-remember", "Remember my fixes for unreadable words", on) {
                out = Some(Out::Setting("rememberFixes", json!(!on), format!("Remember my fixes: {}.", on_off(on))));
            }
            let n = s.settings.get("reference").and_then(|v| v.as_array()).map_or(0, |a| a.len());
            hint(ctl, ui, t, "st-fixes-count", &format!("{} remembered.", crate::state::plural(n, "fix", "fixes")));
            if widgets::button(ctl, ui, t, C::new("st-clear-fixes", "Clear remembered fixes", DIALOG).disabled(n == 0), "Clear remembered fixes", Look::Outline, 0.0) {
                out = Some(Out::AskClearFixes);
            }

            section(ctl, ui, t, "set-export", "Export defaults");
            label(ctl, ui, t, "st-fmt-label", "Combined file");
            if let Some(v) = choice(ctl, ui, t, "st-fmt-", &get_s("exportFormat", "md"), &[("docx", "Word (.docx)"), ("md", "Markdown (.md)"), ("txt", "Plain text (.txt)")]) {
                out = Some(Out::Setting("exportFormat", json!(v), format!("Combined file: {v}.")));
            }
            label(ctl, ui, t, "st-sep-label", "Between pages");
            if let Some(v) = choice(ctl, ui, t, "st-sep-", &get_s("exportSeparator", "label"), &[("label", "A \u{201C}Page N\u{201D} line"), ("break", "A page break"), ("none", "Nothing")]) {
                out = Some(Out::Setting("exportSeparator", json!(v), format!("Between pages: {v}.")));
            }
            label(ctl, ui, t, "st-txt-label", "Text to export");
            if let Some(v) = choice(ctl, ui, t, "st-txt-", &get_s("exportText", "corrected"), &[("corrected", "Corrected"), ("raw", "Original"), ("both", "Both")]) {
                out = Some(Out::Setting("exportText", json!(v), format!("Text to export: {v}.")));
            }

            section(ctl, ui, t, "set-storage", "Storage");
            let on = get_b("keepOriginals", false);
            if check(ctl, ui, t, "st-orig", "Keep untouched originals", on) {
                out = Some(Out::Setting("keepOriginals", json!(!on), format!("Keep untouched originals: {}.", on_off(on))));
            }
            ctl.flush_scroll(ui, &[DIALOG]);
        });
    });
    ui.add_space(4.0);
    if widgets::button(ctl, ui, t, C::new("dialog-close", "Done", DIALOG), "Done", Look::Primary, 0.0) {
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
                if !*new_student {
                    // js: the text box takes focus; a person who cannot see it is told it appeared
                    ctl.want_focus = Some("cs-name".into());
                    ctl.say("A text box for the new student’s name is now shown. Type the name.");
                }
                *new_student = true;
                *error = false;
            }
            ctl.flush_scroll(ui, &[DIALOG]);
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
                // js focuses the select or the name box; here the refusal is said and focus goes to the same place
                ctl.say_alert("Choose a student, or type a name.");
                ctl.want_focus = Some(if *new_student { "cs-name".into() } else if names.is_empty() { "cs-new".into() } else { "cs-student-0".into() });
            } else {
                out = Some(Out::Student { name: chosen, all: *all });
            }
        }
    });
    out
}
