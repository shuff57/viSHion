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

mod a11y;
mod bands;
mod dialog;
pub mod emoji;
mod flow;
pub mod host;
pub mod math;
mod review;
pub mod state;
pub mod theme;
pub mod transcript;
pub mod tree;
pub mod twin;
mod svgpath;
mod widgets;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(test)]
mod app_tests;

pub use host::{Cmd, Dispatch, Event, Inbox, ReadOutcome};
pub use twin::{Node, Part, Reg};

/// Web only: call once after eframe started; makes the canvas a non-tabbable, AT-hidden surface and names
/// eframe's IME input. See `web.rs` for the keyboard model.
#[cfg(target_arch = "wasm32")]
pub fn harden_web_dom(canvas_id: &str) {
    web::harden(canvas_id);
}

/// Messages from the DOM twin to egui: `act:<id>`, `focus:<id>`, `blur:<id>`, `text:<id>\u{1}<value>`.
pub type Queue = Arc<Mutex<Vec<String>>>;

/// A number as JS prints it in a sentence (`4` not `4.0`, `0.5`).
fn jsx_num(v: f64) -> String {
    vishion_core::jsx::num_str(v)
}

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
    /// Activations from the twin, oldest first. A control takes one per frame, so several quick activations
    /// of the same button (key repeat, a script) are each applied instead of coalescing into one.
    act: Vec<String>,
    /// Ids that took an activation this frame (the rest of their queue waits for the next frame).
    consumed: HashSet<String>,
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
    /// The last input was the keyboard, so a focused non-text region shows its ring (`:focus-visible`: a mouse
    /// click on the transcript focuses it without a ring; the first arrow key brings the ring up).
    pub(crate) kbd: bool,
    /// Twin focus arrived since the control last drew: bring it into view (browsers scroll focus into view;
    /// the twin is `overflow: clip`, so the canvas has to do it).
    focus_fresh: bool,
    /// Which regions sit inside a scroll area this frame (`ui.scroll_to_rect` outside one would leak to the
    /// next scroll area drawn). Set by the screen, indexed by region.
    pub(crate) scrolls: [bool; 5],
    /// Height of a sticky tray that covers the bottom of the page this frame: focus is scrolled clear of it.
    pub(crate) tray_cover: f32,
    pending_scroll: Option<(u8, egui::Rect)>,
    /// A control in the (sticky, phone) tray took focus: scroll the page until the tray is on screen.
    pub(crate) reveal_tray: bool,
    /// Rects of this frame's transcript lines (`--dump-controls` prints them, to compare with the DOM).
    pub(crate) lines: Vec<(String, egui::Rect)>,
    /// Sentences for the live region this frame: (text, assertive). `web::announce` speaks them.
    pub(crate) said: Vec<(String, bool)>,
    /// Twin nodes whose text is announced once the next frame has drawn the state they describe.
    /// (node id, words said before its text), see `say_node_after`.
    pub(crate) say_nodes: Vec<(&'static str, String)>,
    say_wait: Vec<(&'static str, String)>,
    /// Native: the egui widget that draws each twin control, so the accessibility tree reuses its node
    /// (activation from a screen reader is then a click on that widget).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) widgets: HashMap<String, egui::Id>,
}

/// What a control is, for the twin.
pub struct C<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub pressed: Option<bool>,
    pub region: u8,
    pub disabled: bool,
    /// Role in the twin (see `Reg::role`).
    pub role: &'static str,
    /// Where the focus ring goes when it is not the hit target (a transcript word: its painted chip, not the
    /// 44pt target around it), and its corner radius.
    pub ring: Option<(egui::Rect, f32)>,
}

impl<'a> C<'a> {
    pub fn new(id: &'a str, label: &'a str, region: u8) -> Self {
        C { id, label, pressed: None, region, disabled: false, role: "button", ring: None }
    }
    pub fn pressed(mut self, on: bool) -> Self {
        self.pressed = Some(on);
        self
    }
    /// `"checkbox"`, `"switch"` or `"listitem"` instead of the plain button.
    pub fn role(mut self, role: &'static str) -> Self {
        self.role = role;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn ring(mut self, rect: egui::Rect, radius: f32) -> Self {
        self.ring = Some((rect, radius));
        self
    }
}

impl Ctl {
    /// Carry out the focus scroll request if its control belongs to one of `regions`. Call at the end of a
    /// scroll area's content, with that area's `ui`.
    pub(crate) fn flush_scroll(&mut self, ui: &Ui, regions: &[u8]) {
        if let Some((r, rect)) = self.pending_scroll {
            if regions.contains(&r) {
                self.pending_scroll = None;
                ui.scroll_to_rect(rect, None);
            }
        }
    }

    fn track_focus(&mut self, ui: &Ui, r: &egui::Response, id: &str, region: u8, ring: Option<(egui::Rect, f32)>) {
        if self.want_focus.as_deref() == Some(id) {
            self.want_focus = None;
            r.request_focus();
            self.focus = Some(id.to_string());
            self.dom_focus = Some(id.to_string());
            self.focus_fresh = true;
        }
        let twin_focus = self.focus.as_deref() == Some(id);
        if twin_focus && self.focus_fresh && self.scrolls.get(region as usize).copied().unwrap_or(false) {
            self.focus_fresh = false;
            // the ring and a little air round it; a tall region (the transcript) only needs its top on screen
            let base = ring.map(|r| r.0).unwrap_or(r.rect);
            // (air round the ring, unless the window is so short that it would not fit)
            let mut want = if base.height() + 16.0 <= ui.clip_rect().height() { base.expand(4.0) } else { base };
            if want.height() > 96.0 {
                want.max.y = want.min.y + 96.0;
            }
            if region == twin::TRAY {
                self.reveal_tray = true;
            } else {
                want.max.y += self.tray_cover;
            }
            // applied by `flush_scroll` at the end of the scroll area it belongs to: a scroll area drawn in
            // between (the page stage) would otherwise take the request
            self.pending_scroll = Some((region, want));
        }
        if twin_focus || r.has_focus() {
            // a mouse click on the transcript focuses it without a ring (`:focus-visible`); the web twin
            // reports `kbd:1` when Tab or a script moved focus there with the keyboard in use
            let visible = self.kbd || id != "transcript";
            if visible {
                let (rect, radius) = ring.unwrap_or((r.rect.expand(2.0), 6.0));
                ui.painter().rect_stroke(rect, radius, Stroke::new(2.0, self.ring));
            }
            self.focused = Some((id.to_string(), region));
        }
    }

    /// The single funnel for interactive widgets. Returns true when activated from the canvas or the twin.
    pub fn reg(&mut self, ui: &Ui, r: &egui::Response, c: C) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        self.widgets.insert(c.id.to_string(), r.id);
        self.push(Reg {
            id: c.id.into(),
            role: c.role,
            label: c.label.into(),
            pressed: c.pressed,
            value: None,
            disabled: c.disabled,
            region: c.region,
            pos: twin::pos_of(r.rect.min),
            at: (r.rect.min.x.round() as i32, r.rect.min.y.round() as i32),
            size: (r.rect.width().round() as u16, r.rect.height().round() as u16),
        });
        self.track_focus(ui, r, c.id, c.region, c.ring);
        let from_dom = match self.act.iter().position(|a| a == c.id) {
            Some(i) => {
                self.act.remove(i);
                self.consumed.insert(c.id.to_string());
                true
            }
            None => false,
        };
        !c.disabled && (r.clicked() || from_dom)
    }

    /// Register a textbox. `value` is what the canvas widget currently holds; text typed into the twin
    /// is returned once (the caller writes it into its buffer).
    pub fn reg_text(&mut self, ui: &Ui, r: &egui::Response, c: C, value: &str) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.widgets.insert(c.id.to_string(), r.id);
        // text typed into the twin this frame is the box's value from now on: the twin reports what the
        // canvas box holds, so a decision in this same frame that clears it shows as a change next frame
        let typed = self.typed.remove(c.id);
        self.push(Reg {
            id: c.id.into(),
            role: "textbox",
            label: c.label.into(),
            pressed: None,
            value: Some(typed.clone().unwrap_or_else(|| value.to_string())),
            disabled: c.disabled,
            region: c.region,
            pos: twin::pos_of(r.rect.min),
            at: (r.rect.min.x.round() as i32, r.rect.min.y.round() as i32),
            size: (r.rect.width().round() as u16, r.rect.height().round() as u16),
        });
        self.track_focus(ui, r, c.id, c.region, c.ring);
        typed
    }

    /// A focusable, named region that takes keys (`keys` is the set the web twin forwards). Not a control:
    /// it is absent from `controls()` and from the parity list.
    pub fn group(&mut self, ui: &Ui, r: &egui::Response, id: &str, label: &str, keys: &'static str, region: u8) {
        #[cfg(not(target_arch = "wasm32"))]
        self.widgets.insert(id.to_string(), r.id);
        // one pixel left of its content, so it sorts just before the first paragraph it governs
        let pos = twin::pos_of(r.rect.min - egui::vec2(1.0, 0.0));
        // the twin sits where the region is on screen: a transcript taller than the window is its visible part
        let vis = r.rect.intersect(ui.clip_rect());
        let vis = if vis.width() > 0.0 && vis.height() > 0.0 { vis } else { r.rect };
        self.nodes.push(Node::Group {
            id: id.into(),
            label: label.into(),
            keys,
            region,
            pos,
            at: (vis.min.x.round() as i32, vis.min.y.round() as i32),
            size: (vis.width().round() as u16, vis.height().round() as u16),
        });
        // the page stage fills its pane: its ring goes inside, or the pane's edge clips it to a hairline
        let ring = if id == "stage" { r.rect.shrink(2.0) } else { r.rect.expand(3.0) };
        self.track_focus(ui, r, id, region, Some((ring, 0.0)));
    }

    fn push(&mut self, reg: Reg) {
        match &mut self.para {
            Some(parts) => {
                // a slot reserved at layout time (`para_slot`) is filled in place, so the control sits in the
                // sentence where the word is, not after it
                let slot = parts.iter().position(|p| matches!(p, Part::Control(c) if c.id == reg.id));
                match slot {
                    Some(i) => parts[i] = Part::Control(reg),
                    None => parts.push(Part::Control(reg)),
                }
            }
            None => self.nodes.push(Node::Control(reg)),
        }
    }

    /// Reserve the place of a control in the paragraph being built: it is registered later (while painting),
    /// but reads where the word stands in the line.
    pub fn para_slot(&mut self, id: &str, label: &str, region: u8) {
        if let Some(p) = &mut self.para {
            p.push(Part::Control(Reg {
                id: id.into(),
                role: "button",
                label: label.into(),
                pressed: None,
                value: None,
                disabled: false,
                region,
                pos: (0, 0),
                at: (0, 0),
                size: (0, 0),
            }));
        }
    }

    /// Announce `text` politely (a decision was made, a student changed).
    pub fn say(&mut self, text: impl Into<String>) {
        self.said.push((text.into(), false));
    }

    /// Announce what the node `id` says once the next frame has drawn it (a page or student that just changed).
    pub fn say_node(&mut self, id: &'static str) {
        self.say_nodes.push((id, String::new()));
    }

    /// `say_node`, with `lead` spoken first in the same sentence: two writes to a live region in one breath
    /// are often reduced to the second, so what belongs together is one utterance.
    pub fn say_node_after(&mut self, lead: &str, id: &'static str) {
        self.say_nodes.push((id, lead.to_string()));
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

    /// Struck-through text in the paragraph being built.
    pub fn para_del(&mut self, s: &str) {
        if let Some(p) = &mut self.para {
            p.push(Part::Del(s.into()));
        }
    }

    /// A picture the twin describes with `label`, at `rect` (clipped to what is on screen).
    pub fn image(&mut self, ui: &Ui, id: &str, label: &str, region: u8, rect: egui::Rect) {
        let vis = rect.intersect(ui.clip_rect());
        let vis = if vis.width() > 0.0 && vis.height() > 0.0 { vis } else { rect };
        self.nodes.push(Node::Image {
            id: id.into(),
            label: label.into(),
            region,
            // just after the stage group, which sorts one pixel before its content
            pos: twin::pos_of(rect.min),
            at: (vis.min.x.round() as i32, vis.min.y.round() as i32),
            size: (vis.width().round() as u16, vis.height().round() as u16),
        });
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
                self.act.push(id.into());
            } else if let Some(id) = m.strip_prefix("focus:") {
                self.focus = Some(id.into());
                self.focus_fresh = true;
            } else if let Some(id) = m.strip_prefix("blur:") {
                if self.focus.as_deref() == Some(id) {
                    self.focus = None;
                }
            } else if m == "kbd:1" {
                self.kbd = true;
            } else if let Some(rest) = m.strip_prefix("key:") {
                let (key, alt) = match rest.split_once('\u{1}') {
                    Some((k, m)) => (k, m.contains("alt")),
                    None => (rest, false),
                };
                self.keys.push(KeyEv { key: key.into(), alt });
                self.kbd = true;
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
    /// Ink bands of the picture (`image.js lineBands`): where the writing is, for pages without stored boxes.
    pub bands: Vec<bands::Band>,
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
    last_screen: (f32, f32),
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
    /// What the live region was last given (the toast, the save state), so a change is spoken once.
    last_toast: Option<String>,
    last_save: Option<String>,
    said_log: Vec<String>,
    /// The last utterance and how many there were (native: the live region of the accessibility tree).
    #[cfg(not(target_arch = "wasm32"))]
    native_said: String,
    #[cfg(not(target_arch = "wasm32"))]
    native_said_n: u32,
    /// The window / tab title last set (`<assignment> | viSHion` once an assignment is open).
    last_title: Option<String>,
    status_err: bool,
    dialog: Option<dialog::Dialog>,
    /// Scroll position and size of the page stage, kept so zoom can hold the middle of the view.
    stage: review::Stage,
    /// Pictures the screen asked for while drawing (inbox thumbnails).
    want: Vec<String>,
    /// Colour emoji: the face (linked in natively, fetched on the web) and the pictures drawn from it.
    emoji: emoji::EmojiCache,
    emoji_requested: bool,
    /// Bumped when a picture or a font arrives: the page may have moved under the selected word.
    layout_epoch: u32,
    font_requested: bool,
    /// The fallback font is installed / was asked for / the text last checked for glyphs it needs.
    fallback_loaded: bool,
    fallback_font: Option<&'static [u8]>,
    fallback_requested: bool,
    fallback_seen: Option<(String, usize)>,
    theme_name: String,
    theme_override: Option<String>,
    /// Open on the assignment overview instead of the review screen (`--inbox`, `?view=inbox`).
    start_inbox: bool,
    /// Select this page when the assignment opens (`--page`, `?page=`).
    start_page: Option<String>,
    /// Open with the word panel closed (`--no-tray`, `?tray=0`), as after its Close button.
    start_no_tray: bool,
    dark_applied: Option<bool>,
    /// The system asks for more contrast (`prefers-contrast: more` or forced colours); read once at start.
    high_contrast: bool,
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
        // display preferences (web): no easing when the person asks for reduced motion; more contrast
        #[cfg(target_arch = "wasm32")]
        let high_contrast = {
            let reduced = web::media("(prefers-reduced-motion: reduce)");
            if reduced {
                ctx.style_mut(|s| s.animation_time = 0.0);
            }
            let hc = web::media("(forced-colors: active)") || web::media("(prefers-contrast: more)");
            web::mark_preferences(reduced, hc);
            hc
        };
        #[cfg(not(target_arch = "wasm32"))]
        let high_contrast = false;
        let mut app = App {
            ctx,
            queue,
            ctl: Ctl::default(),
            last_nodes: vec![],
            last_screen: (0.0, 0.0),
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
            last_toast: None,
            last_save: None,
            said_log: vec![],
            #[cfg(not(target_arch = "wasm32"))]
            native_said: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            native_said_n: 0,
            last_title: None,
            status_err: false,
            dialog: None,
            stage: review::Stage::default(),
            want: vec![],
            emoji: {
                let mut c = emoji::EmojiCache::default();
                if let Some(b) = theme::bundled_emoji() {
                    c.set_font(b);
                }
                c
            },
            emoji_requested: false,
            layout_epoch: 0,
            font_requested: false,
            fallback_loaded: theme::bundled_fallback().is_some(),
            fallback_font: theme::bundled_fallback(),
            fallback_requested: false,
            fallback_seen: None,
            theme_name: "auto".into(),
            theme_override: None,
            start_inbox: false,
            start_page: None,
            start_no_tray: false,
            dark_applied: None,
            high_contrast: false,
            exit_after: None,
            #[cfg(not(target_arch = "wasm32"))]
            screenshot: None,
            #[cfg(not(target_arch = "wasm32"))]
            dump_controls: false,
            #[cfg(not(target_arch = "wasm32"))]
            closing: false,
        };
        app.high_contrast = high_contrast;
        app
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

    /// Open with this page selected.
    pub fn page(mut self, id: &str) -> Self {
        self.start_page = Some(id.to_string());
        self
    }

    /// Open with the word panel closed.
    pub fn no_tray(mut self, on: bool) -> Self {
        self.start_no_tray = on;
        self
    }

    /// Open on the assignment overview (the inbox) instead of the review screen.
    pub fn inbox(mut self, on: bool) -> Self {
        self.start_inbox = on;
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

    /// What was sent to the live region so far (newest last; the web page speaks these).
    pub fn announced(&self) -> &[String] {
        &self.said_log
    }

    /// The whole twin of the last frame.
    pub fn nodes(&self) -> &[Node] {
        &self.last_nodes
    }

    pub fn session(&self) -> Option<&state::Session> {
        self.session.as_ref()
    }

    /// One frame without eframe, and a twin activation: how the host wiring in `crates/app` is tested end to
    /// end (commands go out through the dispatcher, events come back through the inbox).
    #[doc(hidden)]
    pub fn run_frame(&mut self, ctx: &egui::Context) {
        self.step(ctx, false);
    }

    #[doc(hidden)]
    pub fn twin_act(&self, id: &str) {
        self.queue.lock().unwrap().push(format!("act:{id}"));
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
        let drained = match &self.host {
            Some((_, inbox)) => inbox.drain(),
            None => vec![],
        };
        fn flatten(e: Event, out: &mut Vec<Event>) {
            match e {
                Event::Many(v) => v.into_iter().for_each(|x| flatten(x, out)),
                e => out.push(e),
            }
        }
        let mut evs = vec![];
        drained.into_iter().for_each(|e| flatten(e, &mut evs));
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
                    let mut sess = state::Session::new(slug, *assignment, *settings, now_ms());
                    if let Some(id) = &self.start_page {
                        sess.select(id);
                    }
                    if self.start_inbox {
                        sess.view = state::View::Inbox;
                    }
                    if self.start_no_tray {
                        sess.tray_open = false;
                    }
                    self.session = Some(sess);
                    self.boot = Boot::Ready;
                }
                Event::Empty { storage } => {
                    self.storage = storage;
                    self.boot = Boot::Empty;
                }
                Event::Failed(m) => self.boot = Boot::Failed(m),
                Event::Image { id, bytes } => {
                    self.layout_epoch += 1;
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
                    self.layout_epoch += 1;
                    if let Some(s) = &mut self.session {
                        s.page_replaced(&id, &name);
                    }
                    let slot = decode_image(&self.ctx, &id, &bytes).map_or(Slot::Missing, Slot::Ready);
                    self.images.insert(id, slot);
                    self.status_msg = Some("Page replaced. Transcribe it again from this screen.".into());
                    self.status_err = false;
                }
                Event::PagesAdded { pages } => {
                    self.layout_epoch += 1;
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
                Event::Many(_) => {}
                Event::Straightened { id, angle, bytes } => {
                    self.layout_epoch += 1;
                    if let Some(b) = bytes {
                        let slot = decode_image(&self.ctx, &id, &b).map_or(Slot::Missing, Slot::Ready);
                        self.images.insert(id.clone(), slot);
                    }
                    let bands: Vec<bands::Band> = match self.images.get(&id) {
                        Some(Slot::Ready(im)) => im.bands.clone(),
                        _ => vec![],
                    };
                    let n = self.session.as_mut().map_or(0, |s| s.set_boxes_from_ink(&id, &bands));
                    let tilt = angle.abs();
                    let msg = if angle != 0.0 {
                        format!("Straightened by {}°{}", jsx_num(tilt), if n > 0 { " and redrew the line boxes." } else { "." })
                    } else if n > 0 {
                        "The page was already level. Line boxes redrawn.".to_string()
                    } else {
                        "The page was already level.".to_string()
                    };
                    self.status_msg = Some(msg);
                    self.status_err = false;
                }
                Event::Transcribed { id, outcome } => {
                    use host::ReadOutcome::*;
                    match outcome {
                        Done { raw, tokens, ms, guesses } => {
                            let bands: Vec<bands::Band> = match self.images.get(&id) {
                                Some(Slot::Ready(im)) => im.bands.clone(),
                                _ => vec![],
                            };
                            if let Some(s) = &mut self.session {
                                s.page_read(&id, &raw, tokens, ms, guesses, &bands);
                            }
                        }
                        Unreachable(m) => {
                            self.status_msg = Some(m);
                            self.status_err = true;
                        }
                        Failed(m) => {
                            if let Some(s) = &mut self.session {
                                s.page_failed(&id, &m);
                            }
                        }
                    }
                }
                Event::Guessed { id, result } => match result {
                    Ok(found) => {
                        if let Some(s) = &mut self.session {
                            s.page_guessed(&id, found);
                        }
                    }
                    Err(m) => {
                        self.status_msg = Some(m);
                        self.status_err = true;
                    }
                },
                Event::EmojiFont(bytes) => {
                    if !bytes.is_empty() {
                        self.layout_epoch += 1;
                        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                        self.emoji.set_font(leaked);
                    }
                }
                Event::MathFont(bytes) => {
                    self.layout_epoch += 1;
                    self.math.set_font(bytes)
                }
                Event::FallbackFont(bytes) => {
                    self.layout_epoch += 1;
                    if !bytes.is_empty() {
                        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                        self.fallback_font = Some(leaked);
                        self.ctx.set_fonts(theme::fonts(Some(leaked)));
                        self.fallback_loaded = true;
                    }
                }
            }
        }
    }

    /// Ask for the images of the current student's pages that are not loaded yet.
    fn want_images(&mut self) {
        let asked = std::mem::take(&mut self.want);
        if let Some(s) = &self.session {
            let slug = s.slug.clone();
            for id in asked {
                if !self.images.contains_key(&id) {
                    self.images.insert(id.clone(), Slot::Loading);
                    self.send(Cmd::LoadImage { slug: slug.clone(), id });
                }
            }
        }
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

    /// Web: ask for the colour emoji face once the open page has an emoji.
    fn check_emoji_font(&mut self) {
        if self.emoji.has_font() || self.emoji_requested {
            return;
        }
        let Some(s) = &self.session else { return };
        let Some(p) = s.current() else { return };
        if emoji::has_emoji(p.raw()) {
            self.emoji_requested = true;
            self.send(Cmd::LoadEmojiFont);
        }
    }

    /// Web: ask for the fallback font once the open page has a character none of the bundled faces can draw.
    fn check_fallback_font(&mut self, ctx: &egui::Context) {
        if self.fallback_loaded || self.fallback_requested {
            return;
        }
        let Some(s) = &self.session else { return };
        let Some(p) = s.current() else { return };
        let key = (p.id().to_string(), p.raw().len());
        if self.fallback_seen.as_ref() == Some(&key) {
            return;
        }
        let missing = ctx.fonts(|f| {
            let probe = theme::serif(22.0);
            let mut absent = false;
            for c in p.raw().chars().chain(s.group().map(|g| g.name).unwrap_or_default().chars()) {
                if !c.is_control() && !c.is_whitespace() && c as u32 >= 0x2E80 && !f.has_glyph(&probe, c) {
                    absent = true;
                    break;
                }
            }
            absent
        });
        self.fallback_seen = Some(key);
        if missing {
            self.fallback_requested = true;
            self.send(Cmd::LoadFallbackFont);
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
        let t = theme::pick_hc(&self.theme_name, system_dark, self.high_contrast);
        if self.dark_applied != Some(t.dark) {
            theme::apply(ctx, t);
            // a new font atlas, with this theme's coverage gamma
            ctx.set_fonts(theme::fonts(self.fallback_font));
            self.dark_applied = Some(t.dark);
        }
        self.ctl.ring = t.ink;
        self.ctl.scrolls = [false; 5];
        self.ctl.pending_scroll = None;
        self.ctl.nodes.clear();
        #[cfg(not(target_arch = "wasm32"))]
        self.ctl.widgets.clear();
        self.ctl.para = None;
        review::show(self, ctx, t);
    }
}

/// `lineBands` on a 420 pixel wide copy of the page, as the JS canvas does.
fn scan_bands(im: &image::DynamicImage) -> Vec<bands::Band> {
    let w = bands::SCAN_W;
    let h = 40.max((im.height() as f64 * w as f64 / im.width().max(1) as f64).round() as u32);
    let small = im.resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgba8();
    let lum: Vec<u8> = small
        .pixels()
        .map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8)
        .collect();
    bands::line_bands(&lum, w as usize, h as usize)
}

fn decode_image(ctx: &egui::Context, id: &str, bytes: &[u8]) -> Option<Img> {
    let im = image::load_from_memory(bytes).ok()?;
    let im = if im.width().max(im.height()) > 3000 { im.thumbnail(3000, 3000) } else { im };
    let size = (im.width(), im.height());
    let bands = scan_bands(&im);
    let rgba = im.to_rgba8();
    let ci = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], rgba.as_raw());
    let full = ctx.load_texture(format!("page-{id}"), ci, egui::TextureOptions::LINEAR);
    // js/image.js `thumb(blob, 112)`: a canvas draws the page at 112 high with plain bilinear sampling, so thin
    // ink keeps its darkness; the strip then shows that small picture at 44 x 56. Averaging (Lanczos,
    // Triangle) would wash the handwriting out to a grey that the browser's thumbnail does not have.
    let th_h = 112u32;
    let th_w = ((im.width() as f64 * th_h as f64 / im.height().max(1) as f64).round() as u32).max(1);
    let src = im.to_rgba8();
    let mut th = image::RgbaImage::new(th_w, th_h);
    for (x, y, px) in th.enumerate_pixels_mut() {
        let (u, v) = ((x as f32 + 0.5) / th_w as f32, (y as f32 + 0.5) / th_h as f32);
        if let Some(p) = image::imageops::sample_bilinear(&src, u, v) {
            *px = p;
        }
    }
    let ti = egui::ColorImage::from_rgba_unmultiplied([th.width() as usize, th.height() as usize], th.as_raw());
    let thumb = ctx.load_texture(format!("thumb-{id}"), ti, egui::TextureOptions::LINEAR);
    Some(Img { full, thumb, size, bands })
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
        #[cfg(target_arch = "wasm32")]
        web::fit_scale(ctx);
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.ctl.kbd = false;
        }
        self.ctl.drain(&self.queue);
        self.collect_egui_keys(ctx);
        self.handle_events();
        self.ctl.focused = None;
        self.ctl.lines.clear();
        self.ui_frame(ctx, system_dark);
        // an activation or typed text whose control was not on screen this frame is dropped, not kept; what a
        // control has queued beyond the one it took this frame waits for the next frame
        let consumed = std::mem::take(&mut self.ctl.consumed);
        self.ctl.act.retain(|id| consumed.contains(id));
        if !self.ctl.act.is_empty() {
            ctx.request_repaint();
        }
        self.ctl.typed.clear();
        self.ctl.keys.clear();

        self.announce();
        self.set_title(ctx);
        // a modal dialog is the only thing the twin shows while it is open
        // (the screen's h1 stays, so the page keeps its main landmark and its heading behind the dialog)
        if self.dialog.is_some() {
            self.ctl.nodes.retain(|n| n.key().0 == twin::DIALOG || matches!(n, Node::Heading { level: 1, .. }));
        }
        // reading order: region, then 16px row band, then x
        self.ctl.nodes.sort_by_key(|n| n.key());
        #[cfg(not(target_arch = "wasm32"))]
        native::publish(ctx, &tree::of(&self.ctl.nodes), &self.ctl.widgets, &self.native_said, self.native_said_n, self.title());
        let nodes = std::mem::take(&mut self.ctl.nodes);
        let sr = ctx.screen_rect();
        let screen = (sr.width(), sr.height());
        if nodes != self.last_nodes || screen != self.last_screen {
            #[cfg(target_arch = "wasm32")]
            {
                web::place(screen);
                web::sync(&nodes);
            }
            self.last_nodes = nodes;
            self.last_screen = screen;
        }
        if let Some(id) = self.ctl.dom_focus.take() {
            #[cfg(target_arch = "wasm32")]
            web::focus(&id);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = id;
        }
        self.check_fallback_font(ctx);
        self.check_emoji_font();
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
    /// Speak this frame's news through the live region: decisions, a toast, the end of a save, and the text of
    /// nodes whose state was drawn only this frame (the page or student a key press moved to).
    fn announce(&mut self) {
        fn text_of<'a>(nodes: &'a [Node], id: &str) -> Option<&'a str> {
            nodes.iter().find_map(|n| match n {
                Node::Heading { id: i, text, .. } | Node::Status { id: i, text, .. } | Node::Text { id: i, text, .. } if i == id => Some(text.as_str()),
                _ => None,
            })
        }
        for (id, lead) in std::mem::take(&mut self.ctl.say_wait) {
            if let Some(t) = text_of(&self.ctl.nodes, id) {
                let t = if lead.is_empty() { t.to_string() } else { format!("{lead} {t}") };
                self.ctl.said.push((t, false));
            }
        }
        // what the twin said about itself on the last sync (a focused control became unavailable)
        #[cfg(target_arch = "wasm32")]
        if let Some(n) = web::take_notice() {
            self.ctl.said.push((n, false));
        }
        self.ctl.say_wait = std::mem::take(&mut self.ctl.say_nodes);
        if !self.ctl.say_wait.is_empty() {
            self.ctx.request_repaint();
        }
        let toast = text_of(&self.ctl.nodes, "toast").map(String::from);
        if toast != self.last_toast {
            if let Some(t) = &toast {
                self.ctl.said.push((t.clone(), self.status_err));
            }
            self.last_toast = toast;
        }
        let save = text_of(&self.ctl.nodes, "save-state").map(String::from);
        if save != self.last_save {
            match (&self.last_save, &save) {
                (_, Some(now)) if now.starts_with("Couldn’t save") => self.ctl.said.push((now.clone(), true)),
                (Some(was), Some(_)) if was.starts_with("Saving") => self.ctl.said.push(("Saved.".into(), false)),
                _ => {}
            }
            self.last_save = save;
        }
        let said = std::mem::take(&mut self.ctl.said);
        // one write per live region per frame: the sentences of a frame are one utterance
        for assertive in [false, true] {
            let all: Vec<&str> = said.iter().filter(|(_, a)| *a == assertive).map(|(t, _)| t.as_str()).collect();
            if all.is_empty() {
                continue;
            }
            #[cfg(target_arch = "wasm32")]
            web::announce(&all.join(" "), assertive);
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.native_said = all.join(" ");
                self.native_said_n += 1;
            }
        }
        for (text, _) in said {
            self.said_log.push(text);
            if self.said_log.len() > 40 {
                self.said_log.remove(0);
            }
        }
    }

    /// js/main.js: the title names the assignment outside the home screen (`<name> | viSHion`), WCAG 2.4.2.
    fn set_title(&mut self, ctx: &egui::Context) {
        let title = match (&self.boot, &self.session) {
            (Boot::Ready, Some(s)) => {
                let name = s.a.meta.name();
                format!("{} | viSHion", if name.is_empty() { "Assignment" } else { name })
            }
            _ => "viSHion".to_string(),
        };
        if self.last_title.as_deref() != Some(title.as_str()) {
            #[cfg(target_arch = "wasm32")]
            {
                let _ = ctx;
                web::set_title(&title);
            }
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = Some(title);
        }
    }

    /// The title the window or tab has now.
    pub fn title(&self) -> &str {
        self.last_title.as_deref().unwrap_or("viSHion")
    }

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
                self.ctl.kbd = true;
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
            for (id, r) in &self.ctl.lines {
                println!("#line {}", serde_json::json!({"id": id, "at": [r.min.x, r.min.y], "size": [r.width(), r.height()]}));
            }
            for c in self.controls() {
                println!("{}", serde_json::json!({"id": c.id, "role": c.role, "label": c.label, "pressed": c.pressed, "size": [c.size.0, c.size.1], "at": [c.at.0, c.at.1]}));
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
