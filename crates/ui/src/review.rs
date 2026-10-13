//! The review screen: header, page pane (tools, strip, image with marks), transcript with flagged words
//! inline, the "Check this word" tray, a minimal assignment overview, and the toast. Follows
//! design/Main.dc.html and js/ui/review.js + header.js + css/layout.css.
//!
//! Drawing never mutates the session. Interactions are collected as `Act`s and applied after the frame, so
//! the borrow of the session stays shared while egui closures run. Keys are routed by where focus is
//! (transcript, strip, stage, tray, dialog), exactly as the JS handlers hang off those elements.
use crate::dialog::{self, After, Dialog};
use crate::emoji;
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
    sel_until: f64,
    read_left: u8,
    /// `App::layout_epoch` last seen: a picture or a font arrived and may have moved the page.
    epoch_seen: u32,
    /// Height of the transcript's scroll area last frame: when the tray grows (its crop arrives, the options
    /// wrap) the selected word is brought back into view.
    text_h: f32,
    /// Last `Session::scroll_req` applied, the offset the transcript pane was last seen at, and a change
    /// waiting out the 600 ms debounce of review.js (offset, egui time it was seen).
    scroll_epoch: u32,
    scroll_seen: f32,
    scroll_pending: Option<(f32, f64)>,
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
    Filter(state::Filter),
    SelectGroup(String, bool),
    Scrolled(f32),
    RetryIds(Vec<String>),
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
    /// `App::layout_epoch`: counts pictures and fonts that arrived (they move the page).
    epoch: u32,
    emoji: &'a mut emoji::EmojiCache,
    /// Page pictures a row asks for (visible thumbnails): loaded by the host, answered as `Event::Image`.
    want: &'a mut Vec<String>,
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
    // CSS line boxes are exactly `line_h` each (egui rounds its rows to whole pixels)
    let height = g.rows.len().max(1) as f32 * line_h;
    let (rect, _) = ui.allocate_exact_size(vec2(g.size().x.max(1.0), height), Sense::hover());
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
    let App { ctl, session, math, images, typed, typed_for, status_msg, status_err, boot, notice, save, storage, dialog, stage, host, want, layout_epoch, emoji: emoji_cache, .. } = app;
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
                want: &mut *want,
                emoji: &mut *emoji_cache,
                epoch: *layout_epoch,
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
            if !matches!(a, Act::ReadLine(_) | Act::ReadMove(_) | Act::ReadClear | Act::Zoom(_) | Act::ZoomSet(_) | Act::Dialog(_) | Act::CloseDialog | Act::Scrolled(_)) {
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
            if !s.done_next() {
                say("That was the last student.".into(), false);
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
                s.review_again_and_open(&ids);
            }
        }
        Act::ClearMulti => s.multi.clear(),
        Act::Filter(f) => s.filter = f,
        Act::Scrolled(y) => s.set_scroll(y),
        Act::SelectGroup(key, on) => {
            if let Some(g) = s.groups().into_iter().find(|g| g.key == key) {
                for id in g.pages {
                    if on {
                        s.multi.insert(id);
                    } else {
                        s.multi.remove(&id);
                    }
                }
            }
        }
        Act::RetryIds(ids) => {
            if !ids.is_empty() {
                send(Cmd::Transcribe { slug, ids });
            }
        }
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
                        let n = s.review_again_and_open(&ids);
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
            let icon_col = if cx.save.failed.is_some() { t.bad } else { t.ok };
            widgets::paint_icon(ui, Rect::from_center_size(pos2(sr.left() + 9.0, sr.center().y), vec2(18.0, 18.0)), icon, icon_col);
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
        let icon_col = if cx.save.failed.is_some() { t.bad } else { t.ok };
        widgets::paint_icon(ui, Rect::from_center_size(pos2(sr.left() + 9.0, sr.center().y), vec2(18.0, 18.0)), icon, icon_col);
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

/// ------------------------------------------------------------------ inbox (assignment overview)

/// js/ui/inbox.js: title and counts, the run bar, the segmented status bar, the filter tabs and one row per
/// student with its own status, selection box and Retry. Geometry follows css/components.css (`.runbar`,
/// `.stackbar`, `.tab`, `.srows`, `.srow`).
fn inbox(ctx: &egui::Context, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    egui::CentralPanel::default().frame(egui::Frame::none().fill(t.bg)).show(ctx, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_source("inbox-scroll").show(ui, |ui| {
            let avail = ui.available_width();
            // `.page-inner { max-width: 1120px; padding: 40px 24px 56px }` (28 16 48 on a phone)
            let pad = if cx.narrow { 16.0 } else { 24.0 };
            let side = (avail - avail.min(1120.0)) / 2.0 + pad;
            let margin = Margin { left: side, right: side, top: if cx.narrow { 28.0 } else { 40.0 }, bottom: if cx.narrow { 48.0 } else { 56.0 } };
            egui::Frame::none().inner_margin(margin).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                inbox_body(ui, cx, s);
            });
        });
    });
}

/// A button of a run bar: id, label, primary, what it does.
type BarBtn = (&'static str, String, bool, Act);

/// `.runbar`: text (bold line and a muted one) at the left, buttons at the right; the buttons go under the
/// text when the card is too narrow for both.
fn run_bar(ui: &mut Ui, cx: &mut Cx, id: &str, title: &str, sub: &str, buttons: Vec<BarBtn>) {
    let t = cx.t;
    let w = ui.available_width();
    let inner_w = w - 2.0 - 40.0;
    let btn_w: Vec<f32> = buttons.iter().map(|(_, l, _, _)| tw(ui, l, theme::sans_semi(14.0)) + 36.0).collect();
    let btns_w = btn_w.iter().sum::<f32>() + 8.0 * btn_w.len().saturating_sub(1) as f32;
    let beside = buttons.is_empty() || 280.0 + 20.0 + btns_w <= inner_w;
    let frame = egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::symmetric(20.0, 14.0));
    let r = frame.show(ui, |ui| {
        ui.set_width(inner_w);
        let text = |ui: &mut Ui, cx: &mut Cx, width: f32| {
            let r = css_text(ui, title, theme::sans_semi(14.0), t.ink, 14.0 * 1.4, width, false);
            ui.add_space(2.0);
            css_text(ui, sub, theme::sans(13.0), t.muted, 13.0 * 1.4, width, false);
            cx.ctl.status(id, &format!("{title} {sub}"), TEXT, r.min);
        };
        let btns = |ui: &mut Ui, cx: &mut Cx, buttons: Vec<BarBtn>| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (bid, label, primary, act) in buttons {
                let look = if primary { Look::Primary } else { Look::Outline };
                if widgets::button(cx.ctl, ui, t, C::new(bid, &label, TEXT), &label, look, 0.0) {
                    cx.acts.push(act);
                }
            }
        };
        if beside {
            // (`align-items: center`: the text block is one 39.8 line pair, the buttons 44 high)
            ui.horizontal_top(|ui| {
                let tw_ = inner_w - btns_w - 20.0;
                ui.allocate_ui_with_layout(vec2(tw_.max(120.0), 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(if buttons.is_empty() { 0.0 } else { (TARGET - (14.0 * 1.4 + 2.0 + 13.0 * 1.4)) / 2.0 });
                    text(ui, cx, tw_.max(120.0));
                });
                ui.add_space((ui.available_width() - btns_w).max(0.0));
                btns(ui, cx, buttons);
            });
        } else {
            text(ui, cx, inner_w);
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| btns(ui, cx, buttons));
        }
    });
    let _ = r;
}

fn inbox_body(ui: &mut Ui, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    let w = ui.available_width();
    let name = s.a.meta.name();
    let title = if name.is_empty() { "Assignment" } else { name };
    let groups = s.groups();
    let tally = Session::tally(&groups);
    let topic = s.a.meta.topic();
    let sub = format!("{}{}", if topic.is_empty() { String::new() } else { format!("{topic}, ") }, state::plural(tally.total, "student", "students"));
    let size = if cx.narrow { 30.0 } else { 36.0 };
    let r = css_text(ui, title, theme::serif_semi(size), t.ink, size * 1.15, w, true);
    cx.ctl.heading("inbox-title", 1, title, TEXT, r.min);
    ui.add_space(8.0);
    let r = css_text(ui, &sub, theme::sans(14.0), t.muted, 14.0 * 1.4, w, false);
    cx.ctl.text("inbox-sub", &sub, TEXT, r.min);

    if groups.is_empty() {
        ui.add_space(28.0);
        egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::symmetric(24.0, 40.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                widgets::paint_icon(ui, ir, Icon::Page, t.muted);
                ui.add_space(10.0);
                let r = css_text(ui, "No pages in this assignment", theme::sans_semi(18.0), t.ink, 18.0 * 1.4, w, false);
                cx.ctl.heading("inbox-empty", 3, "No pages in this assignment", TEXT, r.min);
            });
        });
        return;
    }

    // run bar
    let c = s.a.counts();
    let failed_btn = (c.error > 0).then(|| ("retry-failed", format!("Retry {} failed", c.error), false, Act::RetryIds(s.failed_ids())));
    ui.add_space(28.0);
    if c.pending > 0 {
        let stopped = if c.done > 0 {
            format!("Stopped. {} left.", state::plural(c.pending as usize, "page", "pages"))
        } else {
            format!("{} a transcript.", state::plural(c.pending as usize, "page needs", "pages need"))
        };
        let subtxt = format!("{} of {} pages have a transcript. The rest will pick up where they left off.", c.done, c.total);
        let go = if c.done > 0 { format!("Resume ({} left)", c.pending) } else { format!("Transcribe {}", state::plural(c.pending as usize, "page", "pages")) };
        let mut b: Vec<BarBtn> = failed_btn.into_iter().collect();
        b.push(("resume", go, true, Act::RetryIds(s.pending_ids())));
        run_bar(ui, cx, "run-status", &stopped, &subtxt, b);
    } else if c.error > 0 {
        let title = format!("{} couldn\u{2019}t be transcribed.", state::plural(c.error as usize, "page", "pages"));
        run_bar(ui, cx, "run-status", &title, "Retry them, or open a student to see what went wrong.", failed_btn.into_iter().collect());
    } else if tally.check > 0 {
        let title = format!("All {} transcribed.", state::plural(c.total as usize, "page is", "pages are"));
        let subtxt = format!("{} words to check before you read them.", state::plural(tally.check, "student has", "students have"));
        let first = groups.iter().find(|g| g.status == vishion_core::groups::Status::Check).and_then(|g| g.pages.first().cloned());
        let b = first.map(|f| ("start-first", "Start with the first one".to_string(), true, Act::OpenReview(f))).into_iter().collect();
        run_bar(ui, cx, "run-status", &title, &subtxt, b);
    } else {
        ui.add_space(-28.0);
    }

    // selection bar
    if !s.multi.is_empty() {
        let names = groups.iter().filter(|g| g.pages.iter().any(|p| s.multi.contains(p))).count();
        ui.add_space(16.0);
        let title = format!("{} selected", state::plural(names, "student", "students"));
        let subtxt = state::plural(s.multi.len(), "page", "pages");
        let b: Vec<BarBtn> = vec![
            ("sel-clear", "Clear selection".into(), false, Act::ClearMulti),
            ("sel-review-again", "Review again".into(), false, Act::ReviewAgain),
            ("sel-export", "Export selected".into(), true, Act::ExportSelected),
        ];
        run_bar(ui, cx, "sel-status", &title, &subtxt, b);
    }

    // `.stackbar { gap: 3px; height: 12px; margin-top: 24px }`: four segments that share what the gaps leave
    ui.add_space(24.0);
    {
        let (rect, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
        let usable = w - 9.0;
        let total = tally.total.max(1) as f32;
        let segs = [
            (tally.ready, t.bar_ok, t.bar_ok_edge),
            (tally.check, t.bar_warn, t.bar_warn_edge),
            (tally.failed, t.bar_bad, t.bar_bad_edge),
            (tally.processing, t.track, t.faint),
        ];
        let mut x = rect.left();
        for (n, fill, edge) in segs {
            let sw = n as f32 / total * usable;
            let r = Rect::from_min_size(pos2(x, rect.top()), vec2(sw, 12.0));
            if sw > 0.5 {
                ui.painter().rect_filled(r, 6.0, fill);
                ui.painter().rect_stroke(r.shrink(0.5), 5.5, Stroke::new(1.0, edge));
            }
            x += sw + 3.0;
        }
        let label = format!(
            "{} ready to read, {} need a check, {} failed, {} not done yet",
            tally.ready, tally.check, tally.failed, tally.processing
        );
        cx.ctl.status("stackbar", &label, TEXT, rect.min);
    }

    // tabs: `.tabs { gap: 8px; margin-top: 16px }`, `.tab { height: 44px; padding: 0 16px; gap: 8px }`
    ui.add_space(16.0);
    let counts = [tally.total, tally.ready, tally.check, tally.processing, tally.failed];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (f, n) in state::Filter::ALL.into_iter().zip(counts) {
            let on = s.filter == f;
            let label = f.label();
            let fg = if on { t.on_ink } else { t.ink };
            let lg = ui.painter().layout_no_wrap(label.to_string(), theme::sans_semi(14.0), fg);
            let ng = ui.painter().layout_no_wrap(n.to_string(), theme::sans_semi(14.0), fg.gamma_multiply(0.75));
            let dot = match f {
                state::Filter::All => None,
                state::Filter::Ready => Some((t.bar_ok, t.bar_ok_edge)),
                state::Filter::Check => Some((t.bar_warn, t.bar_warn_edge)),
                state::Filter::Processing => None,
                state::Filter::Failed => Some((t.bar_bad, t.bar_bad_edge)),
            };
            let dot_w = if dot.is_some() { 10.0 + 8.0 } else { 0.0 };
            let width = 2.0 + 32.0 + dot_w + lg.size().x + 8.0 + ng.size().x;
            let r = ui.allocate_response(vec2(width, TARGET), Sense::click());
            let fill = if on { t.ink } else if r.hovered() { t.hover } else { Color32::TRANSPARENT };
            ui.painter().rect_filled(r.rect, 22.0, fill);
            ui.painter().rect_stroke(r.rect.shrink(0.5), 21.5, Stroke::new(1.0, if on { t.ink } else { t.line2 }));
            let mut x = r.rect.left() + 17.0;
            if let Some((fill, edge)) = dot {
                let c = pos2(x + 5.0, r.rect.center().y);
                ui.painter().circle_filled(c, 5.0, fill);
                ui.painter().circle_stroke(c, 4.5, Stroke::new(1.0, edge));
                x += 18.0;
            }
            ui.painter().galley(pos2(x, r.rect.center().y - lg.size().y / 2.0), lg.clone(), fg);
            x += lg.size().x + 8.0;
            ui.painter().galley(pos2(x, r.rect.center().y - ng.size().y / 2.0), ng, fg.gamma_multiply(0.75));
            let lab = format!("{label} {n}");
            if cx.ctl.reg(ui, &r, C::new(&format!("tab-{}", f.id()), &lab, TEXT).pressed(on)) {
                cx.acts.push(Act::Filter(f));
            }
        }
    });

    // rows
    ui.add_space(20.0);
    let shown: Vec<&vishion_core::groups::Group> = groups.iter().filter(|g| s.filter.matches(g.status)).collect();
    if shown.is_empty() {
        egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::symmetric(24.0, 40.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let (ir, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                widgets::paint_icon(ui, ir, Icon::Check, t.muted);
                ui.add_space(10.0);
                let r = css_text(ui, "Nothing left to check", theme::sans_semi(18.0), t.ink, 18.0 * 1.4, w, false);
                cx.ctl.heading("inbox-nothing", 3, "Nothing left to check", TEXT, r.min);
                ui.add_space(10.0);
                let msg = "Every student in this filter has been read. Pick another filter to see the rest.";
                let r = css_text(ui, msg, theme::sans(14.0), t.muted, 14.0 * 1.4, 36.0 * 8.0, false);
                cx.ctl.text("inbox-nothing-text", msg, TEXT, r.min);
                ui.add_space(16.0);
                if widgets::button(cx.ctl, ui, t, C::new("show-all", "Show all students", TEXT), "Show all students", Look::Outline, 0.0) {
                    cx.acts.push(Act::Filter(state::Filter::All));
                }
            });
        });
    } else {
        let card = egui::Frame::none().fill(t.surface).stroke(Stroke::new(1.0, t.line)).rounding(10.0).inner_margin(Margin::same(1.0));
        let last = shown.len() - 1;
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, g) in shown.iter().enumerate() {
                student_row(ui, cx, s, g, i == 0, i == last);
            }
        });
    }
    ui.add_space(12.0);
    css_text(
        ui,
        "Adding pages in bulk, assignment settings and exporting arrive with phase 5 and 6.",
        theme::sans(13.0),
        t.muted,
        13.0 * 1.4,
        w,
        false,
    );
}

/// `.srow`: selection box, thumbnail, name and counts, the status chip, a chevron, and Retry for a failed one.
fn student_row(ui: &mut Ui, cx: &mut Cx, s: &Session, g: &vishion_core::groups::Group, first: bool, last: bool) {
    use vishion_core::groups::Status;
    let t = cx.t;
    let (kind, label) = s.chip(g);
    let idle = matches!(g.status, Status::Processing | Status::Queued | Status::Waiting | Status::Failed);
    let meta = if idle { state::plural(g.pages.len(), "page", "pages") } else { format!("{}, {} words", state::plural(g.pages.len(), "page", "pages"), g.words) };
    let w = ui.available_width();
    let pill_w = tw(ui, &label, theme::sans_semi(14.0)) + 28.0;
    let retry = g.status == Status::Failed;
    let retry_w = if retry { tw(ui, "Retry", theme::sans_semi(14.0)) + 34.0 } else { 0.0 };
    // the open button's content: pad 20, thumb 40, gap 20, who (160 at least), gap 20, chip, gap 20, icon 20, pad 20
    let open_w = w - 12.0 - TARGET - if retry { retry_w + 20.0 } else { 0.0 };
    let need = 20.0 + 40.0 + 20.0 + 160.0 + 20.0 + pill_w + 20.0 + 20.0 + 20.0;
    let wrap = need > open_w;
    let height = if wrap { 14.0 + 52.0 + 12.0 + PILL_ROW + 14.0 } else { 80.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(w, height + if last { 0.0 } else { 1.0 }), Sense::hover());
    let row = Rect::from_min_size(rect.min, vec2(w, height));
    let open = Rect::from_min_size(pos2(row.left() + 12.0 + TARGET, row.top()), vec2(open_w, height));
    let r_open = ui.interact(open, ui.id().with(("open", &g.key)), Sense::click());
    let hover = r_open.hovered();
    if hover {
        let round = egui::Rounding { nw: if first { 9.0 } else { 0.0 }, ne: if first { 9.0 } else { 0.0 }, sw: if last { 9.0 } else { 0.0 }, se: if last { 9.0 } else { 0.0 } };
        ui.painter().rect_filled(row, round, t.sunken);
    }
    if !last {
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.soft));
    }
    // selection box
    let on = g.pages.iter().all(|p| s.multi.contains(p));
    let cb = Rect::from_min_size(pos2(row.left() + 12.0, row.center().y - TARGET / 2.0), vec2(TARGET, TARGET));
    let rc = ui.interact(cb, ui.id().with(("sel", &g.key)), Sense::click());
    let bx = Rect::from_center_size(cb.center(), vec2(18.0, 18.0));
    if on {
        ui.painter().rect_filled(bx, 3.0, t.ink);
        let c = bx.center();
        ui.painter().add(egui::Shape::line(vec![pos2(c.x - 4.5, c.y + 0.5), pos2(c.x - 1.5, c.y + 3.5), pos2(c.x + 4.5, c.y - 3.5)], Stroke::new(2.0, t.on_ink)));
    } else {
        ui.painter().rect_filled(bx, 3.0, t.surface);
        ui.painter().rect_stroke(bx.shrink(0.5), 3.0, Stroke::new(1.0, t.faint));
    }
    let sel_label = format!("Select {}", g.name);
    if cx.ctl.reg(ui, &rc, C::new(&format!("sel-{}", g.key), &sel_label, TEXT).pressed(on)) {
        cx.acts.push(Act::SelectGroup(g.key.clone(), !on));
    }
    // thumbnail: the first page, `object-fit: cover` on ruled paper; dimmed while the student is not read yet
    let first_page = g.pages.first().cloned().unwrap_or_default();
    let th = Rect::from_min_size(pos2(open.left() + 20.0, open.top() + 14.0), vec2(40.0, 52.0));
    let alpha = if idle && g.status != Status::Failed { 0.55 } else { 1.0 };
    if ui.is_rect_visible(th) {
        ui.painter().rect_filled(th.translate(vec2(0.0, 1.0)).expand(0.5), 2.0, Color32::from_black_alpha(((if t.dark { 90.0 } else { 40.0 }) * alpha) as u8));
        ui.painter().rect_filled(th, 2.0, t.paper.gamma_multiply(alpha));
        match cx.images.get(&first_page) {
            Some(Slot::Ready(im)) => {
                // cover: crop the picture to the 40 x 52 box, centred
                let (iw, ih) = (im.size.0 as f32, im.size.1 as f32);
                let want = th.width() / th.height();
                let have = iw / ih.max(1.0);
                let uv = if have > want {
                    let f = want / have;
                    Rect::from_min_max(pos2((1.0 - f) / 2.0, 0.0), pos2((1.0 + f) / 2.0, 1.0))
                } else {
                    let f = have / want;
                    Rect::from_min_max(pos2(0.0, (1.0 - f) / 2.0), pos2(1.0, (1.0 + f) / 2.0))
                };
                ui.painter().image(im.thumb.id(), th, uv, Color32::WHITE.gamma_multiply(alpha));
            }
            Some(_) => {}
            None => cx.want.push(first_page.clone()),
        }
    }
    // name and counts
    let who_x = th.right() + 20.0;
    let who_w = if wrap { open.right() - 20.0 - who_x } else { open.right() - 20.0 - 20.0 - 20.0 - pill_w - 20.0 - who_x };
    let name_g = ui.painter().layout(g.name.clone(), theme::sans_semi(16.0), t.ink, who_w.max(40.0));
    let meta_g = ui.painter().layout(meta.clone(), theme::sans(13.0), t.muted, who_w.max(40.0));
    let block_h = 22.4 + 2.0 + 18.2;
    let top = if wrap { th.top() + (52.0 - block_h) / 2.0 } else { row.center().y - block_h / 2.0 };
    let dy = |gal: &std::sync::Arc<egui::Galley>, lh: f32| ((lh - gal.size().y.min(lh)) / 2.0 + 0.2).round();
    ui.painter().galley(pos2(who_x, top + dy(&name_g, 22.4)), name_g, t.ink);
    ui.painter().galley(pos2(who_x, top + 24.4 + dy(&meta_g, 18.2)), meta_g, t.muted);
    // chip and chevron
    let (chip_min, icon_c) = if wrap {
        let y = th.bottom() + 12.0;
        (pos2(who_x.min(open.left() + 20.0 + 60.0) - 0.0, y), pos2(open.right() - 20.0 - 10.0, y + PILL_ROW / 2.0))
    } else {
        let icon_c = pos2(open.right() - 20.0 - 10.0, row.center().y);
        (pos2(icon_c.x - 10.0 - 20.0 - pill_w, row.center().y - widgets::PILL_H / 2.0), icon_c)
    };
    widgets::pill_at(ui, t, kind, &label, chip_min);
    widgets::paint_icon(ui, Rect::from_center_size(icon_c, vec2(20.0, 20.0)), Icon::Right, t.muted);
    let open_label = format!("Open {}, {}", g.name, label);
    if cx.ctl.reg(ui, &r_open, C::new(&format!("row-{}", g.key), &open_label, TEXT)) {
        cx.acts.push(Act::OpenReview(first_page));
    }
    if retry {
        let at = pos2(row.right() - 20.0 - retry_w, row.center().y - TARGET / 2.0);
        let label = format!("Retry {}", g.name);
        if widgets::button_at(cx.ctl, ui, t, C::new(&format!("retry-{}", g.key), &label, TEXT), "Retry", Look::Outline, at) {
            cx.acts.push(Act::RetryIds(g.failed.clone()));
        }
    }
}

/// Height of the chip row when a student's row wraps (the chip is taller than the 20px icon).
const PILL_ROW: f32 = widgets::PILL_H;

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
    let now = ctx.input(|i| i.time);
    let epoch = cx.epoch;
    let st = &mut *cx.stage;
    // the reader scrolling takes the page back: the word is no longer pinned
    if ctx.input(|i| i.raw_scroll_delta != Vec2::ZERO) {
        st.sel_until = 0.0;
    }
    if s.sel_word.is_none() {
        st.sel_seen = None;
        st.sel_until = 0.0;
    } else if st.sel_seen != s.sel_word || st.tray_seen != s.tray_open {
        st.sel_seen = s.sel_word.clone();
        st.tray_seen = s.tray_open;
        st.sel_until = now + 1.5;
    } else if st.epoch_seen != epoch {
        // a picture or font arrived and moved things: keep the word where the reader can see it
        st.sel_until = st.sel_until.max(now + 1.2);
    }
    st.epoch_seen = epoch;
    if read.is_none() {
        st.read_seen = None;
        st.read_left = 0;
    } else if st.read_seen != read {
        st.read_seen = read;
        st.read_left = 6;
    }
    cx.scroll_sel = now < cx.stage.sel_until;
    cx.scroll_read = cx.stage.read_left > 0;
    review_panes(ctx, cx, s, narrow);
    let st = &mut *cx.stage;
    if st.read_left > 0 {
        st.read_left -= 1;
        ctx.request_repaint();
    }
    if now < st.sel_until {
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
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
            note_view_height(cx, ctx.input(|i| i.time), out.inner_rect.height());
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
                    // a short window: the tray scrolls inside what is left after 150 for the transcript, so
                    // the word being decided always has room above it
                    let cap = (ui.available_height() - 150.0).max(120.0);
                    egui::TopBottomPanel::bottom("tray")
                        .resizable(false)
                        .frame(egui::Frame::none().fill(t.sunken))
                        .show_inside(ui, |ui| {
                            let out = egui::ScrollArea::vertical()
                                .id_source("tray-scroll")
                                .max_height(cap)
                                .min_scrolled_height(cx.stage.tray_h.min(cap))
                                .auto_shrink([false, true])
                                .show(ui, |ui| tray(ui, cx, s, &entries));
                            cx.stage.tray_h = out.content_size.y;
                        });
                }
                let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_source("text-scroll");
                if let Some((n, off)) = s.scroll_req {
                    if cx.stage.scroll_epoch != n {
                        cx.stage.scroll_epoch = n;
                        cx.stage.scroll_seen = off;
                        area = area.vertical_scroll_offset(off);
                    }
                }
                let out = area.show(ui, |ui| {
                    text_column(ui, cx, s, g.as_ref(), &entries);
                });
                note_view_height(cx, ctx.input(|i| i.time), out.inner_rect.height());
                // review.js: `pane.addEventListener('scroll', debounce(save, 600))`
                let y = out.state.offset.y;
                let now = ui.input(|i| i.time);
                if (y - cx.stage.scroll_seen).abs() > 0.5 {
                    cx.stage.scroll_seen = y;
                    cx.stage.scroll_pending = Some((y, now));
                }
                if let Some((py, at)) = cx.stage.scroll_pending {
                    if now - at >= 0.6 {
                        cx.stage.scroll_pending = None;
                        cx.acts.push(Act::Scrolled(py));
                    } else {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(650));
                    }
                }
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
fn note_view_height(cx: &mut Cx, now: f64, h: f32) {
    if cx.stage.text_h > 0.0 && (cx.stage.text_h - h).abs() > 0.5 {
        cx.stage.sel_until = cx.stage.sel_until.max(now + 1.0);
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
            // `.stream { margin-top: 24px; max-width: 31em; white-space: pre-wrap; font: 20px/1.7 serif }`, then
            // `.tx-foot { margin-top: 28px }`
            ui.add_space(24.0);
            let wrap = ui.available_width().min(31.0 * 20.0);
            css_text(ui, p.raw(), theme::serif(20.0), t.ink, 20.0 * 1.7, wrap, false);
            ui.add_space(28.0);
            css_text(ui, "Reading this page. The text appears as it is read.", theme::sans(13.0), t.muted, 13.0 * 1.4, ui.available_width(), false);
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

/// `.state-pane`: a centred column (icon 40, h3 18px, a paragraph at most 34ch wide, an optional button) with
/// a 10px gap, at least 280 high with 24 of padding. The icon takes the text colour (no `.ico` rule mutes it).
fn state_block(ui: &mut Ui, cx: &mut Cx, title: &str, text: &str, button: Option<(&str, &str, Act)>) {
    let t = cx.t;
    let w = ui.available_width();
    // `max-width: 34ch`: ch is the width of "0" in the paragraph's font
    let ch = tw(ui, "0", theme::sans(14.0));
    let max_p = (34.0 * ch).min((w - 48.0).max(80.0));
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat { font_id: theme::sans(14.0), color: t.muted, line_height: Some(14.0 * 1.4), ..Default::default() },
    );
    job.wrap.max_width = max_p;
    job.halign = Align::Center;
    let pg = ui.painter().layout_job(job);
    let rows = pg.rows.len().max(1) as f32;
    let h3_h = 18.0 * 1.4;
    let content_h = 40.0 + 10.0 + h3_h + 10.0 + rows * 14.0 * 1.4 + if button.is_some() { 10.0 + TARGET } else { 0.0 };
    let height = (content_h + 48.0).max(280.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let cxm = rect.center().x;
    let mut y = rect.center().y - content_h / 2.0;
    widgets::paint_icon(ui, Rect::from_center_size(pos2(cxm, y + 20.0), vec2(40.0, 40.0)), Icon::Page, t.ink);
    y += 50.0;
    let tg = ui.painter().layout_no_wrap(title.to_string(), theme::sans_semi(18.0), t.ink);
    let dy = ((h3_h - tg.size().y) / 2.0 + 0.2).round();
    ui.painter().galley(pos2(cxm - tg.size().x / 2.0, y + dy), tg.clone(), t.ink);
    cx.ctl.heading("state-title", 3, title, TEXT, pos2(cxm - tg.size().x / 2.0, y));
    y += h3_h + 10.0;
    let ny = ((14.0 * 1.4 - ui.painter().layout_no_wrap("Hg".into(), theme::sans(14.0), t.muted).size().y) / 2.0 + 0.2).round();
    ui.painter().galley(pos2(cxm, y + ny), pg, t.muted);
    cx.ctl.text("state-text", text, TEXT, pos2(cxm - max_p / 2.0, y));
    y += rows * 14.0 * 1.4 + 10.0;
    if let Some((id, label, act)) = button {
        let bw = tw(ui, label, theme::sans_semi(14.0)) + 36.0;
        let at = pos2(cxm - bw / 2.0, y);
        if widgets::button_at(cx.ctl, ui, t, C::new(id, label, TEXT), label, Look::Primary, at) {
            cx.acts.push(act);
        }
    }
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
        ui.painter().galley(widgets::centered(r, g.size()), g, t.ink);
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
    /// A colour emoji picture (`emoji.rs`), `size` in points.
    Emoji { tex: egui::TextureId, size: Vec2 },
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
        // the font's own row height: what an emoji's line box is
        let nat_h = ui.painter().layout_no_wrap("Hg".to_string(), font.clone(), color).size().y;
        enum P {
            T(String),
            E(egui::TextureId, Vec2),
        }
        for (word, spaces) in split_tokens(txt) {
            // colour emoji are pictures between the words; everything else is text
            let segs: Vec<emoji::Seg> = if cx.emoji.has_font() && emoji::has_emoji(&word) {
                emoji::segments(&word)
            } else {
                vec![emoji::Seg::Text(&word)]
            };
            let mut parts: Vec<P> = vec![];
            for seg in segs {
                match seg {
                    emoji::Seg::Emoji(c) => match cx.emoji.get(ui.ctx(), c, px) {
                        Some((tex, size)) => parts.push(P::E(tex, size)),
                        None => parts.push(P::T(c.to_string())),
                    },
                    emoji::Seg::Text(s) => {
                        // an overlong token (no spaces, e.g. a URL) breaks by characters so it never leaves the column
                        let g = ui.painter().layout_no_wrap(s.to_string(), font.clone(), color);
                        if g.size().x > width && s.chars().count() > 1 {
                            let mut cur = String::new();
                            for c in s.chars() {
                                let mut probe = cur.clone();
                                probe.push(c);
                                if !cur.is_empty() && ui.painter().layout_no_wrap(probe, font.clone(), color).size().x > width {
                                    parts.push(P::T(std::mem::take(&mut cur)));
                                }
                                cur.push(c);
                            }
                            if !cur.is_empty() {
                                parts.push(P::T(cur));
                            }
                        } else {
                            parts.push(P::T(s.to_string()));
                        }
                    }
                }
            }
            let n = parts.len();
            for (k, part) in parts.into_iter().enumerate() {
                let last = k + 1 == n;
                let trail = if last { spaces as f32 * space_w } else { 0.0 };
                let breakable = !last || spaces > 0;
                match part {
                    P::T(piece) => {
                        let g = ui.painter().layout_no_wrap(piece, font.clone(), color);
                        it.push(flow::Item { w: g.size().x, h: g.size().y, trail, brk: breakable || !last });
                        atoms.push(Atom::Text { g, color, strike: st.strike, code: st.code });
                    }
                    P::E(tex, size) => {
                        it.push(flow::Item { w: size.x, h: nat_h, trail, brk: breakable || !last });
                        atoms.push(Atom::Emoji { tex, size });
                    }
                }
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
            Atom::Emoji { tex, size } => {
                // the picture's baseline sits on the text baseline of its line
                let asc = item.h * if small { 0.795 } else { 0.756 };
                let rect = Rect::from_min_size(pos2(x, top + asc - emoji::BASELINE * size.y), *size);
                ui.painter().image(*tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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
    ui.painter().galley(widgets::centered(chip, g.size()), g.clone(), color);
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
        // `.tray-head { justify-content: space-between; gap: 4px 12px }`: the title at the left and the controls
        // (`.tray-ctl`: use all, the nav, close; gap 8) at the right, or on rows of their own on a phone
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
        let title = if all_done { "All words checked" } else { "Check this word" };
        let title_w = tw(ui, title, theme::sans_semi(16.0));
        let use_all = (unresolved >= 2).then(|| format!("Use all {unresolved} suggestions"));
        let use_all_w = use_all.as_ref().map_or(0.0, |l| tw(ui, l, theme::sans_semi(14.0)) + 36.0);
        let end_w = if all_done { tw(ui, "Done", theme::sans_semi(14.0)) + 36.0 } else { TARGET };
        let nav_w = TARGET + 2.0 + 56.0 + 2.0 + TARGET;
        let ctl_w = use_all_w + if use_all.is_some() { 8.0 } else { 0.0 } + nav_w + 8.0 + end_w;
        let one_row = title_w + 12.0 + ctl_w <= ui.available_width();
        let head_title = |ui: &mut Ui, cx: &mut Cx| {
            let r = ui.allocate_response(vec2(title_w.max(1.0), TARGET), Sense::hover());
            ui.painter().text(pos2(r.rect.left(), r.rect.center().y), egui::Align2::LEFT_CENTER, title, theme::sans_semi(16.0), t.ink);
            cx.ctl.heading("tray-title", 2, title, TRAY, r.rect.min);
        };
        let head_ctl = |ui: &mut Ui, cx: &mut Cx| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if let Some(l) = &use_all {
                if widgets::button(cx.ctl, ui, t, C::new("use-all", l, TRAY), l, Look::Outline, 0.0) {
                    cx.acts.push(Act::UseAll);
                }
            }
            // `.nav { gap: 2px }` and `.count { min-width: 56px }`
            ui.allocate_ui_with_layout(vec2(nav_w, TARGET), Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if widgets::icon_button(cx.ctl, ui, t, C::new("prev-word", "Previous word to check", TRAY), Icon::Left, false) {
                    cx.acts.push(Act::StepWord(-1));
                }
                let count = format!("{} of {}", idx + 1, n);
                let cr = ui.allocate_response(vec2(56.0, TARGET), Sense::hover());
                ui.painter().text(cr.rect.center(), egui::Align2::CENTER_CENTER, &count, theme::sans(13.0), t.muted);
                if widgets::icon_button(cx.ctl, ui, t, C::new("next-word", "Next word to check", TRAY), Icon::Right, false) {
                    cx.acts.push(Act::StepWord(1));
                }
            });
            if all_done {
                if widgets::button(cx.ctl, ui, t, C::new("tray-done", "Done", TRAY), "Done", Look::Primary, 0.0) {
                    cx.acts.push(Act::Tray(false));
                }
            } else if widgets::icon_button(cx.ctl, ui, t, C::new("tray-close", "Close the word panel", TRAY), Icon::Close, false) {
                cx.acts.push(Act::Tray(false));
            }
        };
        if one_row {
            ui.horizontal(|ui| {
                head_title(ui, cx);
                ui.add_space((ui.available_width() - ctl_w).max(0.0));
                head_ctl(ui, cx);
            });
        } else {
            ui.horizontal(|ui| head_title(ui, cx));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                head_ctl(ui, cx);
            });
        }
        // `.tray-note { margin-top: 2px }`, then `.tray-body { margin-top: 14px }`
        ui.add_space(2.0);
        let note = s.note(&cur);
        let r = css_text(ui, &note, theme::sans(14.0), t.muted, 14.0 * 1.4, ui.available_width(), false);
        cx.ctl.text("tray-note", &note, TRAY, r.min);
        ui.add_space(14.0);

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
                    css_text(ui, "This line, as the student wrote it", theme::sans(13.0), t.muted, 13.0 * 1.4, col_w, false);
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
