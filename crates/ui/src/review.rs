//! The review screen: header, page pane (tools, strip, image with marks), transcript with flagged words
//! inline, the "Check this word" tray, a minimal assignment overview, and the toast. Follows
//! design/Main.dc.html and js/ui/review.js + header.js + css/layout.css.
//!
//! Drawing never mutates the session. Interactions are collected as `Act`s and applied after the frame, so
//! the borrow of the session stays shared while egui closures run. Keys are routed by where focus is
//! (transcript, strip, stage, tray, dialog), exactly as the JS handlers hang off those elements.
use crate::dialog::{self, After, Dialog};
use crate::flow;
use crate::state::{self, BoxMode, Entry, Kind, PillKind, Session, View};
use crate::theme::{self, Tokens};
use crate::transcript::{self, Line, Run, Style};
use crate::twin::{HEADER, SCAN, TEXT, TRAY};
use crate::widgets::{self, Icon, Look, TARGET};
use crate::{App, Boot, Cmd, Ctl, Img, KeyEv, SaveState, Slot, C};
use egui::{pos2, vec2, Align, Color32, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use serde_json::Value;
use vishion_core::jsx::truthy as jsx_truthy;
use vishion_core::model::Page;
use vishion_core::words;

/// Zoom and scroll of the page stage, kept between frames so a zoom step holds the middle of the view.
#[derive(Default)]
pub(crate) struct Stage {
    off: Vec2,
    view: Vec2,
    zoom: f32,
    /// Offset to keep applying for a few frames while the content grows to its new size.
    target: Option<(Vec2, u8)>,
    /// Content height of the tray last frame (the phone tray scrolls inside half the screen).
    tray_h: f32,
    /// What was last scrolled into view (review.js `scrollIntoView({ block: 'nearest' })`), so the view moves
    /// when the selection moves and is otherwise left to the reader.
    sel_seen: Option<(String, String)>,
    tray_seen: bool,
    read_seen: Option<usize>,
    mark_seen: Option<(Option<usize>, Option<usize>)>,
    /// Frames left in which the selected word / the reading line is kept in view. The tray (and the page's
    /// layout) settle over a frame or two after a selection, so one request is not enough.
    sel_left: u8,
    read_left: u8,
    /// Height of the transcript's scroll area last frame: when the tray grows (its crop arrives, the options
    /// wrap) the selected word is brought back into view.
    text_h: f32,
}

pub(crate) enum Act {
    Pick(String, String),
    SetSplit(bool),
    Original(bool),
    Student(i32),
    DoneNext,
    Copy,
    Boxes,
    StripClick { id: String, multi: bool, shift: bool },
    StripStep(String, i32),
    StripEdge(bool),
    MovePage(String, i32),
    Choose(&'static str, String),
    UseAll,
    StepWord(i32),
    Tray(bool),
    ReadLine(usize),
    ReadMove(i32),
    ReadClear,
    Zoom(i32),
    ZoomSet(f32),
    Rotate,
    Level,
    Replace,
    RemovePage(String),
    AddPage,
    ChangeStudent,
    TextView(bool),
    Transcribe(String, bool),
    Guess(String),
    OpenSettings,
    CloseDialog,
    Inbox,
    OpenReview(String),
    ExportSelected,
    ReviewAgain,
    ClearMulti,
    Dialog(dialog::Out),
}

struct Cx<'a> {
    t: &'a Tokens,
    ctl: &'a mut Ctl,
    math: &'a mut crate::math::MathCache,
    images: &'a HashMap<String, Slot>,
    typed: &'a mut String,
    acts: &'a mut Vec<Act>,
    save: &'a SaveState,
    storage: &'a str,
    notice: &'a Option<String>,
    narrow: bool,
    screen_h: f32,
    stage: &'a mut Stage,
    /// Keep the selected word / the reading line in view this frame.
    scroll_sel: bool,
    scroll_read: bool,
}

const TEXT_PX: f32 = 22.0;
/// `line-height: 1.7` at 22px: the pitch of a transcript line.
const PITCH: f32 = TEXT_PX * 1.7;
/// `.tx-text { max-width: 31em }`
const TEXT_MAX_W: f32 = 31.0 * TEXT_PX;
/// `.katex { font-size: 1.21em }`: math is set 21% larger than the text around it.
const KATEX_EM: f32 = 1.21;
/// css/layout.css: below this width the panes stack and the header wraps.
const NARROW: f32 = 900.0;

/// Text in a CSS line box: `line_h` per line, wrapped at `wrap_w`. Returns the rect it took.
fn css_text(ui: &mut Ui, text: &str, font: egui::FontId, color: Color32, line_h: f32, wrap_w: f32, anywhere: bool) -> Rect {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat { font_id: font, color, line_height: Some(line_h), ..Default::default() },
    );
    job.wrap.max_width = wrap_w.max(1.0);
    job.wrap.break_anywhere = anywhere;
    let nat = ui.painter().layout_no_wrap("Hg".to_string(), font_id_of(&job), color).size().y;
    let g = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(g.size().x.max(1.0), g.size().y.max(line_h)), Sense::hover());
    // egui keeps the glyphs at the top of a taller or shorter row; a browser centres them (half-leading)
    ui.painter().galley(rect.min + vec2(0.0, ((line_h - nat) / 2.0 + 0.2).round()), g, color);
    rect
}

fn font_id_of(job: &egui::text::LayoutJob) -> egui::FontId {
    job.sections.first().map(|s| s.format.font_id.clone()).unwrap_or_else(|| theme::sans(14.0))
}

fn tw(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x
}

pub(crate) fn show(app: &mut App, ctx: &egui::Context, t: &Tokens) {
    let App { ctl, session, math, images, typed, typed_for, status_msg, status_err, boot, notice, save, storage, dialog, stage, host, .. } = app;
    let mut acts: Vec<Act> = vec![];
    if let Some(s) = session.as_mut() {
        // js renderWho: Original cannot stay set where both versions read the same
        s.normalize();
    }
    let screen = ctx.screen_rect();
    let narrow = screen.width() < NARROW;
    match (&*boot, session.as_ref()) {
        (Boot::Ready, Some(s)) => {
            let mut cx = Cx {
                t,
                ctl: &mut *ctl,
                math: &mut *math,
                images: &*images,
                typed: &mut *typed,
                acts: &mut acts,
                save: &*save,
                storage: &*storage,
                notice: &*notice,
                narrow,
                screen_h: screen.height(),
                stage: &mut *stage,
                scroll_sel: false,
                scroll_read: false,
            };
            // `.topbar`: padding 8 and a 1px border at the bottom only, `gap: 4px 24px`, min-height 64
            let hdr = egui::TopBottomPanel::top("hdr")
                .min_height(64.0)
                .show_separator_line(false)
                .frame(egui::Frame::none().fill(t.surface).inner_margin(Margin { left: if narrow { 16.0 } else { 24.0 }, right: if narrow { 16.0 } else { 24.0 }, top: 8.0, bottom: 9.0 }))
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    header(ui, &mut cx, s)
                });
            let hr = hdr.response.rect;
            ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("hdr-line"))).hline(hr.x_range(), hr.bottom() - 0.5, Stroke::new(1.0, t.line));
            match s.view {
                View::Inbox => inbox(ctx, &mut cx, s),
                View::Review => review(ctx, &mut cx, s, narrow),
            }
            if let Some(m) = status_msg.as_ref() {
                toast(ctx, &mut cx, m, *status_err);
            }
        }
        (b, _) => {
            let (title, body) = match b {
                Boot::Loading => ("Opening your assignment", "Reading it from storage.".to_string()),
                Boot::Empty => ("No assignments yet", format!("Nothing is stored here ({}). Add or import an assignment to start reviewing.", storage)),
                Boot::Failed(m) => ("Couldn\u{2019}t open the assignment", m.clone()),
                Boot::Ready => ("No page selected", "Add pages and the transcript appears here.".to_string()),
            };
            egui::CentralPanel::default().frame(egui::Frame::none().fill(t.bg).inner_margin(Margin::same(40.0))).show(ctx, |ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                widgets::paint_icon(ui, ir, if matches!(b, Boot::Failed(_)) { Icon::Warn } else { Icon::Page }, t.muted);
                let r = ui.label(RichText::new(title).font(theme::serif_semi(32.0)).color(t.ink));
                ctl.heading("empty-title", 1, title, TEXT, r.rect.min);
                let r = ui.label(RichText::new(&body).font(theme::sans(16.0)).color(t.muted));
                ctl.status("empty-note", &body, TEXT, r.rect.min);
            });
        }
    }

    // a modal dialog draws last, over everything
    if let (Some(d), Some(s)) = (dialog.as_mut(), session.as_ref()) {
        if let Some(o) = dialog::show(ctx, &mut *ctl, t, s, d) {
            acts.push(Act::Dialog(o));
        }
    }

    if let Some(s) = session.as_ref() {
        route_keys(&*ctl, s, dialog.is_some(), &mut acts);
    }

    let send = |c: Cmd| {
        if let Some((d, _)) = host.as_ref() {
            d(c);
        }
    };
    let dialog_was = ctl.dialog_was;
    if let Some(s) = session.as_mut() {
        s.now = crate::now_ms();
        for a in acts {
            if !matches!(a, Act::ReadLine(_) | Act::ReadMove(_) | Act::ReadClear | Act::Zoom(_) | Act::ZoomSet(_) | Act::Dialog(_) | Act::CloseDialog) {
                *status_msg = None;
            }
            apply(a, s, ctx, ctl, typed, status_msg, status_err, dialog, &send);
        }
        // the typed fix belongs to one word
        if *typed_for != s.sel_word {
            typed.clear();
            *typed_for = s.sel_word.clone();
        }
        // focus goes into a dialog when it opens and back to what opened it when it closes
        let open_now = dialog.is_some();
        if open_now && !dialog_was {
            ctl.restore_focus = ctl.focused.as_ref().map(|(id, _)| id.clone()).or_else(|| ctl.focus.clone());
            if let Some(d) = dialog.as_ref() {
                ctl.want_focus = Some(d.first_focus(s));
            }
        } else if !open_now && dialog_was {
            if let Some(id) = ctl.restore_focus.take() {
                ctl.want_focus = Some(id);
            }
        }
        ctl.dialog_was = open_now;
    }
}

/// Keys go where focus is: Escape closes the dialog or the tray, arrows move the reading underline inside
/// the transcript, the strip takes its own set, +, - and 0 zoom while the page stage has focus.
fn route_keys(ctl: &Ctl, s: &Session, dialog_open: bool, acts: &mut Vec<Act>) {
    for KeyEv { key, alt } in &ctl.keys {
        if dialog_open {
            if key == "Escape" {
                acts.push(Act::CloseDialog);
            }
            continue;
        }
        if s.view != View::Review {
            continue;
        }
        let in_tray = ctl.focus_region() == Some(TRAY);
        let in_tx = ctl.focus_in("word-") || ctl.focus_in("line-") || ctl.focus_in("transcript");
        let in_stage = ctl.focus_in("stage");
        let strip_id = ctl.focused.as_ref().and_then(|(id, _)| id.strip_prefix("page-")).map(String::from);
        match key.as_str() {
            "Escape" if in_tray => acts.push(Act::Tray(false)),
            "Escape" if in_tx => {
                if s.selected.as_ref().is_some_and(|id| s.read_line.contains_key(id)) {
                    acts.push(Act::ReadClear);
                }
            }
            "ArrowDown" if in_tx => acts.push(Act::ReadMove(1)),
            "ArrowUp" if in_tx => acts.push(Act::ReadMove(-1)),
            "+" | "=" if in_stage => acts.push(Act::Zoom(1)),
            "-" if in_stage => acts.push(Act::Zoom(-1)),
            "0" if in_stage => acts.push(Act::ZoomSet(1.0)),
            k => {
                if let Some(id) = strip_id {
                    match (k, *alt) {
                        ("ArrowLeft", true) => acts.push(Act::MovePage(id, -1)),
                        ("ArrowRight", true) => acts.push(Act::MovePage(id, 1)),
                        ("ArrowLeft", false) => acts.push(Act::StripStep(id, -1)),
                        ("ArrowRight", false) => acts.push(Act::StripStep(id, 1)),
                        ("Home", _) => acts.push(Act::StripEdge(false)),
                        ("End", _) => acts.push(Act::StripEdge(true)),
                        ("Delete", _) => acts.push(Act::RemovePage(id)),
                        _ => {}
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply(
    a: Act,
    s: &mut Session,
    ctx: &egui::Context,
    ctl: &mut Ctl,
    typed: &mut String,
    status: &mut Option<String>,
    status_err: &mut bool,
    dialog: &mut Option<Dialog>,
    send: &dyn Fn(Cmd),
) {
    let slug = s.slug.clone();
    let mut say = |m: String, err: bool| {
        *status = Some(m);
        *status_err = err;
    };
    match a {
        Act::Pick(p, k) => s.pick_word(&p, &k),
        Act::SetSplit(v) => s.set_split(v),
        Act::Original(v) => s.original = v,
        Act::Student(d) => {
            s.step_student(d);
        }
        Act::DoneNext => {
            let name = s.group().map(|g| g.name);
            if !s.done_next() {
                say(format!("{} is marked done. That was the last student.", name.unwrap_or_default()), false);
                s.view = View::Inbox;
            }
        }
        Act::Copy => {
            if let Some(g) = s.group() {
                ctx.copy_text(s.copy_text(&g));
                say(format!("Copied {}\u{2019}s text.", g.name), false);
            }
        }
        Act::Boxes => s.boxes = s.boxes.next(),
        Act::StripClick { id, multi, shift } => {
            if multi || shift {
                s.multi_click(&id, shift);
            } else {
                s.select(&id);
            }
        }
        Act::StripStep(from, d) => {
            if let Some(t) = s.step_page(&from, d) {
                ctl.want_focus = Some(format!("page-{t}"));
            }
        }
        Act::StripEdge(last) => {
            if let Some(t) = s.edge_page(last) {
                ctl.want_focus = Some(format!("page-{t}"));
            }
        }
        Act::MovePage(id, d) => {
            if s.move_page(&id, d) {
                ctl.want_focus = Some(format!("page-{id}"));
            }
        }
        Act::Choose(via, text) => {
            if s.choose_open(via, &text) {
                typed.clear();
            }
        }
        Act::UseAll => s.use_all(),
        Act::StepWord(d) => s.step_word(d),
        Act::Tray(open) => {
            s.tray_open = open;
            if open {
                s.sync_tray();
            } else if let Some((p, k)) = s.sel_word.clone() {
                // focus returns to the word, as `close()` in review.js does
                ctl.want_focus = Some(format!("word-{p}-{k}"));
            }
        }
        Act::ReadLine(i) => s.set_read_line(Some(i)),
        Act::ReadMove(d) => {
            let n = s.current().map(|p| vishion_core::render::units(p.raw()).len()).unwrap_or(0);
            if n > 0 {
                let cur = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied();
                let next = match cur {
                    None => {
                        if d > 0 {
                            0
                        } else {
                            n - 1
                        }
                    }
                    Some(c) => (c as i64 + d as i64).clamp(0, n as i64 - 1) as usize,
                };
                s.set_read_line(Some(next));
            }
        }
        Act::ReadClear => s.set_read_line(None),
        Act::Zoom(d) => s.step_zoom(d),
        Act::ZoomSet(z) => s.set_zoom(z),
        Act::Rotate => {
            s.rotate_current();
        }
        Act::Level => {
            if let Some(id) = s.selected.clone() {
                send(Cmd::Straighten { slug, id });
            }
        }
        Act::Replace => {
            if let Some(id) = s.selected.clone() {
                send(Cmd::ReplaceImage { slug, id });
            }
        }
        Act::RemovePage(id) => match s.page(&id) {
            Some(p) if !p.raw().is_empty() => {
                *dialog = Some(Dialog::Confirm {
                    title: "Remove page".into(),
                    text: format!("Remove \u{201C}{}\u{201D}? Its transcript will be deleted too.", p.name()),
                    ok: "Remove page".into(),
                    danger: true,
                    then: After::RemovePage(id),
                });
            }
            Some(_) => s.remove_page(&id),
            None => {}
        },
        Act::AddPage => {
            if let Some(g) = s.group() {
                send(Cmd::AddPages { slug, student: g.name });
            }
        }
        Act::ChangeStudent => *dialog = Some(Dialog::change_student()),
        Act::TextView(raw) => s.text_raw = raw,
        Act::Transcribe(id, again) => {
            let touched = s.page(&id).is_some_and(|p| s.has_decisions(p));
            if again && touched {
                *dialog = Some(Dialog::Confirm {
                    title: "Transcribe again".into(),
                    text: "Reading this page again replaces the transcript and the choices you made on it.".into(),
                    ok: "Transcribe again".into(),
                    danger: false,
                    then: After::TranscribeAgain(id),
                });
            } else {
                send(Cmd::Transcribe { slug, ids: vec![id] });
            }
        }
        Act::Guess(id) => {
            if let Some(p) = s.page(&id) {
                send(Cmd::Guess { slug, id: id.clone(), raw: p.raw().to_string() });
            }
        }
        Act::OpenSettings => *dialog = Some(Dialog::Settings),
        Act::CloseDialog => *dialog = None,
        Act::Inbox => s.view = View::Inbox,
        Act::OpenReview(id) => {
            s.select(&id);
            s.view = View::Review;
        }
        Act::ExportSelected => {
            let list: Vec<&Page> = s.pages().into_iter().filter(|p| s.multi.contains(p.id())).collect();
            if !list.is_empty() {
                let sep = s.settings.get("exportSeparator").and_then(|v| v.as_str()).unwrap_or("label").to_string();
                let mode = if s.original { "raw" } else { "corrected" };
                let o = vishion_core::export::Opts::new(mode, &sep, "txt");
                if let Some(vishion_core::export::Export::File { name, bytes, .. }) =
                    vishion_core::export::run("combined", &list, &o, &s.a.meta, "")
                {
                    let n = list.len();
                    send(Cmd::Export { slug, name, bytes });
                    say(format!("Exporting {}\u{2026}", state::plural(n, "page", "pages")), false);
                }
            }
        }
        Act::ReviewAgain => {
            let ids: Vec<String> = s.multi.iter().cloned().collect();
            let pages: Vec<&Page> = ids.iter().filter_map(|i| s.page(i)).filter(|p| p.status() == "done").collect();
            if pages.is_empty() {
                say("Only pages with a transcript can be reviewed again.".into(), true);
            } else if pages.iter().any(|p| s.has_decisions(p) || p.reviewed()) {
                let n = pages.len();
                *dialog = Some(Dialog::Confirm {
                    title: "Review again".into(),
                    text: format!(
                        "Review {} again? Your choices on {} cleared and the words come back to check. The transcripts stay.",
                        state::plural(n, "page", "pages"),
                        if n == 1 { "it are" } else { "them are" }
                    ),
                    ok: "Review again".into(),
                    danger: false,
                    then: After::ReviewAgain(ids),
                });
            } else {
                s.review_again(&ids);
            }
        }
        Act::ClearMulti => s.multi.clear(),
        Act::Dialog(o) => {
            match o {
                dialog::Out::Close => {}
                dialog::Out::Student { name, all } => {
                    let n = s.set_student(&name, all);
                    say(format!("Moved {} to {}.", state::plural(n, "page", "pages"), name), false);
                }
                dialog::Out::Theme(v) => {
                    s.settings.set("theme", serde_json::json!(v));
                    s.dirty.settings = true;
                    // the dialog stays open so the change can be seen
                    return;
                }
                dialog::Out::Confirmed(then) => match then {
                    After::RemovePage(id) => s.remove_page(&id),
                    After::TranscribeAgain(id) => send(Cmd::Transcribe { slug, ids: vec![id] }),
                    After::ReviewAgain(ids) => {
                        let n = s.review_again(&ids);
                        say(format!("{} back in review.", state::plural(n, "page is", "pages are")), false);
                    }
                },
            }
            *dialog = None;
        }
    }
}

// ------------------------------------------------------------------ header

fn header(ui: &mut Ui, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    let g = s.group();
    let pos = s.student_pos();
    let any_text = g.as_ref().is_some_and(|g| g.pages.iter().filter_map(|id| s.page(id)).any(|p| !p.raw().is_empty()));
    let (i, n) = pos.unwrap_or((0, 0));
    let name = s.a.meta.name().to_string();
    let who = g.as_ref().map(|g| g.name.clone()).unwrap_or_else(|| "No student".into());
    let save_text = if let Some(m) = &cx.save.failed {
        format!("Couldn\u{2019}t save: {m}")
    } else if cx.save.pending > 0 {
        "Saving\u{2026}".to_string()
    } else {
        cx.storage.to_string()
    };
    let save_col = if cx.save.failed.is_some() { t.bad } else { t.muted };
    let crumb_label = format!("Back to {name}");

    // header.js `forInbox`: wordmark on the left, save state and Settings on the right
    if s.view == View::Inbox {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if widgets::brand(cx.ctl, ui, t, C::new("brand", "viSHion", HEADER)) {
                cx.acts.push(Act::Inbox);
            }
            let sg = ui.painter().layout_no_wrap(save_text.clone(), theme::sans(13.0), save_col);
            let w = sg.size().x + 24.0 + 12.0 + TARGET;
            ui.add_space((ui.available_width() - w - 4.0).max(0.0));
            let (sr, _) = ui.allocate_exact_size(vec2(sg.size().x + 24.0, TARGET), Sense::hover());
            let icon = if cx.save.failed.is_some() { Icon::Warn } else { Icon::Check };
            widgets::paint_icon(ui, Rect::from_center_size(pos2(sr.left() + 9.0, sr.center().y), vec2(18.0, 18.0)), icon, save_col);
            ui.painter().galley(pos2(sr.left() + 24.0, sr.center().y - sg.size().y / 2.0), sg, save_col);
            cx.ctl.status("save-state", &save_text, HEADER, sr.min);
            if widgets::icon_button(cx.ctl, ui, t, C::new("settings", "Settings", HEADER), Icon::Gear, false) {
                cx.acts.push(Act::OpenSettings);
            }
        });
        return;
    }

    // does one row hold everything? (design: flex-wrap with the right group pushed to the end)
    let brand_w = widgets::BRAND_W;
    let crumb_w = (tw(ui, &name, theme::sans_medium(14.0)).min(220.0) + 50.0).max(TARGET);
    let left_w = brand_w + 24.0 + crumb_w + 24.0 + 44.0 + 4.0 + 100.0 + 4.0 + 44.0;
    let right_w = tw(ui, &save_text, theme::sans(13.0))
        + 24.0
        + 12.0
        + (tw(ui, "Side by side", theme::sans_semi(14.0)) + tw(ui, "Text only", theme::sans_semi(14.0)) + 64.0)
        + 12.0
        + (tw(ui, "Copy text", theme::sans_semi(14.0)) + 36.0)
        + 12.0
        + (tw(ui, "Done, next student", theme::sans_semi(14.0)) + 36.0)
        + 12.0
        + 44.0;
    let one_row = left_w + 24.0 + right_w + 8.0 <= ui.available_width();

    // The left group: brand, crumb, student navigation.
    let left = |ui: &mut Ui, cx: &mut Cx| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if widgets::brand(cx.ctl, ui, t, C::new("brand", "viSHion", HEADER)) {
            cx.acts.push(Act::Inbox);
        }
        ui.add_space(if one_row { 16.0 } else { 4.0 });
        if widgets::crumb(cx.ctl, ui, t, C::new("crumb", &crumb_label, HEADER), &name, 220.0) {
            cx.acts.push(Act::Inbox);
        }
        ui.add_space(if one_row { 16.0 } else { 4.0 });
        // previous, name and position, next: one unit, so it wraps as a whole
        ui.allocate_ui_with_layout(vec2(44.0 + 4.0 + 100.0 + 4.0 + 44.0, TARGET), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::icon_button(cx.ctl, ui, t, C::new("prev-student", "Previous student", HEADER).disabled(pos.is_none() || i == 0), Icon::Left, false) {
                cx.acts.push(Act::Student(-1));
            }
            ui.allocate_ui_with_layout(vec2(100.0, TARGET), Layout::top_down(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(&who).font(theme::sans_semi(14.0)).color(t.ink));
                if pos.is_some() {
                    ui.label(RichText::new(format!("{} of {}", i + 1, n)).font(theme::sans(13.0)).color(t.muted));
                }
            });
            if widgets::icon_button(cx.ctl, ui, t, C::new("next-student", "Next student", HEADER).disabled(pos.is_none() || i + 1 >= n), Icon::Right, false) {
                cx.acts.push(Act::Student(1));
            }
        });
    };
    let right = |ui: &mut Ui, cx: &mut Cx, rtl: bool| {
        ui.spacing_mut().item_spacing.x = 12.0;
        // check icon + text (js `saveState`)
        let sg = ui.painter().layout_no_wrap(save_text.clone(), theme::sans(13.0), save_col);
        let (sr, _) = ui.allocate_exact_size(vec2(sg.size().x + 24.0, TARGET), Sense::hover());
        let icon = if cx.save.failed.is_some() { Icon::Warn } else { Icon::Check };
        widgets::paint_icon(ui, Rect::from_center_size(pos2(sr.left() + 9.0, sr.center().y), vec2(18.0, 18.0)), icon, save_col);
        ui.painter().galley(pos2(sr.left() + 24.0, sr.center().y - sg.size().y / 2.0), sg, save_col);
        cx.ctl.status("save-state", &save_text, HEADER, sr.min);
        let split = s.split;
        let items = [("layout-split", "Side by side", split, false), ("layout-text", "Text only", !split, false)];
        match widgets::seg(cx.ctl, ui, t, HEADER, "layout", items) {
            Some(0) => cx.acts.push(Act::SetSplit(true)),
            Some(_) => cx.acts.push(Act::SetSplit(false)),
            None => {}
        }
        if widgets::button(cx.ctl, ui, t, C::new("copy", "Copy text", HEADER).disabled(!any_text), "Copy text", Look::Outline, 0.0) {
            cx.acts.push(Act::Copy);
        }
        if widgets::button(cx.ctl, ui, t, C::new("done", "Done, next student", HEADER).disabled(g.is_none()), "Done, next student", Look::Primary, 0.0) {
            cx.acts.push(Act::DoneNext);
        }
        if widgets::icon_button(cx.ctl, ui, t, C::new("settings", "Settings", HEADER), Icon::Gear, false) {
            cx.acts.push(Act::OpenSettings);
        }
        let _ = rtl;
    };
    if one_row {
        ui.horizontal(|ui| {
            left(ui, cx);
            // `margin-left: auto` on the right group
            ui.add_space((ui.available_width() - right_w - 4.0).max(0.0));
            right(ui, cx, false);
        });
    } else {
        // like the design's flex-wrap: the right group drops to its own row, kept at the right edge when it fits
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
            left(ui, cx);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if right_w <= ui.available_width() {
                ui.add_space((ui.available_width() - right_w - 4.0).max(0.0));
            }
            right(ui, cx, false);
        });
    }
}

// ------------------------------------------------------------------ inbox (assignment overview)

fn inbox(ctx: &egui::Context, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    egui::CentralPanel::default().frame(egui::Frame::none().fill(t.bg)).show(ctx, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let inner = avail.min(1120.0);
            let pad = if cx.narrow { 16.0 } else { 24.0 };
            egui::Frame::none().inner_margin(Margin { left: (avail - inner) / 2.0 + pad, right: (avail - inner) / 2.0 + pad, top: 40.0, bottom: 40.0 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let name = s.a.meta.name();
                let r = ui.label(RichText::new(if name.is_empty() { "Assignment" } else { name }).font(theme::serif_semi(if cx.narrow { 30.0 } else { 36.0 })).color(t.ink));
                cx.ctl.heading("inbox-title", 1, if name.is_empty() { "Assignment" } else { name }, TEXT, r.rect.min);
                let groups = s.groups();
                let pages: usize = groups.iter().map(|g| g.pages.len()).sum();
                let sub = format!("{}, {}.", state::plural(groups.len(), "student", "students"), state::plural(pages, "page", "pages"));
                ui.label(RichText::new(sub).font(theme::sans(14.0)).color(t.muted));
                ui.add_space(24.0);
                for g in groups {
                    let (kind, label) = s.pill(&g);
                    let meta = if g.words > 0 { format!("{}, {} words", state::plural(g.pages.len(), "page", "pages"), g.words) } else { state::plural(g.pages.len(), "page", "pages") };
                    let w = ui.available_width();
                    let r = ui.allocate_response(vec2(w, 64.0), Sense::click());
                    let hover = r.hovered();
                    ui.painter().rect_filled(r.rect, 10.0, if hover { t.sunken } else { t.surface });
                    ui.painter().rect_stroke(r.rect, 10.0, Stroke::new(1.0, t.line));
                    let gn = ui.painter().layout_no_wrap(g.name.clone(), theme::sans_semi(16.0), t.ink);
                    let gm = ui.painter().layout_no_wrap(meta, theme::sans(13.0), t.muted);
                    let h = gn.size().y + gm.size().y;
                    let top = r.rect.center().y - h / 2.0;
                    ui.painter().galley(pos2(r.rect.left() + 16.0, top), gn.clone(), t.ink);
                    ui.painter().galley(pos2(r.rect.left() + 16.0, top + gn.size().y), gm, t.muted);
                    let pg = ui.painter().layout_no_wrap(label.clone(), theme::sans_semi(13.0), t.ink);
                    let pr = Rect::from_center_size(pos2(r.rect.right() - 16.0 - (pg.size().x + 24.0) / 2.0, r.rect.center().y), pg.size() + vec2(24.0, 10.0));
                    let (bg, fg) = match kind {
                        PillKind::Warn => (t.warn, t.ink),
                        PillKind::Ok => (t.ok_bg, t.ok),
                        PillKind::Bad => (t.bad_bg, t.bad),
                        PillKind::Busy => (t.hover, t.ink),
                        PillKind::Queued => (Color32::TRANSPARENT, t.muted),
                    };
                    ui.painter().rect_filled(pr, 999.0, bg);
                    if kind == PillKind::Queued {
                        ui.painter().rect_stroke(pr, 999.0, Stroke::new(1.0, t.line2));
                    }
                    ui.painter().galley(pr.center() - pg.size() / 2.0, pg, fg);
                    let first = g.pages.first().cloned().unwrap_or_default();
                    let lab = format!("Open {}, {}", g.name, label);
                    if cx.ctl.reg(ui, &r, C::new(&format!("row-{}", g.key), &lab, TEXT)) {
                        cx.acts.push(Act::OpenReview(first));
                    }
                    ui.add_space(8.0);
                }
                ui.add_space(12.0);
                ui.label(RichText::new("Adding pages in bulk, running transcription and exporting arrive with phase 5 and 6.").font(theme::sans(13.0)).color(t.muted));
            });
        });
    });
}

// ------------------------------------------------------------------ toast

fn toast(ctx: &egui::Context, cx: &mut Cx, msg: &str, err: bool) {
    let t = cx.t;
    let area = egui::Area::new(egui::Id::new("toast"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -20.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::none().fill(t.ink).rounding(8.0).inner_margin(Margin::symmetric(16.0, 12.0)).show(ui, |ui| {
                ui.set_max_width((ctx.screen_rect().width() - 64.0).max(160.0));
                ui.label(RichText::new(msg).font(theme::sans(14.0)).color(if err { Color32::from_rgb(0xFF, 0x9C, 0x92) } else { t.on_ink }));
            });
        });
    let r = area.response.rect;
    cx.ctl.status("toast", msg, TEXT, r.min);
}

// ------------------------------------------------------------------ review

fn review(ctx: &egui::Context, cx: &mut Cx, s: &Session, narrow: bool) {
    // the word being decided is never left behind the tray; the reading line follows the arrow keys
    // (review.js `scrollIntoView({ block: 'nearest' })`). A change starts a short run of frames, since the
    // tray and the page settle over a frame or two.
    let read = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied();
    let st = &mut *cx.stage;
    if s.sel_word.is_none() {
        st.sel_seen = None;
        st.sel_left = 0;
    } else if st.sel_seen != s.sel_word || st.tray_seen != s.tray_open {
        st.sel_seen = s.sel_word.clone();
        st.tray_seen = s.tray_open;
        st.sel_left = 6;
    }
    if read.is_none() {
        st.read_seen = None;
        st.read_left = 0;
    } else if st.read_seen != read {
        st.read_seen = read;
        st.read_left = 6;
    }
    cx.scroll_sel = cx.stage.sel_left > 0;
    cx.scroll_read = cx.stage.read_left > 0;
    review_panes(ctx, cx, s, narrow);
    let st = &mut *cx.stage;
    for left in [&mut st.sel_left, &mut st.read_left] {
        if *left > 0 {
            *left -= 1;
            ctx.request_repaint();
        }
    }
}

fn review_panes(ctx: &egui::Context, cx: &mut Cx, s: &Session, narrow: bool) {
    let t = cx.t;
    let g = s.group();
    let entries: Vec<Entry> = g.as_ref().map(|g| s.entries(g)).unwrap_or_default();
    let tray_on = !entries.is_empty() && s.tray_open;
    if narrow {
        if tray_on {
            egui::TopBottomPanel::bottom("tray")
                .resizable(false)
                .frame(egui::Frame::none().fill(t.sunken))
                .show(ctx, |ui| {
                    let cap = cx.screen_h * 0.4;
                    let out = egui::ScrollArea::vertical()
                        .max_height(cap)
                        .min_scrolled_height(cx.stage.tray_h.min(cap))
                        .auto_shrink([false, true])
                        .show(ui, |ui| tray(ui, cx, s, &entries));
                    cx.stage.tray_h = out.content_size.y;
                });
        }
        egui::CentralPanel::default().frame(egui::Frame::none().fill(t.surface)).show(ctx, |ui| {
            let out = egui::ScrollArea::vertical().auto_shrink([false, false]).id_source("review-scroll").show(ui, |ui| {
                if s.split {
                    egui::Frame::none().fill(t.scanbg).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        scan(ui, cx, s, true);
                    });
                    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.line));
                }
                text_column(ui, cx, s, g.as_ref(), &entries);
            });
            note_view_height(cx, out.inner_rect.height());
        });
    } else {
        if s.split {
            let w = (ctx.screen_rect().width() * 0.5).round();
            egui::SidePanel::left("scan").exact_width(w).resizable(false).frame(egui::Frame::none().fill(t.scanbg)).show(ctx, |ui| scan(ui, cx, s, false));
        }
        let panel = egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(t.surface).inner_margin(Margin { left: if s.split { 1.0 } else { 0.0 }, ..Margin::ZERO }))
            .show(ctx, |ui| {
                if tray_on {
                    egui::TopBottomPanel::bottom("tray")
                        .resizable(false)
                        .frame(egui::Frame::none().fill(t.sunken))
                        .show_inside(ui, |ui| tray(ui, cx, s, &entries));
                }
                let out = egui::ScrollArea::vertical().auto_shrink([false, false]).id_source("text-scroll").show(ui, |ui| {
                    text_column(ui, cx, s, g.as_ref(), &entries);
                });
                note_view_height(cx, out.inner_rect.height());
            });
        if s.split {
            // `.text { border-left: 1px solid var(--line) }`
            let pr = panel.response.rect;
            ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("tx-border")))
                .vline(pr.left() + 0.5, pr.y_range(), Stroke::new(1.0, t.line));
        }
    }
}

/// The tray changed height (its crop arrived, the options wrapped, a window resize): keep the word in view.
fn note_view_height(cx: &mut Cx, h: f32) {
    if cx.stage.text_h > 0.0 && (cx.stage.text_h - h).abs() > 0.5 {
        cx.stage.sel_left = cx.stage.sel_left.max(4);
    }
    cx.stage.text_h = h;
}

/// `.tx-inner`: at most 760 wide, centred, 40 (20 on a phone) of padding.
fn text_column(ui: &mut Ui, cx: &mut Cx, s: &Session, g: Option<&vishion_core::groups::Group>, entries: &[Entry]) {
    let avail = ui.available_width();
    let inner = avail.min(760.0);
    let pad = if cx.narrow { 20.0 } else { 40.0 };
    let side = (avail - inner) / 2.0 + pad;
    egui::Frame::none()
        .inner_margin(Margin { left: side, right: side, top: if cx.narrow { 24.0 } else { 32.0 }, bottom: if cx.narrow { 20.0 } else { 24.0 } })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            transcript_pane(ui, cx, s, g, entries);
        });
}

// ------------------------------------------------------------------ page pane

fn dot_color(t: &Tokens, p: &Page) -> Color32 {
    if p.status() == "error" {
        t.bad
    } else if p.status() != "done" {
        t.faint
    } else if words::count(p) > 0 {
        t.bar_warn
    } else {
        Color32::from_rgb(0x2E, 0x9A, 0x66)
    }
}

fn scan(ui: &mut Ui, cx: &mut Cx, s: &Session, stacked: bool) {
    let t = cx.t;
    let g = s.group();
    let cur = s.current();
    let page_i = g.as_ref().and_then(|g| g.pages.iter().position(|id| Some(id) == s.selected.as_ref()));
    let busy = cur.is_some_and(|p| p.status() == "transcribing");

    ui.spacing_mut().item_spacing.y = 0.0;
    // title + tools: `.scan-head` is `padding: 4px 12px 4px 20px; justify-content: space-between; gap: 4px 8px`
    // with a 1px border below. The tools sit at the right edge when they fit beside the title.
    let head = egui::Frame::none().fill(t.scanhead).inner_margin(Margin { left: 20.0, right: 12.0, top: 4.0, bottom: 4.0 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let title = match (page_i, &g) {
            (Some(i), Some(g)) => format!("Page {} of {}", i + 1, g.pages.len()),
            _ => String::new(),
        };
        let tg = ui.painter().layout_no_wrap(title.clone(), theme::sans_semi(14.0), t.ink);
        let tools_w = 44.0 * 7.0 + 56.0 + 7.0 * 2.0;
        let one_row = tg.size().x + 8.0 + tools_w <= ui.available_width();
        let title_ui = |ui: &mut Ui, cx: &mut Cx| {
            let r = ui.allocate_response(vec2(tg.size().x.max(1.0), TARGET), Sense::hover());
            ui.painter().text(pos2(r.rect.left(), r.rect.center().y), egui::Align2::LEFT_CENTER, &title, theme::sans_semi(14.0), t.ink);
            cx.ctl.heading("scan-title", 2, &title, SCAN, r.rect.min);
        };
        let has = cur.is_some();
        let z = s.zoom;
        let tools = |ui: &mut Ui, cx: &mut Cx| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::icon_button(cx.ctl, ui, t, C::new("zoom-out", "Zoom out", SCAN).disabled(!has || z <= state::ZOOMS[0]), Icon::ZoomOut, false) {
                cx.acts.push(Act::Zoom(-1));
            }
            let zl = format!("{}%", (z * 100.0).round() as i32);
            if widgets::button_pad(cx.ctl, ui, t, C::new("zoom-fit", "Fit the page to the pane", SCAN).disabled(!has), &zl, Look::Ghost, 56.0, 12.0) {
                cx.acts.push(Act::ZoomSet(1.0));
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("zoom-in", "Zoom in", SCAN).disabled(!has || z >= state::ZOOMS[state::ZOOMS.len() - 1]), Icon::ZoomIn, false) {
                cx.acts.push(Act::Zoom(1));
            }
            let label = match s.boxes {
                BoxMode::Off => "Marks on the page: off. Click to change.",
                BoxMode::Flagged => "Marks on the page: underlines. Click to change.",
                BoxMode::All => "Marks on the page: underlines and a box on every line. Click to change.",
            };
            // aria-pressed is true for both "underlines" and "all lines", but the button is not drawn
            // pressed: review.js only sets the attribute
            if widgets::icon_button(cx.ctl, ui, t, C::new("boxes", label, SCAN).pressed(s.boxes != BoxMode::Off).disabled(!has), Icon::Boxes, false) {
                cx.acts.push(Act::Boxes);
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("level", "Straighten this page and redo the line boxes", SCAN).disabled(!has || busy), Icon::Level, false) {
                cx.acts.push(Act::Level);
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("rotate", "Rotate page 90 degrees", SCAN).disabled(!has || busy), Icon::Rotate, false) {
                cx.acts.push(Act::Rotate);
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("replace", "Replace this page", SCAN).disabled(!has), Icon::Swap, false) {
                cx.acts.push(Act::Replace);
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("remove", "Remove this page", SCAN).disabled(!has), Icon::Trash, false) {
                if let Some(id) = &s.selected {
                    cx.acts.push(Act::RemovePage(id.clone()));
                }
            }
        };
        if one_row {
            ui.horizontal(|ui| {
                title_ui(ui, cx);
                let w = ui.available_width() - tools_w;
                ui.add_space(w.max(0.0));
                tools(ui, cx);
            });
        } else {
            ui.horizontal(|ui| title_ui(ui, cx));
            ui.add_space(4.0);
            // the pane is narrower than the tools: they wrap inside it instead of leaving the screen
            ui.horizontal_wrapped(|ui| tools(ui, cx));
        }
    });
    ui.painter().hline(head.response.rect.x_range(), head.response.rect.bottom() + 0.5, Stroke::new(1.0, t.scanline));
    ui.add_space(1.0);

    // strip of this student's pages: `.scan-pages` padding 10 20, gap 8, 1px border below
    let mut rects: Vec<(String, Rect)> = vec![];
    let mut dragged: Option<String> = None;
    let strip = egui::Frame::none().fill(t.scanhead).inner_margin(Margin::symmetric(20.0, 10.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let Some(g) = &g else { return };
            for (i, id) in g.pages.iter().enumerate() {
                let Some(p) = s.page(id) else { continue };
                let on = Some(id) == s.selected.as_ref();
                let n = words::count(p);
                let label = format!(
                    "Page {}{}",
                    i + 1,
                    if p.status() == "error" {
                        ", could not transcribe".to_string()
                    } else if p.status() != "done" {
                        ", not transcribed yet".to_string()
                    } else if n > 0 {
                        format!(", {} to check", state::plural(n, "word", "words"))
                    } else {
                        ", all words checked".to_string()
                    }
                );
                let r = ui.allocate_response(vec2(TARGET, 56.0), Sense::click_and_drag());
                rects.push((id.clone(), r.rect));
                if r.drag_stopped() {
                    dragged = Some(id.clone());
                }
                let rect = r.rect;
                // `box-shadow: 0 1px 2px rgba(20, 27, 45, .3)` under the paper
                ui.painter().rect_filled(rect.translate(vec2(0.0, 1.0)).expand(0.5), 2.0, Color32::from_black_alpha(if t.dark { 90 } else { 40 }));
                ui.painter().rect_filled(rect, 2.0, t.paper);
                if let Some(Slot::Ready(im)) = cx.images.get(id) {
                    ui.painter().image(im.thumb.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                let num = ui.painter().layout_no_wrap((i + 1).to_string(), theme::sans_semi(12.0), t.paper_ink);
                let np = rect.left_bottom() + vec2(4.0, -2.0 - num.size().y);
                ui.painter().rect_filled(Rect::from_min_size(np - vec2(3.0, 0.0), num.size() + vec2(6.0, 0.0)), 3.0, t.paper);
                ui.painter().galley(np, num, t.paper_ink);
                let dc = rect.right_top() + vec2(-2.0, 2.0);
                ui.painter().circle(dc, 6.0, dot_color(t, p), Stroke::new(2.0, t.scanhead));
                if on {
                    ui.painter().rect_stroke(rect.expand(2.0), 3.0, Stroke::new(2.0, t.ink));
                }
                if s.multi.contains(id) {
                    ui.painter().rect_stroke(rect.expand(4.0), 4.0, Stroke::new(3.0, t.accent));
                }
                if cx.ctl.reg(ui, &r, C::new(&format!("page-{id}"), &label, SCAN).pressed(on)) {
                    let m = ui.input(|i| i.modifiers);
                    cx.acts.push(Act::StripClick { id: id.clone(), multi: m.command || m.ctrl, shift: m.shift });
                }
            }
            if !s.multi.is_empty() {
                let n = s.multi.len();
                let l = format!("Export {n} selected");
                if widgets::button(cx.ctl, ui, t, C::new("strip-export", &l, SCAN), &l, Look::Outline, 0.0) {
                    cx.acts.push(Act::ExportSelected);
                }
                if widgets::button(cx.ctl, ui, t, C::new("strip-review-again", "Review again", SCAN), "Review again", Look::Outline, 0.0) {
                    cx.acts.push(Act::ReviewAgain);
                }
                if widgets::button(cx.ctl, ui, t, C::new("strip-clear", "Clear", SCAN), "Clear", Look::Ghost, 0.0) {
                    cx.acts.push(Act::ClearMulti);
                }
            }
            let l = format!("Add a page for {}", g.name);
            if widgets::icon_text_button(cx.ctl, ui, t, C::new("add-page", &l, SCAN), Icon::Plus, "Add page", Look::Ghost) {
                cx.acts.push(Act::AddPage);
            }
        });
    });
    ui.painter().hline(strip.response.rect.x_range(), strip.response.rect.bottom() + 0.5, Stroke::new(1.0, t.scanline));
    ui.add_space(1.0);
    if let (Some(from), Some(pos)) = (dragged, ui.input(|i| i.pointer.interact_pos())) {
        if let (Some(g), Some((to, _))) = (&g, rects.iter().find(|(_, r)| r.contains(pos))) {
            let a = g.pages.iter().position(|x| *x == from);
            let b = g.pages.iter().position(|x| x == to);
            if let (Some(a), Some(b)) = (a, b) {
                if a != b {
                    cx.acts.push(Act::MovePage(from, b as i32 - a as i32));
                }
            }
        }
    }

    // the page
    let pad = if cx.narrow { 16.0 } else { 32.0 };
    let max_h = if stacked { Some(cx.screen_h * 0.8) } else { None };
    let mut area = egui::ScrollArea::both().id_source("stage").auto_shrink([false, stacked]);
    if let Some(h) = max_h {
        area = area.max_height(h);
    }
    // keep the middle of the view where it was while the zoom changes
    if cx.stage.zoom == 0.0 {
        cx.stage.zoom = s.zoom;
    }
    if (cx.stage.zoom - s.zoom).abs() > 0.001 {
        let ratio = s.zoom / cx.stage.zoom;
        let centre = cx.stage.off + cx.stage.view / 2.0;
        let off = (centre * ratio - cx.stage.view / 2.0).max(Vec2::ZERO);
        cx.stage.target = Some((off, 4));
        cx.stage.zoom = s.zoom;
    }
    if let Some((off, left)) = cx.stage.target {
        area = area.scroll_offset(off);
        cx.stage.target = if left > 1 { Some((off, left - 1)) } else { None };
        ui.ctx().request_repaint();
    }
    let out = area.show(ui, |ui| {
        ui.add_space(pad);
        let Some(p) = cur else {
            ui.vertical_centered(|ui| ui.label(RichText::new("No page selected.").color(t.ink)));
            return;
        };
        match cx.images.get(p.id()) {
            Some(Slot::Ready(im)) => page_image(ui, cx, s, p, im, pad),
            Some(Slot::Missing) => {
                ui.vertical_centered(|ui| ui.label(RichText::new("The image of this page is missing.").color(t.ink)));
            }
            _ => {
                ui.vertical_centered(|ui| ui.label(RichText::new("Loading the page\u{2026}").color(t.ink)));
            }
        }
        ui.add_space(pad + 24.0);
    });
    cx.stage.off = out.state.offset;
    cx.stage.view = out.inner_rect.size();
    // keys, wheel and double click on the stage
    let stage_rect = out.inner_rect;
    let r = ui.interact(stage_rect, ui.id().with("stage-keys"), Sense::focusable_noninteractive());
    cx.ctl.group(ui, &r, "stage", "Page image. Press plus or minus to zoom, zero to fit.", "stage", SCAN);
    let dbl = ui.interact(stage_rect, ui.id().with("stage-dbl"), Sense::click());
    if dbl.double_clicked() {
        cx.acts.push(Act::ZoomSet(if s.zoom > 1.0 { 1.0 } else { 2.0 }));
    }
    if dbl.hovered() {
        let zd = ui.ctx().input(|i| i.zoom_delta());
        if zd > 1.01 {
            cx.acts.push(Act::Zoom(1));
        } else if zd < 0.99 {
            cx.acts.push(Act::Zoom(-1));
        }
    }
}

fn page_image(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page, im: &Img, pad: f32) {
    let t = cx.t;
    let avail = ui.available_width();
    // `.scan-sheet { width: zoom * 100% }` of the stage's content width
    let w = ((avail - 2.0 * pad).max(120.0) * s.zoom).max(120.0);
    let h = w * im.size.1 as f32 / im.size.0.max(1) as f32;
    let (outer, _) = ui.allocate_exact_size(vec2(w.max(avail - 2.0 * pad) + 2.0 * pad, h), Sense::hover());
    let rect = Rect::from_min_size(pos2(outer.left() + pad + ((outer.width() - 2.0 * pad - w) / 2.0).max(0.0), outer.top()), vec2(w, h));
    let pt = ui.painter();
    // soft shadow: two translucent layers instead of a blur
    pt.rect_filled(rect.translate(vec2(0.0, 10.0)).expand(2.0), 6.0, Color32::from_black_alpha(26));
    pt.rect_filled(rect.translate(vec2(0.0, 2.0)), 3.0, Color32::from_black_alpha(46));
    pt.image(im.full.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);

    if s.boxes == BoxMode::Off {
        return;
    }
    let n = vishion_core::render::units(p.raw()).len();
    let Some((boxes, approx)) = state::geometry(p, n, &im.bands) else { return };
    let (kinds, sel_unit) = s.flagged_lines(p);
    let at = |b: &state::BBox| Rect::from_min_size(rect.min + vec2(b.x * w, b.y * h), vec2(b.w * w, b.h * h));
    if s.boxes == BoxMode::All && !approx {
        for b in &boxes {
            let r = at(b);
            pt.rect_filled(r, 2.0, t.accent.gamma_multiply(0.10));
            pt.rect_stroke(r, 2.0, Stroke::new(1.0, t.accent.gamma_multiply(0.55)));
        }
    }
    let read = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied();
    for (i, kind) in kinds {
        let Some(b) = boxes.get(i) else { continue };
        let r = at(b);
        let sel = sel_unit == Some(i);
        let thick = if sel { 6.0 } else { 4.0 };
        let y = r.bottom() + if read == Some(i) { 9.0 } else { 1.0 };
        let (x0, x1) = (r.left(), r.right());
        match kind {
            Kind::Warn => {
                pt.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1, y + thick)), 2.0, t.bar_warn);
                pt.rect_stroke(Rect::from_min_max(pos2(x0, y), pos2(x1, y + thick)), 2.0, Stroke::new(1.0, Color32::from_rgb(0xD9, 0xA4, 0x00)));
            }
            Kind::Bad => {
                pt.add(egui::Shape::dashed_line(&[pos2(x0, y + 2.0), pos2(x1, y + 2.0)], Stroke::new(thick, t.bad), 8.0, 5.0));
            }
            Kind::Ok => {
                pt.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1, y + thick)), 2.0, t.bar_ok);
            }
        }
    }
    if let Some(i) = read {
        if let Some(b) = boxes.get(i) {
            let r = at(b);
            let y = r.bottom() + 1.0;
            pt.rect_filled(Rect::from_min_max(pos2(r.left(), y), pos2(r.right(), y + 4.0)), 2.0, t.accent);
        }
    }
    // the selected mark (or the reading line) is scrolled into view, as `drawBoxes` does
    let now = (sel_unit, read);
    if cx.stage.mark_seen != Some(now) {
        let target = sel_unit.or(read).and_then(|i| boxes.get(i)).map(|b| at(b));
        if let Some(r) = target {
            ui.scroll_to_rect(Rect::from_min_max(r.min, pos2(r.max.x, r.max.y + 14.0)), None);
        }
        cx.stage.mark_seen = Some(now);
    }
}

// ------------------------------------------------------------------ transcript pane

fn transcript_pane(ui: &mut Ui, cx: &mut Cx, s: &Session, g: Option<&vishion_core::groups::Group>, entries: &[Entry]) {
    let t = cx.t;
    let (Some(p), Some(g)) = (s.current(), g) else {
        state_block(ui, cx, "No page selected", "Add pages and the transcript appears here.", None);
        return;
    };
    if let Some(n) = cx.notice {
        egui::Frame::none().fill(t.warn_bg).stroke(Stroke::new(1.0, t.warn)).rounding(8.0).inner_margin(Margin::same(12.0)).show(ui, |ui| {
            ui.label(RichText::new(n).color(t.ink));
        });
        ui.add_space(12.0);
    }
    // who + status
    let (kind, label) = s.pill(g);
    let meta = {
        let base = if g.words > 0 {
            format!("{}, {} words", state::plural(g.pages.len(), "page", "pages"), g.words)
        } else {
            state::plural(g.pages.len(), "page", "pages")
        };
        if g.source.as_ref().and_then(|v| v.as_str()) == Some("file") {
            format!("{base}. No name was found on the page.")
        } else {
            base
        }
    };
    // `.who-head { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 12px }`: the name block
    // (h1 32/1.15, p 14/1.4 with 6 above, "Change student" with 4 above and -16 to the left) and the pill
    // (6 14 padding, 31.6 high) share a row when both fit, else the pill goes below.
    ui.spacing_mut().item_spacing.y = 0.0;
    let pill_w = tw(ui, &label, theme::sans_semi(14.0)) + 28.0;
    let avail = ui.available_width();
    let change_w = tw(ui, "Change student", theme::sans_semi(14.0)) + 34.0 - 16.0;
    let block_w = tw(ui, &g.name, theme::serif_semi(32.0)).max(tw(ui, &meta, theme::sans(14.0))).max(change_w).ceil();
    let same_row = block_w + 12.0 + pill_w <= avail;
    let wrap_w = if same_row { block_w } else { avail };
    let top = ui.cursor().min;
    let name_r = css_text(ui, &g.name, theme::serif_semi(32.0), t.ink, 32.0 * 1.15, wrap_w, true);
    cx.ctl.heading("student-name", 1, &g.name, TEXT, name_r.min);
    ui.add_space(6.0);
    css_text(ui, &meta, theme::sans(14.0), t.muted, 14.0 * 1.4, wrap_w, false);
    ui.add_space(4.0);
    let at = ui.cursor().min + vec2(-16.0, 0.0);
    let clicked = widgets::button_at(cx.ctl, ui, t, C::new("change-student", "Change student", TEXT), "Change student", Look::Ghost, at);
    ui.allocate_space(vec2(1.0, TARGET));
    if clicked {
        cx.acts.push(Act::ChangeStudent);
    }
    let pill_min = if same_row {
        pos2(top.x + avail - pill_w, top.y)
    } else {
        ui.add_space(12.0);
        let p = ui.cursor().min;
        ui.allocate_space(vec2(1.0, widgets::PILL_H));
        p
    };
    let r = widgets::pill_at(ui, t, kind, &label, pill_min);
    cx.ctl.status("student-status", &label, TEXT, r.min);

    // legend + version: `.legend-row { margin-top: 18px; gap: 10px 20px }`
    ui.add_space(18.0);
    // rows are as tall as their content (the legend is one 13px line; the toggle is 44)
    let row_min = std::mem::replace(&mut ui.spacing_mut().interact_size.y, 13.0 * 1.4);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(20.0, 10.0);
        legend(ui, t);
        let same = s.versions_same();
        let orig = s.original && !same;
        let items = [("ver-corrected", "Corrected", !orig, false), ("ver-original", "Original", orig, same)];
        match widgets::seg(cx.ctl, ui, t, TEXT, "version", items) {
            Some(0) => cx.acts.push(Act::Original(false)),
            Some(_) => cx.acts.push(Act::Original(true)),
            None => {}
        }
    });
    ui.spacing_mut().interact_size.y = row_min;

    // body by page state
    match p.status() {
        "transcribing" => {
            ui.add_space(18.0);
            ui.label(RichText::new(p.raw()).font(theme::serif(TEXT_PX)).color(t.ink));
            ui.add_space(8.0);
            ui.label(RichText::new("Reading this page. The text appears as it is read.").font(theme::sans(13.0)).color(t.muted));
        }
        "pending" if p.raw().is_empty() => {
            let id = p.id().to_string();
            state_block(
                ui,
                cx,
                "This page has no transcript yet",
                "Transcribe this page now, or run every page from the assignment inbox.",
                Some(("transcribe-page", "Transcribe this page", Act::Transcribe(id, false))),
            );
        }
        _ => {
            if p.status() == "error" {
                let id = p.id().to_string();
                ui.add_space(24.0);
                egui::Frame::none().fill(t.bad_bg).stroke(Stroke::new(1.0, t.bad)).rounding(10.0).inner_margin(Margin::symmetric(18.0, 16.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let msg = if p.s("error").is_empty() { "The page could not be transcribed." } else { p.s("error") };
                    ui.horizontal_top(|ui| {
                        let (ir, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        widgets::paint_icon(ui, ir, Icon::Warn, t.bad);
                        let r = ui.add(egui::Label::new(RichText::new(msg).color(t.ink)).wrap());
                        cx.ctl.status("page-error", msg, TEXT, r.rect.min);
                    });
                    ui.add_space(12.0);
                    if widgets::button(cx.ctl, ui, t, C::new("retry-page", "Retry this page", TEXT), "Retry this page", Look::Outline, 0.0) {
                        cx.acts.push(Act::Transcribe(id, false));
                    }
                });
            }
            if !p.raw().is_empty() {
                ui.add_space(24.0);
                if s.text_raw {
                    plain_text(ui, cx, s, p);
                } else {
                    lines(ui, cx, s, p);
                }
                foot(ui, cx, s, p, !entries.is_empty());
            }
        }
    }
}

fn state_block(ui: &mut Ui, cx: &mut Cx, title: &str, text: &str, button: Option<(&str, &str, Act)>) {
    let t = cx.t;
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        let (ir, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        widgets::paint_icon(ui, ir, Icon::Page, t.muted);
        ui.add_space(6.0);
        let r = ui.label(RichText::new(title).font(theme::sans_semi(18.0)).color(t.ink));
        cx.ctl.heading("state-title", 3, title, TEXT, r.rect.min);
        let r = ui.add(egui::Label::new(RichText::new(text).color(t.muted)).wrap());
        cx.ctl.text("state-text", text, TEXT, r.rect.min);
        if let Some((id, label, act)) = button {
            ui.add_space(8.0);
            if widgets::button(cx.ctl, ui, t, C::new(id, label, TEXT), label, Look::Primary, 0.0) {
                cx.acts.push(act);
            }
        }
        ui.add_space(24.0);
    });
}

fn legend(ui: &mut Ui, t: &Tokens) {
    // one allocation per swatch (a nested `horizontal` inside a wrapping row cannot wrap and widens the pane)
    let sw = |ui: &mut Ui, label: &str, kind: Kind| {
        let g = ui.painter().layout_no_wrap("word".into(), theme::sans(13.0), if kind == Kind::Bad { t.bad } else { t.ink });
        let l = ui.painter().layout_no_wrap(label.to_string(), theme::sans(13.0), t.muted);
        let sw_w = g.size().x + 12.0;
        // `.legend span` is a flex row: the swatch is blockified, one 13px line at 1.4 high
        let (rect, _) = ui.allocate_exact_size(vec2(sw_w + 8.0 + l.size().x, 13.0 * 1.4), Sense::hover());
        let r = Rect::from_min_size(rect.min, vec2(sw_w, rect.height()));
        match kind {
            Kind::Warn => {
                ui.painter().rect_filled(r, 4.0, t.warn);
            }
            Kind::Bad => {
                ui.painter().rect_filled(r, 4.0, t.bad_bg);
                ui.painter().add(egui::Shape::dashed_line(&[pos2(r.left() + 4.0, r.bottom() - 1.0), pos2(r.right() - 4.0, r.bottom() - 1.0)], Stroke::new(2.0, t.bad), 4.0, 2.0));
            }
            Kind::Ok => {
                ui.painter().line_segment([pos2(r.left() + 4.0, r.bottom() - 1.0), pos2(r.right() - 4.0, r.bottom() - 1.0)], Stroke::new(2.0, t.ok));
            }
        }
        ui.painter().galley(r.center() - g.size() / 2.0, g, t.ink);
        ui.painter().galley(pos2(r.right() + 8.0, rect.center().y - l.size().y / 2.0), l, t.muted);
    };
    sw(ui, "Spelling to check", Kind::Warn);
    sw(ui, "Couldn\u{2019}t read", Kind::Bad);
    sw(ui, "You checked it", Kind::Ok);
}

/// `textView === 'raw'`: the transcript as plain monospace text.
fn plain_text(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page) {
    let t = cx.t;
    let text = s.plain_text(p);
    // `.tx-text.plain { white-space: pre-wrap; overflow-wrap: anywhere; font: 13px/1.65 mono }`
    let w = ui.available_width();
    let r = css_text(ui, &text, egui::FontId::new(13.0, egui::FontFamily::Monospace), t.ink, 13.0 * 1.65, w, true);
    cx.ctl.text(&format!("plain-{}", p.id()), &text, TEXT, r.min);
    transcript_group(ui, cx, r);
}

/// The transcript as one focusable region: arrow keys inside it move the reading underline.
fn transcript_group(ui: &mut Ui, cx: &mut Cx, block: Rect) {
    let block = Rect::from_min_size(block.min, vec2(block.width(), block.height().max(TARGET)));
    let r = ui.interact(block, ui.id().with("transcript-keys"), Sense::focusable_noninteractive());
    cx.ctl.group(
        ui,
        &r,
        "transcript",
        "Transcript. Use the up and down arrow keys to move the underline to the line you are reading.",
        "transcript",
        TEXT,
    );
}

// ---- inline layout of transcript lines

enum Atom {
    Text { g: std::sync::Arc<egui::Galley>, color: Color32, strike: bool, code: bool },
    Math { tex: String, display: bool },
    Chip { id: String, label: String, g: std::sync::Arc<egui::Galley>, kind: Kind, checked: bool, selected: bool, key: String },
}

fn split_tokens(text: &str) -> Vec<(String, usize)> {
    // (word, trailing spaces): "so  it " -> [("so",2), ("it",1)]; leading spaces give ("", n)
    let mut out: Vec<(String, usize)> = vec![];
    let mut cur = String::new();
    let mut sp = 0usize;
    for c in text.chars() {
        if c.is_whitespace() {
            sp += 1;
        } else {
            if sp > 0 || (!cur.is_empty() && false) {
                out.push((std::mem::take(&mut cur), sp));
                sp = 0;
            }
            cur.push(c);
        }
    }
    if !cur.is_empty() || sp > 0 {
        out.push((cur, sp));
    }
    out
}

/// The transcript: one block per display unit, math typeset, flagged words as controls. Units are
/// contiguous (`.ln` padding is 1px), `Margin:` units get 14px above.
fn lines(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page) {
    let marked = s.marked(p);
    let ls = transcript::lines(&marked);
    let items = words::items(p);
    let read = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied();
    let sel_key = s.sel_word.as_ref().filter(|(id, _)| id == p.id()).map(|(_, k)| k.clone());
    let width = ui.available_width().min(TEXT_MAX_W);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let block_top = ui.cursor().min;
    let mut block_h = 0.0;
    for line in &ls {
        if line.margin {
            ui.add_space(14.0);
            block_h += 14.0;
        }
        let rect = unit(ui, cx, s, p, line, &items, sel_key.as_deref(), width, read == Some(line.index));
        block_h += rect.height();
    }
    transcript_group(ui, cx, Rect::from_min_size(block_top, vec2(width, block_h)));
}

#[allow(clippy::too_many_arguments)]
fn unit(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page, line: &Line, items: &[words::Item], sel_key: Option<&str>, width: f32, reading: bool) -> Rect {
    let t = cx.t;
    let id = format!("line-{}-{}", p.id(), line.index);
    let small = line.margin;
    let px = if small { 16.0 } else { TEXT_PX };
    let pitch = if small { 16.0 * 1.5 } else { PITCH };
    cx.ctl.begin_para();

    // display math alone on its line: a centred block
    if let Some(m) = line.display_only() {
        let size = cx.math.get(&m.tex, true, px * KATEX_EM, t.ink).map(|r| r.size);
        let h = size.map_or(PITCH, |sz| sz.y + 14.0) + 2.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
        cx.ctl.para_math(&m.tex);
        match size {
            Some(sz) => {
                let at = pos2(rect.center().x - sz.x / 2.0, rect.top() + 1.0 + 7.0);
                paint_math(ui, cx, &m.tex, true, px * KATEX_EM, at);
            }
            None => {
                let col = if cx.math.pending() { t.muted } else { t.bad };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, &m.tex, egui::FontId::new(px * 0.85, egui::FontFamily::Monospace), col);
            }
        }
        click_line(ui, cx, &id, rect, line.index);
        if reading {
            ui.painter().rect_filled(Rect::from_min_size(pos2(rect.left(), rect.bottom() - 4.0), vec2(rect.width(), 3.0)), 1.0, t.accent);
            if cx.scroll_read {
                ui.scroll_to_rect(rect, None);
            }
        }
        cx.ctl.end_para(&id, TEXT, rect.min, true, reading);
        return rect;
    }

    // atoms
    let mut atoms: Vec<Atom> = vec![];
    let mut it: Vec<flow::Item> = vec![];
    let space_w = ui.painter().layout_no_wrap(" ".into(), if small { theme::sans(px) } else { theme::serif(px) }, t.ink).size().x;
    let push_text = |ui: &Ui, cx: &mut Cx, atoms: &mut Vec<Atom>, it: &mut Vec<flow::Item>, txt: &str, st: &Style, force_ink: bool| {
        let font = if small {
            theme::sans(px)
        } else if st.bold {
            theme::serif_semi(px)
        } else if st.italic {
            theme::serif_italic(px)
        } else if st.code {
            egui::FontId::new(px * 0.85, egui::FontFamily::Monospace)
        } else {
            theme::serif(px)
        };
        let color = if st.strike || small { t.muted } else { t.ink };
        let color = if force_ink { t.ink } else { color };
        cx.ctl.para_text(txt);
        for (word, spaces) in split_tokens(txt) {
            let mut pieces: Vec<String> = vec![];
            // an overlong token (no spaces, e.g. a URL) breaks by characters so it never leaves the column
            let g = ui.painter().layout_no_wrap(word.clone(), font.clone(), color);
            if g.size().x > width && word.chars().count() > 1 {
                let mut cur = String::new();
                for c in word.chars() {
                    let mut probe = cur.clone();
                    probe.push(c);
                    if !cur.is_empty() && ui.painter().layout_no_wrap(probe, font.clone(), color).size().x > width {
                        pieces.push(std::mem::take(&mut cur));
                    }
                    cur.push(c);
                }
                if !cur.is_empty() {
                    pieces.push(cur);
                }
            } else {
                pieces.push(word.clone());
            }
            let n = pieces.len();
            for (k, piece) in pieces.into_iter().enumerate() {
                let g = ui.painter().layout_no_wrap(piece, font.clone(), color);
                let last = k + 1 == n;
                let trail = if last { spaces as f32 * space_w } else { 0.0 };
                let breakable = !last || spaces > 0;
                it.push(flow::Item { w: g.size().x, h: g.size().y, trail, brk: breakable || !last });
                atoms.push(Atom::Text { g, color, strike: st.strike, code: st.code });
            }
        }
    };
    if line.margin {
        let st = Style { bold: true, italic: false, strike: false, code: false };
        let lead = RichText::new("Margin note").font(theme::sans_semi(16.0));
        let _ = lead;
        let g = ui.painter().layout_no_wrap("Margin note".into(), theme::sans_semi(16.0), t.ink);
        cx.ctl.para_text("Margin note: ");
        it.push(flow::Item { w: g.size().x, h: g.size().y, trail: 2.0 * space_w.max(4.0), brk: true });
        atoms.push(Atom::Text { g, color: t.ink, strike: false, code: false });
        let _ = st;
    }
    for run in &line.runs {
        match run {
            Run::Text { s: txt, st } => push_text(ui, cx, &mut atoms, &mut it, txt, st, false),
            Run::Math(n) => {
                let m = &line.math[*n];
                cx.ctl.para_math(&m.tex);
                match cx.math.get(&m.tex, m.display, px * KATEX_EM, t.ink).map(|r| r.size) {
                    Some(sz) => {
                        it.push(flow::Item { w: sz.x + 4.0, h: sz.y + 2.0, trail: 0.0, brk: false });
                        atoms.push(Atom::Math { tex: m.tex.clone(), display: m.display });
                    }
                    None => {
                        // plan rule 4: the source in monospace red (muted while the font is on its way)
                        let col = if cx.math.pending() { t.muted } else { t.bad };
                        let g = ui.painter().layout_no_wrap(m.tex.clone(), egui::FontId::new(px * 0.85, egui::FontFamily::Monospace), col);
                        it.push(flow::Item { w: g.size().x, h: g.size().y, trail: 0.0, brk: false });
                        atoms.push(Atom::Text { g, color: col, strike: false, code: false });
                    }
                }
            }
            Run::Word { key, text, st } => {
                let Some(item) = items.iter().find(|i| &i.key == key) else {
                    push_text(ui, cx, &mut atoms, &mut it, text, st, false);
                    continue;
                };
                let (kind, checked) = s.word_kind(item);
                let shown = if text.is_empty() { s.word_text(item) } else { text.clone() };
                let label = if checked {
                    format!("Checked word: {shown}. Show options.")
                } else if item.kind == "illegible" {
                    "Word that could not be read. Show options.".to_string()
                } else {
                    format!("Spelling to check: {shown}. Show options.")
                };
                let font = if small {
                    theme::sans(px)
                } else if st.bold {
                    theme::serif_semi(px)
                } else if st.italic {
                    theme::serif_italic(px)
                } else {
                    theme::serif(px)
                };
                let color = if kind == Kind::Bad && !checked { t.bad } else { t.ink };
                let g = ui.painter().layout_no_wrap(shown, font, color);
                // `.w { padding: 0 4px }`; no gap after it: the following text carries its own space
                it.push(flow::Item { w: g.size().x + 8.0, h: px * 1.35, trail: 0.0, brk: false });
                atoms.push(Atom::Chip {
                    id: format!("word-{}-{}", p.id(), key),
                    label,
                    g,
                    kind,
                    checked,
                    selected: sel_key == Some(key.as_str()),
                    key: key.clone(),
                });
            }
        }
    }

    let fl = flow::layout(&it, width, pitch);
    let pad_y = 1.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, fl.height + 2.0 * pad_y), Sense::hover());
    // the click that moves the reading underline sits under the chips (they are interacted after it)
    click_line(ui, cx, &id, rect, line.index);
    let origin = rect.min + vec2(0.0, pad_y);
    for (a, (pl, item)) in atoms.iter().zip(fl.placed.iter().zip(it.iter())) {
        let lh = fl.lines[pl.line].h;
        let top = origin.y + pl.line_top + (lh - item.h) / 2.0;
        let x = origin.x + pl.x;
        match a {
            Atom::Text { g, color, strike, code } => {
                if *code {
                    ui.painter().rect_filled(Rect::from_min_size(pos2(x - 2.0, top), vec2(item.w + 4.0, item.h)), 3.0, t.sunken);
                }
                ui.painter().galley(pos2(x, top), g.clone(), *color);
                if *strike {
                    let y = top + item.h * 0.55;
                    ui.painter().line_segment([pos2(x, y), pos2(x + item.w, y)], Stroke::new(1.0, *color));
                }
            }
            Atom::Math { tex, display } => {
                paint_math(ui, cx, tex, *display, px * KATEX_EM, pos2(x, top));
            }
            Atom::Chip { id, label, g, kind, checked, selected, key } => {
                let chip = Rect::from_min_size(pos2(x, origin.y + pl.line_top + (lh - item.h) / 2.0), vec2(item.w, item.h));
                // hit target: at least 44 x 44, centred on the chip (it overlaps the neighbouring line a little)
                let hit = Rect::from_center_size(chip.center(), vec2(chip.width().max(TARGET), TARGET));
                let r = ui.interact(hit, ui.id().with(id.as_str()), Sense::click());
                paint_chip(ui, t, chip, g, *kind, *checked, *selected, r.hovered());
                if *selected && cx.scroll_sel {
                    // `scrollIntoView({ block: 'nearest' })` on the word itself: only as far as the painted
                    // word needs (the 44pt hit target is not what the reader sees)
                    ui.scroll_to_rect(chip, None);
                }
                // `.w.sel { outline: 2px solid ink; outline-offset: 2px }` and the focus ring are the same ring
                if cx.ctl.reg(ui, &r, C::new(id, label, TEXT).ring(chip.expand(3.0), 7.0)) {
                    cx.acts.push(Act::Pick(p.id().to_string(), key.clone()));
                }
            }
        }
    }
    if reading && cx.scroll_read {
        ui.scroll_to_rect(rect, None);
    }
    if reading {
        // `text-decoration: underline 3px accent; text-underline-offset: 7px` on every visual line
        for l in &fl.lines {
            let y = origin.y + l.top + l.h / 2.0 + px * 0.35 + 7.0;
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(origin.x + l.x0, y), pos2(origin.x + l.x1.max(l.x0 + 4.0), y + 3.0)),
                1.0,
                t.accent,
            );
        }
    }
    cx.ctl.end_para(&id, TEXT, rect.min, true, reading);
    rect
}

fn click_line(ui: &mut Ui, cx: &mut Cx, id: &str, rect: Rect, index: usize) {
    let r = ui.interact(rect, ui.id().with(("ln", id)), Sense::click());
    if r.clicked() {
        cx.acts.push(Act::ReadLine(index));
    }
}

fn paint_math(ui: &mut Ui, cx: &mut Cx, tex: &str, display: bool, px: f32, at: egui::Pos2) {
    let ink = cx.t.ink;
    if let Some(r) = cx.math.get(tex, display, px, ink) {
        let d = at.to_vec2() + vec2(2.0, 1.0);
        if ui.is_rect_visible(Rect::from_min_size(at, r.size)) {
            ui.painter().extend(r.shapes().map(|mut s| {
                s.translate(d);
                s
            }));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_chip(ui: &mut Ui, t: &Tokens, chip: Rect, g: &std::sync::Arc<egui::Galley>, kind: Kind, checked: bool, selected: bool, hover: bool) {
    let color = if kind == Kind::Bad && !checked { t.bad } else { t.ink };
    match (kind, checked) {
        (_, true) | (Kind::Ok, _) => {
            ui.painter().line_segment([pos2(chip.left() + 2.0, chip.bottom() - 2.0), pos2(chip.right() - 2.0, chip.bottom() - 2.0)], Stroke::new(2.0, t.ok));
        }
        (Kind::Bad, false) => {
            ui.painter().rect_filled(chip, 4.0, t.bad_bg);
            ui.painter().add(egui::Shape::dashed_line(
                &[pos2(chip.left() + 2.0, chip.bottom() - 2.0), pos2(chip.right() - 2.0, chip.bottom() - 2.0)],
                Stroke::new(2.0, t.bad),
                5.0,
                3.0,
            ));
        }
        (Kind::Warn, false) => {
            ui.painter().rect_filled(chip, 4.0, t.warn);
        }
    }
    if hover {
        ui.painter().rect_stroke(chip.expand(1.0), 4.0, Stroke::new(1.0, t.line2));
    }
    if selected {
        ui.painter().rect_stroke(chip.expand(3.0), 7.0, Stroke::new(2.0, t.ink));
    }
    ui.painter().galley(chip.center() - g.size() / 2.0, g.clone(), color);
}

enum FootItem {
    Text(String),
    Btn { id: &'static str, label: &'static str, look: Look, act: Act },
}

/// `.tx-foot`: the token stat, the hint and the buttons as one flex-wrap row, left aligned. A hint that does
/// not fit beside the stat takes a row of its own (it is never split to fill the rest of a row).
fn foot(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page, has_words: bool) {
    let t = cx.t;
    ui.add_space(28.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let avail = ui.available_width();
    let mut items: Vec<FootItem> = vec![];
    let tokens = p.get("timings").and_then(|x| x.get("tokens")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    if tokens > 0.0 {
        let ms = p.get("timings").and_then(|x| x.get("ms")).and_then(|v| v.as_f64()).unwrap_or(0.0);
        items.push(FootItem::Text(format!("{} tokens in {:.1}s", tokens, ms / 1000.0)));
    }
    if !s.text_raw && s.selected.as_ref().and_then(|id| s.read_line.get(id)).is_none() {
        items.push(FootItem::Text("Click a line, or use the up and down arrow keys, to underline where you are.".into()));
    }
    if has_words && !s.tray_open {
        items.push(FootItem::Btn { id: "tray-open", label: "Check words", look: Look::Outline, act: Act::Tray(true) });
    }
    items.push(FootItem::Btn {
        id: "text-view",
        label: if s.text_raw { "Show formatted text" } else { "Show plain text" },
        look: Look::Ghost,
        act: Act::TextView(!s.text_raw),
    });
    items.push(FootItem::Btn { id: "transcribe-again", label: "Transcribe this page again", look: Look::Ghost, act: Act::Transcribe(p.id().to_string(), true) });

    const GAP: f32 = 16.0;
    let width = |ui: &Ui, it: &FootItem| match it {
        FootItem::Text(x) => tw(ui, x, theme::sans(13.0)).min(avail),
        FootItem::Btn { label, .. } => tw(ui, label, theme::sans_semi(14.0)) + 36.0,
    };
    let mut rows: Vec<Vec<FootItem>> = vec![];
    let mut used = 0.0;
    for it in items {
        let w = width(ui, &it);
        match rows.last_mut() {
            Some(row) if used + GAP + w <= avail + 0.5 => {
                row.push(it);
                used += GAP + w;
            }
            _ => {
                rows.push(vec![it]);
                used = w;
            }
        }
    }
    let n_rows = rows.len();
    for (ri, row) in rows.into_iter().enumerate() {
        let has_btn = row.iter().any(|i| matches!(i, FootItem::Btn { .. }));
        let h = if has_btn { TARGET } else { 13.0 * 1.4 };
        let single_wide = row.len() == 1 && matches!(&row[0], FootItem::Text(x) if tw(ui, x, theme::sans(13.0)) > avail);
        let mut clicked: Vec<Act> = vec![];
        let body = |ui: &mut Ui, cx: &mut Cx, clicked: &mut Vec<Act>| {
            ui.spacing_mut().item_spacing.x = GAP;
            for it in row {
                match it {
                    FootItem::Text(x) => {
                        ui.add(egui::Label::new(RichText::new(x).font(theme::sans(13.0)).color(t.muted)).wrap());
                    }
                    FootItem::Btn { id, label, look, act } => {
                        if widgets::button(cx.ctl, ui, t, C::new(id, label, TEXT), label, look, 0.0) {
                            clicked.push(act);
                        }
                    }
                }
            }
        };
        if single_wide {
            ui.with_layout(Layout::top_down(Align::Min), |ui| body(ui, cx, &mut clicked));
        } else {
            ui.allocate_ui_with_layout(vec2(avail, h), Layout::left_to_right(Align::Center), |ui| body(ui, cx, &mut clicked));
        }
        if ri + 1 < n_rows {
            ui.add_space(4.0);
        }
        cx.acts.extend(clicked);
    }
}

// ------------------------------------------------------------------ tray

fn tray(ui: &mut Ui, cx: &mut Cx, s: &Session, entries: &[Entry]) {
    let t = cx.t;
    let Some((cur, idx, n)) = s.open_entry() else { return };
    let page = s.page(&cur.page);
    let all_done = !entries.iter().any(|e| e.item.via.is_none());
    let unresolved = s.unresolved_suggestions();
    let avail = ui.available_width();
    let inner = avail.min(760.0);
    let pad = if cx.narrow { 20.0 } else { 40.0 };
    let side = (avail - inner) / 2.0 + pad;
    egui::Frame::none().inner_margin(Margin { left: side, right: side, top: 12.0, bottom: 24.0 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        // head: title on the left, controls on the right (wrapping on a phone)
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 2.0);
            let title = if all_done { "All words checked" } else { "Check this word" };
            let r = ui.allocate_response(vec2(tw(ui, title, theme::sans_semi(16.0)) + 12.0, TARGET), Sense::hover());
            ui.painter().text(pos2(r.rect.left(), r.rect.center().y), egui::Align2::LEFT_CENTER, title, theme::sans_semi(16.0), t.ink);
            cx.ctl.heading("tray-title", 2, title, TRAY, r.rect.min);
            if unresolved >= 2 {
                let l = format!("Use all {unresolved} suggestions");
                if widgets::button(cx.ctl, ui, t, C::new("use-all", &l, TRAY), &l, Look::Outline, 0.0) {
                    cx.acts.push(Act::UseAll);
                }
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("prev-word", "Previous word to check", TRAY), Icon::Left, false) {
                cx.acts.push(Act::StepWord(-1));
            }
            let count = format!("{} of {}", idx + 1, n);
            let cr = ui.allocate_response(vec2(tw(ui, &count, theme::sans(13.0)) + 4.0, TARGET), Sense::hover());
            ui.painter().text(cr.rect.center(), egui::Align2::CENTER_CENTER, &count, theme::sans(13.0), t.muted);
            if widgets::icon_button(cx.ctl, ui, t, C::new("next-word", "Next word to check", TRAY), Icon::Right, false) {
                cx.acts.push(Act::StepWord(1));
            }
            if all_done {
                if widgets::button(cx.ctl, ui, t, C::new("tray-done", "Done", TRAY), "Done", Look::Primary, 0.0) {
                    cx.acts.push(Act::Tray(false));
                }
            } else if widgets::icon_button(cx.ctl, ui, t, C::new("tray-close", "Close the word panel", TRAY), Icon::Close, false) {
                cx.acts.push(Act::Tray(false));
            }
        });
        let note = s.note(&cur);
        let r = ui.add(egui::Label::new(RichText::new(&note).font(theme::sans(14.0)).color(t.muted)).wrap());
        cx.ctl.text("tray-note", &note, TRAY, r.rect.min);
        ui.add_space(8.0);

        let crop = page.and_then(|p| crop_for(p, &cur, cx.images));
        // `.tray-body { display: flex; flex-wrap: wrap; gap: 20px }`: the crop is `0 1 280px`, the options
        // `1 1 300px`, so they sit side by side only when 280 + 20 + 300 fit; otherwise the crop comes first
        // and the options take the whole row.
        let total = ui.available_width();
        let stack = crop.is_none() || total < 280.0 + 20.0 + 300.0;
        let mut body = |ui: &mut Ui, alts_w: f32| {
            if let Some((tex, uv, size)) = crop {
                let col_w = 280.0f32.min(total);
                ui.allocate_ui_with_layout(vec2(col_w, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let dh = size.y.max(56.0);
                    let (r, _) = ui.allocate_exact_size(vec2(col_w, dh), Sense::hover());
                    ui.painter().rect_filled(r, 6.0, t.paper);
                    let im = Rect::from_center_size(r.center(), size.min(vec2(col_w, 120.0)));
                    ui.painter().image(tex, im, uv, Color32::WHITE);
                    ui.label(RichText::new("This line, as the student wrote it").font(theme::sans(13.0)).color(t.muted));
                });
            }
            ui.allocate_ui_with_layout(vec2(alts_w, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(alts_w);
                ui.spacing_mut().item_spacing.y = 8.0;
                alternatives(ui, cx, s, &cur);
            });
        };
        if stack {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 20.0;
                body(ui, total);
            });
        } else {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(20.0, 8.0);
                body(ui, total - 280.0 - 20.0);
            });
        }
    });
}

fn alternatives(ui: &mut Ui, cx: &mut Cx, s: &Session, cur: &Entry) {
    let t = cx.t;
    let w = ui.available_width();
    for a in s.alts(cur).iter() {
        // `.alt { padding: 6px 14px; gap: 12px }` with the radio (16px) before the text; the text wraps in the card
        let text_x = 14.0 + 16.0 + 12.0;
        let wrap = (w - text_x - 14.0 - 2.0).max(40.0);
        let main = ui.painter().layout(a.label.clone(), theme::serif_semi(19.0), t.ink, wrap);
        let hint = ui.painter().layout(a.hint.clone(), theme::sans(13.0), t.muted, wrap);
        // css line boxes: 19px at 1.4 and 13px at 1.4 (a wrapped line adds its own)
        let main_h = main.size().y.max(19.0 * 1.4);
        let hint_h = hint.size().y.max(13.0 * 1.4);
        let h = (2.0 + 12.0 + main_h + hint_h).max(TARGET).round();
        let r = ui.allocate_response(vec2(w, h), Sense::click());
        let fill = if a.on { t.ok_bg } else if r.hovered() { t.sunken } else { t.surface };
        let line = if a.on { t.ok } else { t.line2 };
        ui.painter().rect_filled(r.rect, 8.0, fill);
        ui.painter().rect_stroke(r.rect, 8.0, Stroke::new(1.0, line));
        let c = pos2(r.rect.left() + 14.0 + 8.0, r.rect.center().y);
        ui.painter().circle(c, 8.0, t.surface, Stroke::new(if a.on { 5.0 } else { 2.0 }, if a.on { t.ok } else { t.faint }));
        let x = r.rect.left() + text_x;
        let top = (r.rect.center().y - (main_h + hint_h) / 2.0).round();
        ui.painter().galley(pos2(x, (top + (main_h - main.size().y) / 2.0).round()), main, t.ink);
        ui.painter().galley(pos2(x, (top + main_h + (hint_h - hint.size().y) / 2.0).round()), hint, t.muted);
        let label = format!("{}. {}", a.label, a.hint);
        if cx.ctl.reg(ui, &r, C::new(&a.id, &label, TRAY).pressed(a.on)) {
            cx.acts.push(Act::Choose(a.via, a.text.clone()));
        }
    }
    // unreadable words: the guesses' own caveat, or the way to ask for them (review.js `alts.append`)
    if cur.item.kind == "illegible" {
        let page = s.page(&cur.page);
        let has = page
            .and_then(|p| p.get("guesses"))
            .and_then(|g| g.get(&cur.item.key))
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        if has {
            let msg = "The percentages are the model’s own estimate, not a measured probability.";
            let r = ui.add(egui::Label::new(RichText::new(msg).font(theme::sans(13.0)).color(t.muted)).wrap());
            cx.ctl.text("guess-hint", msg, TRAY, r.rect.min);
        } else {
            let guessed = page.is_some_and(|p| jsx_truthy(p.get("guessed")));
            let label = if guessed { "Look again for likely words" } else { "Suggest likely words" };
            if widgets::button(cx.ctl, ui, t, C::new("suggest-guesses", label, TRAY), label, Look::Outline, w) {
                cx.acts.push(Act::Guess(cur.page.clone()));
            }
        }
    }
    // typed fix
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let btn_w = 104.0;
        let tw_ = (ui.available_width() - btn_w - 8.0).max(120.0);
        // the field is drawn inside a 44pt slot; the slot is the target the twin reports
        let (slot, _) = ui.allocate_exact_size(vec2(tw_, TARGET), Sense::hover());
        let edit = egui::TextEdit::singleline(&mut *cx.typed)
            .hint_text("Type what it says")
            .font(theme::sans(14.0))
            .margin(vec2(12.0, 13.0))
            .min_size(slot.size())
            .desired_width(tw_ - 24.0);
        let mut r = ui.put(slot, edit);
        r.rect = slot;
        if let Some(v) = cx.ctl.reg_text(ui, &r, C::new("typed-word", "Type what the word says", TRAY), &*cx.typed) {
            *cx.typed = v;
        }
        let entered = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let use_it = widgets::button(cx.ctl, ui, t, C::new("use-typed", "Use this", TRAY), "Use this", Look::Primary, btn_w);
        if (use_it || entered) && !cx.typed.trim().is_empty() {
            cx.acts.push(Act::Choose("typed", cx.typed.trim().to_string()));
        }
    });
}

/// The handwriting of the open word's line, cropped from the page image. Only when the line boxes are
/// exact (as many as transcript lines), as review.js `cropFor` insists.
fn crop_for(p: &Page, e: &Entry, images: &HashMap<String, Slot>) -> Option<(egui::TextureId, Rect, egui::Vec2)> {
    let Some(Slot::Ready(im)) = images.get(&e.page) else { return None };
    let n = vishion_core::render::units(p.raw()).len();
    // only when the page has its own boxes and the ink scan found exactly as many lines as the transcript has
    let boxes = state::stored_boxes(p);
    if n == 0 || boxes.len() != n || im.bands.len() != n {
        return None;
    }
    let idx = state::unit_of(p, &e.item)?;
    let b = boxes.get(idx)?;
    if b.h > 0.2 || b.w < 0.05 {
        return None;
    }
    let pad = b.h * 0.25;
    let x = (b.x - 0.01).max(0.0);
    let y = (b.y - pad).max(0.0);
    let w = (b.w + 0.02).min(1.0 - x);
    let h = (b.h + 2.0 * pad).min(1.0 - y);
    let (pw, ph) = (w * im.size.0 as f32, h * im.size.1 as f32);
    let mut dw = 280.0f32.min(pw);
    let mut dh = dw * ph / pw.max(1.0);
    if dh > 120.0 {
        dh = 120.0;
        dw = dh * pw / ph.max(1.0);
    }
    Some((im.full.id(), Rect::from_min_max(pos2(x, y), pos2(x + w, y + h)), vec2(dw, dh)))
}
