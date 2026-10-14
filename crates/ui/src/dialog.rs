//! Modal dialogs of the review screen: Change student, the confirmations (remove a page, transcribe again,
//! review again) and a small Settings dialog. While one is open it is the only thing the twin exposes and
//! Escape closes it. Everything goes through `Ctl` like the rest of the screen.
use crate::state::Session;
use crate::theme::{self, Tokens};
use crate::twin::DIALOG;
use crate::widgets::{self, Look, TARGET};
use crate::{Ctl, C};
use egui::{pos2, vec2, Align2, Margin, Order, RichText, Sense, Stroke, Ui};

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
        /// The list of students under the select is shown (the `<select>` popup).
        open: bool,
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
        Dialog::ChangeStudent { pick: None, new_student: false, name: String::new(), all: false, error: false, open: false }
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
                // js: the first thing in the body, the select
                "cs-select".into()
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
    ctl.heading("dialog-title", 2, text, DIALOG, r.rect);
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
    // `.modal.narrow { width: 440px }` for Change student; the others keep their width
    let narrow = matches!(d, Dialog::ChangeStudent { .. });
    let width = (screen.width() - 32.0).clamp(280.0, if narrow { 440.0 } else { 480.0 });
    let margin = if narrow { 0.0 } else { 20.0 };
    // `.backdrop { align-items: flex-start; padding: 48px 16px }`
    let anchor = if narrow { (Align2::CENTER_TOP, vec2(0.0, 48.0)) } else { (Align2::CENTER_CENTER, vec2(0.0, 0.0)) };
    egui::Area::new(egui::Id::new("dialog")).order(Order::Foreground).anchor(anchor.0, anchor.1).show(ctx, |ui| {
        let shadow = egui::epaint::Shadow { offset: vec2(0.0, 24.0), blur: 64.0, spread: -8.0, color: egui::Color32::from_rgba_unmultiplied(5, 7, 11, 128) };
        let frame = egui::Frame::none()
            .fill(t.surface)
            .stroke(if narrow { Stroke::NONE } else { Stroke::new(1.0, t.line) })
            .rounding(12.0)
            .shadow(if narrow { shadow } else { egui::epaint::Shadow::NONE })
            .inner_margin(Margin::same(margin));
        frame
            .show(ui, |ui| {
                ui.set_width(width - 2.0 * margin);
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                match d {
                    Dialog::Settings { drafts } => out = settings(ctl, ui, t, s, drafts),
                    Dialog::ChangeStudent { pick, new_student, name, all, error, open } => {
                        out = change_student(ctl, ui, t, s, pick, new_student, name, all, error, open)
                    }
                    Dialog::Confirm { title, text, ok, danger, then } => {
                        heading(ctl, ui, t, title);
                        let r = ui.label(RichText::new(text.as_str()).font(theme::sans(14.0)).color(t.ink));
                        ctl.text("dialog-text", text, DIALOG, r.rect);
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
    ctl.heading(id, 3, text, DIALOG, r.rect);
}

fn label(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &'static str, text: &str) {
    let r = ui.label(RichText::new(text).font(theme::sans_semi(14.0)).color(t.ink));
    ctl.text(id, text, DIALOG, r.rect);
}

fn hint(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &'static str, text: &str) {
    let r = ui.label(RichText::new(text).font(theme::sans(13.0)).color(t.muted));
    ctl.text(id, text, DIALOG, r.rect);
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

const FMTS: &[(&str, &str)] = &[("docx", "Word (.docx)"), ("md", "Markdown (.md)"), ("txt", "Plain text (.txt)")];
const SEPS: &[(&str, &str)] = &[("label", "A \u{201C}Page N\u{201D} line"), ("break", "A page break"), ("none", "Nothing")];
const TXTS: &[(&str, &str)] = &[("corrected", "Corrected"), ("raw", "Original"), ("both", "Both")];
pub const THEMES: &[(&str, &str)] = &[("auto", "Match my device"), ("light", "Light"), ("dark", "Dark")];

/// What the button for `v` says: a setting is announced by the words on its button, not its stored value.
pub fn opt_label(opts: &[(&str, &str)], v: &str) -> String {
    opts.iter().find(|(k, _)| *k == v).map_or(v, |(_, l)| *l).to_string()
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
    let from = ctl.node_count();
    let mut clip = ui.clip_rect();
    egui::ScrollArea::vertical().max_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
        clip = ui.clip_rect();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            section(ctl, ui, t, "set-appearance", "Appearance");
            label(ctl, ui, t, "settings-theme-label", "Theme");
            let cur = get_s("theme", "auto");
            let cur = if cur == "light" || cur == "dark" { cur } else { "auto".to_string() };
            if let Some(v) = choice(ctl, ui, t, "theme-", &cur, THEMES) {
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
            if let Some(v) = choice(ctl, ui, t, "st-fmt-", &get_s("exportFormat", "md"), FMTS) {
                out = Some(Out::Setting("exportFormat", json!(v), format!("Combined file: {}.", opt_label(FMTS, &v))));
            }
            // (not "st-sep-label": that is the id of the "A Page N line" choice; one id on two nodes gave it two parents)
            label(ctl, ui, t, "st-sep-title", "Between pages");
            if let Some(v) = choice(ctl, ui, t, "st-sep-", &get_s("exportSeparator", "label"), SEPS) {
                out = Some(Out::Setting("exportSeparator", json!(v), format!("Between pages: {}.", opt_label(SEPS, &v))));
            }
            label(ctl, ui, t, "st-txt-label", "Text to export");
            if let Some(v) = choice(ctl, ui, t, "st-txt-", &get_s("exportText", "corrected"), TXTS) {
                out = Some(Out::Setting("exportText", json!(v), format!("Text to export: {}.", opt_label(TXTS, &v))));
            }

            section(ctl, ui, t, "set-storage", "Storage");
            let on = get_b("keepOriginals", false);
            if check(ctl, ui, t, "st-orig", "Keep untouched originals", on) {
                out = Some(Out::Setting("keepOriginals", json!(!on), format!("Keep untouched originals: {}.", on_off(on))));
            }
            ctl.flush_scroll(ui, &[DIALOG]);
        });
    });
    ctl.clip_since(from, clip);
    ui.add_space(4.0);
    if widgets::button(ctl, ui, t, C::new("dialog-close", "Done", DIALOG), "Done", Look::Primary, 0.0) {
        out = Some(Out::Close);
    }
    out
}


/// A line of text of `font` at the left of a box `w` wide and `h` high (a CSS line box); returns the text's box.
fn line(ui: &mut Ui, w: f32, h: f32, text: &str, font: egui::FontId, col: egui::Color32) -> egui::Rect {
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let g = ui.painter().layout_no_wrap(text.to_string(), font, col);
    let size = g.size();
    let at = egui::Rect::from_min_size(r.min, vec2(size.x, h));
    ui.painter().galley(widgets::centered(at, size), g, col);
    egui::Rect::from_min_size(r.min, vec2(size.x.min(w), h))
}

/// A Chrome radio button of 18px (`input[type=radio]`, `accent-color: var(--ink)`): a grey ring, and when on
/// a ring in the accent with a dot of 10px inside it.
fn radio(p: &egui::Painter, c: egui::Pos2, on: bool, t: &Tokens) {
    let edge = egui::Color32::from_rgb(118, 118, 118);
    let bg = if t.dark { egui::Color32::from_rgb(59, 59, 59) } else { egui::Color32::WHITE };
    p.circle_filled(c, 9.0, bg);
    p.circle_stroke(c, 8.5, Stroke::new(1.0, if on && !t.dark { t.ink } else { edge }));
    if on {
        p.circle_filled(c, 5.0, t.ink);
    }
}

/// A row of the student list: a `.select` option, 44 high.
fn option_row(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, id: &str, text: &str, on: bool) -> bool {
    let r = ui.allocate_response(vec2(ui.available_width(), TARGET), Sense::click());
    let hover = r.hovered();
    ui.painter().rect_filled(r.rect, 8.0, if hover { t.sunken } else { t.surface });
    ui.painter().rect_stroke(r.rect, 8.0, Stroke::new(1.0, if on { t.ink } else { t.line2 }));
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans(14.0), t.ink);
    let size = g.size();
    let at = egui::Rect::from_min_size(r.rect.min + vec2(12.0, 0.0), vec2(size.x, TARGET));
    ui.painter().galley(widgets::centered(at, size), g, t.ink);
    ctl.reg(ui, &r, C::new(id, text, DIALOG).pressed(on).role("listitem"))
}

/// `js/ui/review.js changeStudent`: a modal with a head (title, close), a select "This page belongs to"
/// (default "Choose a student"), a "New student name" field when the last choice is picked, a radio group
/// "Only this page" / "All N pages for <name>" when there is more than one page, and a foot with Cancel, Save.
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
    open: &mut bool,
) -> Option<Out> {
    let mut out = None;
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let w = ui.available_width();
    let names = s.other_students();

    // `.modal-head { padding: 12px 12px 12px 28px; border-bottom: 1px }`
    let (head, _) = ui.allocate_exact_size(vec2(w, 69.0), Sense::hover());
    let g = ui.painter().layout_no_wrap("Change student".to_string(), theme::serif_semi(24.0), t.ink);
    let size = g.size();
    let title = egui::Rect::from_center_size(pos2(head.left() + 28.0 + size.x / 2.0, head.top() + 34.0), size);
    ui.painter().galley(widgets::centered(title, size), g, t.ink);
    ctl.heading("dialog-title", 2, "Change student", DIALOG, title);
    let xr = egui::Rect::from_min_size(pos2(head.right() - 12.0 - TARGET, head.top() + 12.0), vec2(TARGET, TARGET));
    let mut cui = ui.child_ui(xr, egui::Layout::top_down(egui::Align::Min), None);
    if widgets::icon_button(ctl, &mut cui, t, C::new("dialog-x", "Close", DIALOG), widgets::Icon::Close, false) {
        out = Some(Out::Close);
    }
    ui.painter().hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, t.line));

    // `.modal-body { padding: 24px 28px; gap: 20px }`
    ui.add_space(24.0);
    egui::Frame::none().inner_margin(Margin::symmetric(28.0, 0.0)).show(ui, |ui| {
        let iw = w - 56.0;
        ui.set_width(iw);
        let r = line(ui, iw, 19.6, "This page belongs to", theme::sans_semi(14.0), t.ink);
        ctl.text("cs-label", "This page belongs to", DIALOG, r);
        ui.add_space(6.0);

        // the select: shows the choice, opens the list under it
        let shown = if *new_student { "A new student\u{2026}".to_string() } else { pick.clone().unwrap_or_else(|| "Choose a student".to_string()) };
        let sr = ui.allocate_response(vec2(iw, TARGET), Sense::click());
        ui.painter().rect_filled(sr.rect, 8.0, t.surface);
        ui.painter().rect_stroke(sr.rect, 8.0, Stroke::new(1.0, t.line2));
        let g = ui.painter().layout_no_wrap(shown.clone(), theme::sans(14.0), t.ink);
        let size = g.size();
        let at = egui::Rect::from_min_size(sr.rect.min + vec2(12.0, 0.0), vec2(size.x, TARGET));
        ui.painter().galley(widgets::centered(at, size), g, t.ink);
        let c = pos2(sr.rect.right() - 16.0, sr.rect.center().y);
        let chev = Stroke::new(1.6, t.ink);
        ui.painter().line_segment([c + vec2(-4.5, -2.0), c + vec2(0.0, 2.5)], chev);
        ui.painter().line_segment([c + vec2(0.0, 2.5), c + vec2(4.5, -2.0)], chev);
        let label = format!("This page belongs to, {shown}");
        if ctl.reg(ui, &sr, C::new("cs-select", &label, DIALOG).pressed(*open)) {
            *open = !*open;
            if *open {
                let first = pick.as_ref().and_then(|p| names.iter().position(|n| n == p)).map_or("cs-student-0".to_string(), |i| format!("cs-student-{i}"));
                ctl.want_focus = Some(if names.is_empty() { "cs-new".into() } else { first });
            }
        }
        if *open {
            ui.add_space(4.0);
            let from = ctl.node_count();
            let mut clip = ui.clip_rect();
            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([true, true]).show(ui, |ui| {
                clip = ui.clip_rect();
                ui.set_width(iw);
                ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                for (i, n) in names.iter().enumerate() {
                    let on = pick.as_deref() == Some(n.as_str()) && !*new_student;
                    if option_row(ctl, ui, t, &format!("cs-student-{i}"), n, on) {
                        *pick = Some(n.clone());
                        *new_student = false;
                        *error = false;
                        *open = false;
                        ctl.want_focus = Some("cs-select".into());
                        ctl.say(format!("{n} chosen."));
                    }
                }
                if option_row(ctl, ui, t, "cs-new", "A new student\u{2026}", *new_student) {
                    // js: the text box takes focus; a person who cannot see it is told it appeared
                    ctl.want_focus = Some("cs-name".into());
                    ctl.say("A text box for the new student\u{2019}s name is now shown. Type the name.");
                    *new_student = true;
                    *error = false;
                    *open = false;
                }
                ctl.flush_scroll(ui, &[DIALOG]);
            });
            ctl.clip_since(from, clip);
        }

        if *new_student {
            ui.add_space(20.0);
            let r = line(ui, iw, 19.6, "New student name", theme::sans_semi(14.0), t.ink);
            ctl.text("cs-name-label", "New student name", DIALOG, r);
            ui.add_space(6.0);
            let (slot, _) = ui.allocate_exact_size(vec2(iw, TARGET), Sense::hover());
            let edit = egui::TextEdit::singleline(name).hint_text("Student name").font(theme::sans(14.0)).margin(vec2(12.0, 13.0)).desired_width(slot.width() - 24.0);
            let mut r = ui.put(slot, edit);
            r.rect = slot;
            // egui's single-line box gives focus up on Enter; a js input keeps it, so the person (and a screen
            // reader) stays in the box instead of landing on the window
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                r.request_focus();
            }
            if let Some(v) = ctl.reg_text(ui, &r, C::new("cs-name", "New student name", DIALOG), name) {
                *name = v;
            }
        }

        let pages = s.group().map(|g| g.pages.len()).unwrap_or(1);
        if pages > 1 {
            ui.add_space(20.0);
            let who = s.group().map(|g| g.name).unwrap_or_default();
            let rows = [("cs-scope-page", "Only this page".to_string(), false), ("cs-scope-all", format!("All {pages} pages for {who}"), true)];
            for (i, (id, label, is_all)) in rows.into_iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                let on = *all == is_all;
                let r = ui.allocate_response(vec2(iw, TARGET), Sense::click());
                ui.painter().rect_stroke(r.rect, 8.0, Stroke::new(1.0, if on { t.ink } else { t.line2 }));
                radio(ui.painter(), pos2(r.rect.left() + 12.0 + 9.0, r.rect.center().y), on, t);
                let g = ui.painter().layout_no_wrap(label.clone(), theme::sans(14.0), t.ink);
                let size = g.size();
                let at = egui::Rect::from_min_size(pos2(r.rect.left() + 12.0 + 18.0 + 12.0, r.rect.top()), vec2(size.x, TARGET));
                ui.painter().galley(widgets::centered(at, size), g, t.ink);
                if ctl.reg(ui, &r, C::new(id, &label, DIALOG).pressed(on).role("radio")) {
                    *all = is_all;
                }
            }
        }
        if *error {
            ui.add_space(20.0);
            let m = "Choose a student, or type a name.";
            let r = line(ui, iw, 18.0, m, theme::sans(13.0), t.bad);
            ctl.status("cs-error", m, DIALOG, r);
        }
    });

    // `.modal-foot { padding: 14px 28px; border-top: 1px; justify-content: flex-end; gap: 8px }`
    ui.add_space(24.0);
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y + 0.5, Stroke::new(1.0, t.line));
    ui.add_space(15.0);
    let bw = |ui: &Ui, s: &str| ui.painter().layout_no_wrap(s.to_string(), theme::sans_semi(14.0), t.ink).size().x + widgets::BTN_PAD;
    let (cw, sw) = (bw(ui, "Cancel"), bw(ui, "Save"));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space((w - 28.0 - 28.0 - cw - sw - 8.0).max(0.0) + 28.0);
        if widgets::button(ctl, ui, t, C::new("dialog-cancel", "Cancel", DIALOG), "Cancel", Look::Outline, 0.0) {
            out = Some(Out::Close);
        }
        if widgets::button(ctl, ui, t, C::new("dialog-ok", "Save", DIALOG), "Save", Look::Primary, 0.0) {
            let chosen = if *new_student { name.trim().to_string() } else { pick.clone().unwrap_or_default() };
            if chosen.trim().is_empty() {
                *error = true;
                // js focuses the select or the name box; here the refusal is said and focus goes to the same place
                ctl.say_alert("Choose a student, or type a name.");
                ctl.want_focus = Some(if *new_student { "cs-name".into() } else { "cs-select".into() });
            } else {
                out = Some(Out::Student { name: chosen, all: *all });
            }
        }
    });
    ui.add_space(14.0);
    out
}
