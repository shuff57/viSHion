//! viSHion UI: the review screen (design/Main.dc.html) in egui, one codebase for native and web.
//!
//! Layers, each in its own module:
//! * `state`      review flows over `vishion-core` (pure, unit-tested natively)
//! * `transcript` marked text to drawable runs (pure)
//! * `math`       latex-rust isolation with the red-source fallback
//! * `theme`      design tokens (light + dark) and the bundled fonts
//! * `twin`       the hidden-DOM twin as data; `web` reconciles it into `#controls`
//! * `host`       commands and events: the UI never touches storage
//! * `widgets`, `review` drawing
//!
//! Every interactive widget goes through `Ctl::reg` (plan rule 3): it reports activation from egui OR from
//! its twin, records role/label/size for the twin, and draws the focus ring.
use egui::{Stroke, Ui};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod dialog;
mod flow;
pub mod host;
pub mod math;
mod review;
pub mod state;
pub mod theme;
pub mod transcript;
pub mod twin;
mod widgets;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(test)]
mod app_tests;

pub use host::{Cmd, Dispatch, Event, Inbox};
pub use twin::{Node, Part, Reg};

/// Web only: call once after eframe started; makes the canvas a non-tabbable, AT-hidden surface and names
/// eframe's IME input. See `web.rs` for the keyboard model.
#[cfg(target_arch = "wasm32")]
pub fn harden_web_dom(canvas_id: &str) {
    web::harden(canvas_id);
}

/// Messages from the DOM twin to egui: `act:<id>`, `focus:<id>`, `blur:<id>`, `text:<id>\u{1}<value>`.
pub type Queue = Arc<Mutex<Vec<String>>>;

/// Milliseconds since the Unix epoch (egui time is relative; this is for `updated`/`at` fields).
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
}

/// A key press routed to the screen: from egui (native, or the canvas on the web) or forwarded by the DOM
/// twin as `key:<name>` / `key:<name>\u{1}alt`. Names follow `KeyboardEvent.key`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEv {
    pub key: String,
    pub alt: bool,
}

/// The control layer: activation, twin focus and the node list of this frame.
#[derive(Default)]
pub struct Ctl {
    act: HashSet<String>,
    focus: Option<String>,
    /// Text typed into a twin textbox, by id (consumed by the widget that owns it).
    typed: HashMap<String, String>,
    nodes: Vec<Node>,
    para: Option<Vec<Part>>,
    ring: egui::Color32,
    /// Keys of this frame (twin and egui).
    pub(crate) keys: Vec<KeyEv>,
    /// The control or group that has focus this frame, with its region (egui focus or twin focus).
    pub(crate) focused: Option<(String, u8)>,
    /// Move focus to this id when it is next registered (strip arrow keys).
    pub(crate) want_focus: Option<String>,
    /// Ask the platform to move DOM focus (web) once the twin has been synced.
    pub(crate) dom_focus: Option<String>,
    /// A dialog was open at the end of the last frame, and where focus goes back to when it closes.
    pub(crate) dialog_was: bool,
    pub(crate) restore_focus: Option<String>,
}

/// What a control is, for the twin.
pub struct C<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub pressed: Option<bool>,
    pub region: u8,
    pub disabled: bool,
}

impl<'a> C<'a> {
    pub fn new(id: &'a str, label: &'a str, region: u8) -> Self {
        C { id, label, pressed: None, region, disabled: false }
    }
    pub fn pressed(mut self, on: bool) -> Self {
        self.pressed = Some(on);
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
}

impl Ctl {
    fn track_focus(&mut self, ui: &Ui, r: &egui::Response, id: &str, region: u8) {
        if self.want_focus.as_deref() == Some(id) {
            self.want_focus = None;
            r.request_focus();
            self.focus = Some(id.to_string());
            self.dom_focus = Some(id.to_string());
        }
        if self.focus.as_deref() == Some(id) || r.has_focus() {
            ui.painter().rect_stroke(r.rect.expand(2.0), 6.0, Stroke::new(2.0, self.ring));
            self.focused = Some((id.to_string(), region));
        }
    }

    /// The single funnel for interactive widgets. Returns true when activated from the canvas or the twin.
    pub fn reg(&mut self, ui: &Ui, r: &egui::Response, c: C) -> bool {
        self.push(Reg {
            id: c.id.into(),
            role: "button",
            label: c.label.into(),
            pressed: c.pressed,
            value: None,
            disabled: c.disabled,
            region: c.region,
            pos: twin::pos_of(r.rect.min),
            size: (r.rect.width().round() as u16, r.rect.height().round() as u16),
        });
        self.track_focus(ui, r, c.id, c.region);
        let from_dom = self.act.remove(c.id);
        !c.disabled && (r.clicked() || from_dom)
    }

    /// Register a textbox. `value` is what the canvas widget currently holds; text typed into the twin
    /// is returned once (the caller writes it into its buffer).
    pub fn reg_text(&mut self, ui: &Ui, r: &egui::Response, c: C, value: &str) -> Option<String> {
        self.push(Reg {
            id: c.id.into(),
            role: "textbox",
            label: c.label.into(),
            pressed: None,
            value: Some(value.into()),
            disabled: c.disabled,
            region: c.region,
            pos: twin::pos_of(r.rect.min),
            size: (r.rect.width().round() as u16, r.rect.height().round() as u16),
        });
        self.track_focus(ui, r, c.id, c.region);
        self.typed.remove(c.id)
    }

    /// A focusable, named region that takes keys (`keys` is the set the web twin forwards). Not a control:
    /// it is absent from `controls()` and from the parity list.
    pub fn group(&mut self, ui: &Ui, r: &egui::Response, id: &str, label: &str, keys: &'static str, region: u8) {
        // one pixel left of its content, so it sorts just before the first paragraph it governs
        let pos = twin::pos_of(r.rect.min - egui::vec2(1.0, 0.0));
        self.nodes.push(Node::Group { id: id.into(), label: label.into(), keys, region, pos });
        self.track_focus(ui, r, id, region);
    }

    fn push(&mut self, reg: Reg) {
        match &mut self.para {
            Some(parts) => parts.push(Part::Control(reg)),
            None => self.nodes.push(Node::Control(reg)),
        }
    }

    pub fn heading(&mut self, id: &str, level: u8, text: &str, region: u8, min: egui::Pos2) {
        self.nodes.push(Node::Heading { id: id.into(), level, text: text.into(), region, pos: twin::pos_of(min) });
    }

    pub fn status(&mut self, id: &str, text: &str, region: u8, min: egui::Pos2) {
        self.nodes.push(Node::Status { id: id.into(), text: text.into(), region, pos: twin::pos_of(min) });
    }

    pub fn text(&mut self, id: &str, text: &str, region: u8, min: egui::Pos2) {
        self.nodes.push(Node::Text { id: id.into(), text: text.into(), region, pos: twin::pos_of(min) });
    }

    /// Start a paragraph: controls registered until `end_para` are inline in it.
    pub fn begin_para(&mut self) {
        self.para = Some(vec![]);
    }

    pub fn para_text(&mut self, s: &str) {
        if let Some(p) = &mut self.para {
            match p.last_mut() {
                Some(Part::Text(t)) => t.push_str(s),
                _ => p.push(Part::Text(s.into())),
            }
        }
    }

    pub fn para_math(&mut self, tex: &str) {
        if let Some(p) = &mut self.para {
            p.push(Part::Math(tex.into()));
        }
    }

    pub fn end_para(&mut self, id: &str, region: u8, min: egui::Pos2, keys: bool, reading: bool) {
        if let Some(parts) = self.para.take() {
            self.nodes.push(Node::Para { id: id.into(), parts, region, pos: twin::pos_of(min), keys, reading });
        }
    }

    /// Is the focus on a control or group whose id starts with `prefix`?
    pub(crate) fn focus_in(&self, prefix: &str) -> bool {
        self.focused.as_ref().is_some_and(|(id, _)| id.starts_with(prefix))
    }

    pub(crate) fn focus_region(&self) -> Option<u8> {
        self.focused.as_ref().map(|(_, r)| *r)
    }

    fn drain(&mut self, q: &Queue) {
        let msgs: Vec<String> = std::mem::take(&mut *q.lock().unwrap());
        for m in msgs {
            if let Some(id) = m.strip_prefix("act:") {
                self.act.insert(id.into());
            } else if let Some(id) = m.strip_prefix("focus:") {
                self.focus = Some(id.into());
            } else if let Some(id) = m.strip_prefix("blur:") {
                if self.focus.as_deref() == Some(id) {
                    self.focus = None;
                }
            } else if let Some(rest) = m.strip_prefix("key:") {
                let (key, alt) = match rest.split_once('\u{1}') {
                    Some((k, m)) => (k, m.contains("alt")),
                    None => (rest, false),
                };
                self.keys.push(KeyEv { key: key.into(), alt });
            } else if let Some(rest) = m.strip_prefix("text:") {
                if let Some((id, v)) = rest.split_once('\u{1}') {
                    self.typed.insert(id.into(), v.into());
                }
            }
        }
    }
}

/// A page image on the GPU: the full texture for the page pane and a small one for the strip.
pub(crate) struct Img {
    pub full: egui::TextureHandle,
    pub thumb: egui::TextureHandle,
    /// Pixel size of the full image.
    pub size: (u32, u32),
}

pub(crate) enum Slot {
    Loading,
    Missing,
    Ready(Img),
}

/// Save indicator state (`S.d.save`).
#[derive(Default)]
pub(crate) struct SaveState {
    pub pending: u32,
    pub failed: Option<String>,
    pub ever: bool,
}

pub(crate) enum Boot {
    Loading,
    Empty,
    Failed(String),
    Ready,
}

pub struct App {
    ctx: egui::Context,
    queue: Queue,
    ctl: Ctl,
    last_nodes: Vec<Node>,
    host: Option<(Dispatch, Inbox)>,
    boot: Boot,
    session: Option<state::Session>,
    storage: String,
    notice: Option<String>,
    save: SaveState,
    images: HashMap<String, Slot>,
    math: math::MathCache,
    typed: String,
    typed_for: Option<(String, String)>,
    status_msg: Option<String>,
    status_err: bool,
    dialog: Option<dialog::Dialog>,
    /// Scroll position and size of the page stage, kept so zoom can hold the middle of the view.
    stage: review::Stage,
    font_requested: bool,
    theme_name: String,
    theme_override: Option<String>,
    dark_applied: Option<bool>,
    exit_after: Option<Duration>,
    #[cfg(not(target_arch = "wasm32"))]
    screenshot: Option<(std::path::PathBuf, f64)>,
    #[cfg(not(target_arch = "wasm32"))]
    dump_controls: bool,
    #[cfg(not(target_arch = "wasm32"))]
    closing: bool,
}

impl App {
    /// An app without a host: shows the empty state. Used by tests and previews.
    pub fn new(ctx: egui::Context) -> Self {
        let queue: Queue = Arc::default();
        #[cfg(target_arch = "wasm32")]
        web::install(queue.clone(), ctx.clone());
        theme::install(&ctx);
        App {
            ctx,
            queue,
            ctl: Ctl::default(),
            last_nodes: vec![],
            host: None,
            boot: Boot::Loading,
            session: None,
            storage: String::new(),
            notice: None,
            save: SaveState::default(),
            images: HashMap::new(),
            math: math::MathCache::new(),
            typed: String::new(),
            typed_for: None,
            status_msg: None,
            status_err: false,
            dialog: None,
            stage: review::Stage::default(),
            font_requested: false,
            theme_name: "auto".into(),
            theme_override: None,
            dark_applied: None,
            exit_after: None,
            #[cfg(not(target_arch = "wasm32"))]
            screenshot: None,
            #[cfg(not(target_arch = "wasm32"))]
            dump_controls: false,
            #[cfg(not(target_arch = "wasm32"))]
            closing: false,
        }
    }

    /// Connect the storage host. `dispatch` carries `Cmd`s out; the host pushes `Event`s into `inbox`.
    pub fn with_host(mut self, dispatch: Dispatch, inbox: Inbox) -> Self {
        self.host = Some((dispatch, inbox));
        self
    }

    /// Force a theme (`"light"` or `"dark"`) over `settings.theme`.
    pub fn theme(mut self, name: &str) -> Self {
        self.theme_override = Some(name.into());
        self
    }

    /// Close the window after `d` (native smoke test; `vishion --exit-after-ms N`).
    pub fn exit_after(mut self, d: Duration) -> Self {
        self.exit_after = Some(d);
        self
    }

    /// Native: after `settle` seconds of egui time, write the frame to `path` (PNG), then close.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn screenshot_to(mut self, path: std::path::PathBuf, settle: f64) -> Self {
        self.screenshot = Some((path, settle));
        self
    }

    /// Native: print the control list as JSON lines when the app closes (parity check against the twin).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn dump_controls(mut self, on: bool) -> Self {
        self.dump_controls = on;
        self
    }

    /// Controls of the last frame in reading order, including those inline in transcript paragraphs.
    pub fn controls(&self) -> Vec<&Reg> {
        twin::controls(&self.last_nodes)
    }

    /// The whole twin of the last frame.
    pub fn nodes(&self) -> &[Node] {
        &self.last_nodes
    }

    pub fn session(&self) -> Option<&state::Session> {
        self.session.as_ref()
    }

    /// Hand a session to the app directly (tests; the host path is `Event::Opened`).
    pub fn open_session(&mut self, s: state::Session, storage: &str) {
        self.session = Some(s);
        self.storage = storage.into();
        self.boot = Boot::Ready;
    }

    fn send(&self, c: Cmd) {
        if let Some((d, _)) = &self.host {
            d(c);
        }
    }

    fn handle_events(&mut self) {
        let evs = match &self.host {
            Some((_, inbox)) => inbox.drain(),
            None => vec![],
        };
        for e in evs {
            match e {
                Event::Opened { slug, assignment, settings, storage, unreadable, notice } => {
                    self.storage = storage;
                    let mut msg = notice;
                    if !unreadable.is_empty() {
                        let n = format!(
                            "{} could not be read and was left untouched.",
                            state::plural(unreadable.len(), "page", "pages")
                        );
                        msg = Some(msg.map_or(n.clone(), |m| format!("{m} {n}")));
                    }
                    self.notice = msg;
                    self.images.clear();
                    self.session = Some(state::Session::new(slug, *assignment, *settings, now_ms()));
                    self.boot = Boot::Ready;
                }
                Event::Empty { storage } => {
                    self.storage = storage;
                    self.boot = Boot::Empty;
                }
                Event::Failed(m) => self.boot = Boot::Failed(m),
                Event::Image { id, bytes } => {
                    let slot = bytes.and_then(|b| decode_image(&self.ctx, &id, &b)).map_or(Slot::Missing, Slot::Ready);
                    self.images.insert(id, slot);
                }
                Event::Saved => {
                    self.save.pending = self.save.pending.saturating_sub(1);
                    self.save.ever = true;
                    if self.save.pending == 0 {
                        self.save.failed = None;
                    }
                }
                Event::SaveFailed(m) => {
                    self.save.pending = self.save.pending.saturating_sub(1);
                    self.save.failed = Some(m);
                }
                Event::Notice { text, error } => {
                    if !text.is_empty() {
                        self.status_msg = Some(text);
                        self.status_err = error;
                    }
                }
                Event::PageReplaced { id, name, bytes } => {
                    if let Some(s) = &mut self.session {
                        s.page_replaced(&id, &name);
                    }
                    let slot = decode_image(&self.ctx, &id, &bytes).map_or(Slot::Missing, Slot::Ready);
                    self.images.insert(id, slot);
                    self.status_msg = Some("Page replaced. Transcribe it again from this screen.".into());
                    self.status_err = false;
                }
                Event::PagesAdded { pages } => {
                    let n = pages.len();
                    let mut first = None;
                    let mut list = vec![];
                    for (p, bytes) in pages {
                        let id = p.id().to_string();
                        let slot = decode_image(&self.ctx, &id, &bytes).map_or(Slot::Missing, Slot::Ready);
                        self.images.insert(id.clone(), slot);
                        first.get_or_insert(id);
                        list.push(p);
                    }
                    if let Some(s) = &mut self.session {
                        s.pages_added(list);
                        if let Some(f) = first {
                            s.select(&f);
                        }
                    }
                    if n > 0 {
                        self.status_msg = Some(format!("Added {}.", state::plural(n, "page", "pages")));
                        self.status_err = false;
                    }
                }
                Event::MathFont(bytes) => self.math.set_font(bytes),
            }
        }
    }

    /// Ask for the images of the current student's pages that are not loaded yet.
    fn want_images(&mut self) {
        let Some(s) = &self.session else { return };
        let Some(g) = s.group() else { return };
        let mut ids = g.pages.clone();
        // the selected page first
        if let Some(cur) = &s.selected {
            ids.sort_by_key(|i| i != cur);
        }
        let slug = s.slug.clone();
        for id in ids {
            if !self.images.contains_key(&id) {
                self.images.insert(id.clone(), Slot::Loading);
                self.send(Cmd::LoadImage { slug: slug.clone(), id });
            }
        }
    }

    fn flush_dirty(&mut self) {
        let Some(s) = &mut self.session else { return };
        if !s.dirty.any() {
            return;
        }
        let d = s.take_dirty();
        let slug = s.slug.clone();
        let mut cmds = vec![];
        for id in d.removed {
            cmds.push(Cmd::RemovePage { slug: slug.clone(), id });
        }
        for id in d.rotated {
            cmds.push(Cmd::RotateImage { slug: slug.clone(), id });
        }
        for id in d.pages {
            if let Some(p) = s.a.pages.get(&id) {
                cmds.push(Cmd::SavePage { slug: slug.clone(), page: p.clone() });
            }
        }
        if d.meta {
            cmds.push(Cmd::SaveMeta { slug: slug.clone(), meta: s.a.meta.clone() });
        }
        if d.settings {
            cmds.push(Cmd::SaveSettings { settings: s.settings.clone() });
        }
        self.save.pending += cmds.len() as u32;
        for c in cmds {
            self.send(c);
        }
    }

    fn ui_frame(&mut self, ctx: &egui::Context, system_dark: bool) {
        // theme
        let name = self
            .theme_override
            .clone()
            .or_else(|| self.session.as_ref().and_then(|s| s.settings.get("theme").and_then(|v| v.as_str().map(String::from))))
            .unwrap_or_else(|| self.theme_name.clone());
        self.theme_name = name;
        let t = theme::pick(&self.theme_name, system_dark);
        if self.dark_applied != Some(t.dark) {
            theme::apply(ctx, t);
            self.dark_applied = Some(t.dark);
        }
        self.ctl.ring = t.ink;
        self.ctl.nodes.clear();
        self.ctl.para = None;
        review::show(self, ctx, t);
    }
}

fn decode_image(ctx: &egui::Context, id: &str, bytes: &[u8]) -> Option<Img> {
    let im = image::load_from_memory(bytes).ok()?;
    let im = if im.width().max(im.height()) > 3000 { im.thumbnail(3000, 3000) } else { im };
    let size = (im.width(), im.height());
    let rgba = im.to_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], rgba.as_raw());
    let full = ctx.load_texture(format!("page-{id}"), ci, egui::TextureOptions::LINEAR);
    let th = im.thumbnail(132, 168).to_rgba8();
    let ti = egui::ColorImage::from_rgba_unmultiplied([th.width() as usize, th.height() as usize], th.as_raw());
    let thumb = ctx.load_texture(format!("thumb-{id}"), ti, egui::TextureOptions::LINEAR);
    Some(Img { full, thumb, size })
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let system_dark = frame.info().system_theme.map_or(false, |t| t == eframe::Theme::Dark);
        self.step(ctx, system_dark);
    }
}

impl App {
    /// One frame: twin and host input, the screen, the twin output, saves, image requests, exit rules.
    fn step(&mut self, ctx: &egui::Context, system_dark: bool) {
        self.ctl.drain(&self.queue);
        self.collect_egui_keys(ctx);
        self.handle_events();
        self.ctl.focused = None;
        self.ui_frame(ctx, system_dark);
        // an activation or typed text whose control was not on screen this frame is dropped, not kept
        self.ctl.act.clear();
        self.ctl.typed.clear();
        self.ctl.keys.clear();

        // a modal dialog is the only thing the twin shows while it is open
        if self.dialog.is_some() {
            self.ctl.nodes.retain(|n| n.key().0 == twin::DIALOG);
        }
        // reading order: region, then 16px row band, then x
        self.ctl.nodes.sort_by_key(|n| n.key());
        let nodes = std::mem::take(&mut self.ctl.nodes);
        if nodes != self.last_nodes {
            #[cfg(target_arch = "wasm32")]
            web::sync(&nodes);
            self.last_nodes = nodes;
        }
        if let Some(id) = self.ctl.dom_focus.take() {
            #[cfg(target_arch = "wasm32")]
            web::focus(&id);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = id;
        }
        if self.math.wants_font() && !self.font_requested {
            self.font_requested = true;
            self.send(Cmd::LoadMathFont);
        }

        self.want_images();
        self.flush_dirty();

        let t = ctx.input(|i| i.time);
        #[cfg(not(target_arch = "wasm32"))]
        self.native_exit(ctx, t);
        if let Some(d) = self.exit_after {
            // egui time (not std Instant, which panics on wasm32)
            if t >= d.as_secs_f64() {
                #[cfg(not(target_arch = "wasm32"))]
                self.finish(ctx);
                #[cfg(target_arch = "wasm32")]
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }
}

impl App {
    /// Keys egui saw (native, or the canvas on the web) become the same `KeyEv`s the twin forwards. Not while
    /// a text field has the keyboard.
    fn collect_egui_keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let evs = ctx.input(|i| i.events.clone());
        for e in evs {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                let name = match key {
                    egui::Key::ArrowDown => "ArrowDown",
                    egui::Key::ArrowUp => "ArrowUp",
                    egui::Key::ArrowLeft => "ArrowLeft",
                    egui::Key::ArrowRight => "ArrowRight",
                    egui::Key::Home => "Home",
                    egui::Key::End => "End",
                    egui::Key::Delete => "Delete",
                    egui::Key::Escape => "Escape",
                    egui::Key::Plus => "+",
                    egui::Key::Equals => "=",
                    egui::Key::Minus => "-",
                    egui::Key::Num0 => "0",
                    _ => continue,
                };
                self.ctl.keys.push(KeyEv { key: name.into(), alt: modifiers.alt });
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl App {
    fn native_exit(&mut self, ctx: &egui::Context, t: f64) {
        if let Some((path, settle)) = self.screenshot.clone() {
            let shot = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = shot {
                let _ = image::save_buffer(&path, img.as_raw(), img.width() as u32, img.height() as u32, image::ColorType::Rgba8);
                self.finish(ctx);
            } else if t >= settle {
                let ready = !self.images.values().any(|s| matches!(s, Slot::Loading)) && !matches!(self.boot, Boot::Loading);
                if ready {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                }
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }

    fn finish(&mut self, ctx: &egui::Context) {
        if std::mem::replace(&mut self.closing, true) {
            return;
        }
        if self.dump_controls {
            for c in self.controls() {
                println!("{}", serde_json::json!({"id": c.id, "role": c.role, "label": c.label, "pressed": c.pressed, "size": [c.size.0, c.size.1]}));
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
