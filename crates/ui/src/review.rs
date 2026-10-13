//! The review screen: header, page pane (image, strip, marks), transcript with flagged words inline, and the
//! "Check this word" tray. Follows design/Main.dc.html and js/ui/review.js + header.js.
//!
//! Drawing never mutates the session. Interactions are collected as `Act`s and applied after the frame, so
//! the borrow of the session stays shared while egui closures run.
use crate::state::{self, BoxMode, Entry, Kind, Session};
use crate::theme::{self, Tokens};
use crate::transcript::{self, Line, Run, Style};
use crate::twin::{HEADER, SCAN, TEXT, TRAY};
use crate::widgets::{self, Icon, Look, TARGET};
use crate::{App, Boot, Ctl, Img, SaveState, Slot, C};
use egui::{pos2, vec2, Align, Color32, Layout, Margin, Rect, RichText, Sense, Stroke, Ui};
use std::collections::HashMap;
use vishion_core::words;
use vishion_core::model::Page;

pub(crate) enum Act {
    Pick(String, String),
    SetSplit(bool),
    Original(bool),
    Student(i32),
    DoneNext,
    Copy,
    Boxes,
    SelectPage(String),
    Choose(&'static str, String),
    UseAll,
    StepWord(i32),
    Tray(bool),
    ReadLine(usize),
    ReadMove(i32),
    ReadClear,
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
    status: &'a Option<String>,
}

const TEXT_PX: f32 = 22.0;

pub(crate) fn show(app: &mut App, ctx: &egui::Context, t: &Tokens) {
    let App { ctl, session, math, images, typed, typed_for, status_msg, boot, notice, save, storage, .. } = app;
    let mut acts: Vec<Act> = vec![];
    match (&*boot, session.as_ref()) {
        (Boot::Ready, Some(s)) => {
            let split = s.split && ctx.screen_rect().width() >= 760.0;
            let mut cx = Cx { t, ctl: &mut *ctl, math: &mut *math, images: &*images, typed: &mut *typed, acts: &mut acts, save: &*save, storage: &*storage, notice: &*notice, status: &*status_msg };
            egui::TopBottomPanel::top("hdr")
                .exact_height(64.0)
                .frame(egui::Frame::none().fill(t.surface).inner_margin(Margin::symmetric(24.0, 8.0)).stroke(Stroke::new(1.0, t.line)))
                .show(ctx, |ui| header(ui, &mut cx, s));
            if split {
                let w = (ctx.screen_rect().width() * 0.5).clamp(380.0, 900.0);
                egui::SidePanel::left("scan")
                    .exact_width(w)
                    .resizable(false)
                    .frame(egui::Frame::none().fill(t.scanbg))
                    .show(ctx, |ui| scan(ui, &mut cx, s));
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(t.surface).stroke(Stroke::new(if split { 1.0 } else { 0.0 }, t.line)))
                .show(ctx, |ui| right_pane(ui, &mut cx, s));
        }
        (b, _) => {
            let (title, body) = match b {
                Boot::Loading => ("Opening your assignment", "Reading it from storage.".to_string()),
                Boot::Empty => ("No assignments yet", format!("Nothing is stored here ({}). Add or import an assignment to start reviewing.", storage)),
                Boot::Failed(m) => ("Couldn\u{2019}t open the assignment", m.clone()),
                Boot::Ready => ("No page selected", "Add pages and the transcript appears here.".to_string()),
            };
            egui::CentralPanel::default().frame(egui::Frame::none().fill(t.bg).inner_margin(Margin::same(40.0))).show(ctx, |ui| {
                let r = ui.label(RichText::new(title).font(theme::serif_semi(32.0)).color(t.ink));
                ctl.heading("empty-title", 1, title, TEXT, r.rect.min);
                let r = ui.label(RichText::new(&body).font(theme::sans(16.0)).color(t.muted));
                ctl.status("empty-note", &body, TEXT, r.rect.min);
            });
        }
    }

    // keyboard: arrow keys move the reading underline when nothing is being typed
    if session.is_some() && !ctx.wants_keyboard_input() {
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) {
                acts.push(Act::ReadMove(1));
            } else if i.key_pressed(egui::Key::ArrowUp) {
                acts.push(Act::ReadMove(-1));
            } else if i.key_pressed(egui::Key::Escape) {
                acts.push(Act::ReadClear);
            }
        });
    }

    if let Some(s) = session.as_mut() {
        for a in acts {
            if !matches!(a, Act::ReadLine(_) | Act::ReadMove(_) | Act::ReadClear) {
                *status_msg = None;
            }
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
                        *status_msg = Some(format!("{} is marked done. That was the last student.", name.unwrap_or_default()));
                    }
                }
                Act::Copy => {
                    if let Some(g) = s.group() {
                        ctx.copy_text(s.copy_text(&g));
                        *status_msg = Some(format!("Copied {}\u{2019}s text.", g.name));
                    }
                }
                Act::Boxes => s.boxes = s.boxes.next(),
                Act::SelectPage(id) => s.select(&id),
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
                    }
                }
                Act::ReadLine(i) => s.set_read_line(Some(i)),
                Act::ReadMove(d) => {
                    let n = s.current().map(|p| vishion_core::render::units(p.raw()).len()).unwrap_or(0);
                    if n > 0 {
                        let cur = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied();
                        let next = match cur {
                            None => if d > 0 { 0 } else { n - 1 },
                            Some(c) => (c as i64 + d as i64).clamp(0, n as i64 - 1) as usize,
                        };
                        s.set_read_line(Some(next));
                    }
                }
                Act::ReadClear => s.set_read_line(None),
            }
        }
        // the typed fix belongs to one word
        if *typed_for != s.sel_word {
            typed.clear();
            *typed_for = s.sel_word.clone();
        }
    }
}

// ------------------------------------------------------------------ header

fn header(ui: &mut Ui, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    let g = s.group();
    let pos = s.student_pos();
    let any_text = g.as_ref().is_some_and(|g| g.pages.iter().filter_map(|id| s.page(id)).any(|p| !p.raw().is_empty()));
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        if widgets::button(cx.ctl, ui, t, C::new("done", "Done, next student", HEADER).disabled(g.is_none()), "Done, next student", Look::Primary, 0.0) {
            cx.acts.push(Act::DoneNext);
        }
        if widgets::button(cx.ctl, ui, t, C::new("copy", "Copy text", HEADER).disabled(!any_text), "Copy text", Look::Outline, 0.0) {
            cx.acts.push(Act::Copy);
        }
        let split = s.split;
        let items = [("layout-split", "Side by side", split, false), ("layout-text", "Text only", !split, false)];
        match widgets::seg(cx.ctl, ui, t, HEADER, "layout", items) {
            Some(0) => cx.acts.push(Act::SetSplit(true)),
            Some(_) => cx.acts.push(Act::SetSplit(false)),
            None => {}
        }
        // save indicator
        let (text, col) = if let Some(m) = &cx.save.failed {
            (format!("Couldn\u{2019}t save: {m}"), t.bad)
        } else if cx.save.pending > 0 {
            ("Saving\u{2026}".to_string(), t.muted)
        } else {
            (cx.storage.to_string(), t.muted)
        };
        let r = ui.label(RichText::new(&text).font(theme::sans(13.0)).color(col));
        cx.ctl.status("save-state", &text, HEADER, r.rect.min);

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.label(RichText::new("Handwriting Transcriber").font(theme::sans_semi(15.0)).color(t.ink));
            let name = s.a.meta.name();
            if !name.is_empty() {
                ui.label(RichText::new(name).font(theme::sans_medium(14.0)).color(t.muted));
            }
            ui.spacing_mut().item_spacing.x = 4.0;
            let (i, n) = pos.unwrap_or((0, 0));
            if widgets::icon_button(cx.ctl, ui, t, C::new("prev-student", "Previous student", HEADER).disabled(pos.is_none() || i == 0), Icon::Left, false) {
                cx.acts.push(Act::Student(-1));
            }
            ui.allocate_ui_with_layout(vec2(100.0, TARGET), Layout::top_down(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let who = g.as_ref().map(|g| g.name.as_str()).unwrap_or("No student");
                ui.label(RichText::new(who).font(theme::sans_semi(14.0)).color(t.ink));
                if pos.is_some() {
                    ui.label(RichText::new(format!("{} of {}", i + 1, n)).font(theme::sans(13.0)).color(t.muted));
                }
            });
            if widgets::icon_button(cx.ctl, ui, t, C::new("next-student", "Next student", HEADER).disabled(pos.is_none() || i + 1 >= n), Icon::Right, false) {
                cx.acts.push(Act::Student(1));
            }
        });
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

fn scan(ui: &mut Ui, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    let g = s.group();
    let cur = s.current();
    let page_i = g.as_ref().and_then(|g| g.pages.iter().position(|id| Some(id) == s.selected.as_ref()));

    // title + tools
    egui::Frame::none()
        .fill(t.scanhead)
        .inner_margin(Margin { left: 20.0, right: 12.0, top: 4.0, bottom: 4.0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let title = match (page_i, &g) {
                    (Some(i), Some(g)) => format!("Original, page {} of {}", i + 1, g.pages.len()),
                    _ => "Original".to_string(),
                };
                let r = ui.label(RichText::new(&title).font(theme::sans_semi(14.0)).color(t.ink));
                cx.ctl.heading("scan-title", 2, &title, SCAN, r.rect.min);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = match s.boxes {
                        BoxMode::Off => "Marks on the page: off. Click to change.",
                        BoxMode::Flagged => "Marks on the page: underlines. Click to change.",
                        BoxMode::All => "Marks on the page: underlines and a box on every line. Click to change.",
                    };
                    if widgets::icon_button(cx.ctl, ui, t, C::new("boxes", label, SCAN).pressed(s.boxes != BoxMode::Off), Icon::Boxes, s.boxes != BoxMode::Off) {
                        cx.acts.push(Act::Boxes);
                    }
                });
            });
        });

    // strip of this student's pages
    egui::Frame::none()
        .fill(t.scanhead)
        .stroke(Stroke::new(1.0, t.scanline))
        .inner_margin(Margin::symmetric(20.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
                if let Some(g) = &g {
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
                        let r = ui.allocate_response(vec2(TARGET, 56.0), Sense::click());
                        let rect = r.rect;
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
                        if cx.ctl.reg(ui, &r, C::new(&format!("page-{id}"), &label, SCAN).pressed(on)) {
                            cx.acts.push(Act::SelectPage(id.clone()));
                        }
                    }
                }
            });
        });

    // the page
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(32.0);
        let Some(p) = cur else {
            ui.vertical_centered(|ui| ui.label(RichText::new("No page selected.").color(t.ink)));
            return;
        };
        let slot = cx.images.get(p.id());
        match slot {
            Some(Slot::Ready(im)) => page_image(ui, cx, s, p, im),
            Some(Slot::Missing) => {
                ui.vertical_centered(|ui| ui.label(RichText::new("The image of this page is missing.").color(t.ink)));
            }
            _ => {
                ui.vertical_centered(|ui| ui.label(RichText::new("Loading the page\u{2026}").color(t.ink)));
            }
        }
        ui.add_space(56.0);
    });
}

fn page_image(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page, im: &Img) {
    let t = cx.t;
    let avail = ui.available_width();
    let w = (avail - 64.0).clamp(120.0, 1100.0);
    let h = w * im.size.1 as f32 / im.size.0.max(1) as f32;
    let (outer, _) = ui.allocate_exact_size(vec2(avail, h), Sense::hover());
    let rect = Rect::from_center_size(outer.center(), vec2(w, h));
    let pt = ui.painter();
    // soft shadow: two translucent layers instead of a blur
    pt.rect_filled(rect.translate(vec2(0.0, 10.0)).expand(2.0), 6.0, Color32::from_black_alpha(26));
    pt.rect_filled(rect.translate(vec2(0.0, 2.0)), 3.0, Color32::from_black_alpha(46));
    pt.image(im.full.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);

    if s.boxes == BoxMode::Off {
        return;
    }
    let n = vishion_core::render::units(p.raw()).len();
    let Some((boxes, approx)) = state::geometry(p, n) else { return };
    let (kinds, sel_unit) = s.flagged_lines(p);
    let at = |b: &state::BBox| Rect::from_min_size(rect.min + vec2(b.x * w, b.y * h), vec2(b.w * w, b.h * h));
    if s.boxes == BoxMode::All && !approx {
        for b in &boxes {
            let r = at(b);
            pt.rect_filled(r, 2.0, t.accent.gamma_multiply(0.10));
            pt.rect_stroke(r, 2.0, Stroke::new(1.0, t.accent.gamma_multiply(0.55)));
        }
    }
    for (i, kind) in kinds {
        let Some(b) = boxes.get(i) else { continue };
        let r = at(b);
        let sel = sel_unit == Some(i);
        let thick = if sel { 6.0 } else { 4.0 };
        let y = r.bottom() + 1.0;
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
    if let Some(i) = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied() {
        if let Some(b) = boxes.get(i) {
            let r = at(b);
            pt.rect_filled(Rect::from_min_max(pos2(r.left(), r.bottom() + 8.0), pos2(r.right(), r.bottom() + 10.0)), 1.0, t.accent);
        }
    }
}

// ------------------------------------------------------------------ right pane

fn right_pane(ui: &mut Ui, cx: &mut Cx, s: &Session) {
    let t = cx.t;
    let g = s.group();
    // the tray is a bottom panel inside this pane so the transcript scrolls above it
    let entries: Vec<Entry> = g.as_ref().map(|g| s.entries(g)).unwrap_or_default();
    if !entries.is_empty() && s.tray_open {
        egui::TopBottomPanel::bottom("tray")
            .resizable(false)
            .frame(egui::Frame::none().fill(t.sunken).stroke(Stroke::new(1.0, t.line)))
            .show_inside(ui, |ui| tray(ui, cx, s, &entries));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let avail = ui.available_width();
        let side = ((avail - 760.0) / 2.0).max(40.0).min(avail / 4.0).max(16.0);
        egui::Frame::none().inner_margin(Margin { left: side, right: side, top: 32.0, bottom: 24.0 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            transcript_pane(ui, cx, s, g.as_ref(), &entries);
        });
    });
}

fn transcript_pane(ui: &mut Ui, cx: &mut Cx, s: &Session, g: Option<&vishion_core::groups::Group>, entries: &[Entry]) {
    let t = cx.t;
    let (Some(p), Some(g)) = (s.current(), g) else {
        ui.label(RichText::new("No page selected").font(theme::serif_semi(24.0)).color(t.ink));
        ui.label(RichText::new("Add pages and the transcript appears here.").color(t.muted));
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
    let words_total = g.words;
    let meta = {
        let base = if words_total > 0 {
            format!("{}, {} words", state::plural(g.pages.len(), "page", "pages"), words_total)
        } else {
            state::plural(g.pages.len(), "page", "pages")
        };
        if g.source.as_ref().and_then(|v| v.as_str()) == Some("file") {
            format!("{base}. No name was found on the page.")
        } else {
            base
        }
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(12.0, 6.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let r = ui.label(RichText::new(&g.name).font(theme::serif_semi(32.0)).color(t.ink));
            cx.ctl.heading("student-name", 1, &g.name, TEXT, r.rect.min);
            ui.label(RichText::new(&meta).font(theme::sans(14.0)).color(t.muted));
        });
        let r = widgets::pill(ui, t, kind, &label);
        cx.ctl.status("student-status", &label, TEXT, r.min);
    });

    // legend + version
    ui.add_space(14.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(20.0, 8.0);
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
    ui.add_space(18.0);

    // body by page state
    match p.status() {
        "transcribing" => {
            ui.label(RichText::new(p.raw()).font(theme::serif(TEXT_PX)).color(t.ink));
            ui.add_space(8.0);
            ui.label(RichText::new("Reading this page. The text appears as it is read.").font(theme::sans(13.0)).color(t.muted));
        }
        "pending" if p.raw().is_empty() => {
            state_block(ui, t, "This page has no transcript yet", "Transcribe it from the assignment inbox.");
        }
        _ => {
            if p.status() == "error" {
                egui::Frame::none().fill(t.bad_bg).rounding(8.0).inner_margin(Margin::same(14.0)).show(ui, |ui| {
                    let msg = if p.s("error").is_empty() { "The page could not be transcribed." } else { p.s("error") };
                    let r = ui.label(RichText::new(msg).color(t.bad));
                    cx.ctl.status("page-error", msg, TEXT, r.rect.min);
                });
                ui.add_space(12.0);
            }
            if !p.raw().is_empty() {
                lines(ui, cx, s, p);
                foot(ui, cx, s, p, !entries.is_empty());
            }
        }
    }
}

fn state_block(ui: &mut Ui, t: &Tokens, title: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(title).font(theme::sans_semi(16.0)).color(t.ink));
        ui.label(RichText::new(text).color(t.muted));
        ui.add_space(24.0);
    });
}

fn legend(ui: &mut Ui, t: &Tokens) {
    let sw = |ui: &mut Ui, label: &str, kind: Kind| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let g = ui.painter().layout_no_wrap("word".into(), theme::sans(13.0), if kind == Kind::Bad { t.bad } else { t.ink });
            let (r, _) = ui.allocate_exact_size(g.size() + vec2(12.0, 2.0), Sense::hover());
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
            ui.label(RichText::new(label).font(theme::sans(13.0)).color(t.muted));
        });
    };
    sw(ui, "Spelling to check", Kind::Warn);
    sw(ui, "Couldn\u{2019}t read", Kind::Bad);
    sw(ui, "You checked it", Kind::Ok);
}

/// The transcript: one wrapped paragraph per display unit, math typeset, flagged words as controls.
fn lines(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page) {
    let t = cx.t;
    let marked = s.marked(p);
    let ls = transcript::lines(&marked);
    let items = words::items(p);
    let read = s.selected.as_ref().and_then(|id| s.read_line.get(id)).copied();
    let sel_key = s.sel_word.as_ref().filter(|(id, _)| id == p.id()).map(|(_, k)| k.clone());
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    for line in &ls {
        let id = format!("line-{}-{}", p.id(), line.index);
        let top = ui.cursor().min;
        cx.ctl.begin_para();
        let block = if line.margin {
            ui.add_space(8.0);
            let r = ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                ui.label(RichText::new("Margin note ").font(theme::sans_semi(16.0)).color(t.ink));
                cx.ctl.para_text("Margin note: ");
                line_runs(ui, cx, s, p, line, &items, sel_key.as_deref(), true);
            });
            r.response.rect
        } else if let Some(m) = line.display_only() {
            let r = ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                if !cx.math.paint(ui, &m.tex, true, TEXT_PX, t.ink) {
                    ui.label(RichText::new(&m.tex).monospace().color(t.bad));
                }
                ui.add_space(6.0);
            });
            cx.ctl.para_math(&m.tex);
            r.response.rect
        } else {
            let r = ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
                ui.set_min_height(34.0);
                line_runs(ui, cx, s, p, line, &items, sel_key.as_deref(), false);
            });
            r.response.rect
        };
        cx.ctl.end_para(&id, TEXT, top);
        if read == Some(line.index) {
            let y = block.bottom() + 1.0;
            ui.painter().rect_filled(Rect::from_min_max(pos2(block.left(), y), pos2(block.right(), y + 2.0)), 1.0, t.accent);
        }
        // a click on the line text (not on a word) moves the reading underline
        let clicked = ui.input(|i| i.pointer.primary_clicked() && i.pointer.interact_pos().is_some_and(|p| block.contains(p)));
        if clicked && !cx.acts.iter().any(|a| matches!(a, Act::Pick(..))) {
            cx.acts.push(Act::ReadLine(line.index));
        }
        ui.add_space(10.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn line_runs(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page, line: &Line, items: &[words::Item], sel_key: Option<&str>, small: bool) {
    let t = cx.t;
    let px = if small { 16.0 } else { TEXT_PX };
    for run in &line.runs {
        match run {
            Run::Text { s: txt, st } => {
                cx.ctl.para_text(txt);
                ui.label(styled(txt, st, px, t, small));
            }
            Run::Math(n) => {
                let m = &line.math[*n];
                cx.ctl.para_math(&m.tex);
                if !cx.math.paint(ui, &m.tex, m.display, px, t.ink) {
                    // plan rule 4: source in monospace red, as render.js does
                    ui.label(RichText::new(&m.tex).monospace().size(px * 0.85).color(t.bad));
                }
            }
            Run::Word { key, text, st } => {
                let Some(it) = items.iter().find(|i| &i.key == key) else {
                    cx.ctl.para_text(text);
                    ui.label(styled(text, st, px, t, small));
                    continue;
                };
                let (kind, checked) = s.word_kind(it);
                let shown = if text.is_empty() { s.word_text(it) } else { text.clone() };
                let label = if checked {
                    format!("Checked word: {shown}. Show options.")
                } else if it.kind == "illegible" {
                    "Word that could not be read. Show options.".to_string()
                } else {
                    format!("Spelling to check: {shown}. Show options.")
                };
                let selected = sel_key == Some(key.as_str());
                let id = format!("word-{}-{}", p.id(), key);
                if word_chip(ui, cx, &id, &label, &shown, st, kind, checked, selected) {
                    cx.acts.push(Act::Pick(p.id().to_string(), key.clone()));
                }
            }
        }
    }
}

fn styled(txt: &str, st: &Style, px: f32, t: &Tokens, small: bool) -> RichText {
    let font = if small {
        theme::sans(px)
    } else if st.bold {
        theme::serif_semi(px)
    } else if st.italic {
        theme::serif_italic(px)
    } else {
        theme::serif(px)
    };
    let mut r = RichText::new(txt).font(font).color(if small { t.muted } else { t.ink });
    if st.code {
        r = r.monospace().size(px * 0.85).background_color(t.sunken);
    }
    if st.strike {
        r = r.strikethrough().color(t.muted);
    }
    r
}

/// A flagged word inline in the transcript. Layout height keeps the line rhythm of the design; the hit
/// target is at least 44 x 44 (the S8 gap).
#[allow(clippy::too_many_arguments)]
fn word_chip(ui: &mut Ui, cx: &mut Cx, id: &str, label: &str, text: &str, st: &Style, kind: Kind, checked: bool, selected: bool) -> bool {
    let t = cx.t;
    let font = if st.bold { theme::serif_semi(TEXT_PX) } else if st.italic { theme::serif_italic(TEXT_PX) } else { theme::serif(TEXT_PX) };
    let color = if kind == Kind::Bad && !checked { t.bad } else { t.ink };
    let g = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let w = (g.size().x + 8.0).max(TARGET);
    let (slot, _) = ui.allocate_exact_size(vec2(w + 6.0, 42.0), Sense::hover());
    let chip = Rect::from_min_size(slot.min, vec2(w, 42.0));
    let hit = chip.expand2(vec2(0.0, (TARGET - 42.0) / 2.0));
    let r = ui.interact(hit, ui.id().with(id), Sense::click());
    let hl = Rect::from_center_size(chip.center(), g.size() + vec2(8.0, 4.0));
    let hover = r.hovered();
    match (kind, checked) {
        (_, true) | (Kind::Ok, _) => {
            ui.painter().line_segment([pos2(hl.left() + 2.0, hl.bottom() + 1.0), pos2(hl.right() - 2.0, hl.bottom() + 1.0)], Stroke::new(2.0, t.ok));
        }
        (Kind::Bad, false) => {
            ui.painter().rect_filled(hl, 4.0, t.bad_bg);
            ui.painter().add(egui::Shape::dashed_line(
                &[pos2(hl.left() + 2.0, hl.bottom() + 1.0), pos2(hl.right() - 2.0, hl.bottom() + 1.0)],
                Stroke::new(2.0, t.bad),
                5.0,
                3.0,
            ));
        }
        (Kind::Warn, false) => {
            ui.painter().rect_filled(hl, 4.0, t.warn);
        }
    }
    if hover {
        ui.painter().rect_stroke(hl.expand(1.0), 4.0, Stroke::new(1.0, t.line2));
    }
    if selected {
        ui.painter().rect_stroke(hl.expand(2.0), 5.0, Stroke::new(2.0, t.ink));
    }
    ui.painter().galley(chip.center() - g.size() / 2.0, g, color);
    cx.ctl.reg(ui, &r, C::new(id, label, TEXT))
}

fn foot(ui: &mut Ui, cx: &mut Cx, s: &Session, p: &Page, has_words: bool) {
    let t = cx.t;
    ui.add_space(18.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(16.0, 6.0);
        let tokens = p.get("timings").and_then(|x| x.get("tokens")).and_then(|v| v.as_f64()).unwrap_or(0.0);
        if tokens > 0.0 {
            let ms = p.get("timings").and_then(|x| x.get("ms")).and_then(|v| v.as_f64()).unwrap_or(0.0);
            ui.label(RichText::new(format!("{} tokens in {:.1}s", tokens, ms / 1000.0)).font(theme::sans(13.0)).color(t.muted));
        }
        if s.selected.as_ref().and_then(|id| s.read_line.get(id)).is_none() {
            ui.label(RichText::new("Click a line, or use the up and down arrow keys, to underline where you are.").font(theme::sans(13.0)).color(t.muted));
        }
        if has_words && !s.tray_open && widgets::button(cx.ctl, ui, t, C::new("tray-open", "Check words", TEXT), "Check words", Look::Outline, 0.0) {
            cx.acts.push(Act::Tray(true));
        }
    });
    if let Some(m) = cx.status {
        let r = ui.label(RichText::new(m).font(theme::sans(14.0)).color(t.ok));
        cx.ctl.status("toast", m, TEXT, r.rect.min);
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
    let side = ((avail - 760.0) / 2.0).max(20.0).min(avail / 4.0).max(12.0);
    egui::Frame::none().inner_margin(Margin { left: side, right: side, top: 12.0, bottom: 20.0 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        // head
        ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if all_done {
                if widgets::button(cx.ctl, ui, t, C::new("tray-done", "Done", TRAY), "Done", Look::Primary, 0.0) {
                    cx.acts.push(Act::Tray(false));
                }
            } else if widgets::icon_button(cx.ctl, ui, t, C::new("tray-close", "Close the word panel", TRAY), Icon::Close, false) {
                cx.acts.push(Act::Tray(false));
            }
            if widgets::icon_button(cx.ctl, ui, t, C::new("next-word", "Next word to check", TRAY), Icon::Right, false) {
                cx.acts.push(Act::StepWord(1));
            }
            ui.label(RichText::new(format!("{} of {}", idx + 1, n)).font(theme::sans(13.0)).color(t.muted));
            if widgets::icon_button(cx.ctl, ui, t, C::new("prev-word", "Previous word to check", TRAY), Icon::Left, false) {
                cx.acts.push(Act::StepWord(-1));
            }
            if unresolved >= 2 {
                let l = format!("Use all {unresolved} suggestions");
                if widgets::button(cx.ctl, ui, t, C::new("use-all", &l, TRAY), &l, Look::Outline, 0.0) {
                    cx.acts.push(Act::UseAll);
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let title = if all_done { "All words checked" } else { "Check this word" };
                let r = ui.label(RichText::new(title).font(theme::sans_semi(16.0)).color(t.ink));
                cx.ctl.heading("tray-title", 2, title, TRAY, r.rect.min);
            });
        }));
        let note = s.note(&cur);
        let r = ui.label(RichText::new(&note).font(theme::sans(14.0)).color(t.muted));
        cx.ctl.text("tray-note", &note, TRAY, r.rect.min);
        ui.add_space(8.0);

        let crop = page.and_then(|p| crop_for(p, &cur, cx.images));
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(20.0, 8.0);
            if let Some((tex, uv, size)) = crop {
                ui.vertical(|ui| {
                    ui.set_width(size.x);
                    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                    ui.painter().rect_filled(r.expand(4.0), 6.0, t.paper);
                    ui.painter().image(tex, r, uv, Color32::WHITE);
                    ui.label(RichText::new("This line, as the student wrote it").font(theme::sans(13.0)).color(t.muted));
                });
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.set_min_width(240.0);
                alternatives(ui, cx, s, &cur);
            });
        });
    });
}

fn alternatives(ui: &mut Ui, cx: &mut Cx, s: &Session, cur: &Entry) {
    let t = cx.t;
    let w = ui.available_width();
    for (i, a) in s.alts(cur).iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(w, 52.0), Sense::click());
        let r = ui.interact(r, ui.id().with(("alt", i)), Sense::click());
        let fill = if a.on { t.ok_bg } else if r.hovered() { t.sunken } else { t.surface };
        let line = if a.on { t.ok } else { t.line2 };
        ui.painter().rect_filled(r.rect, 8.0, fill);
        ui.painter().rect_stroke(r.rect, 8.0, Stroke::new(1.0, line));
        let c = pos2(r.rect.left() + 14.0 + 8.0, r.rect.center().y);
        ui.painter().circle(c, 8.0, t.surface, Stroke::new(if a.on { 5.0 } else { 2.0 }, if a.on { t.ok } else { t.faint }));
        let main = ui.painter().layout_no_wrap(a.label.clone(), theme::serif_semi(19.0), t.ink);
        let hint = ui.painter().layout_no_wrap(a.hint.clone(), theme::sans(13.0), t.muted);
        let x = r.rect.left() + 14.0 + 16.0 + 12.0;
        let top = r.rect.center().y - (main.size().y + hint.size().y) / 2.0;
        let hy = top + main.size().y;
        ui.painter().galley(pos2(x, top), main, t.ink);
        ui.painter().galley(pos2(x, hy), hint, t.muted);
        let label = format!("{}. {}", a.label, a.hint);
        if cx.ctl.reg(ui, &r, C::new(&format!("alt-{}", a.via), &label, TRAY).pressed(a.on)) {
            cx.acts.push(Act::Choose(a.via, a.text.clone()));
        }
    }
    // typed fix
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let btn_w = 104.0;
        let tw = (ui.available_width() - btn_w - 8.0).max(120.0);
        // the field is drawn inside a 44pt slot; the slot is the target the twin reports
        let (slot, _) = ui.allocate_exact_size(vec2(tw, TARGET), Sense::hover());
        let edit = egui::TextEdit::singleline(&mut *cx.typed)
            .hint_text("Type what it says")
            .font(theme::sans(14.0))
            .margin(vec2(12.0, 13.0))
            .min_size(slot.size())
            .desired_width(tw - 24.0);
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
    let (boxes, approx) = state::geometry(p, n)?;
    if approx {
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
