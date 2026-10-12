//! Hidden-DOM twin for the canvas (web only). Ported from spike S8: delegated listeners on `#controls`,
//! nodes reconciled by `data-id` so focus survives re-syncs.
use crate::{Queue, Reg};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn host() -> Option<web_sys::Element> {
    web_sys::window()?.document()?.get_element_by_id("controls")
}

fn data_id(e: &web_sys::Event) -> Option<String> {
    e.target()?
        .dyn_into::<web_sys::Element>()
        .ok()?
        .get_attribute("data-id")
}

pub fn install(q: Queue, ctx: egui::Context) {
    let Some(h) = host() else { return };
    let (q1, c1) = (q.clone(), ctx.clone());
    let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        if let Some(id) = data_id(&e) {
            q1.lock().unwrap().push(format!("act:{id}"));
            c1.request_repaint();
        }
    });
    let on_focus = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        if let Some(id) = data_id(&e) {
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
    let (Some(h), Some(doc)) = (host(), web_sys::window().and_then(|w| w.document())) else {
        return;
    };
    let mut want: Vec<&str> = vec![];
    for r in regs {
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
                let _ = h.append_child(&e);
                e
            }
        };
        let _ = el.set_attribute("aria-label", &r.label);
        match r.pressed {
            Some(p) => {
                let _ = el.set_attribute("aria-pressed", if p { "true" } else { "false" });
            }
            None => {
                let _ = el.remove_attribute("aria-pressed");
            }
        }
        el.set_text_content(Some(&r.label));
        // keep DOM order == sorted order
        let _ = h.append_child(&el);
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
