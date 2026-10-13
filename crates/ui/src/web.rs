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
use crate::twin::{Node, Part, Reg};
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
    if !matches!(el.tag_name().as_str(), "BUTTON" | "INPUT") {
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
                q.lock().unwrap().push(format!("{prefix}:{id}"));
                ctx.request_repaint();
            }
        });
        let _ = h.add_event_listener_with_callback(kind, cb.as_ref().unchecked_ref());
        cb.forget();
    };
    listen("click", "act");
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
    let (q3, c3) = (q, ctx);
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
}

impl Spec {
    fn new(key: &str, tag: &'static str) -> Spec {
        Spec { key: key.into(), tag, attrs: vec![], text: None, value: None, children: None }
    }
    fn attr(mut self, k: &'static str, v: impl Into<String>) -> Spec {
        self.attrs.push((k, Some(v.into())));
        self
    }
    fn text(mut self, t: &str) -> Spec {
        self.text = Some(t.into());
        self
    }
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
        Spec::new(&r.id, "button").attr("type", "button").text(&r.label)
    };
    s.attrs.push(("aria-pressed", r.pressed.map(|p| if p { "true".to_string() } else { "false".to_string() })));
    s.attrs.push(("disabled", if r.disabled { Some(String::new()) } else { None }));
    s
}

fn spec_of(n: &Node) -> Spec {
    match n {
        Node::Heading { id, level, text, .. } => Spec::new(id, match level {
            1 => "h1",
            2 => "h2",
            _ => "h3",
        })
        .text(text),
        Node::Status { id, text, .. } => Spec::new(id, "div").attr("role", "status").text(text),
        Node::Text { id, text, .. } => Spec::new(id, "p").text(text),
        Node::Para { id, parts, .. } => {
            let mut p = Spec::new(id, "p");
            let mut kids = vec![];
            for (i, part) in parts.iter().enumerate() {
                kids.push(match part {
                    Part::Text(t) => Spec::new(&format!("{id}#t{i}"), "span").text(t),
                    Part::Math(t) => Spec::new(&format!("{id}#m{i}"), "span").attr("role", "math").text(t),
                    Part::Control(r) => control(r),
                });
            }
            p.children = Some(kids);
            p
        }
        Node::Control(r) => control(r),
    }
}

fn find_child(parent: &web_sys::Element, key: &str) -> Option<web_sys::Element> {
    let kids = parent.children();
    (0..kids.length()).filter_map(|i| kids.item(i)).find(|k| k.get_attribute("data-id").as_deref() == Some(key))
}

/// Make `parent`'s children equal `specs`, reusing nodes by `data-id`. Stale nodes go first, then each
/// wanted node is placed after its predecessor only when it is not already there: moving an element in
/// the DOM drops its focus, and a focused twin must survive every re-sync.
fn reconcile(doc: &web_sys::Document, parent: &web_sys::Element, specs: &[Spec]) {
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
        if let Some(t) = &sp.text {
            if el.text_content().as_deref() != Some(t.as_str()) {
                el.set_text_content(Some(t));
            }
        }
        if let (Some(v), Some(input)) = (&sp.value, el.dyn_ref::<web_sys::HtmlInputElement>()) {
            let focused = doc.active_element().is_some_and(|a| a.is_same_node(Some(&el)));
            if !focused && input.value() != *v {
                input.set_value(v);
            }
        }
        if let Some(kids) = &sp.children {
            reconcile(doc, &el, kids);
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

pub fn sync(nodes: &[Node]) {
    let (Some(h), Some(doc)) = (host(), document()) else { return };
    let specs: Vec<Spec> = nodes.iter().map(spec_of).collect();
    reconcile(&doc, &h, &specs);
}
