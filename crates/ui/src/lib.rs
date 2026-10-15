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
mod home;
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
#[cfg(test)]
mod a11y_critic_tests;

pub use host::{Card, FolderInfo, StagedItem, Cmd, Conn, Dispatch, Event, Inbox, ReadOutcome};
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
    /// The modifier keys (ctrl or command, shift) that were down for a pending activation from the twin: a
    /// click on a DOM button carries them, as a click on the canvas carries egui's (Shift+Enter on a page
    /// thumbnail selects a range, as Shift+click does).
    act_mods: HashMap<String, (bool, bool)>,
    /// Ids that took an activation this frame (the rest of their queue waits for the next frame).
    consumed: HashSet<String>,
    focus: Option<String>,
    /// A transcript line to bring into view (`reveal:<line id>` from the page: a formula that scrolls sideways
    /// took keyboard focus).
    pub(crate) reveal: Option<String>,
    /// The control the app moved focus to after a decision (js re-renders and keeps the pane where it is): it
    /// takes focus without scrolling anything into view.
    pub(crate) quiet_focus: Option<String>,
    /// Points the page asks the transcript to scroll by (`scrollby:<dy>`: find-in-page or a drag-selection
    /// at the edge moved the DOM text, which the canvas's scroll area has to follow).
    pub(crate) scroll_by: f32,
    /// Native: the control that had the keyboard focus last (see `track_focus`).
    restore_kb: Option<String>,
    /// Text typed into a twin textbox, by id (consumed by the widget that owns it).
    typed: HashMap<String, String>,
    nodes: Vec<Node>,
    para: Option<Vec<Part>>,
    /// `twin::look` of each part of the paragraph being built, and whether it is a margin line.
    para_flags: Vec<u16>,
    para_margin: bool,
    /// Draw the transcript as DOM text (the web): its lines are described to the twin with their look and
    /// take the height the page measured for them, instead of being laid out and painted here.
    pub(crate) dom_text: bool,
    /// What the page measured of the DOM transcript last frame: line heights by line id, and the rectangle of
    /// each flagged word (x, y, w, h) relative to its line's top left corner.
    pub(crate) dom_h: HashMap<String, f32>,
    pub(crate) dom_chips: HashMap<String, [f32; 4]>,
    /// Where DOM transcript lines may be seen (the scroll area they are in): set when they are drawn.
    pub(crate) dom_vis: Option<egui::Rect>,
    /// Web: the text of the pane the page lays out itself (the student's name, the counts, the legend, the
    /// stat and hint, the pill, streamed text): twin node id -> (CSS class, where it may be seen). Set when the
    /// screen describes the node, cleared every frame.
    pub(crate) chrome: HashMap<String, (String, (i32, i32, u16, u16))>,
    /// The transcript line the pointer is over (`line-<page>-<index>`): the page lights its box.
    pub(crate) hover_line: Option<String>,
    ring: egui::Color32,
    /// Keys of this frame (twin and egui).
    pub(crate) keys: Vec<KeyEv>,
    /// A keyboard pan of the page stage waiting for the stage to be drawn (js: `.scan-stage` scrolls).
    pub(crate) pan: Option<crate::review::Pan>,
    /// The control or group that has focus this frame, with its region (egui focus or twin focus).
    pub(crate) focused: Option<(String, u8)>,
    /// Move focus to this id when it is next registered (strip arrow keys).
    pub(crate) want_focus: Option<String>,
    /// Ask the platform to move DOM focus (web) once the twin has been synced.
    pub(crate) dom_focus: Option<String>,
    /// A dialog was open at the end of the last frame, and where focus goes back to when it closes.
    pub(crate) dialog_was: bool,
    /// The person asked for the home screen (the wordmark); the app shell takes it after the frame.
    pub(crate) want_home: bool,
    /// This host has a camera to offer (the web build; tests switch it on).
    pub(crate) camera_ok: bool,
    /// The dialog a confirmation is stacked on (Settings under "Clear remembered fixes", as modal.js stacks
    /// them) and the control that gets focus back when the confirmation closes.
    pub(crate) under: Option<(crate::dialog::Dialog, &'static str)>,
    pub(crate) restore_focus: Option<String>,
    /// The control that was activated this frame (mouse, keyboard or the twin): the opener of a dialog.
    pub(crate) activated: Option<String>,
    /// The last input was the keyboard, so a focused non-text region shows its ring (`:focus-visible`: a mouse
    /// click on the transcript focuses it without a ring; the first arrow key brings the ring up).
    pub(crate) kbd: bool,
    /// A mouse press arrived with this frame's messages: the focus it gave is not scrolled to (Chrome only
    /// scrolls a focus made by key or script).
    pub(crate) pointer_down: bool,
    /// The control being registered is a text box (it always shows its ring).
    in_text: bool,
    /// `now_ms()` of the last mouse press on the twin.
    pub(crate) pointer_at: f64,
    /// Twin focus arrived since the control last drew: bring it into view (browsers scroll focus into view;
    /// the twin is `overflow: clip`, so the canvas has to do it).
    focus_fresh: bool,
    /// Which regions sit inside a scroll area this frame (`ui.scroll_to_rect` outside one would leak to the
    /// next scroll area drawn). Set by the screen, indexed by region.
    pub(crate) scrolls: [bool; 5],
    /// Height of the header's content on the last frame and whether it was laid out inside the page (it is
    /// when it is too tall for a panel of its own): see `review::show`.
    pub(crate) header_h: f32,
    pub(crate) header_inline: bool,
    /// Counts every time a line is put on the toast, so the same words said again (Copy text twice) are
    /// spoken again: the live region only reads what changed, and the JS toast adds a new node each time.
    pub(crate) toast_serial: u64,
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
    /// A heading that takes keyboard focus once the next screen has drawn it (the logo goes home).
    pub(crate) focus_title: Option<&'static str>,
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
            self.focus_fresh = self.quiet_focus.take().as_deref() != Some(id);
        }
        let twin_focus = self.focus.as_deref() == Some(id);
        // Native: egui let go of the focus this control had (egui ids are positional: a control whose neighbours
        // came or went in the frame of its action is another widget to egui, and egui drops a focus whose
        // widget it did not see). The platform focus would fall to the window element, a screen reader would
        // read the title and Tab would start again at the top. A mouse press clears the record.
        #[cfg(not(target_arch = "wasm32"))]
        if self.restore_kb.as_deref() == Some(id) && !r.has_focus() && ui.ctx().memory(|m| m.focused().is_none()) {
            r.request_focus();
        }
        // (a mouse press within the last moments, and no key since)
        if twin_focus && self.focus_fresh && (self.pointer_down || (now_ms() as f64 - self.pointer_at) < 800.0) {
            // a mouse click focuses without scrolling (Chrome only scrolls a focus made by key or script)
            self.focus_fresh = false;
        }
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
            // (`:focus-visible` is also what a text box always has; anything else focused by script after a
            // mouse press has no ring)
            let visible = self.kbd || self.in_text;
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
            shown: None,
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
        // a dimmed control that can still be reached says why it does nothing
        if c.disabled && from_dom {
            if let Some(why) = crate::a11y::disabled_reason(c.id) {
                self.say(why);
            }
        }
        let hit = !c.disabled && (r.clicked() || from_dom);
        if hit {
            self.activated = Some(c.id.to_string());
        }
        hit
    }

    /// An activation of `id` from the twin that no widget of egui's owns (a click on a DOM transcript line):
    /// true once per activation.
    pub(crate) fn take_act(&mut self, id: &str) -> bool {
        match self.act.iter().position(|a| a == id) {
            Some(i) => {
                self.act.remove(i);
                self.consumed.insert(id.to_string());
                true
            }
            None => false,
        }
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
            shown: None,
        });
        self.in_text = true;
        self.track_focus(ui, r, c.id, c.region, c.ring);
        self.in_text = false;
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
                    Some(i) => {
                        // the word shows what the slot was reserved with
                        let shown = match &parts[i] {
                            Part::Control(c) => c.shown.clone(),
                            _ => None,
                        };
                        parts[i] = Part::Control(Reg { shown: reg.shown.clone().or(shown), ..reg });
                    }
                    None => {
                        parts.push(Part::Control(reg));
                        self.para_flags.push(0);
                    }
                }
            }
            None => self.nodes.push(Node::Control(reg)),
        }
    }

    /// Reserve the place of a control in the paragraph being built: it is registered later (while painting),
    /// but reads where the word stands in the line.
    pub fn para_slot(&mut self, id: &str, label: &str, region: u8) {
        self.para_slot_look(id, label, region, None, 0);
    }

    /// `para_slot` for a word the web draws as DOM text: `shown` is the word, `look` how it is marked.
    pub fn para_slot_look(&mut self, id: &str, label: &str, region: u8, shown: Option<String>, look: u16) {
        if let Some(p) = &mut self.para {
            self.para_flags.push(look);
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
                shown,
            }));
        }
    }

    /// Announce `text` politely (a decision was made, a student changed).
    pub fn say(&mut self, text: impl Into<String>) {
        self.said.push((text.into(), false));
    }

    /// Announce `text` at once (a refusal, a failure): the assertive live region.
    pub fn say_alert(&mut self, text: impl Into<String>) {
        self.said.push((text.into(), true));
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

    pub fn heading(&mut self, id: &str, level: u8, text: &str, region: u8, rect: egui::Rect) {
        let (at, size) = twin::geo(rect);
        self.nodes.push(Node::Heading { id: id.into(), level, text: text.into(), region, pos: twin::pos_of(rect.min), at, size });
    }

    pub fn status(&mut self, id: &str, text: &str, region: u8, rect: egui::Rect) {
        let (at, size) = twin::geo(rect);
        self.nodes.push(Node::Status { id: id.into(), text: text.into(), region, pos: twin::pos_of(rect.min), at, size });
    }

    /// Native: what is drawn since `from` inside a scroll area is reported where it can be seen, not where the
    /// content is (a control far below the fold has a rectangle far outside the window, which UI Automation
    /// clients take for visible). A node wholly outside the clip has no rectangle.
    pub(crate) fn clip_since(&mut self, from: usize, clip: egui::Rect) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let fix = |at: &mut (i32, i32), size: &mut (u16, u16)| {
                let r = egui::Rect::from_min_size(egui::pos2(at.0 as f32, at.1 as f32), egui::vec2(size.0 as f32, size.1 as f32)).intersect(clip);
                if r.width() > 0.0 && r.height() > 0.0 {
                    *at = (r.min.x.round() as i32, r.min.y.round() as i32);
                    *size = (r.width().round() as u16, r.height().round() as u16);
                } else {
                    *size = (0, 0);
                }
            };
            for n in self.nodes.iter_mut().skip(from) {
                match n {
                    Node::Heading { at, size, .. }
                    | Node::Status { at, size, .. }
                    | Node::Text { at, size, .. }
                    | Node::Para { at, size, .. }
                    | Node::Group { at, size, .. }
                    | Node::Image { at, size, .. } => fix(at, size),
                    Node::Control(c) => fix(&mut c.at, &mut c.size),
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (from, clip);
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn text(&mut self, id: &str, text: &str, region: u8, rect: egui::Rect) {
        let (at, size) = twin::geo(rect);
        self.nodes.push(Node::Text { id: id.into(), text: text.into(), region, pos: twin::pos_of(rect.min), at, size });
    }

    /// Start a paragraph: controls registered until `end_para` are inline in it.
    pub fn begin_para(&mut self) {
        self.para = Some(vec![]);
        self.para_flags.clear();
        self.para_margin = false;
    }

    pub fn para_text(&mut self, s: &str) {
        self.para_text_look(s, 0);
    }

    /// Text with the `twin::look` it is drawn in (bold, italic, code, the lead-in of a margin note).
    pub fn para_text_look(&mut self, s: &str, look: u16) {
        if let Some(p) = &mut self.para {
            match p.last_mut() {
                Some(Part::Text(t)) if self.para_flags.last() == Some(&look) => t.push_str(s),
                _ => {
                    p.push(Part::Text(s.into()));
                    self.para_flags.push(look);
                }
            }
        }
    }

    /// Struck-through text in the paragraph being built.
    pub fn para_del(&mut self, s: &str) {
        self.para_del_look(s, 0);
    }

    pub fn para_del_look(&mut self, s: &str, look: u16) {
        if let Some(p) = &mut self.para {
            p.push(Part::Del(s.into()));
            self.para_flags.push(look);
        }
    }

    /// The paragraph being built is a `Margin:` line.
    pub fn para_margin(&mut self) {
        self.para_margin = true;
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
        self.para_math_look(tex, 0);
    }

    pub fn para_math_look(&mut self, tex: &str, look: u16) {
        if let Some(p) = &mut self.para {
            p.push(Part::Math(tex.into()));
            self.para_flags.push(look);
        }
    }

    pub fn end_para(&mut self, id: &str, region: u8, rect: egui::Rect, keys: bool, reading: bool) {
        if let Some(parts) = self.para.take() {
            let (at, size) = twin::geo(rect);
            // the looks are for the web's DOM text; egui-drawn lines (and every native line) carry none
            let (flags, vis) = if self.dom_text {
                let v = self.dom_vis.unwrap_or(rect);
                let (vat, vsize) = twin::geo(v);
                (std::mem::take(&mut self.para_flags), (vat.0, vat.1, vsize.0, vsize.1))
            } else {
                self.para_flags.clear();
                (vec![], (0, 0, 0, 0))
            };
            self.nodes.push(Node::Para {
                id: id.into(),
                parts,
                region,
                pos: twin::pos_of(rect.min),
                keys,
                reading,
                at,
                size,
                flags,
                margin: std::mem::take(&mut self.para_margin),
                vis,
            });
        }
    }

    /// Modifier keys (ctrl or command, shift) that came with the activation this control took from the twin.
    pub(crate) fn dom_mods(&self, id: &str) -> (bool, bool) {
        self.act_mods.get(id).copied().unwrap_or((false, false))
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
        self.pointer_down = false;
        for m in msgs {
            if let Some(id) = m.strip_prefix("act:") {
                // "act:<id>" or "act:<id>\u{1}<c|s|cs>" with the modifiers that were held
                match id.split_once('\u{1}') {
                    Some((id, mods)) => {
                        self.act_mods.insert(id.into(), (mods.contains('c'), mods.contains('s')));
                        self.act.push(id.into());
                    }
                    None => self.act.push(id.into()),
                }
            } else if let Some(id) = m.strip_prefix("focus:") {
                self.focus = Some(id.into());
                self.focus_fresh = true;
            } else if let Some(d) = m.strip_prefix("scrollby:") {
                self.scroll_by += d.parse::<f32>().unwrap_or(0.0);
            } else if let Some(id) = m.strip_prefix("reveal:") {
                self.reveal = Some(id.into());
            } else if let Some(id) = m.strip_prefix("hover:") {
                self.hover_line = if id.is_empty() { None } else { Some(id.into()) };
            } else if let Some(id) = m.strip_prefix("blur:") {
                if self.focus.as_deref() == Some(id) {
                    self.focus = None;
                }
            } else if m == "kbd:1" {
                self.kbd = true;
                self.pointer_at = 0.0;
            } else if m == "kbd:0" {
                self.kbd = false;
                self.pointer_down = true;
                self.pointer_at = now_ms() as f64;
            } else if let Some(rest) = m.strip_prefix("key:") {
                let (key, alt) = match rest.split_once('\u{1}') {
                    Some((k, m)) => (k, m.contains("alt")),
                    None => (rest, false),
                };
                self.keys.push(KeyEv { key: key.into(), alt });
                self.kbd = true;
                self.pointer_at = 0.0;
            } else if let Some(rest) = m.strip_prefix("text:") {
                if let Some((id, v)) = rest.split_once('\u{1}') {
                    self.typed.insert(id.into(), v.into());
                }
            }
        }
    }
}

impl Img {
    /// The texture to draw the page at `px_w` x `px_h` device pixels, and the pixel size it really has.
    ///
    /// Shrinking is what the browser does with a `<img>` at the pane's width: its resampler averages the source
    /// pixels each output pixel covers. A GPU texture sampled bilinearly at 0.64 x reads two taps per pixel and
    /// the ruled lines and thin ink alias, so the page is shrunk once, on the CPU, with a triangle filter (the
    /// same support as the browser's) and then drawn one texel to one pixel. The last few sizes are kept.
    pub(crate) fn page_texture(&self, ctx: &egui::Context, px_w: u32, px_h: u32) -> (egui::TextureId, (u32, u32)) {
        if px_w == 0 || px_h == 0 || px_w >= self.size.0 {
            return (self.full.id(), self.size);
        }
        let mut cache = self.scaled.borrow_mut();
        if let Some((_, t)) = cache.iter().find(|((w, h), _)| (*w, *h) == (px_w, px_h)) {
            return (t.id(), (px_w, px_h));
        }
        let small = image::imageops::resize(&*self.src, px_w, px_h, image::imageops::FilterType::Triangle);
        let ci = egui::ColorImage::from_rgba_unmultiplied([px_w as usize, px_h as usize], small.as_raw());
        let t = ctx.load_texture(format!("page-{}-{px_w}", self.id), ci, egui::TextureOptions::LINEAR);
        let id = t.id();
        cache.push(((px_w, px_h), t));
        if cache.len() > 4 {
            cache.remove(0);
        }
        (id, (px_w, px_h))
    }
}

/// A page image on the GPU: the full texture for the page pane and a small one for the strip.
pub(crate) struct Img {
    pub full: egui::TextureHandle,
    pub thumb: egui::TextureHandle,
    /// The page's pixels, to shrink it to the size it is drawn at (`page_texture`).
    pub src: std::sync::Arc<image::RgbaImage>,
    /// Shrunk copies by size (the latest few).
    pub scaled: std::cell::RefCell<Vec<((u32, u32), egui::TextureHandle)>>,
    pub id: String,
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
    /// The save indicator says "Saving..." until this time (egui clock): the JS app writes half a second after
    /// the change (`fs.schedule`, 500 ms) and shows "Saving..." for that long, so a quick write is still seen.
    pub until: f64,
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
    last_chrome: HashMap<String, (String, (i32, i32, u16, u16))>,
    last_screen: (f32, f32),
    host: Option<(Dispatch, Inbox)>,
    boot: Boot,
    home: home::Home,
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
    last_toast_serial: u64,
    /// The control that had focus at the end of the last frame (native Tab), and the Tab presses waiting for
    /// the next frame (`true`: with Shift).
    prev_focus: Option<String>,
    /// Native: the focus that was asked for when the frame began (still pending at its end: nothing drew it),
    /// and the tab stops of the last frame that had no dialog open (where a closed dialog's opener stood).
    #[cfg(not(target_arch = "wasm32"))]
    want_at_start: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    was_focus: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    screen_stops: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    tab_queue: Vec<bool>,
    /// Escape was pressed this frame (native: taken out of egui's input, see `filter_tab`).
    esc_pressed: bool,
    last_save: Option<String>,
    /// A write was in flight at the last announcement ("Saved." is said when it ends, not when the header's
    /// half second of "Saving..." is over: that would be a second utterance after the decision's own).
    last_busy: bool,
    /// Per page: its status and a hash of its transcript as last seen, so the start and the end of a
    /// transcription (and a transcript replaced in one step) are spoken (the JS app has a status pill).
    run_seen: HashMap<String, (String, u64)>,
    said_log: Vec<String>,
    /// The last utterance and how many there were (native: the live region of the accessibility tree).
    #[cfg(not(target_arch = "wasm32"))]
    native_said: String,
    #[cfg(not(target_arch = "wasm32"))]
    native_said_n: u32,
    /// The window / tab title last set (`<assignment> | viSHion` once an assignment is open).
    last_title: Option<String>,
    status_err: bool,
    /// The toast on show and the egui time it first showed (JS toasts leave after 4 s, 8 s for an error).
    status_seen: Option<((String, u64), f64)>,
    dialog: Option<dialog::Dialog>,
    /// Scroll position and size of the page stage, kept so zoom can hold the middle of the view.
    stage: review::Stage,
    /// Pictures the screen asked for while drawing (inbox thumbnails).
    want: Vec<String>,
    /// Colour emoji: the face (linked in natively, fetched on the web) and the pictures drawn from it.
    emoji: emoji::EmojiCache,
    /// The camera's latest frame as a texture (the Capture view).
    cam_tex: Option<egui::TextureHandle>,
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
    /// Forced colours (web): the user's system colours as tokens, read once at start.
    forced: Option<theme::Tokens>,
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
                widgets::set_reduced_motion(&ctx, true);
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
            ctl: Ctl { dom_text: cfg!(target_arch = "wasm32"), camera_ok: cfg!(target_arch = "wasm32"), ..Ctl::default() },
            last_nodes: vec![],
            last_chrome: HashMap::new(),
            last_screen: (0.0, 0.0),
            host: None,
            boot: Boot::Loading,
            home: Default::default(),
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
            last_toast_serial: 0,
            prev_focus: None,
            #[cfg(not(target_arch = "wasm32"))]
            want_at_start: None,
            #[cfg(not(target_arch = "wasm32"))]
            was_focus: None,
            #[cfg(not(target_arch = "wasm32"))]
            screen_stops: vec![],
            #[cfg(not(target_arch = "wasm32"))]
            tab_queue: vec![],
            esc_pressed: false,
            last_save: None,
            last_busy: false,
            run_seen: HashMap::new(),
            said_log: vec![],
            #[cfg(not(target_arch = "wasm32"))]
            native_said: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            native_said_n: 0,
            last_title: None,
            status_err: false,
            status_seen: None,
            dialog: None,
            stage: review::Stage::default(),
            want: vec![],
            cam_tex: None,
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
            forced: None,
            exit_after: None,
            #[cfg(not(target_arch = "wasm32"))]
            screenshot: None,
            #[cfg(not(target_arch = "wasm32"))]
            dump_controls: false,
            #[cfg(not(target_arch = "wasm32"))]
            closing: false,
        };
        app.high_contrast = high_contrast;
        #[cfg(target_arch = "wasm32")]
        {
            app.forced = web::forced_tokens(&theme::LIGHT);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            app.forced = native::forced_tokens(&theme::LIGHT);
            app.high_contrast = app.forced.is_some();
        }
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

    /// Offer (or hide) the camera on the Add screen.
    #[doc(hidden)]
    pub fn camera(mut self, on: bool) -> Self {
        self.ctl.camera_ok = on;
        self
    }

    #[doc(hidden)]
    pub fn twin_text(&self, id: &str, value: &str) {
        self.queue.lock().unwrap().push(format!("text:{id}\u{1}{value}"));
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
                    // js/fs.js `notice()`: once, when the work lives in this browser and the person chose browser
                    // storage (`settings.storage === 'idb'`, set by `useBrowserStorage`); it then writes `noticeSeen`
                    let mut settings = settings;
                    let chose_browser = settings.get("storage").and_then(|v| v.as_str()) == Some("idb");
                    let seen = settings.get("noticeSeen").and_then(|v| v.as_bool()) == Some(true);
                    let mut notice_seen_now = false;
                    if self.storage == "Saved in this browser" && chose_browser && !seen {
                        if self.status_msg.is_none() {
                            self.status_msg = Some("Saves live in this browser only. Use Export to keep a copy.".into());
                            self.status_err = false;
                            self.ctl.toast_serial += 1;
                        }
                        settings.set("noticeSeen", serde_json::json!(true));
                        notice_seen_now = true;
                    }
                    self.home = Default::default();
                    self.images.clear();
                    let mut sess = state::Session::new(slug, *assignment, *settings, now_ms());
                    if let Some(id) = &self.start_page {
                        sess.select(id);
                    }
                    // an assignment with no pages has nothing to review: it opens on the overview, where "Add pages" is
                    if self.start_inbox || sess.a.pages.is_empty() {
                        sess.view = state::View::Inbox;
                    }
                    if self.start_no_tray {
                        sess.tray_open = false;
                    }
                    // (`S.saveSettings({ noticeSeen: true })` writes `settings.json` and the browser copy)
                    sess.dirty.settings |= notice_seen_now;
                    self.session = Some(sess);
                    self.boot = Boot::Ready;
                }
                Event::Folder(f) => self.home.folder = Some(f),
                Event::LegacyAvailable(on) => self.home.legacy = on,
                Event::ConnectionTested { ok, text } => {
                    if let Some(s) = &mut self.session {
                        s.conn_test = Some((ok, text));
                    }
                }
                Event::Projects(cards) => {
                    self.home.cards = Some(cards);
                    self.home.asked = false;
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
                        self.ctl.toast_serial += 1;
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
                    self.ctl.toast_serial += 1;
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
                    let mut start = None;
                    if let Some(s) = &mut self.session {
                        let ids: Vec<String> = list.iter().map(|p| p.id().to_string()).collect();
                        s.pages_added(list);
                        if s.add.starting {
                            // the Add screen's "Transcribe N pages": back to the inbox, and the run begins
                            s.add.starting = false;
                            s.view = state::View::Inbox;
                            start = Some((s.slug.clone(), ids, s.read_conn()));
                        } else if let Some(f) = first {
                            s.select(&f);
                        }
                    }
                    if let Some((slug, ids, conn)) = start {
                        self.send(Cmd::Transcribe { slug, ids, conn });
                    }
                    if n > 0 {
                        self.status_msg = Some(format!("Added {}.", state::plural(n, "page", "pages")));
                        self.ctl.toast_serial += 1;
                        self.status_err = false;
                    }
                }
                Event::Staged { item, names_down } => {
                    if let Some(s) = &mut self.session {
                        s.add.seq += 1;
                        let key = s.add.seq;
                        let slot = decode_image(&self.ctx, &format!("staged-{key}"), &item.bytes).map_or(Slot::Missing, Slot::Ready);
                        self.images.insert(format!("staged-{key}"), slot);
                        s.add.names_down |= names_down;
                        s.add.rows.push(state::StagedRow { key, item, picking: false, draft: String::new() });
                    }
                }
                Event::CameraFrame { w, h, rgba } => {
                    if rgba.len() == (w * h * 4) as usize {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                        self.cam_tex = Some(self.ctx.load_texture("camera", ci, egui::TextureOptions::LINEAR));
                    }
                    let mut shot = None;
                    if let Some(s) = &mut self.session {
                        if s.view == state::View::Capture && rgba.len() == (w * h * 4) as usize {
                            let found = vishion_core::capture::detect_page(&rgba, w as usize, h as usize);
                            let c = &mut s.capture;
                            c.error = None;
                            c.found = found;
                            c.frame = Some((w, h, rgba));
                            if c.cooldown > 0 {
                                // just after a shot: the same page must not be taken twice, and counting starts over
                                c.cooldown -= 1;
                                c.steady.reset();
                            } else if c.steady.feed(found) && c.auto {
                                shot = Some(());
                            }
                        }
                    }
                    if shot.is_some() {
                        if let Some(s) = &mut self.session {
                            if let Some(cmd) = review::capture_shot(s) {
                                self.send(cmd);
                            }
                        }
                    }
                }
                Event::CameraError(m) => {
                    if let Some(s) = &mut self.session {
                        s.capture.error = Some(m);
                    }
                }
                Event::Skipped(list) => {
                    if let Some(s) = &mut self.session {
                        s.add.skipped.extend(list);
                        s.add.working = false;
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
                    self.ctl.toast_serial += 1;
                    self.status_err = false;
                }
                Event::Reading { id, text } => {
                    if let Some(s) = &mut self.session {
                        s.page_reading(&id, &text);
                    }
                }
                Event::Transcribed { id, outcome } => {
                    use host::ReadOutcome::*;
                    match outcome {
                        Stopped => {
                            if let Some(s) = &mut self.session {
                                s.page_unread(&id);
                            }
                            self.status_msg = Some("Stopped.".into());
                            self.ctl.toast_serial += 1;
                            self.status_err = false;
                        }
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
                            if let Some(s) = &mut self.session {
                                s.page_unread(&id);
                            }
                            self.status_msg = Some(m);
                            self.ctl.toast_serial += 1;
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
                        self.ctl.toast_serial += 1;
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
        if !cmds.is_empty() {
            let now = self.ctx.input(|i| i.time);
            self.save.until = now + 0.5;
            self.ctx.request_repaint_after(Duration::from_millis(520));
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
        let forced = self.forced;
        let t: &theme::Tokens = match &forced {
            Some(f) => f,
            None => theme::pick_hc(&self.theme_name, system_dark, self.high_contrast),
        };
        if self.dark_applied != Some(t.dark) {
            theme::apply(ctx, t);
            // a new font atlas, with this theme's coverage gamma
            ctx.set_fonts(theme::fonts(self.fallback_font));
            self.dark_applied = Some(t.dark);
        }
        self.ctl.ring = t.ink;
        // the DOM transcript is painted in the same colours
        #[cfg(target_arch = "wasm32")]
        web::set_tokens(t);
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
    Some(Img { full, thumb, src: std::sync::Arc::new(rgba), scaled: Default::default(), id: id.to_string(), size, bands })
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let system_dark = frame.info().system_theme.map_or(false, |t| t == eframe::Theme::Dark);
        self.step(ctx, system_dark);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.filter_tab(raw);
    }
}

impl App {
    /// One frame: twin and host input, the screen, the twin output, saves, image requests, exit rules.
    fn step(&mut self, ctx: &egui::Context, system_dark: bool) {
        #[cfg(target_arch = "wasm32")]
        web::fit_scale(ctx);
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.ctl.kbd = false;
            self.ctl.pointer_at = now_ms() as f64;
            // (native: focus the mouse gave is egui's; the twin's own record of it is only for Tab)
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.ctl.focus = None;
                self.ctl.restore_kb = None;
            }
        }
        // `OCR.toast`: info toasts leave after 4 s, errors after 8 s, the storage notice after 9 s
        if let Some(m) = self.status_msg.clone() {
            let now = ctx.input(|i| i.time);
            let key = (m.clone(), self.ctl.toast_serial);
            if self.status_seen.as_ref().map(|(k, _)| k) != Some(&key) {
                self.status_seen = Some((key, now));
            }
            let ms = if m.starts_with("Saves live in this browser") { 9.0 } else if self.status_err { 8.0 } else { 4.0 };
            let at = self.status_seen.as_ref().map_or(now, |(_, t)| *t);
            if now - at >= ms {
                self.status_msg = None;
                self.status_seen = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        } else {
            self.status_seen = None;
        }
        // what the page made of the DOM transcript last frame (line heights, where the words landed)
        #[cfg(target_arch = "wasm32")]
        {
            let (h, c) = web::measured();
            self.ctl.dom_h = h;
            self.ctl.dom_chips = c;
        }
        self.ctl.drain(&self.queue);
        self.collect_egui_keys(ctx);
        self.handle_events();
        #[cfg(not(target_arch = "wasm32"))]
        self.run_tabs();
        self.ctl.focused = None;
        self.ctl.activated = None;
        self.ctl.lines.clear();
        self.ctl.chrome.clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.want_at_start = self.ctl.want_focus.clone();
        }
        self.ui_frame(ctx, system_dark);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.was_focus = self.prev_focus.clone();
        }
        self.prev_focus = self.ctl.focused.as_ref().map(|f| f.0.clone());
        if self.prev_focus.is_some() {
            self.ctl.restore_kb = self.prev_focus.clone();
        }
        // an activation or typed text whose control was not on screen this frame is dropped, not kept; what a
        // control has queued beyond the one it took this frame waits for the next frame
        let consumed = std::mem::take(&mut self.ctl.consumed);
        self.ctl.act.retain(|id| consumed.contains(id));
        {
            let pending = &self.ctl.act;
            self.ctl.act_mods.retain(|id, _| pending.contains(id));
        }
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
        // (a dialog reads and tabs in the order it was drawn, top to bottom: the footer button must not sort by
        // its place on the screen against controls that scroll past it, nor a scrolled control by its place
        // far below the window; the sort is stable)
        self.ctl.nodes.sort_by_key(|n| {
            let (r, y, x) = n.key();
            if r == twin::DIALOG {
                (r, 0, 0)
            } else {
                (r, y, x)
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        self.keep_focus_somewhere();
        #[cfg(not(target_arch = "wasm32"))]
        native::publish(ctx, &tree::of(&self.ctl.nodes), &self.ctl.widgets, &self.native_said, self.native_said_n, self.title());
        let nodes = std::mem::take(&mut self.ctl.nodes);
        let sr = ctx.screen_rect();
        let screen = (sr.width(), sr.height());
        let chrome = std::mem::take(&mut self.ctl.chrome);
        if nodes != self.last_nodes || screen != self.last_screen || chrome != self.last_chrome {
            #[cfg(target_arch = "wasm32")]
            {
                web::place(screen);
                web::set_chrome(&chrome);
                web::sync(&nodes);
            }
            self.last_nodes = nodes;
            self.last_screen = screen;
            self.last_chrome = chrome;
        }
        // the browser lays the transcript out: when it came to other heights than the ones this frame
        // allocated (a user stylesheet, a font that arrived), draw the screen again with them
        #[cfg(target_arch = "wasm32")]
        if web::measure() {
            ctx.request_repaint();
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
        self.announce_runs();
        if let Some(id) = self.ctl.focus_title {
            if text_of(&self.ctl.nodes, id).is_some() {
                self.ctl.focus_title = None;
                self.ctl.dom_focus = Some(id.to_string());
            }
        }
        self.ctl.say_wait = std::mem::take(&mut self.ctl.say_nodes);
        if !self.ctl.say_wait.is_empty() {
            self.ctx.request_repaint();
        }
        let toast = text_of(&self.ctl.nodes, "toast").map(String::from);
        if toast != self.last_toast || (toast.is_some() && self.ctl.toast_serial != self.last_toast_serial) {
            if let Some(t) = &toast {
                self.ctl.said.push((t.clone(), self.status_err));
            }
            self.last_toast = toast;
        }
        self.last_toast_serial = self.ctl.toast_serial;
        let save = text_of(&self.ctl.nodes, "save-state").map(String::from);
        if save != self.last_save {
            match (&self.last_save, &save) {
                (_, Some(now)) if now.starts_with("Couldn’t save") => self.ctl.said.push((now.clone(), true)),
                _ => {}
            }
            self.last_save = save;
        }
        let busy = self.save.pending > 0;
        if self.last_busy && !busy && self.save.failed.is_none() {
            self.ctl.said.push(("Saved.".into(), false));
        }
        self.last_busy = busy;
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

    /// Say when the open page starts being read, when its transcript arrives (or is replaced) and when it
    /// fails. Without this "Transcribe this page again" replaces the text in silence.
    fn announce_runs(&mut self) {
        let Some(s) = &self.session else { return };
        let mut news: Vec<(String, bool)> = vec![];
        let mut seen: HashMap<String, (String, u64)> = HashMap::new();
        for p in s.pages() {
            let status = p.status().to_string();
            let hash = if status == "transcribing" {
                0
            } else {
                p.raw().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
            };
            if s.selected.as_deref() == Some(p.id()) {
                if let Some((was, was_hash)) = self.run_seen.get(p.id()) {
                    if status == "transcribing" && was != "transcribing" {
                        news.push(("Reading this page. The text appears as it is read.".into(), false));
                    } else if status == "error" && was != "error" {
                        let why = p.s("error");
                        let why = if why.is_empty() { "" } else { why };
                        news.push((format!("This page could not be transcribed. {why}").trim().to_string(), true));
                    } else if status == "done" && (was == "transcribing" || *was_hash != hash) {
                        let label = s.group().map(|g| s.pill(&g).1).unwrap_or_default();
                        news.push((format!("The page was transcribed. {label}."), false));
                    }
                }
            }
            seen.insert(p.id().to_string(), (status, hash));
        }
        self.run_seen = seen;
        for n in news {
            self.ctl.said.push(n);
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
        // Escape always goes through (as on the web twin); egui never saw it, so it did not drop focus
        if std::mem::take(&mut self.esc_pressed) {
            self.ctl.keys.push(KeyEv { key: "Escape".into(), alt: false });
            self.ctl.kbd = true;
        }
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
                    egui::Key::PageUp => "PageUp",
                    egui::Key::PageDown => "PageDown",
                    egui::Key::Delete => "Delete",
                    egui::Key::Escape => "Escape",
                    egui::Key::Plus => "+",
                    egui::Key::Equals => "=",
                    egui::Key::Minus => "-",
                    egui::Key::Num0 => "0",
                    egui::Key::Z if modifiers.command && !modifiers.shift => "undo",
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
    /// Tab is the screen's, not egui's: the keys are taken out of the input before egui sees them (it would
    /// walk its widgets in drawing order) and `step` moves focus along the twin's reading order instead.
    fn filter_tab(&mut self, raw: &mut egui::RawInput) {
        let mut tabs = vec![];
        // A modal dialog keeps the arrow keys inside it: egui's own arrow navigation looks for the nearest widget
        // in that direction among everything it drew, including the screen behind the dialog (focus then landed
        // on an unnamed widget out of the dialog). Arrows step along the dialog's tab order instead, except in a
        // text box, where they move the caret.
        let in_text = self.prev_focus.as_deref().is_some_and(|f| {
            self.last_nodes.iter().any(|n| matches!(n, Node::Control(c) if c.id == f && c.role == "textbox"))
        });
        let arrows = self.ctl.dialog_was && !in_text;
        let mut esc = false;
        raw.events.retain(|e| match e {
            egui::Event::Key { key, pressed, modifiers, .. }
                if arrows
                    && matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight)
                    && !modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.command =>
            {
                if *pressed {
                    tabs.push(matches!(key, egui::Key::ArrowUp | egui::Key::ArrowLeft));
                }
                false
            }
            // Escape is the screen's too: egui answers it by dropping focus to the window, so a person on a plain
            // control (Zoom in) who presses it would lose their place; the js page does nothing there.
            egui::Event::Key { key: egui::Key::Escape, pressed, .. } => {
                esc |= *pressed;
                false
            }
            egui::Event::Key { key: egui::Key::Tab, pressed, modifiers, .. } if !modifiers.ctrl && !modifiers.alt && !modifiers.command => {
                if *pressed {
                    tabs.push(modifiers.shift);
                }
                false
            }
            _ => true,
        });
        self.tab_queue.extend(tabs);
        self.esc_pressed |= esc;
    }

    /// Carry out the Tab presses of this frame: the next stop of the twin's reading order gets focus.
    fn run_tabs(&mut self) {
        let mut cur = self.prev_focus.clone();
        for back in std::mem::take(&mut self.tab_queue) {
            if let Some(next) = twin::next_stop(&self.last_nodes, cur.as_deref(), back) {
                self.ctl.focus = None;
                self.ctl.want_focus = Some(next.clone());
                self.ctl.kbd = true;
                cur = Some(next);
            }
        }
    }

    /// Focus never falls to the window. A control that had focus and is gone (a button of a confirmation that
    /// was accepted, a page that was removed, a button that is unavailable while the work it started runs), or
    /// a focus that was asked for and that nothing drew, moves to the nearest stop that is there: the one
    /// before it in the order it had, else the one after, else the first. Otherwise a screen reader reads the
    /// window's title and Tab starts again at the top.
    fn keep_focus_somewhere(&mut self) {
        let stops: Vec<String> = twin::tab_stops(&self.ctl.nodes).into_iter().map(String::from).collect();
        let pending = self.want_at_start.take().filter(|w| self.ctl.want_focus.as_deref() == Some(w.as_str()));
        let lost = match &pending {
            Some(w) => Some(w.clone()),
            None => self.was_focus.clone().filter(|f| self.ctl.focused.is_none() && !stops.contains(f)),
        };
        if self.dialog.is_none() {
            self.screen_stops = stops.clone();
        }
        let Some(lost) = lost else { return };
        if stops.is_empty() || stops.contains(&lost) {
            return;
        }
        let before: Vec<String> = if self.screen_stops.contains(&lost) {
            self.screen_stops.clone()
        } else {
            twin::tab_stops(&self.last_nodes).into_iter().map(String::from).collect()
        };
        let target = match before.iter().position(|s| *s == lost) {
            Some(i) => before[..i].iter().rev().chain(before[i + 1..].iter()).find(|s| stops.contains(s)).cloned(),
            None => None,
        }
        .unwrap_or_else(|| stops[0].clone());
        self.ctl.focus = None;
        // (js: the focused word is gone and focus falls to the body; nothing scrolls to find it a home)
        self.ctl.quiet_focus = Some(target.clone());
        self.ctl.want_focus = Some(target);
        self.ctx.request_repaint();
    }

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
