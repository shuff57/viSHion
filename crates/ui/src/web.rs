//! Hidden-DOM twin for the canvas (web only). Ported from spike S8: delegated listeners on `#controls`,
//! nodes reconciled by `data-id` and by position so focus survives re-syncs.
//!
//! Keyboard model (plan rule 3, WCAG 2.1.1 / 2.1.2 / 4.1.2):
//! - the canvas is `tabindex=-1` + `aria-hidden`: pointer-focusable (so egui gets keys after a click) but
//!   never part of the Tab order and never exposed to assistive tech;
//! - the twin buttons in `#controls` are the keyboard and AT surface, in on-screen reading order;
//! - egui would swallow Tab on the canvas (eframe `preventDefault`s it), so a capture-phase listener keeps
//!   Tab away from eframe and lets the browser move focus: no keyboard trap;
//! - eframe's IME text agent `<input>` is named and removed from the Tab order.
use crate::a11y;
use crate::twin::{Node, Part, Reg, DIALOG};
use crate::Queue;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn host() -> Option<web_sys::Element> {
    document()?.get_element_by_id("controls")
}

/// `data-id` of the event target when it is a control (a paragraph or heading never activates anything).
fn data_id(e: &web_sys::Event) -> Option<String> {
    let el = e.target()?.dyn_into::<web_sys::Element>().ok()?;
    // buttons, text boxes and the focusable key regions (`role=group`, which carry `data-keys`)
    if !matches!(el.tag_name().as_str(), "BUTTON" | "INPUT") && !el.has_attribute("data-keys") {
        return None;
    }
    el.get_attribute("data-id")
}

/// eframe's hidden IME `<input type=text>`: a direct child of `<body>` (not one of ours).
fn text_agent() -> Option<web_sys::Element> {
    document()?
        .query_selector("body > input[type=text]")
        .ok()
        .flatten()
}

pub fn install(q: Queue, ctx: egui::Context) {
    let Some(h) = host() else { return };
    let listen = |kind: &'static str, prefix: &'static str| {
        let (q, ctx) = (q.clone(), ctx.clone());
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            if let Some(id) = data_id(&e) {
                // a click made with Shift or Ctrl held (Shift+Enter on a page thumbnail) says so
                let mut mods = String::new();
                if prefix == "act" {
                    if let Some(m) = e.dyn_ref::<web_sys::MouseEvent>() {
                        if m.ctrl_key() || m.meta_key() {
                            mods.push('c');
                        }
                        if m.shift_key() {
                            mods.push('s');
                        }
                    }
                }
                if mods.is_empty() {
                    q.lock().unwrap().push(format!("{prefix}:{id}"));
                } else {
                    q.lock().unwrap().push(format!("{prefix}:{id}\u{1}{mods}"));
                }
                ctx.request_repaint();
            }
        });
        let _ = h.add_event_listener_with_callback(kind, cb.as_ref().unchecked_ref());
        cb.forget();
    };
    listen("click", "act");
    // focus arriving by keyboard (`:focus-visible`: Tab, or a script while the keyboard is in use) brings up
    // the rings of regions that hide theirs for a mouse click
    {
        let (q, ctx) = (q.clone(), ctx.clone());
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
            if el.matches(":focus-visible").unwrap_or(true) {
                q.lock().unwrap().push("kbd:1".to_string());
                ctx.request_repaint();
            }
        });
        let _ = h.add_event_listener_with_callback("focusin", cb.as_ref().unchecked_ref());
        cb.forget();
    }
    listen("focusin", "focus");
    listen("focusout", "blur");
    // text typed into a twin textbox goes to the egui buffer; Enter in it presses its `data-enter` control
    let (q2, c2) = (q.clone(), ctx.clone());
    let on_input = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) else { return };
        if let Some(id) = el.get_attribute("data-id") {
            q2.lock().unwrap().push(format!("text:{id}\x01{}", el.value()));
            c2.request_repaint();
        }
    });
    let _ = h.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref());
    on_input.forget();
    let (q3, c3) = (q.clone(), ctx.clone());
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if e.key() != "Enter" {
            return;
        }
        let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if let Some(id) = el.get_attribute("data-enter") {
            e.prevent_default();
            q3.lock().unwrap().push(format!("act:{id}"));
            c3.request_repaint();
        }
    });
    let _ = h.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    // Keys the screen handles by where focus is (js: handlers on #tx-text, .scan-pages, the stage, .tray).
    // Escape always goes through (the tray and dialogs close on it); the others only inside their region.
    let (q4, c4) = (q.clone(), ctx.clone());
    let on_region_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        let key = e.key();
        let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") && key != "Escape" {
            return;
        }
        let allowed = match key.as_str() {
            "Escape" => true,
            k => match scope_of(&el) {
                Some("stage") => matches!(k, "+" | "=" | "-" | "0" | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "PageUp" | "PageDown" | "Home" | "End"),
                Some("transcript") => matches!(k, "ArrowUp" | "ArrowDown"),
                Some("strip") => matches!(k, "ArrowLeft" | "ArrowRight" | "Home" | "End" | "Delete"),
                _ => false,
            },
        };
        if !allowed {
            return;
        }
        if key != "Escape" {
            e.prevent_default();
        }
        let m = if e.alt_key() { format!("key:{key}\u{1}alt") } else { format!("key:{key}") };
        q4.lock().unwrap().push(m);
        c4.request_repaint();
    });
    let _ = h.add_event_listener_with_callback("keydown", on_region_key.as_ref().unchecked_ref());
    on_region_key.forget();

    // A modal dialog keeps Tab inside it (WCAG: focus is not left behind the scrim); Escape always closes it,
    // so it is never a trap.
    let on_dialog_tab = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if e.key() != "Tab" {
            return;
        }
        let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        let Some(dialog) = el.closest("[role=dialog]").ok().flatten() else { return };
        let list = focusables(&dialog);
        let (Some(first), Some(last)) = (list.first(), list.last()) else { return };
        let on_first = first.is_same_node(Some(&el));
        let on_last = last.is_same_node(Some(&el));
        if e.shift_key() && on_first {
            e.prevent_default();
            let _ = last.focus();
        } else if !e.shift_key() && on_last {
            e.prevent_default();
            let _ = first.focus();
        }
    });
    let _ = h.add_event_listener_with_callback("keydown", on_dialog_tab.as_ref().unchecked_ref());
    on_dialog_tab.forget();

    // Escape when nothing in the twin has focus (a dialog just replaced the control that did): the page
    // body gets the key. The canvas and eframe's text agent deliver Escape to egui themselves.
    if let Some(doc) = document() {
        let (q5, c5) = (q, ctx);
        let on_doc_escape = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if e.key() != "Escape" {
                return;
            }
            let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
            let inside_twin = el.closest("#controls").ok().flatten().is_some();
            if inside_twin || matches!(el.tag_name().as_str(), "CANVAS" | "INPUT") {
                return;
            }
            q5.lock().unwrap().push("key:Escape".to_string());
            c5.request_repaint();
        });
        let _ = doc.add_event_listener_with_callback("keydown", on_doc_escape.as_ref().unchecked_ref());
        on_doc_escape.forget();
    }
}

/// Which keyboard region an element belongs to, by its own `data-id` or an ancestor's.
fn scope_of(el: &web_sys::Element) -> Option<&'static str> {
    let mut cur = Some(el.clone());
    while let Some(e) = cur {
        if let Some(id) = e.get_attribute("data-id") {
            if id == "stage" {
                return Some("stage");
            }
            if id == "transcript" || id.starts_with("word-") || id.starts_with("line-") {
                return Some("transcript");
            }
            if id.starts_with("page-") {
                return Some("strip");
            }
        }
        if e.id() == "controls" {
            break;
        }
        cur = e.parent_element();
    }
    None
}

/// Move DOM focus to the twin with this `data-id` (strip arrows, tray close).
pub fn focus(id: &str) {
    let Some(doc) = document() else { return };
    let Ok(list) = doc.query_selector_all("#controls [data-id]") else { return };
    for i in 0..list.length() {
        if let Some(el) = list.item(i).and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
            if el.get_attribute("data-id").as_deref() == Some(id) {
                let _ = el.focus();
                return;
            }
        }
    }
}

/// Call once after eframe has started (it sets `tabindex=0` on the canvas itself).
pub fn harden(canvas_id: &str) {
    let Some(doc) = document() else { return };
    let canvas = doc.get_element_by_id(canvas_id);
    if let Some(c) = &canvas {
        let _ = c.set_attribute("tabindex", "-1");
        let _ = c.set_attribute("aria-hidden", "true");
    }
    let agent = text_agent();
    if let Some(a) = &agent {
        let _ = a.set_attribute("tabindex", "-1");
        let _ = a.set_attribute("aria-hidden", "true");
        let _ = a.set_attribute("aria-label", "Text input bridge");
        let _ = a.set_attribute("autocomplete", "off");
    }
    // Tab pressed while the canvas (or the text agent) has focus: hide it from eframe so it is not
    // preventDefault-ed; the browser then moves focus to the next/previous tabbable element.
    let canvas_id = canvas_id.to_string();
    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if e.key() != "Tab" {
            return;
        }
        let Some(t) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        if t.id() == canvas_id
            || t.parent_element().is_some_and(|p| p.tag_name() == "BODY") && t.tag_name() == "INPUT"
        {
            e.stop_propagation();
        }
    });
    let _ =
        doc.add_event_listener_with_callback_and_bool("keydown", cb.as_ref().unchecked_ref(), true);
    cb.forget();
}

/// One DOM element of the twin, described before it touches the document.
struct Spec {
    key: String,
    tag: &'static str,
    attrs: Vec<(&'static str, Option<String>)>,
    text: Option<String>,
    value: Option<String>,
    children: Option<Vec<Spec>>,
    /// Position and size in CSS pixels. The twin is laid over the canvas where its widget is drawn, so
    /// touch exploration (VoiceOver, TalkBack) and magnifier focus tracking land on the right place.
    rect: Option<(i32, i32, i32, i32)>,
}

impl Spec {
    fn new(key: &str, tag: &'static str) -> Spec {
        Spec { key: key.into(), tag, attrs: vec![], text: None, value: None, children: None, rect: None }
    }
    fn attr(mut self, k: &'static str, v: impl Into<String>) -> Spec {
        self.attrs.push((k, Some(v.into())));
        self
    }
    fn text(mut self, t: &str) -> Spec {
        self.text = Some(t.into());
        self
    }
    fn kids(mut self, kids: Vec<Spec>) -> Spec {
        self.children = Some(kids);
        self
    }
}

/// A rectangle in points as the twin lays it out: left, top, width, height; none when it is not known.
fn rect_of(at: (i32, i32), size: (u16, u16)) -> Option<(i32, i32, i32, i32)> {
    (size.0 > 0 && size.1 > 0).then_some((at.0, at.1, size.0 as i32, size.1 as i32))
}

fn control(r: &Reg) -> Spec {
    let mut s = if r.role == "textbox" {
        let mut s = Spec::new(&r.id, "input").attr("type", "text").attr("aria-label", r.label.clone()).attr("autocomplete", "off");
        s.value = r.value.clone();
        // Enter in the twin textbox presses the control next to it
        if r.id == "typed-word" {
            s = s.attr("data-enter", "use-typed");
        }
        s
    } else {
        let mut s = Spec::new(&r.id, "button").attr("type", "button").text(&r.label);
        if matches!(r.role, "checkbox" | "switch") {
            s = s.attr("role", r.role);
        }
        s
    };
    let state = r.pressed.map(|p| if p { "true".to_string() } else { "false".to_string() });
    if matches!(r.role, "checkbox" | "switch") {
        s.attrs.push(("aria-checked", state));
    } else {
        s.attrs.push(("aria-pressed", state));
    }
    s.attrs.push(("disabled", if r.disabled { Some(String::new()) } else { None }));
    s.attrs.push(("aria-description", if r.disabled { a11y::disabled_reason(&r.id).map(String::from) } else { None }));
    if r.size.0 > 0 && r.size.1 > 0 {
        s.rect = Some((r.at.0, r.at.1, r.size.0 as i32, r.size.1 as i32));
    }
    s
}

fn spec_of(n: &Node) -> Spec {
    match n {
        Node::Heading { id, level, text, at, size, .. } => {
            let mut s = Spec::new(id, match level {
                1 => "h1",
                2 => "h2",
                _ => "h3",
            })
            .text(text);
            s.rect = rect_of(*at, *size);
            // the screen's h1 takes focus when a different screen replaces it (see `restore_focus`)
            if *level == 1 {
                s.attr("tabindex", "-1")
            } else {
                s
            }
        }
        // a problem the person has to act on is announced at once (role=alert), the rest politely. The
        // toast is spoken through `announce` (an element that arrives with its text is often not read).
        Node::Status { id, text, .. } if id == "toast" => Spec::new(id, "p").text(text),
        Node::Status { id, text, at, size, .. } if id.ends_with("-error") => {
            let mut s = Spec::new(id, "div").attr("role", "alert").text(text);
            s.rect = rect_of(*at, *size);
            s
        }
        // counts and the save state are read in place; what changed is said once, through `announce`
        Node::Status { id, text, at, size, .. } => {
            let mut s = Spec::new(id, "p").text(text);
            s.rect = rect_of(*at, *size);
            s
        }
        Node::Group { id, label, keys, at, size, .. } => {
            let mut s = Spec::new(id, "div").attr("role", "group").attr("tabindex", "0").attr("aria-label", label.clone()).attr("data-keys", *keys);
            if size.0 > 0 && size.1 > 0 {
                s.rect = Some((at.0, at.1, size.0 as i32, size.1 as i32));
            }
            s
        }
        Node::Text { id, text, at, size, .. } => {
            let mut s = Spec::new(id, "p").text(text);
            s.rect = rect_of(*at, *size);
            s
        }
        Node::Image { id, label, at, size, .. } => {
            let mut s = Spec::new(id, "div").attr("role", "img").attr("aria-label", label.clone());
            if size.0 > 0 && size.1 > 0 {
                s.rect = Some((at.0, at.1, size.0 as i32, size.1 as i32));
            }
            s
        }
        Node::Para { id, parts, reading, at, size, .. } => {
            let mut p = Spec::new(id, "p");
            p.rect = rect_of(*at, *size);
            // always listed, so the attribute is removed again when the underline moves on
            p.attrs.push(("data-reading", if *reading { Some("1".to_string()) } else { None }));
            let mut kids = vec![];
            for (i, part) in parts.iter().enumerate() {
                kids.push(match part {
                    Part::Text(t) => Spec::new(&format!("{id}#t{i}"), "span").text(t),
                    // spoken as words (aria-label), still readable as TeX (the text)
                    Part::Math(t) => Spec::new(&format!("{id}#m{i}"), "span")
                        .attr("role", "math")
                        .attr("aria-label", a11y::tex_speech(t))
                        .text(t),
                    Part::Del(t) => Spec::new(&format!("{id}#d{i}"), "del").text(t),
                    Part::Control(r) => control(r),
                });
            }
            p.children = Some(kids);
            p
        }
        Node::Control(r) => control(r),
    }
}

/// The nodes of one region as specs: consecutive related controls (`a11y::group_of`) share a group element,
/// and list items sit in `<li>`s of their `<ul>`.
fn region_specs(nodes: &[&Node]) -> Vec<Spec> {
    let mut out: Vec<Spec> = vec![];
    let mut open: Option<(&'static str, Spec)> = None;
    let flush = |open: &mut Option<(&'static str, Spec)>, out: &mut Vec<Spec>| {
        if let Some((_, g)) = open.take() {
            out.push(g);
        }
    };
    for n in nodes {
        let grp = match n {
            Node::Control(r) => a11y::group_of(&r.id),
            _ => None,
        };
        let Some((key, role, label)) = grp else {
            flush(&mut open, &mut out);
            out.push(spec_of(n));
            continue;
        };
        if open.as_ref().map(|(k, _)| *k) != Some(key) {
            flush(&mut open, &mut out);
            let g = if role == "list" {
                Spec::new(&format!("grp-{key}"), "ul").attr("role", "list")
            } else {
                Spec::new(&format!("grp-{key}"), "div").attr("role", role)
            };
            let mut g = g.attr("aria-label", label).kids(vec![]);
            // the same words as the JS group's `title`, read when the group is entered
            if let Some(d) = a11y::group_description(key, nodes) {
                g = g.attr("aria-description", d).attr("title", d);
            }
            open = Some((key, g));
        }
        let item = spec_of(n);
        if let Some((_, g)) = open.as_mut() {
            let kids = g.children.get_or_insert_with(Vec::new);
            if role == "list" {
                kids.push(Spec::new(&format!("li:{}", item.key), "li").kids(vec![item]));
            } else {
                kids.push(item);
            }
        }
    }
    flush(&mut open, &mut out);
    out
}

fn main_spec(kids: Vec<Spec>, modal: bool) -> Spec {
    let m = Spec::new("main", "div").attr("role", "main");
    let mut m = m;
    m.attrs.push(("inert", modal.then(String::new)));
    m.attrs.push(("aria-hidden", modal.then(|| "true".to_string())));
    m.kids(kids)
}

/// The whole twin: banner, `<main>` (page image, transcript), the word panel and a dialog, each a landmark.
fn structure(nodes: &[Node]) -> Vec<Spec> {
    let mut regions: Vec<(u8, Vec<&Node>)> = vec![];
    for n in nodes {
        // the toast is spoken through the live region; as a paragraph it would sit inside the transcript
        if matches!(n, Node::Status { id, .. } if id == "toast") {
            continue;
        }
        let r = n.key().0;
        match regions.last_mut() {
            Some((k, v)) if *k == r => v.push(n),
            _ => regions.push((r, vec![n])),
        }
    }
    let mut top: Vec<Spec> = vec![];
    let mut main: Vec<Spec> = vec![];
    // a modal dialog: the screen behind it is out of the tree and out of the Tab order (`inert`), with
    // aria-hidden for readers that do not know `inert`. It stays in the DOM so focus can come back to it.
    let modal = regions.iter().any(|(r, _)| *r == DIALOG);
    for (region, list) in &regions {
        let kids = region_specs(list);
        let mut s = Spec::new(&format!("region-{region}"), "div").attr("data-region", region.to_string());
        s.attrs.push(("inert", (modal && *region != DIALOG).then(String::new)));
        s.attrs.push(("aria-hidden", (modal && *region != DIALOG).then(|| "true".to_string())));
        if let Some((role, label)) = a11y::landmark(*region, list) {
            s = s.attr("role", role);
            if !label.is_empty() {
                s = s.attr("aria-label", label);
            }
            if role == "dialog" {
                s = s.attr("aria-modal", "true");
            }
        }
        let s = s.kids(kids);
        if a11y::in_main(*region) {
            main.push(s);
        } else {
            // `main` goes where its first region is, between the banner and the word panel
            if !main.is_empty() {
                top.push(main_spec(std::mem::take(&mut main), modal));
            }
            top.push(s);
        }
    }
    if !main.is_empty() {
        top.push(main_spec(main, modal));
    }
    // a landmark, a group or a list holds only positioned things: it would be a box with no size. It takes the
    // rectangle of what it holds, as `tree::of` does for the native tree.
    contain(&mut top);
    top
}

/// Give every container without a rectangle of its own the union of its children's, and return the union of
/// `specs`. The positions of the children are made relative to it by `reconcile`.
fn contain(specs: &mut [Spec]) -> Option<(i32, i32, i32, i32)> {
    let mut all: Option<(i32, i32, i32, i32)> = None;
    for s in specs.iter_mut() {
        if let Some(kids) = s.children.as_mut() {
            let inner = contain(kids);
            if s.rect.is_none() {
                s.rect = inner;
            }
        }
        if let Some((x, y, w, h)) = s.rect {
            all = Some(match all {
                None => (x, y, w, h),
                Some((ax, ay, aw, ah)) => {
                    let (l, t) = (ax.min(x), ay.min(y));
                    let (r, b) = ((ax + aw).max(x + w), (ay + ah).max(y + h));
                    (l, t, r - l, b - t)
                }
            });
        }
    }
    all
}

fn find_child(parent: &web_sys::Element, key: &str) -> Option<web_sys::Element> {
    let kids = parent.children();
    (0..kids.length()).filter_map(|i| kids.item(i)).find(|k| k.get_attribute("data-id").as_deref() == Some(key))
}

/// `origin` is the top-left of the nearest positioned ancestor (a paragraph is laid over its line and its
/// controls are placed inside it), so every rectangle stays where the canvas draws it.
fn set_style(el: &web_sys::Element, rect: Option<(i32, i32, i32, i32)>, origin: (i32, i32)) {
    let Some(style) = el.dyn_ref::<web_sys::HtmlElement>().map(|h| h.style()) else { return };
    match rect {
        Some((x, y, w, h)) => {
            let (x, y) = (x - origin.0, y - origin.1);
            let want = [("position", "absolute".to_string()), ("left", format!("{x}px")), ("top", format!("{y}px")), ("width", format!("{w}px")), ("height", format!("{h}px")), ("margin", "0".to_string())];
            for (k, v) in want {
                if style.get_property_value(k).ok().as_deref() != Some(v.as_str()) {
                    let _ = style.set_property(k, &v);
                }
            }
        }
        None => {
            if style.length() > 0 {
                for k in ["position", "left", "top", "width", "height", "margin"] {
                    let _ = style.remove_property(k);
                }
            }
        }
    }
}

/// Make `parent`'s children equal `specs`, reusing nodes by `data-id`. Stale nodes go first, then each
/// wanted node is placed after its predecessor only when it is not already there: moving an element in
/// the DOM drops its focus, and a focused twin must survive every re-sync.
fn reconcile(doc: &web_sys::Document, parent: &web_sys::Element, specs: &[Spec], origin: (i32, i32)) {
    let want: Vec<&str> = specs.iter().map(|s| s.key.as_str()).collect();
    let kids = parent.children();
    let mut seen: Vec<String> = vec![];
    for i in (0..kids.length()).rev() {
        if let Some(k) = kids.item(i) {
            match k.get_attribute("data-id") {
                Some(id) if want.contains(&id.as_str()) && !seen.contains(&id) => seen.push(id),
                _ => k.remove(),
            }
        }
    }
    let mut prev: Option<web_sys::Element> = None;
    for sp in specs {
        let existing = find_child(parent, &sp.key);
        let el = match existing {
            Some(e) if e.tag_name().eq_ignore_ascii_case(sp.tag) => e,
            other => {
                if let Some(stale) = other {
                    stale.remove();
                }
                match doc.create_element(sp.tag) {
                    Ok(e) => e,
                    Err(_) => continue,
                }
            }
        };
        let _ = el.set_attribute("data-id", &sp.key);
        for (k, v) in &sp.attrs {
            match v {
                Some(v) => {
                    if el.get_attribute(k).as_deref() != Some(v.as_str()) {
                        let _ = el.set_attribute(k, v);
                    }
                }
                None => {
                    if el.has_attribute(k) {
                        let _ = el.remove_attribute(k);
                    }
                }
            }
        }
        // attributes a role change leaves behind (a button that was a checkbox) are not kept
        set_style(&el, sp.rect, origin);
        if let Some(t) = &sp.text {
            if el.text_content().as_deref() != Some(t.as_str()) {
                el.set_text_content(Some(t));
            }
        }
        if let (Some(v), Some(input)) = (&sp.value, el.dyn_ref::<web_sys::HtmlInputElement>()) {
            // `data-app-value` is what the canvas box held when the twin was last told. Typing changes the
            // box in the twin first and the canvas follows a frame later, so a box that holds what the person
            // typed is left alone; when the screen itself changed the value (a decision cleared the field)
            // the twin follows even though it has focus, or a screen reader would read text that is not there.
            let told = el.get_attribute("data-app-value");
            let app_changed = told.as_deref() != Some(v.as_str());
            if app_changed {
                let _ = el.set_attribute("data-app-value", v);
            }
            let focused = doc.active_element().is_some_and(|a| a.is_same_node(Some(&el)));
            if input.value() != *v && (!focused || app_changed) {
                input.set_value(v);
            }
        }
        if let Some(kids) = &sp.children {
            let o = sp.rect.map_or(origin, |r| (r.0, r.1));
            reconcile(doc, &el, kids, o);
        }
        let in_place = match &prev {
            None => el.previous_element_sibling().is_none() && el.parent_element().is_some_and(|p| p.is_same_node(Some(parent))),
            Some(p) => el.previous_element_sibling().is_some_and(|s| s.is_same_node(Some(p))),
        };
        if !in_place {
            let before = match &prev {
                None => parent.first_element_child(),
                Some(p) => p.next_element_sibling(),
            };
            let _ = parent.insert_before(&el, before.as_deref());
        }
        prev = Some(el);
    }
}

const FOCUSABLE: &str = "button:not([disabled]), input:not([disabled]), [tabindex=\"0\"]";

fn region_of(el: &web_sys::Element) -> Option<web_sys::Element> {
    el.closest("[data-region]").ok().flatten()
}

fn focusables(root: &web_sys::Element) -> Vec<web_sys::HtmlElement> {
    let Ok(list) = root.query_selector_all(FOCUSABLE) else { return vec![] };
    (0..list.length()).filter_map(|i| list.item(i)).filter_map(|n| n.dyn_into::<web_sys::HtmlElement>().ok()).collect()
}

fn h1_ids(h: &web_sys::Element) -> Vec<String> {
    let Ok(list) = h.query_selector_all("h1") else { return vec![] };
    (0..list.length()).filter_map(|i| list.item(i)).filter_map(|n| n.dyn_into::<web_sys::Element>().ok()).filter_map(|e| e.get_attribute("data-id")).collect()
}

/// Reconcile the twin with this frame's nodes and keep keyboard focus somewhere sensible (WCAG 2.4.3):
/// when the focused control is removed or disabled by the action it started, focus goes to the new screen's
/// heading if a different screen came up, else to the neighbour that took its place in the same region.
/// Without this it would fall to `<body>` and the next Tab would start from the top of the page.
pub fn sync(nodes: &[Node]) {
    let (Some(h), Some(doc)) = (host(), document()) else { return };
    reconcile_into(&h, &doc, nodes);
}

/// Keep an egui point equal to a CSS pixel. egui draws `pixels_per_point` canvas pixels per point and the
/// page lays the twin out in CSS pixels; they agree when the canvas backing store is `devicePixelRatio` times
/// its CSS size. When a browser sizes the backing store otherwise (device-pixel size not reported, as in
/// emulated scale factors) the page would be laid out for a screen several times narrower than the window.
/// The zoom factor corrects the points so the layout is always for the window's CSS size.
pub fn fit_scale(ctx: &egui::Context) {
    let Some(w) = web_sys::window() else { return };
    let Some(canvas) = document().and_then(|d| d.get_element_by_id("c")).and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else { return };
    let css = canvas.client_width() as f32;
    let native = w.device_pixel_ratio() as f32;
    if css < 1.0 || canvas.width() == 0 || native <= 0.0 {
        return;
    }
    let want = (canvas.width() as f32 / css / native).clamp(0.1, 4.0);
    // within a rounding error of 1 is 1 (a canvas one pixel off the exact size)
    let want = if (want - 1.0).abs() < 0.02 { 1.0 } else { want };
    if (ctx.zoom_factor() - want).abs() > 0.005 {
        ctx.set_zoom_factor(want);
    }
}

/// The twin's rectangles are in egui points. Points are CSS pixels when the canvas is sized as the page
/// reports it; when a browser sizes the canvas differently (a backing store that does not follow
/// `devicePixelRatio`) a point is some other number of CSS pixels. The twin is a box `screen` points
/// across, scaled to fill the canvas, so every control stays on the widget it describes.
pub fn place(screen: (f32, f32)) {
    let (Some(h), Some(doc)) = (host(), document()) else { return };
    let Some(canvas) = doc.get_element_by_id("c") else { return };
    let Some(style) = h.dyn_ref::<web_sys::HtmlElement>().map(|e| e.style()) else { return };
    let css_w = canvas.client_width() as f32;
    if screen.0 < 1.0 || css_w < 1.0 {
        return;
    }
    let k = css_w / screen.0;
    if (k - 1.0).abs() < 0.002 {
        for p in ["width", "height", "transform", "transform-origin", "right", "bottom"] {
            let _ = style.remove_property(p);
        }
    } else {
        for (p, v) in [
            ("right", "auto".to_string()),
            ("bottom", "auto".to_string()),
            ("width", format!("{}px", screen.0)),
            ("height", format!("{}px", screen.1)),
            ("transform-origin", "0 0".to_string()),
            ("transform", format!("scale({k})")),
        ] {
            let _ = style.set_property(p, &v);
        }
    }
}

fn reconcile_into(h: &web_sys::Element, doc: &web_sys::Document, nodes: &[Node]) {
    let (h, doc) = (h.clone(), doc.clone());
    let was = doc.active_element().filter(|a| h.contains(Some(a)) && !a.is_same_node(Some(&h)));
    // where the focused control was: its region and place among that region's focusable controls, and its
    // place among the controls of its own group (the zoom buttons, the page strip)
    let was_at = was.as_ref().and_then(|a| {
        let reg = region_of(a)?;
        let idx = focusables(&reg).iter().position(|f| f.is_same_node(Some(a)))?;
        Some((reg.get_attribute("data-id")?, idx))
    });
    let was_in = was.as_ref().and_then(|a| {
        let g = a.parent_element()?.closest("[role=group], [role=list]").ok().flatten().filter(|g| h.contains(Some(g)))?;
        let idx = focusables(&g).iter().position(|f| f.is_same_node(Some(a)))?;
        Some((g.get_attribute("data-id")?, idx))
    });
    let was_id = was.as_ref().and_then(|a| a.get_attribute("data-id"));
    let was_name = was.as_ref().map(|a| a.get_attribute("aria-label").or_else(|| a.text_content()).unwrap_or_default());
    let h1_before = h1_ids(&h);
    let specs = structure(nodes);
    reconcile(&doc, &h, &specs, (0, 0));
    let Some(was) = was else { return };
    // A control that disables itself loses focus at once in current browsers (`activeElement` is the body
    // before the next frame), in older ones it keeps it: either way focus must be handed on.
    let lost = !was.is_connected() || was.has_attribute("disabled");
    if !lost {
        return; // focus is where the person put it
    }
    let now = doc.active_element();
    let elsewhere = now.as_ref().is_some_and(|n| h.contains(Some(n)) && !n.is_same_node(Some(&h)) && !n.is_same_node(Some(&was)));
    if elsewhere {
        return;
    }
    // a new screen heading wins
    if let Some(new_h1) = h1_ids(&h).into_iter().find(|i| !h1_before.contains(i)) {
        focus(&new_h1);
        return;
    }
    // the same control, rebuilt (its element type changed). A disabled one is not that: it still reads as the
    // active element until the browser's next focus fix-up, and then focus falls to the body.
    if let Some(id) = &was_id {
        if !was.has_attribute("disabled") || !was.is_connected() {
            focus(id);
            if doc.active_element().is_some_and(|a| a.get_attribute("data-id").as_deref() == Some(id.as_str()) && !a.has_attribute("disabled")) {
                return;
            }
        }
    }
    let mut landed = false;
    // a control of its own group first (Zoom in at the largest size: the control beside it)
    if let Some((group, idx)) = was_in {
        if let Some(g) = find_child_deep_any(&h, &group) {
            let list = focusables(&g);
            if !list.is_empty() {
                let _ = list[idx.min(list.len() - 1)].focus();
                landed = true;
            }
        }
    }
    if !landed {
        if let Some((region, idx)) = was_at {
            if let Some(reg) = find_child_deep(&h, &region) {
                let list = focusables(&reg);
                if !list.is_empty() {
                    let _ = list[idx.min(list.len() - 1)].focus();
                    landed = true;
                }
            }
        }
    }
    if !landed {
        if let Some(first) = focusables(&h).first() {
            let _ = first.focus();
            landed = true;
        }
    }
    // a control that went unavailable under the person's hands is said, or the change of focus is a puzzle
    if landed && was.is_connected() && was.has_attribute("disabled") {
        if let Some(n) = was_name.filter(|n| !n.is_empty()) {
            NOTICE.with(|c| *c.borrow_mut() = Some(format!("{n} is not available now.")));
        }
    }
}

thread_local! {
    /// News the twin itself produced while syncing (a focused control became unavailable): the app speaks it
    /// on its next frame, with the rest of the announcements.
    static NOTICE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The sentence `sync` left for the live region, once.
pub fn take_notice() -> Option<String> {
    NOTICE.with(|c| c.borrow_mut().take())
}

/// An element with this `data-id` anywhere under the twin (groups and lists as well as regions).
fn find_child_deep_any(h: &web_sys::Element, id: &str) -> Option<web_sys::Element> {
    let list = h.query_selector_all("[data-id]").ok()?;
    (0..list.length()).filter_map(|i| list.item(i)).filter_map(|n| n.dyn_into::<web_sys::Element>().ok()).find(|e| e.get_attribute("data-id").as_deref() == Some(id))
}

fn find_child_deep(h: &web_sys::Element, id: &str) -> Option<web_sys::Element> {
    let list = h.query_selector_all("[data-region]").ok()?;
    (0..list.length()).filter_map(|i| list.item(i)).filter_map(|n| n.dyn_into::<web_sys::Element>().ok()).find(|e| e.get_attribute("data-id").as_deref() == Some(id))
}

/// The browser tab title (`document.title`).
pub fn set_title(title: &str) {
    if let Some(d) = document() {
        if d.title() != title {
            d.set_title(title);
        }
    }
}

thread_local! {
    /// When each live region (polite, assertive) was last written, and what it holds.
    static SPOKEN: std::cell::RefCell<[(f64, String); 2]> = std::cell::RefCell::new([(0.0, String::new()), (0.0, String::new())]);
}

/// Speak `text` through the page's live regions (`#announce` polite, `#announce-alert` assertive). News that
/// arrives within a few frames of the last (one action, spoken in two steps) is added to it, not written over: a live
/// region rewritten twice in a moment is usually read as the second write alone, and the first (which
/// student this is, what was decided) would be lost. The same sentence twice in a row gets a trailing
/// no-break space so the second one is still a change.
pub fn announce(text: &str, assertive: bool) {
    let Some(doc) = document() else { return };
    let Some(el) = doc.get_element_by_id(if assertive { "announce-alert" } else { "announce" }) else { return };
    let now = js_sys::Date::now();
    let mut t = text.to_string();
    SPOKEN.with(|s| {
        let mut s = s.borrow_mut();
        let slot = &mut s[assertive as usize];
        let prev = slot.1.trim_end_matches('\u{a0}').to_string();
        if now - slot.0 < 300.0 && !prev.is_empty() && prev != t && prev.len() < 400 {
            t = format!("{prev} {t}");
        }
        if el.text_content().as_deref() == Some(t.as_str()) {
            t.push('\u{a0}');
        }
        *slot = (now, t.clone());
    });
    el.set_text_content(Some(&t));
}

/// A media query, e.g. `(prefers-reduced-motion: reduce)`.
pub fn media(query: &str) -> bool {
    web_sys::window().and_then(|w| w.match_media(query).ok().flatten()).is_some_and(|m| m.matches())
}

/// A CSS system colour (`Canvas`, `CanvasText`, `GrayText`, `LinkText`, `Highlight`) as the browser resolves it.
fn system_color(name: &str) -> Option<egui::Color32> {
    use wasm_bindgen::JsCast;
    let doc = document()?;
    let el = doc.create_element("div").ok()?.dyn_into::<web_sys::HtmlElement>().ok()?;
    el.style().set_property("color", name).ok()?;
    let body = doc.body()?;
    body.append_child(&el).ok()?;
    let css = web_sys::window()?.get_computed_style(&el).ok()??.get_property_value("color").ok();
    let _ = body.remove_child(&el);
    let css = css?;
    let nums: Vec<f32> = css.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    match nums.as_slice() {
        [r, g, b, ..] => Some(egui::Color32::from_rgb(*r as u8, *g as u8, *b as u8)),
        _ => None,
    }
}

/// Windows contrast themes and other forced colours: the canvas is drawn in the user's own colours, as the
/// DOM of the JS app is. Quiet greys, fills and tints collapse to `Canvas`; ink, rules and focus are
/// `CanvasText`; links and the accent are `LinkText`; disabled text is `GrayText`.
pub fn forced_tokens(base: &crate::theme::Tokens) -> Option<crate::theme::Tokens> {
    if !media("(forced-colors: active)") {
        return None;
    }
    let canvas = system_color("Canvas")?;
    let text = system_color("CanvasText")?;
    let gray = system_color("GrayText").unwrap_or(text);
    let link = system_color("LinkText").unwrap_or(text);
    Some(crate::theme::forced(base, canvas, text, gray, link))
}

/// Record the user's display preferences on the twin, where tests (and curious people) can see them.
pub fn mark_preferences(reduced_motion: bool, high_contrast: bool) {
    if let Some(h) = host() {
        let _ = h.set_attribute("data-reduced-motion", if reduced_motion { "1" } else { "0" });
        let _ = h.set_attribute("data-high-contrast", if high_contrast { "1" } else { "0" });
    }
}
