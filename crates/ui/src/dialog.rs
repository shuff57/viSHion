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
    UseProvider(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// `drafts`: text the person is typing in the settings boxes, by control id; they are saved when the
    /// dialog closes (as the JS inputs save on `change`).
    Settings {
        drafts: std::collections::BTreeMap<&'static str, String>,
        /// Which select of the dialog has its list shown.
        open: Option<&'static str>,
        /// The "Another model name" box is shown.
        custom: bool,
    },
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
        Dialog::Settings { drafts: Default::default(), open: None, custom: false }
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
    /// The provider select changed (a cloud one is confirmed first).
    Provider(String),
}

/// Save what was typed in the settings boxes (the JS inputs save on `change`).
pub fn commit(s: &mut Session, d: &Dialog) {
    let Dialog::Settings { drafts, .. } = d else { return };
    let mut changed = false;
    for (id, v) in drafts {
        let (key, val) = match *id {
            "st-ocr" => {
                let id = setting_str(s, "provider", "local");
                ("ocrUrl", serde_json::json!(engine_url(s, provider(&id).0, v)))
            }
            "st-model-custom" => ("model", serde_json::json!(v.trim())),
            "st-idle" => ("idleUnloadMin", serde_json::json!(v.trim().parse::<f64>().ok().map_or(0.0, |n| n.round().max(0.0)))),
            "st-key" => ("apiKey", serde_json::json!(v.trim())),
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
        remember(s);
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
    let narrow = matches!(d, Dialog::ChangeStudent { .. } | Dialog::Settings { .. });
    let width = (screen.width() - 32.0).clamp(280.0, if matches!(d, Dialog::Settings { .. }) { 600.0 } else if narrow { 440.0 } else { 480.0 });
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
                    Dialog::Settings { drafts, open, custom } => out = settings(ctl, ui, t, s, drafts, open, custom),
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

type Drafts = std::collections::BTreeMap<&'static str, String>;

fn text_box(ctl: &mut Ctl, ui: &mut Ui, id: &'static str, name: &str, hint: &str, cur: String, drafts: &mut Drafts, secret: bool, w: f32) {
    let buf = drafts.entry(id).or_insert(cur);
    let (slot, _) = ui.allocate_exact_size(vec2(w, TARGET), Sense::hover());
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
const JMODES: &[(&str, &str)] = &[("auto", "Ask the model"), ("heuristic", "Built-in matching only")];
pub const THEMES: &[(&str, &str)] = &[("auto", "System"), ("light", "Light"), ("dark", "Dark")];

/// `models.js PROVIDERS`: id, label, address, cloud, name.
pub const PROVIDERS: &[(&str, &str, &str, bool, &str)] = &[
    ("local", "Local server (llama.cpp or Ollama)", "http://localhost:8188", false, "this computer"),
    ("ollama-cloud", "Ollama Cloud", "https://ollama.com", true, "Ollama Cloud"),
    ("openrouter", "OpenRouter", "https://openrouter.ai/api", true, "OpenRouter"),
    ("custom", "Another OpenAI-compatible endpoint", "", true, "the server you entered"),
];

pub fn provider(id: &str) -> &'static (&'static str, &'static str, &'static str, bool, &'static str) {
    PROVIDERS.iter().find(|p| p.0 == id).unwrap_or(&PROVIDERS[0])
}

fn trim_url(u: &str) -> String {
    u.trim().trim_end_matches('/').to_string()
}

/// `M.engineUrl`: for a relayed provider (Ollama Cloud) the relay, with the host in the path.
fn engine_url(s: &Session, provider_id: &str, shown: &str) -> String {
    let u = trim_url(shown);
    if provider_id != "ollama-cloud" || u.is_empty() {
        return u;
    }
    let relay = s.settings.get("relayUrl").and_then(|v| v.as_str()).map(trim_url).filter(|r| !r.is_empty()).unwrap_or_else(|| "http://localhost:8189".into());
    let host = u.trim_start_matches("https://").trim_start_matches("http://");
    let host = host.split('/').next().unwrap_or("");
    format!("{relay}/u/{host}")
}

/// `M.shownUrl`: the address a person recognises, recovered from what the engine uses.
fn shown_url(provider_id: &str, engine: &str) -> String {
    let e = trim_url(engine);
    if provider_id == "ollama-cloud" {
        if let Some((_, host)) = e.rsplit_once("/u/") {
            if !host.is_empty() && !host.contains('/') {
                return format!("https://{host}");
            }
        }
    }
    e
}

fn setting_str(s: &Session, k: &str, d: &str) -> String {
    s.settings.get(k).and_then(|v| v.as_str()).unwrap_or(d).to_string()
}

/// `M.remember`: the active address, model and key are kept under the active provider.
pub fn remember(s: &mut Session) {
    use serde_json::json;
    let id = setting_str(s, "provider", "local");
    let id = provider(&id).0;
    let mut profiles = s.settings.get("profiles").cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
    let entry = json!({
        "url": shown_url(id, &setting_str(s, "ocrUrl", "")),
        "model": setting_str(s, "model", ""),
        "key": setting_str(s, "apiKey", ""),
    });
    if let Some(m) = profiles.as_object_mut() {
        m.insert(id.to_string(), entry);
    }
    s.settings.set("profiles", profiles);
}

/// `M.switchTo`: remember the values in use, then take the saved ones of `id` (or its defaults).
pub fn switch_provider(s: &mut Session, id: &str) {
    use serde_json::json;
    remember(s);
    let base = provider(id);
    let saved = s.settings.get("profiles").and_then(|p| p.get(base.0)).cloned().unwrap_or_else(|| json!({}));
    let url = saved.get("url").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| base.2.to_string());
    let model = saved.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let key = saved.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let engine = engine_url(s, base.0, &url);
    s.settings.set("provider", json!(base.0));
    s.settings.set("ocrUrl", json!(engine));
    s.settings.set("model", json!(model));
    s.settings.set("apiKey", json!(key));
    s.dirty.settings = true;
}

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

/// A paragraph of `w` wide, one line box of `lh` per row; returns its box.
fn para(ui: &mut Ui, w: f32, text: &str, font: egui::FontId, col: egui::Color32, lh: f32) -> egui::Rect {
    let g = ui.painter().layout(text.to_string(), font.clone(), col, w);
    let rows: Vec<String> = g.rows.iter().map(|r| r.text().trim_end().to_string()).collect();
    let n = rows.len().max(1);
    let (r, _) = ui.allocate_exact_size(vec2(w, lh * n as f32), Sense::hover());
    for (i, row) in rows.iter().enumerate() {
        let rg = ui.painter().layout_no_wrap(row.clone(), font.clone(), col);
        let dy = ((lh - rg.size().y.min(lh)) / 2.0 + 0.2).round();
        ui.painter().galley(pos2(r.left(), r.top() + i as f32 * lh + dy), rg, col);
    }
    r
}

fn hint_p(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, w: f32, id: &'static str, text: &str) {
    let r = para(ui, w, text, theme::sans(13.0), t.muted, 18.2);
    ctl.text(id, text, DIALOG, r);
}

fn field_label(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, w: f32, id: &'static str, text: &str) {
    let r = line(ui, w, 19.6, text, theme::sans_semi(14.0), t.ink);
    ctl.text(id, text, DIALOG, r);
    ui.add_space(6.0);
}

/// `<select class="select">`: the shown value and a chevron; returns whether it was pressed.
fn select_box(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, w: f32, id: &str, name: &str, shown: &str, open: bool) -> bool {
    let sr = ui.allocate_response(vec2(w, TARGET), Sense::click());
    let sb = widgets::snap(sr.rect);
    ui.painter().rect_filled(sb, 8.0, t.surface);
    ui.painter().rect_stroke(sb.shrink(0.5), 7.5, Stroke::new(1.0, t.line2));
    let g = ui.painter().layout_no_wrap(shown.to_string(), theme::sans(14.0), t.ink);
    let size = g.size();
    let at = egui::Rect::from_min_size(sr.rect.min + vec2(12.0, 0.0), vec2(size.x, TARGET));
    ui.painter().galley(widgets::centered(at, size), g, t.ink);
    let c = pos2(sr.rect.right() - 16.0, sr.rect.center().y);
    let chev = Stroke::new(1.6, t.ink);
    ui.painter().line_segment([c + vec2(-4.5, -2.0), c + vec2(0.0, 2.5)], chev);
    ui.painter().line_segment([c + vec2(0.0, 2.5), c + vec2(4.5, -2.0)], chev);
    let label = format!("{name}, {shown}");
    ctl.reg(ui, &sr, C::new(id, &label, DIALOG).pressed(open))
}

/// A `.field` holding a select over `items` (value, label), with the popup list inline under it. Returns the
/// value picked. `open` is which select of the dialog has its list shown.
#[allow(clippy::too_many_arguments)]
fn select_field(
    ctl: &mut Ctl,
    ui: &mut Ui,
    t: &Tokens,
    w: f32,
    open: &mut Option<&'static str>,
    id: &'static str,
    label_id: &'static str,
    name: &str,
    items: &[(String, String)],
    cur: &str,
    opt_prefix: &str,
) -> Option<String> {
    field_label(ctl, ui, t, w, label_id, name);
    let shown = items.iter().find(|(v, _)| v == cur).map_or(cur.to_string(), |(_, l)| l.clone());
    let is_open = *open == Some(id);
    if select_box(ctl, ui, t, w, id, name, &shown, is_open) {
        *open = if is_open { None } else { Some(id) };
        if *open == Some(id) {
            let on = items.iter().position(|(v, _)| v == cur).unwrap_or(0);
            ctl.want_focus = Some(format!("{opt_prefix}{}", items.get(on).map_or("", |i| i.0.as_str())));
        }
    }
    let mut out = None;
    if *open == Some(id) {
        ui.add_space(4.0);
        for (i, (v, l)) in items.iter().enumerate() {
            if i > 0 {
                ui.add_space(4.0);
            }
            let oid = format!("{opt_prefix}{v}");
            if option_row(ctl, ui, t, &oid, l, v == cur) {
                out = Some(v.clone());
                *open = None;
                ctl.want_focus = Some(id.into());
            }
        }
    }
    out
}

/// `.checkrow`: a bordered row with an 18px checkbox, the label in bold and what it does under it.
#[allow(clippy::too_many_arguments)]
fn checkrow(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, w: f32, id: &str, title: &str, hint: &str, on: bool) -> bool {
    let tw = w - 2.0 - 24.0 - 18.0 - 12.0;
    let hg = ui.painter().layout(hint.to_string(), theme::sans(13.0), t.muted, tw);
    let n = hg.rows.len().max(1);
    let h = 2.0 + 20.0 + 19.6 + 18.2 * n as f32;
    let r = ui.allocate_response(vec2(w, h.max(TARGET)), Sense::click());
    let hover = r.hovered();
    let p = ui.painter();
    if hover {
        p.rect_filled(r.rect, 8.0, t.sunken);
    }
    p.rect_stroke(widgets::snap(r.rect).shrink(0.5), 7.5, Stroke::new(1.0, t.line2));
    // the box: 18px, accent ink when on
    let b = egui::Rect::from_min_size(pos2(r.rect.left() + 13.0, r.rect.top() + 13.0), vec2(18.0, 18.0));
    if on {
        p.rect_filled(b, 3.0, t.ink);
        let c = b.center();
        let st = Stroke::new(2.0, t.surface);
        p.line_segment([c + vec2(-4.5, 0.5), c + vec2(-1.5, 3.5)], st);
        p.line_segment([c + vec2(-1.5, 3.5), c + vec2(4.5, -3.0)], st);
    } else {
        p.rect_filled(b, 3.0, if t.dark { egui::Color32::from_rgb(59, 59, 59) } else { egui::Color32::WHITE });
        p.rect_stroke(widgets::snap(b).shrink(0.5), 2.5, Stroke::new(1.0, egui::Color32::from_rgb(118, 118, 118)));
    }
    let x = r.rect.left() + 13.0 + 18.0 + 12.0;
    let y = r.rect.top() + 11.0;
    let tg = p.layout_no_wrap(title.to_string(), theme::sans_semi(14.0), t.ink);
    let dy = ((19.6 - tg.size().y.min(19.6)) / 2.0 + 0.2).round();
    p.galley(pos2(x, y + dy), tg, t.ink);
    for (i, row) in hg.rows.iter().enumerate() {
        let rg = p.layout_no_wrap(row.text().trim_end().to_string(), theme::sans(13.0), t.muted);
        let dy = ((18.2 - rg.size().y.min(18.2)) / 2.0 + 0.2).round();
        p.galley(pos2(x, y + 19.6 + i as f32 * 18.2 + dy), rg, t.muted);
    }
    ctl.reg(ui, &r, C::new(id, title, DIALOG).pressed(on).role("checkbox"))
}

/// `.modal-head`: the title and a close button; returns whether it was pressed.
fn modal_head(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, w: f32, title: &str) -> bool {
    let (head, _) = ui.allocate_exact_size(vec2(w, 69.0), Sense::hover());
    let g = ui.painter().layout_no_wrap(title.to_string(), theme::serif_semi(24.0), t.ink);
    let size = g.size();
    let rect = egui::Rect::from_center_size(pos2(head.left() + 28.0 + size.x / 2.0, head.top() + 34.0), size);
    ui.painter().galley(widgets::centered(rect, size), g, t.ink);
    ctl.heading("dialog-title", 2, title, DIALOG, rect);
    let xr = egui::Rect::from_min_size(pos2(head.right() - 12.0 - TARGET, head.top() + 12.0), vec2(TARGET, TARGET));
    let mut cui = ui.child_ui(xr, egui::Layout::top_down(egui::Align::Min), None);
    let closed = widgets::icon_button(ctl, &mut cui, t, C::new("dialog-x", "Close", DIALOG), widgets::Icon::Close, false);
    ui.painter().hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, t.line));
    closed
}

fn section_head(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, w: f32, id: &'static str, text: &str) {
    let g = ui.painter().layout_no_wrap(text.to_string(), theme::sans_semi(16.0), t.ink);
    let (r, _) = ui.allocate_exact_size(vec2(w, 22.4), Sense::hover());
    let size = g.size();
    let dy = ((22.4 - size.y.min(22.4)) / 2.0 + 0.2).round();
    ui.painter().galley(pos2(r.left(), r.top() + dy), g, t.ink);
    ctl.heading(id, 3, text, DIALOG, egui::Rect::from_min_size(r.min, vec2(size.x, 22.4)));
    ui.add_space(14.0);
}

/// `.set-sec`: 20px above and below, a soft line under it (not the last one).
fn section_end(ui: &mut Ui, t: &Tokens, w: f32) {
    ui.add_space(20.0);
    let y = ui.cursor().top();
    let x0 = ui.min_rect().left();
    ui.painter().hline(x0..=x0 + w, y + 0.5, Stroke::new(1.0, t.soft));
    ui.add_space(21.0);
}

fn settings(ctl: &mut Ctl, ui: &mut Ui, t: &Tokens, s: &Session, drafts: &mut Drafts, open: &mut Option<&'static str>, custom: &mut bool) -> Option<Out> {
    use serde_json::json;
    let mut out: Option<Out> = None;
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let w = ui.available_width();
    let get_b = |k: &str, d: bool| s.settings.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
    if modal_head(ctl, ui, t, w, "Settings") {
        out = Some(Out::Close);
    }
    // `.modal { max-height: calc(100dvh - 96px) }`: head 69, foot 73, the body scrolls in what is left
    let max_body = (ui.ctx().screen_rect().height() - 96.0 - 69.0 - 73.0).max(120.0);
    let from = ctl.node_count();
    let mut clip = ui.clip_rect();
    let pid = provider(&setting_str(s, "provider", "local"));
    let cloud = pid.3;
    egui::ScrollArea::vertical().max_height(max_body).auto_shrink([false, true]).show(ui, |ui| {
        clip = ui.clip_rect();
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        egui::Frame::none().inner_margin(Margin::symmetric(28.0, 24.0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let iw = w - 56.0;
            ui.set_width(iw);
            let opt = |v: &[(&str, &str)]| -> Vec<(String, String)> { v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };

            // ---- Appearance
            section_head(ctl, ui, t, iw, "set-appearance", "Appearance");
            field_label(ctl, ui, t, iw, "settings-theme-label", "Theme");
            let cur = setting_str(s, "theme", "auto");
            let cur = if cur == "light" || cur == "dark" { cur } else { "auto".to_string() };
            let ids = ["theme-auto", "theme-light", "theme-dark"];
            let items: Vec<(&str, &str, bool, bool)> = THEMES.iter().zip(ids).map(|((v, l), id)| (id, *l, *v == cur, false)).collect();
            if let Some(i) = widgets::seg(ctl, ui, t, DIALOG, "theme", &items) {
                out = Some(Out::Theme(THEMES[i].0.to_string()));
            }
            section_end(ui, t, iw);

            // ---- Model
            section_head(ctl, ui, t, iw, "set-model", "Model");
            let prov_items: Vec<(String, String)> = PROVIDERS.iter().map(|p| (p.0.to_string(), p.1.to_string())).collect();
            if let Some(v) = select_field(ctl, ui, t, iw, open, "st-provider", "st-provider-label", "Where the model runs", &prov_items, pid.0, "st-provider-") {
                if v != pid.0 {
                    out = Some(Out::Provider(v));
                }
            }
            ui.add_space(6.0);
            hint_p(ctl, ui, t, iw, "set-model-note", "One model reads the pages, finds the student\u{2019}s name and checks spelling.");
            if cloud {
                ui.add_space(14.0);
                let msg = format!("Page images, and the student names read from them, are sent to {} over the internet. Use this only if that is allowed for your students\u{2019} work.", pid.4);
                egui::Frame::none().fill(t.warn_bg).stroke(Stroke::new(1.0, t.warn)).rounding(8.0).inner_margin(Margin::same(12.0)).show(ui, |ui| {
                    let r = para(ui, iw - 26.0, &msg, theme::sans(14.0), t.ink, 19.6);
                    ctl.text("st-cloud-note", &msg, DIALOG, r);
                });
            }
            ui.add_space(14.0);
            field_label(ctl, ui, t, iw, "st-ocr-label", "Server address");
            let shown = shown_url(pid.0, &setting_str(s, "ocrUrl", ""));
            text_box(ctl, ui, "st-ocr", "Server address", if pid.0 == "custom" { "https://example.com" } else { "" }, shown, drafts, false, iw);
            ui.add_space(6.0);
            hint_p(ctl, ui, t, iw, "st-ocr-hint", "A local llama.cpp or Ollama server, or the endpoint\u{2019}s base address.");
            ui.add_space(14.0);
            field_label(ctl, ui, t, iw, "st-key-label", "API key");
            text_box(ctl, ui, "st-key", "API key", "", setting_str(s, "apiKey", ""), drafts, true, iw);
            ui.add_space(14.0);
            let key_hint = match pid.0 {
                "ollama-cloud" => "Create one at ollama.com/settings/keys. Requests go through tools/relay.js on this computer (run it once: node tools/relay.js). Kept in this browser only.",
                _ if cloud => "Needed for this endpoint. Kept in this browser only, never written to a project folder or an export.",
                _ => "Not needed for a server on this computer. Kept in this browser only, never written to a project folder or an export.",
            };
            hint_p(ctl, ui, t, iw, "st-key-hint", key_hint);
            ui.add_space(14.0);
            // the model: the one in use (or the server's own choice), and another name typed
            let cur_model = setting_str(s, "model", "");
            let mut mitems = vec![(String::new(), "Whatever the server has loaded".to_string())];
            if !cur_model.is_empty() {
                mitems.push((cur_model.clone(), cur_model.clone()));
            }
            mitems.push(("__other".to_string(), "Another model name\u{2026}".to_string()));
            if let Some(v) = select_field(ctl, ui, t, iw, open, "st-model", "st-model-label", "Model", &mitems, &cur_model, "st-model-opt-") {
                if v == "__other" {
                    *custom = true;
                    ctl.want_focus = Some("st-model-custom".into());
                } else {
                    *custom = false;
                    drafts.remove("st-model-custom");
                    let said = format!("Model: {}.", if v.is_empty() { "whatever the server has loaded" } else { v.as_str() });
                    out = Some(Out::Setting("model", json!(v), said));
                }
            }
            ui.add_space(6.0);
            hint_p(ctl, ui, t, iw, "st-model-hint", "Choose which model reads the pages. A local server swaps it in when you pick another.");
            if *custom {
                ui.add_space(14.0);
                text_box(ctl, ui, "st-model-custom", "Model name", "Model name", cur_model.clone(), drafts, false, iw);
            }
            if !cloud {
                ui.add_space(14.0);
                field_label(ctl, ui, t, iw, "st-idle-label", "Unload the model after it has been idle for (minutes)");
                let idle = s.settings.get("idleUnloadMin").and_then(|v| v.as_f64()).unwrap_or(10.0);
                text_box(ctl, ui, "st-idle", "Unload the model after it has been idle for (minutes)", "", format!("{idle}"), drafts, false, 140.0);
                ui.add_space(6.0);
                hint_p(ctl, ui, t, iw, "st-idle-hint", "Frees your graphics memory when you stop. 0 keeps it loaded. This only runs while this page is open.");
            }
            ui.add_space(14.0);
            field_label(ctl, ui, t, iw, "st-timeout-label", "Give up on a page after (seconds)");
            let secs = s.settings.get("timeoutSec").and_then(|v| v.as_f64()).unwrap_or(180.0);
            text_box(ctl, ui, "st-timeout", "Give up on a page after (seconds)", "180", format!("{secs}"), drafts, false, iw);
            ui.add_space(6.0);
            hint_p(ctl, ui, t, iw, "st-timeout-hint", "A page that takes longer is marked as failed and the batch moves on.");
            ui.add_space(14.0);
            let on = get_b("straighten", true);
            if checkrow(ctl, ui, t, iw, "st-straighten", "Straighten tilted pages", "Turns a page that was photographed at a slant so the text runs level. This helps the line boxes.", on) {
                out = Some(Out::Setting("straighten", json!(!on), format!("Straighten tilted pages: {}.", on_off(on))));
            }
            ui.add_space(14.0);
            let on = get_b("demo", false);
            if checkrow(ctl, ui, t, iw, "st-demo", "Demo mode", "Uses sample text instead of a model, so you can try the app without a server.", on) {
                out = Some(Out::Setting("demo", json!(!on), format!("Demo mode: {}.", on_off(on))));
            }
            section_end(ui, t, iw);

            // ---- Spelling check
            section_head(ctl, ui, t, iw, "set-spelling", "Spelling check");
            let cur = if setting_str(s, "judgeMode", "auto") == "heuristic" { "heuristic" } else { "auto" };
            if let Some(v) = select_field(ctl, ui, t, iw, open, "st-jmode", "st-jmode-label", "How to suggest", &opt(JMODES), cur, "st-jmode-") {
                let said = format!("How to suggest: {}.", if v == "auto" { "ask the model" } else { "built-in matching only" });
                out = Some(Out::Setting("judgeMode", json!(v), said));
            }
            ui.add_space(6.0);
            hint_p(ctl, ui, t, iw, "st-jmode-hint", "The same model that reads the page also checks spelling against the subject\u{2019}s words. If it fails, built-in matching is used. A failure never blocks a transcript.");
            ui.add_space(14.0);
            let topic = s.a.meta.topic().to_string();
            let name = if topic.is_empty() { "Course words".to_string() } else { format!("Course words for {topic}") };
            field_label(ctl, ui, t, iw, "st-words-label", &name);
            text_box(ctl, ui, "st-words", &name, "derivative, integral", s.settings.terms(&topic).join(", "), drafts, false, iw);
            ui.add_space(6.0);
            hint_p(ctl, ui, t, iw, "st-words-hint", "Separate words with commas. Used to catch course words that get misread.");
            section_end(ui, t, iw);

            // ---- Unreadable words
            section_head(ctl, ui, t, iw, "set-unreadable", "Unreadable words");
            let on = get_b("guessIllegible", true);
            if checkrow(ctl, ui, t, iw, "st-guess", "Suggest likely words", "After a page is read, the model looks again at each word it could not read and offers its best guesses with a percentage. Those percentages are its own estimate. This adds one more request per page that has unreadable words.", on) {
                out = Some(Out::Setting("guessIllegible", json!(!on), format!("Suggest likely words: {}.", on_off(on))));
            }
            section_end(ui, t, iw);

            // ---- Remembered fixes
            section_head(ctl, ui, t, iw, "set-fixes", "Remembered fixes");
            let on = get_b("rememberFixes", true);
            if checkrow(ctl, ui, t, iw, "st-remember", "Remember my fixes for unreadable words", "When you type a word for handwriting the model could not read, it is kept with the words around it. The model sees recent ones as hints when it reads or guesses later pages. This guides the model; it does not retrain it.", on) {
                out = Some(Out::Setting("rememberFixes", json!(!on), format!("Remember my fixes: {}.", on_off(on))));
            }
            ui.add_space(14.0);
            let n = s.settings.get("reference").and_then(|v| v.as_array()).map_or(0, |a| a.len());
            let count = format!("{} remembered.", crate::state::plural(n, "fix", "fixes"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 0.0);
                let g = ui.painter().layout_no_wrap(count.clone(), theme::sans(14.0), t.muted);
                let (r, _) = ui.allocate_exact_size(vec2(g.size().x, TARGET), Sense::hover());
                ui.painter().galley(widgets::centered(r, g.size()), g, t.muted);
                ctl.text("st-fixes-count", &count, DIALOG, r);
                if widgets::button(ctl, ui, t, C::new("st-clear-fixes", "Clear remembered fixes", DIALOG).disabled(n == 0), "Clear remembered fixes", Look::Outline, 0.0) {
                    out = Some(Out::AskClearFixes);
                }
            });
            section_end(ui, t, iw);

            // ---- Export defaults
            section_head(ctl, ui, t, iw, "set-export", "Export defaults");
            let cur = setting_str(s, "exportFormat", "md");
            if let Some(v) = select_field(ctl, ui, t, iw, open, "st-fmt", "st-fmt-label", "Combined file", &opt(FMTS), &cur, "st-fmt-") {
                out = Some(Out::Setting("exportFormat", json!(v), format!("Combined file: {}.", opt_label(FMTS, &v))));
            }
            ui.add_space(14.0);
            let cur = setting_str(s, "exportSeparator", "label");
            if let Some(v) = select_field(ctl, ui, t, iw, open, "st-sep", "st-sep-title", "Between pages", &opt(SEPS), &cur, "st-sep-") {
                out = Some(Out::Setting("exportSeparator", json!(v), format!("Between pages: {}.", opt_label(SEPS, &v))));
            }
            ui.add_space(14.0);
            let cur = setting_str(s, "exportText", "corrected");
            if let Some(v) = select_field(ctl, ui, t, iw, open, "st-txt", "st-txt-title", "Text to export", &opt(TXTS), &cur, "st-txt-") {
                out = Some(Out::Setting("exportText", json!(v), format!("Text to export: {}.", opt_label(TXTS, &v))));
            }
            section_end(ui, t, iw);

            // ---- Storage
            section_head(ctl, ui, t, iw, "set-storage", "Storage");
            let where_ = "This browser only";
            let g = ui.painter().layout_no_wrap(where_.to_string(), theme::sans(14.0), t.muted);
            let (r, _) = ui.allocate_exact_size(vec2(iw, TARGET), Sense::hover());
            ui.painter().galley(pos2(r.left(), widgets::centered(r, g.size()).y), g, t.muted);
            ctl.text("st-where", where_, DIALOG, r);
            ui.add_space(14.0);
            let on = get_b("keepOriginals", false);
            if checkrow(ctl, ui, t, iw, "st-orig", "Keep untouched originals", "Saves each upload as it came, next to the resized copy. Uses more disk space.", on) {
                out = Some(Out::Setting("keepOriginals", json!(!on), format!("Keep untouched originals: {}.", on_off(on))));
            }
            ctl.flush_scroll(ui, &[DIALOG]);
        });
    });
    ctl.clip_since(from, clip);

    // `.modal-foot { padding: 14px 28px; border-top: 1px solid var(--line) }`
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y + 0.5, Stroke::new(1.0, t.line));
    ui.add_space(15.0);
    let dw = ui.painter().layout_no_wrap("Done".to_string(), theme::sans_semi(14.0), t.ink).size().x + widgets::BTN_PAD;
    ui.horizontal(|ui| {
        ui.add_space((w - 28.0 - dw - 28.0).max(0.0) + 28.0);
        if widgets::button(ctl, ui, t, C::new("dialog-close", "Done", DIALOG), "Done", Look::Primary, 0.0) {
            out = Some(Out::Close);
        }
    });
    ui.add_space(14.0);
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
    ui.painter().rect_stroke(widgets::snap(r.rect).shrink(0.5), 7.5, Stroke::new(1.0, if on { t.ink } else { t.line2 }));
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
    if modal_head(ctl, ui, t, w, "Change student") {
        out = Some(Out::Close);
    }

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
        ui.painter().rect_stroke(widgets::snap(sr.rect).shrink(0.5), 7.5, Stroke::new(1.0, t.line2));
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
                ui.painter().rect_stroke(widgets::snap(r.rect).shrink(0.5), 7.5, Stroke::new(1.0, if on { t.ink } else { t.line2 }));
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
