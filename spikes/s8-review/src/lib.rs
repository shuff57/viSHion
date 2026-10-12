//! S8: the Main/review screen (design/Main.dc.html) in egui, with a keyed hidden-DOM control layer on web.
//! Every interactive widget goes through `App::ctl`, which (1) reports activation from egui OR from the
//! DOM twin, (2) registers a role/label for the twin, (3) draws a focus ring when the twin has focus.
use egui::{Color32, FontId, Response, RichText, Stroke, Ui, Vec2};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const INK: Color32 = Color32::from_rgb(0x1B, 0x22, 0x30);
const MUTED: Color32 = Color32::from_rgb(0x55, 0x60, 0x73);
const WARN: Color32 = Color32::from_rgb(0xFF, 0xE9, 0xA8);
const BAD: Color32 = Color32::from_rgb(0xB3, 0x26, 0x1E);
const BAD_BG: Color32 = Color32::from_rgb(0xFB, 0xE4, 0xE1);
const OK: Color32 = Color32::from_rgb(0x1E, 0x7A, 0x46);
const LINE: Color32 = Color32::from_rgb(0xD9, 0xDE, 0xE7);
const PAPER: Color32 = Color32::from_rgb(0xFF, 0xFE, 0xF8);

#[derive(Clone, Copy, PartialEq)]
enum Via { Suggested, Written, Typed }

struct Item {
    base: String,
    term: Option<String>, // None => [illegible]
    via: Option<Via>,
    typed: String,
}

impl Item {
    fn shown(&self, corrected: bool) -> String {
        match (self.via, &self.term) {
            (Some(Via::Typed), _) if corrected => self.typed.clone(),
            (Some(Via::Suggested), Some(t)) if corrected => t.clone(),
            _ => self.base.clone(),
        }
    }
}

enum Tok { W(&'static str), Struck(&'static str), Flag(usize) }

#[derive(Clone, PartialEq)]
struct Reg { id: String, role: &'static str, label: String, pressed: Option<bool>, pos: (i32, i32) }

/// Queue between DOM events and egui: "act:<id>" activates, "focus:<id>" moves the ring.
type Queue = Arc<Mutex<Vec<String>>>;

pub struct App {
    students: Vec<&'static str>,
    si: usize,
    split: bool,
    corrected: bool,
    items: Vec<Item>,
    paras: Vec<Vec<Tok>>,
    cur: usize,
    done: HashSet<usize>,
    copied: bool,
    // control layer
    queue: Queue,
    act: HashSet<String>,
    focus: Option<String>,
    regs: Vec<Reg>,
    last_regs: Vec<Reg>,
    ctx: egui::Context,
}

impl App {
    pub fn new(ctx: egui::Context) -> Self {
        let items = vec![
            Item { base: "confidance".into(), term: Some("confidence".into()), via: None, typed: String::new() },
            Item { base: "[illegible]".into(), term: None, via: None, typed: String::new() },
            Item { base: "standerd".into(), term: Some("standard".into()), via: None, typed: String::new() },
        ];
        use Tok::*;
        let paras = vec![
            vec![W("The"), W("sample"), W("mean"), W("gives"), W("a"), Flag(0), W("interval"), W("for"), W("the"), W("true"), W("mean"), W("of"), Flag(1), W("minutes."),
                 Struck("probably"), W("We"), W("use"), W("the"), Flag(2), W("error"), W("of"), W("the"), W("mean"), W("here.")],
            vec![W("So"), W("the"), W("real"), W("average"), W("is"), W("likely"), W("between"), W("18.4"), W("and"), W("24.1"), W("minutes.")],
        ];
        let app = Self {
            students: vec!["Ana L.", "Ben T.", "Maya R.", "Omar K."], si: 2, split: true, corrected: true, items, paras, cur: 0,
            done: HashSet::new(), copied: false, queue: Arc::new(Mutex::new(vec![])), act: HashSet::new(), focus: None,
            regs: vec![], last_regs: vec![], ctx,
        };
        #[cfg(target_arch = "wasm32")]
        web::install(app.queue.clone(), app.ctx.clone());
        app
    }

    fn open_count(&self) -> usize {
        self.items.iter().filter(|i| i.via.is_none()).count()
    }

    /// The single funnel for interactive widgets. See module doc.
    fn ctl(&mut self, ui: &Ui, r: &Response, id: &str, role: &'static str, label: &str, pressed: Option<bool>) -> bool {
        self.regs.push(Reg { id: id.into(), role, label: label.into(), pressed, pos: (r.rect.min.y as i32 / 16, r.rect.min.x as i32) });
        if self.focus.as_deref() == Some(id) {
            ui.painter().rect_stroke(r.rect.expand(2.0), 6.0, Stroke::new(2.0, INK));
        }
        let from_dom = self.act.remove(id);
        r.clicked() || from_dom
    }

    fn seg(&mut self, ui: &mut Ui, id: &str, label: &str, on: bool) -> bool {
        let t = RichText::new(label).strong().color(if on { Color32::WHITE } else { INK });
        let b = egui::Button::new(t).fill(if on { INK } else { Color32::from_rgb(0xEC, 0xEF, 0xF4) }).rounding(8.0).min_size(Vec2::new(0.0, 44.0));
        let r = ui.add(b);
        self.ctl(ui, &r, id, "button", label, Some(on))
    }

    fn plain_btn(&mut self, ui: &mut Ui, id: &str, label: &str, w: f32) -> bool {
        let r = ui.add_sized([w, 44.0], egui::Button::new(RichText::new(label).strong()).rounding(8.0));
        self.ctl(ui, &r, id, "button", label, None)
    }

    fn drain_queue(&mut self) {
        let msgs: Vec<String> = std::mem::take(&mut *self.queue.lock().unwrap());
        for m in msgs {
            if let Some(id) = m.strip_prefix("act:") {
                self.act.insert(id.into());
            } else if let Some(id) = m.strip_prefix("focus:") {
                self.focus = Some(id.into());
            }
        }
    }

    fn flag_button(&mut self, ui: &mut Ui, k: usize) {
        let it = &self.items[k];
        let text = it.shown(self.corrected);
        let (bg, fg) = match (it.via, it.term.is_some()) {
            (None, true) => (WARN, INK),
            (None, false) => (BAD_BG, BAD),
            (Some(_), _) => (Color32::TRANSPARENT, INK),
        };
        let resolved = it.via.is_some();
        let label = format!("{}. {} word to check: {}", k + 1, if resolved { "Checked" } else { "Open" }, text);
        let mut rt = RichText::new(text).font(FontId::proportional(20.0)).color(fg).background_color(bg);
        if resolved { rt = rt.underline(); }
        let r = ui.add(egui::Button::new(rt).frame(false));
        if resolved {
            let y = r.rect.bottom() - 2.0;
            ui.painter().line_segment([egui::pos2(r.rect.left(), y), egui::pos2(r.rect.right(), y)], Stroke::new(2.0, OK));
        }
        if self.ctl(ui, &r, &format!("word-{k}"), "button", &label, None) {
            self.cur = k;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        self.drain_queue();
        self.regs.clear();

        let mut v = egui::Visuals::light();
        v.override_text_color = Some(INK);
        ctx.set_visuals(v);

        egui::TopBottomPanel::top("hdr").min_height(64.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Handwriting Transcriber").strong());
                ui.add_space(12.0);
                let r = ui.add_sized([44.0, 44.0], egui::Button::new("‹").frame(false));
                if self.ctl(ui, &r, "prev-student", "button", "Previous student", None) && self.si > 0 { self.si -= 1; }
                ui.vertical(|ui| {
                    ui.label(RichText::new(self.students[self.si]).strong());
                    ui.label(RichText::new(format!("{} of {}", self.si + 1, self.students.len())).color(MUTED).small());
                });
                let r = ui.add_sized([44.0, 44.0], egui::Button::new("›").frame(false));
                if self.ctl(ui, &r, "next-student", "button", "Next student", None) && self.si + 1 < self.students.len() { self.si += 1; }
                ui.add_space(24.0);
                let s = self.split;
                if self.seg(ui, "layout-split", "Side by side", s) { self.split = true; }
                if self.seg(ui, "layout-text", "Text only", !s) { self.split = false; }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.plain_btn(ui, "done", "Done, next student", 170.0) && self.si + 1 < self.students.len() { self.si += 1; }
                    if self.plain_btn(ui, "copy", "Copy text", 110.0) {
                        let text: String = self.paras.iter().map(|p| p.iter().map(|t| match t {
                            Tok::W(w) => w.to_string(), Tok::Struck(_) => String::new(), Tok::Flag(k) => self.items[*k].shown(self.corrected),
                        }).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n");
                        ctx.copy_text(text);
                        self.copied = true;
                    }
                });
            });
        });

        if !self.items.is_empty() {
            egui::TopBottomPanel::bottom("tray").min_height(150.0).show(ctx, |ui| {
                let k = self.cur;
                let (base, term, via) = (self.items[k].base.clone(), self.items[k].term.clone(), self.items[k].via);
                ui.horizontal(|ui| {
                    ui.heading("Check this word");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = ui.add_sized([44.0, 44.0], egui::Button::new("›").frame(false));
                        if self.ctl(ui, &r, "next-word", "button", "Next word to check", None) { self.cur = (self.cur + 1) % self.items.len(); }
                        ui.label(RichText::new(format!("{} of {}", k + 1, self.items.len())).color(MUTED));
                        let r = ui.add_sized([44.0, 44.0], egui::Button::new("‹").frame(false));
                        if self.ctl(ui, &r, "prev-word", "button", "Previous word to check", None) { self.cur = (self.cur + self.items.len() - 1) % self.items.len(); }
                    });
                });
                let note = match (&term, via) {
                    (Some(t), None) => format!("The handwriting looks like “{base}”. A likely word is “{t}”."),
                    (None, None) => "This word was not readable. Type what you think it says.".to_string(),
                    (_, Some(_)) => "You checked this one.".to_string(),
                };
                ui.label(RichText::new(note).color(MUTED));
                ui.horizontal(|ui| {
                    if let Some(t) = term.clone() {
                        if self.plain_btn(ui, "use-suggested", &format!("Use “{t}”"), 200.0) { self.items[k].via = Some(Via::Suggested); }
                        if self.plain_btn(ui, "keep-written", "Keep what was written", 200.0) { self.items[k].via = Some(Via::Written); }
                    } else {
                        let mut typed = std::mem::take(&mut self.items[k].typed);
                        let r = ui.add_sized([240.0, 44.0], egui::TextEdit::singleline(&mut typed).hint_text("Type the word"));
                        self.items[k].typed = typed;
                        let entered = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        self.regs.push(Reg { id: "type-word".into(), role: "textbox", label: "Type the word".into(), pressed: None, pos: (r.rect.min.y as i32 / 16, r.rect.min.x as i32) });
                        if self.focus.as_deref() == Some("type-word") && self.act.remove("type-word") { r.request_focus(); }
                        let save = self.plain_btn(ui, "save-typed", "Save", 90.0);
                        if (save || entered) && !self.items[k].typed.trim().is_empty() { self.items[k].via = Some(Via::Typed); }
                    }
                    if via.is_some() && self.plain_btn(ui, "undo", "Undo", 90.0) { self.items[k].via = None; }
                });
            });
        }

        if self.split {
            egui::SidePanel::left("scan").min_width(380.0).show(ctx, |ui| {
                ui.label(RichText::new("Page 1").strong());
                ui.add_space(8.0);
                egui::Frame::none().fill(PAPER).stroke(Stroke::new(1.0, LINE)).inner_margin(24.0).show(ui, |ui| {
                    ui.set_min_height(360.0);
                    for p in &self.paras {
                        let text: Vec<String> = p.iter().map(|t| match t { Tok::W(w) | Tok::Struck(w) => w.to_string(), Tok::Flag(k) => self.items[*k].base.clone() }).collect();
                        ui.label(RichText::new(text.join(" ")).font(FontId::monospace(17.0)).color(Color32::from_rgb(0x26, 0x35, 0x8F)));
                        ui.add_space(14.0);
                    }
                });
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(self.students[self.si]);
            let open = self.open_count();
            let status = if open == 0 { "Ready".to_string() } else { format!("{open} words to check") };
            ui.label(RichText::new(status).color(if open == 0 { OK } else { MUTED }));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let c = self.corrected;
                if self.seg(ui, "ver-corrected", "Corrected", c) { self.corrected = true; }
                if self.seg(ui, "ver-original", "Original", !c) { self.corrected = false; }
            });
            ui.add_space(12.0);
            let paras = std::mem::take(&mut self.paras);
            for p in &paras {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(5.0, 10.0);
                    for t in p {
                        match t {
                            Tok::W(w) => { ui.label(RichText::new(*w).font(FontId::proportional(20.0))); }
                            Tok::Struck(w) => { ui.label(RichText::new(*w).font(FontId::proportional(20.0)).strikethrough().color(MUTED)); }
                            Tok::Flag(k) => self.flag_button(ui, *k),
                        }
                    }
                });
                ui.add_space(10.0);
            }
            self.paras = paras;
            if self.copied { ui.label(RichText::new("Copied.").color(OK)); }
        });

        // reading order = visual order (row bands of 16px, then x), so Tab matches what a sighted user sees
        self.regs.sort_by_key(|r| r.pos);
        // publish the control list for the DOM twin only when it changed
        if self.regs != self.last_regs {
            #[cfg(target_arch = "wasm32")]
            web::sync(&self.regs);
            self.last_regs = self.regs.clone();
        }
        let _ = &self.done;
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{Queue, Reg};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    pub fn host() -> Option<web_sys::Element> {
        web_sys::window()?.document()?.get_element_by_id("controls")
    }

    /// One delegated listener pair on the container. Nodes are reconciled by id so focus survives a re-sync.
    pub fn install(q: Queue, ctx: egui::Context) {
        let Some(h) = host() else { return };
        let (q1, c1) = (q.clone(), ctx.clone());
        let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            if let Some(id) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).and_then(|el| el.get_attribute("data-id")) {
                q1.lock().unwrap().push(format!("act:{id}"));
                c1.request_repaint();
            }
        });
        let on_focus = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            if let Some(id) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).and_then(|el| el.get_attribute("data-id")) {
                q.lock().unwrap().push(format!("focus:{id}"));
                ctx.request_repaint();
            }
        });
        let _ = h.add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref());
        let _ = h.add_event_listener_with_callback("focusin", on_focus.as_ref().unchecked_ref());
        on_click.forget();
        on_focus.forget();
    }

    pub fn sync(regs: &[Reg]) {
        let (Some(h), Some(doc)) = (host(), web_sys::window().and_then(|w| w.document())) else { return };
        let mut want: Vec<String> = vec![];
        for r in regs {
            want.push(r.id.clone());
            let sel = format!("[data-id=\"{}\"]", r.id);
            let el = match h.query_selector(&sel).ok().flatten() {
                Some(e) => e,
                None => {
                    let tag = if r.role == "textbox" { "input" } else { "button" };
                    let Ok(e) = doc.create_element(tag) else { continue };
                    let _ = e.set_attribute("data-id", &r.id);
                    if r.role != "textbox" { let _ = e.set_attribute("type", "button"); }
                    let _ = h.append_child(&e);
                    e
                }
            };
            let _ = el.set_attribute("aria-label", &r.label);
            match r.pressed {
                Some(p) => { let _ = el.set_attribute("aria-pressed", if p { "true" } else { "false" }); }
                None => { let _ = el.remove_attribute("aria-pressed"); }
            }
            el.set_text_content(if r.role == "textbox" { None } else { Some(&r.label) });
        }
        // drop twins whose widget is gone
        let kids = h.children();
        for i in (0..kids.length()).rev() {
            if let Some(k) = kids.item(i) {
                if k.get_attribute("data-id").map_or(false, |id| !want.contains(&id)) { k.remove(); }
            }
        }
    }

    #[wasm_bindgen]
    pub async fn start(canvas_id: String) -> Result<(), JsValue> {
        eframe::WebLogger::init(log::LevelFilter::Warn).ok();
        eframe::WebRunner::new()
            .start(&canvas_id, eframe::WebOptions::default(), Box::new(|cc| Ok(Box::new(super::App::new(cc.egui_ctx.clone())))))
            .await
    }
}
