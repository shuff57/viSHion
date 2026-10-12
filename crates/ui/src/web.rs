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
use crate::{Queue, Reg};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn host() -> Option<web_sys::Element> {
    document()?.get_element_by_id("controls")
}

fn data_id(e: &web_sys::Event) -> Option<String> {
    e.target()?
        .dyn_into::<web_sys::Element>()
        .ok()?
        .get_attribute("data-id")
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

pub fn sync(regs: &[Reg]) {
    let (Some(h), Some(doc)) = (host(), document()) else {
        return;
    };
    let mut want: Vec<&str> = vec![];
    for (i, r) in regs.iter().enumerate() {
        want.push(&r.id);
        let el = match h
            .query_selector(&format!("[data-id=\"{}\"]", r.id))
            .ok()
            .flatten()
        {
            Some(e) => e,
            None => {
                let Ok(e) = doc.create_element("button") else {
                    continue;
                };
                let _ = e.set_attribute("data-id", &r.id);
                let _ = e.set_attribute("type", "button");
                e
            }
        };
        if el.text_content().as_deref() != Some(r.label.as_str()) {
            el.set_text_content(Some(&r.label));
        }
        match r.pressed {
            Some(p) => {
                let _ = el.set_attribute("aria-pressed", if p { "true" } else { "false" });
            }
            None => {
                let _ = el.remove_attribute("aria-pressed");
            }
        }
        // Keep DOM order == reading order, but never move a node that is already in place:
        // moving a focused element drops its focus.
        let cur = h.children().item(i as u32);
        if !el.is_same_node(cur.as_deref()) {
            let _ = h.insert_before(&el, cur.as_deref());
        }
    }
    let kids = h.children();
    for i in (0..kids.length()).rev() {
        if let Some(k) = kids.item(i) {
            if k.get_attribute("data-id")
                .is_some_and(|id| !want.contains(&id.as_str()))
            {
                k.remove();
            }
        }
    }
}
