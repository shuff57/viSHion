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
use crate::twin::{look, Node, Part, Reg, DIALOG};
use std::collections::HashMap;
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
        // a click on the text of a DOM transcript line moves the reading underline (js: `.ln` click), unless
        // the person has just selected some of it
        if e.type_() == "click" && !has_selection() {
            return el.closest("p[data-line]").ok().flatten().and_then(|p| p.get_attribute("data-id"));
        }
        return None;
    }
    el.get_attribute("data-id")
}

fn has_selection() -> bool {
    web_sys::window().and_then(|w| w.get_selection().ok().flatten()).is_some_and(|s| !s.is_collapsed())
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
                if mods.is_empty() && prefix == "act" && id.starts_with("line-") && e.type_() == "click" {
                    // A click on the text of a line underlines it, but the first click of a double or triple click
                    // (which selects a word or the line to copy it) must not: it is acted on once the multi-click
                    // window has passed and no selection came of it.
                    let (q, ctx, msg) = (q.clone(), ctx.clone(), format!("{prefix}:{id}"));
                    let f = Closure::once_into_js(move || {
                        if !has_selection() {
                            q.lock().unwrap().push(msg);
                            ctx.request_repaint();
                        }
                    });
                    if let Some(w) = web_sys::window() {
                        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(f.unchecked_ref(), 280);
                    }
                    return;
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
    forward_scroll(&h);
    // js `R.hoverSync`: the pointer over a transcript line lights its box on the page. The DOM text takes the pointer
    // (the canvas under it never sees it), so the page says which line it is over.
    {
        let last = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        for kind in ["mouseover", "mouseout"] {
            let (q, ctx, last) = (q.clone(), ctx.clone(), last.clone());
            let line_of = |t: Option<web_sys::EventTarget>| -> Option<String> {
                t.and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .and_then(|el| el.closest("p[data-line]").ok().flatten())
                    .and_then(|p| p.get_attribute("data-id"))
            };
            let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
                let id = if e.type_() == "mouseover" { line_of(e.target()) } else { line_of(e.related_target()) };
                let id = id.unwrap_or_default();
                if *last.borrow() != id {
                    *last.borrow_mut() = id.clone();
                    q.lock().unwrap().push(format!("hover:{id}"));
                    ctx.request_repaint();
                }
            });
            let _ = h.add_event_listener_with_callback(kind, cb.as_ref().unchecked_ref());
            cb.forget();
        }
    }
    install_transcript(&h, q.clone(), ctx.clone());
    observe_sizes(ctx.clone());
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
    // a mouse press on the twin (a click on a word of the DOM transcript) ends "the keyboard is in use"
    {
        let (q, ctx) = (q.clone(), ctx.clone());
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| {
            q.lock().unwrap().push("kbd:0".to_string());
            ctx.request_repaint();
        });
        let _ = h.add_event_listener_with_callback("pointerdown", cb.as_ref().unchecked_ref());
        cb.forget();
    }
    listen("focusin", "focus");
    // a formula that scrolls sideways takes keyboard focus: the screen brings its line into view (the twin is
    // `overflow: clip`, so the browser cannot)
    {
        let (q, ctx) = (q.clone(), ctx.clone());
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let line = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .filter(|el| el.class_list().contains("tx-math"))
                .and_then(|el| el.closest("p[data-line]").ok().flatten())
                .and_then(|p| p.get_attribute("data-id"));
            if let Some(id) = line {
                q.lock().unwrap().push(format!("reveal:{id}"));
                ctx.request_repaint();
            }
        });
        let _ = h.add_event_listener_with_callback("focusin", cb.as_ref().unchecked_ref());
        cb.forget();
    }
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

    // a new key press or click is a new action: what it says is not added to what the last one said
    if let Some(doc) = document() {
        for kind in ["keydown", "pointerdown"] {
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(|_: web_sys::Event| {
                SPOKEN.with(|s| s.borrow_mut().iter_mut().for_each(|slot| slot.0 = 0.0));
            });
            let _ = doc.add_event_listener_with_callback_and_bool(kind, cb.as_ref().unchecked_ref(), true);
            cb.forget();
        }
    }

    // Keys the screen handles by where focus is (js: handlers on #tx-text, .scan-pages, the stage, .tray).
    // Escape always goes through (the tray and dialogs close on it); the others only inside their region.
    let (q4, c4) = (q.clone(), ctx.clone());
    let on_region_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        let key = e.key();
        let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT") && key != "Escape" {
            return;
        }
        // Ctrl+Z (Cmd+Z) takes back the last word decision, from anywhere on the page but a text box
        if (e.ctrl_key() || e.meta_key()) && !e.shift_key() && !e.alt_key() && key.eq_ignore_ascii_case("z") {
            e.prevent_default();
            q4.lock().unwrap().push("key:undo".to_string());
            c4.request_repaint();
            return;
        }
        // The keys that scroll a pane scroll the transcript, as they do the JS app's `#tx-pane` (the canvas owns the
        // position, so the page is asked: `scrollby:`). Space on a button is the button's; a formula that scrolls
        // sideways keeps what it does with them.
        if scope_of(&el) == Some("transcript") && !(e.ctrl_key() || e.meta_key() || e.alt_key()) && !el.class_list().contains("tx-math") {
            let page = web_sys::window().and_then(|w| w.inner_height().ok()).and_then(|h| h.as_f64()).unwrap_or(600.0) * 0.8;
            let on_button = el.tag_name() == "BUTTON";
            let d = match key.as_str() {
                "PageDown" => Some(page),
                "PageUp" => Some(-page),
                " " if !on_button => Some(if e.shift_key() { -page } else { page }),
                "End" => Some(1.0e6),
                "Home" => Some(-1.0e6),
                _ => None,
            };
            if let Some(d) = d {
                e.prevent_default();
                q4.lock().unwrap().push(format!("scrollby:{}", d.round() as i64));
                c4.request_repaint();
                return;
            }
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

/// The DOM transcript is above the canvas, so a wheel turn or a finger drag over it would never reach the
/// page's scroll area (egui). They are handed to the canvas as the wheel events it listens for.
fn forward_scroll(h: &web_sys::Element) {
    let Some(canvas) = document().and_then(|d| d.get_element_by_id("c")) else { return };
    let send = {
        let canvas = canvas.clone();
        move |dx: f64, dy: f64, mode: u32, x: i32, y: i32, ctrl: bool| {
            let init = web_sys::WheelEventInit::new();
            init.set_delta_x(dx);
            init.set_delta_y(dy);
            init.set_delta_mode(mode);
            init.set_client_x(x);
            init.set_client_y(y);
            init.set_ctrl_key(ctrl);
            init.set_bubbles(true);
            init.set_cancelable(true);
            if let Ok(ev) = web_sys::WheelEvent::new_with_event_init_dict("wheel", &init) {
                let _ = canvas.dispatch_event(&ev);
            }
        }
    };
    let in_text = |e: &web_sys::Event| e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|el| el.closest(".tx-clip").ok().flatten().is_some());
    let s1 = send.clone();
    let on_wheel = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
        if !in_text(&e) || e.ctrl_key() {
            // ctrl+wheel (and a trackpad pinch, which Chrome sends as ctrl+wheel) is the browser's page zoom
            return;
        }
        // a formula wider than its column scrolls sideways by itself
        if e.delta_x().abs() > e.delta_y().abs() && scrolls_sideways(&e) {
            return;
        }
        e.prevent_default();
        s1(e.delta_x(), e.delta_y(), e.delta_mode(), e.client_x() as i32, e.client_y() as i32, e.ctrl_key());
    });
    let _ = h.add_event_listener_with_callback("wheel", on_wheel.as_ref().unchecked_ref());
    on_wheel.forget();
    let last = std::rc::Rc::new(std::cell::Cell::new(None::<(i32, i32)>));
    let l1 = last.clone();
    let on_start = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |e: web_sys::TouchEvent| {
        l1.set(if e.touches().length() == 1 { e.touches().get(0).map(|t| (t.client_x(), t.client_y())) } else { None });
    });
    let _ = h.add_event_listener_with_callback("touchstart", on_start.as_ref().unchecked_ref());
    on_start.forget();
    let on_move = Closure::<dyn FnMut(web_sys::TouchEvent)>::new(move |e: web_sys::TouchEvent| {
        if e.touches().length() != 1 || !in_text(&e) {
            return;
        }
        let Some(t) = e.touches().get(0) else { return };
        if let Some((px, py)) = last.get() {
            if (px - t.client_x()).abs() > (py - t.client_y()).abs() && scrolls_sideways(&e) {
                last.set(Some((t.client_x(), t.client_y())));
                return;
            }
            if e.cancelable() {
                e.prevent_default();
            }
            send((px - t.client_x()) as f64, (py - t.client_y()) as f64, 0, t.client_x(), t.client_y(), false);
        }
        last.set(Some((t.client_x(), t.client_y())));
    });
    let _ = h.add_event_listener_with_callback("touchmove", on_move.as_ref().unchecked_ref());
    on_move.forget();
}

/// How far the lines of a `.tx-clip` sit below its top edge at rest: the box scrolls (`overflow: hidden`, so the
/// browser can bring a find-in-page hit or a focused control into view, which `overflow: clip` and `hidden` never do), but a
/// box cannot scroll above its content. Resting at this offset leaves room to scroll up as well; every scroll
/// the browser makes is handed to the canvas (which owns the position) and the box goes back to rest.
const TX_BIAS: i32 = 100_000;
const BIAS_F: f64 = TX_BIAS as f64;

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

/// Find-in-page, focus and selection on the DOM transcript: the scroll the browser makes of the box is the
/// canvas's to make (`scrollby:`), a copy gives formulas as TeX, and a drag-selection held at the foot or head of
/// the box scrolls it.
fn install_transcript(h: &web_sys::Element, q: Queue, ctx: egui::Context) {
    let focused_at = std::rc::Rc::new(std::cell::Cell::new(f64::NEG_INFINITY));
    // a control in the box that took focus: the screen reveals it by itself (`reveal:`), so the scroll the browser
    // made for the focus is not forwarded as well
    {
        let f = focused_at.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let inside = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()).is_some_and(|el| el.closest(".tx-clip").ok().flatten().is_some());
            if inside {
                f.set(now_ms());
            }
        });
        let _ = h.add_event_listener_with_callback("focusin", cb.as_ref().unchecked_ref());
        cb.forget();
    }
    // `scroll` does not bubble: seen in the capture phase
    {
        let (q, ctx, f) = (q.clone(), ctx.clone(), focused_at.clone());
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
            if !el.class_list().contains("tx-clip") {
                return;
            }
            let d = (el.scroll_top() - BIAS_F).round() as i32;
            if el.scroll_left() != 0.0 {
                el.set_scroll_left(0.0);
            }
            if d == 0 {
                return;
            }
            el.set_scroll_top(BIAS_F);
            if now_ms() - f.replace(f64::NEG_INFINITY) < 400.0 {
                return;
            }
            q.lock().unwrap().push(format!("scrollby:{d}"));
            ctx.request_repaint();
        });
        let _ = h.add_event_listener_with_callback_and_bool("scroll", cb.as_ref().unchecked_ref(), true);
        cb.forget();
    }
    // The selection moved into view-less text (find-in-page: `window.find` selects its hit and the browser does not
    // scroll a box inside a page that cannot scroll): the screen scrolls to it, unless a drag is making the
    // selection (it is the person's pointer that decides what is in view then)
    if let Some(doc) = document() {
        let down = std::rc::Rc::new(std::cell::Cell::new(false));
        for (kind, v) in [("mousedown", true), ("mouseup", false), ("dragend", false)] {
            let d = down.clone();
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| d.set(v));
            let _ = doc.add_event_listener_with_callback_and_bool(kind, cb.as_ref().unchecked_ref(), true);
            cb.forget();
        }
        let (q, ctx) = (q.clone(), ctx.clone());
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            if down.get() {
                return;
            }
            if let Some(d) = selection_offscreen() {
                q.lock().unwrap().push(format!("scrollby:{d}"));
                ctx.request_repaint();
            }
        });
        let _ = doc.add_event_listener_with_callback("selectionchange", cb.as_ref().unchecked_ref());
        cb.forget();
    }
    // Ctrl+C / the context menu's Copy: the lines apart, a formula as the TeX it is written in
    if let Some(doc) = document() {
        let cb = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(move |e: web_sys::ClipboardEvent| {
            if let Some(text) = selected_text() {
                if let Some(dt) = e.clipboard_data() {
                    if dt.set_data("text/plain", &text).is_ok() {
                        e.prevent_default();
                    }
                }
            }
        });
        let _ = doc.add_event_listener_with_callback("copy", cb.as_ref().unchecked_ref());
        cb.forget();
    }
}

/// How far the transcript has to scroll for the end of a selection made inside it to be in the box (0 when it is).
fn selection_offscreen() -> Option<i32> {
    let doc = document()?;
    let sel = web_sys::window()?.get_selection().ok().flatten()?;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let (a, f) = (sel.anchor_node()?, sel.focus_node()?);
    let clip_of = |n: &web_sys::Node| -> Option<web_sys::Element> {
        let e = if n.node_type() == web_sys::Node::ELEMENT_NODE { n.clone().dyn_into::<web_sys::Element>().ok()? } else { n.parent_element()? };
        e.closest(".tx-clip").ok().flatten()
    };
    let clip = clip_of(&a)?;
    clip_of(&f)?;
    let bounds = clip.get_bounding_client_rect();
    // the end the selection grew to (a collapsed range has a box when it is in a text node), else the whole range
    let mut r = None;
    if let Ok(c) = doc.create_range() {
        if c.set_start(&f, sel.focus_offset()).is_ok() {
            c.collapse_with_to_start(true);
            let b = c.get_bounding_client_rect();
            if b.height() > 0.0 {
                r = Some(b);
            }
        }
    }
    let r = r.or_else(|| sel.get_range_at(0).ok().map(|g| g.get_bounding_client_rect()))?;
    if r.height() <= 0.0 || (r.top() >= bounds.top() && r.bottom() <= bounds.bottom()) {
        return None;
    }
    let (rc, bc) = ((r.top() + r.bottom()) / 2.0, (bounds.top() + bounds.bottom()) / 2.0);
    Some((rc - bc).round() as i32)
}

/// What a selection that starts or ends in the DOM transcript copies: its text, a formula as its TeX (`$...$`,
/// `$$...$$` for display) instead of the typeset symbols.
fn selected_text() -> Option<String> {
    let doc = document()?;
    let sel = web_sys::window()?.get_selection().ok().flatten()?;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let inside = |n: Option<web_sys::Node>| {
        n.and_then(|n| if n.node_type() == web_sys::Node::ELEMENT_NODE { n.dyn_into::<web_sys::Element>().ok() } else { n.parent_element() })
            .is_some_and(|e| e.closest(".tx-clip").ok().flatten().is_some())
    };
    if !(inside(sel.anchor_node()) || inside(sel.focus_node())) {
        return None;
    }
    let frag = sel.get_range_at(0).ok()?.clone_contents().ok()?;
    let list = frag.query_selector_all(".tx-math").ok()?;
    for i in 0..list.length() {
        let Some(m) = list.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        let tex = m.get_attribute("data-tex").unwrap_or_default();
        let d = if m.class_list().contains("disp") { "$$" } else { "$" };
        let t = doc.create_text_node(&format!("{d}{tex}{d}"));
        if let Some(parent) = m.parent_node() {
            let _ = parent.replace_child(&t, &m);
        }
    }
    frag.text_content()
}

/// The event is over a formula that is wider than its box (it scrolls sideways).
fn scrolls_sideways(e: &web_sys::Event) -> bool {
    e.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.closest(".tx-math").ok().flatten())
        .is_some_and(|m| m.scroll_width() > m.client_width())
}

thread_local! {
    /// Watches every DOM transcript line: its box changes when the page's CSS does (a user stylesheet, a
    /// font arriving, text spacing), and egui is asked for a frame then.
    static SIZES: std::cell::RefCell<Option<web_sys::ResizeObserver>> = const { std::cell::RefCell::new(None) };
    /// The context, for what the page reports on its own (KaTeX arriving).
    static CTX: std::cell::RefCell<Option<egui::Context>> = const { std::cell::RefCell::new(None) };
}

fn observe_sizes(ctx: egui::Context) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
    let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::ResizeObserver)>::new(move |_: js_sys::Array, _: web_sys::ResizeObserver| {
        ctx.request_repaint();
    });
    if let Ok(ro) = web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
        SIZES.with(|s| *s.borrow_mut() = Some(ro));
    }
    cb.forget();
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
    /// The height is the content's (a DOM transcript line): only its position and width are set.
    auto_h: bool,
    /// Inline CSS properties besides the position (removed when `None`).
    props: Vec<(&'static str, Option<String>)>,
    /// A formula: (TeX, display), typeset by KaTeX once it has arrived.
    math: Option<(String, bool)>,
}

impl Spec {
    fn new(key: &str, tag: &'static str) -> Spec {
        Spec { key: key.into(), tag, attrs: vec![], text: None, value: None, children: None, rect: None, auto_h: false, props: vec![], math: None }
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
        let mut s = Spec::new(&r.id, "input").attr("type", if r.id == "st-key" { "password" } else { "text" }).attr("aria-label", r.label.clone()).attr("autocomplete", "off");
        s.value = r.value.clone();
        // Enter in the twin textbox presses the control next to it
        if r.id == "typed-word" {
            s = s.attr("data-enter", "use-typed");
        }
        s
    } else {
        let mut s = Spec::new(&r.id, "button").attr("type", "button").text(&r.label);
        if matches!(r.role, "checkbox" | "switch" | "radio") {
            s = s.attr("role", r.role);
        }
        s
    };
    let state = r.pressed.map(|p| if p { "true".to_string() } else { "false".to_string() });
    if matches!(r.role, "checkbox" | "switch" | "radio") {
        s.attrs.push(("aria-checked", state));
    } else {
        s.attrs.push(("aria-pressed", state));
    }
    // a control with a reason stays in the Tab order (aria-disabled), so a keyboard user can find out why
    let why = if r.disabled { a11y::disabled_reason(&r.id) } else { None };
    s.attrs.push(("disabled", if r.disabled && why.is_none() { Some(String::new()) } else { None }));
    s.attrs.push(("aria-disabled", if why.is_some() { Some("true".to_string()) } else { None }));
    s.attrs.push(("aria-description", why.map(String::from)));
    if r.size.0 > 0 && r.size.1 > 0 {
        s.rect = Some((r.at.0, r.at.1, r.size.0 as i32, r.size.1 as i32));
    }
    s
}

/// The CSS classes of a run of transcript text.
fn look_class(f: u16) -> String {
    let mut c = vec![];
    for (bit, name) in [(look::BOLD, "b"), (look::ITALIC, "i"), (look::CODE, "c"), (look::LEAD, "lead")] {
        if f & bit != 0 {
            c.push(name);
        }
    }
    c.join(" ")
}

/// A transcript line as real, visible DOM text (`#controls .tx-clip p.tx`), styled by the page's CSS the way
/// `.tx-text .ln` is in the JS app: the browser wraps it, a user stylesheet can restyle it, find-in-page and
/// selection work on it, and its flagged words are inline buttons.
fn dom_para(id: &str, parts: &[Part], flags: &[u16], reading: bool, margin: bool, at: (i32, i32), size: (u16, u16)) -> Spec {
    let plain = flags.iter().any(|f| f & look::PLAIN != 0);
    let display = parts.len() == 1 && matches!(parts[0], Part::Math(_)) && flags[0] & look::DISPLAY != 0;
    let mut class = String::from("tx");
    for (on, name) in [(plain, "plain"), (margin, "margin"), (display, "display")] {
        if on {
            class.push(' ');
            class.push_str(name);
        }
    }
    let mut p = Spec::new(id, "p").attr("class", class);
    if id.starts_with("line-") {
        p = p.attr("data-line", "1");
    }
    p.rect = rect_of(at, size);
    p.auto_h = true;
    // what egui made room for: the page compares it with what the browser laid out
    p.attrs.push(("data-alloc-h", Some(size.1.to_string())));
    p.attrs.push(("data-reading", if reading { Some("1".to_string()) } else { None }));
    let mut kids = vec![];
    for (i, part) in parts.iter().enumerate() {
        let f = flags.get(i).copied().unwrap_or(0);
        kids.push(match part {
            Part::Text(t) => Spec::new(&format!("{id}#t{i}"), "span").attr("class", look_class(f)).text(t),
            Part::Del(t) => Spec::new(&format!("{id}#d{i}"), "del").attr("class", look_class(f)).text(t),
            Part::Math(t) => {
                let disp = f & look::DISPLAY != 0;
                let mut m = Spec::new(&format!("{id}#m{i}"), "span")
                    .attr("class", if disp { "tx-math disp" } else { "tx-math" })
                    // what it says, in words: the name of the formula (a math node whose children are its
                    // typeset MathML)
                    .attr("role", "math")
                    .attr("aria-label", a11y::tex_speech(t))
                    .attr("data-tex", t.clone());
                m.math = Some((t.clone(), disp));
                m
            }
            Part::Control(r) => {
                let mut class = String::from("w");
                for (bit, name) in [(look::WARN, "suggest"), (look::BAD, "illegible"), (look::CHECKED, "checked"), (look::SEL, "sel"), (look::BOLD, "b"), (look::ITALIC, "i")] {
                    if f & bit != 0 {
                        class.push(' ');
                        class.push_str(name);
                    }
                }
                let mut b = Spec::new(&r.id, "button").attr("type", "button").attr("class", class).attr("aria-label", r.label.clone());
                b.text = Some(r.shown.clone().unwrap_or_else(|| r.label.clone()));
                b.attrs.push(("disabled", if r.disabled { Some(String::new()) } else { None }));
                b
            }
        });
    }
    // The end of a line, for the text a selection or a copy gives: the lines are separate, absolutely placed
    // boxes, which the browser joins without a break ("samples,about").
    kids.push(Spec::new(&format!("{id}#nl"), "span").attr("class", "tx-nl").attr("aria-hidden", "true").text("\n"));
    p.children = Some(kids);
    p
}

type Chrome = (String, (i32, i32, u16, u16));

thread_local! {
    /// The text of the pane that is DOM text on the web (the student's name, the counts, the legend, the pill,
    /// the stat and hint, streamed text), by twin node id: its CSS class and where it may be seen. Set by the
    /// screen each frame (`set_chrome`).
    static CHROME: std::cell::RefCell<HashMap<String, Chrome>> = std::cell::RefCell::new(HashMap::new());
}

pub fn set_chrome(m: &HashMap<String, Chrome>) {
    CHROME.with(|c| *c.borrow_mut() = m.clone());
}

fn chrome_of(id: &str) -> Option<Chrome> {
    CHROME.with(|c| c.borrow().get(id).cloned())
}

/// A heading, count or note of the pane as visible DOM text (`#controls .txc`): the person's stylesheet reaches
/// it, it is found and selected like the rest of the page, and the page measures what it takes.
fn chrome_spec(n: &Node, class: &str) -> Spec {
    let (id, text, at, size, h1) = match n {
        Node::Heading { id, text, at, size, level, .. } => (id, text, *at, *size, *level == 1),
        Node::Status { id, text, at, size, .. } | Node::Text { id, text, at, size, .. } => (id, text, *at, *size, false),
        _ => unreachable!(),
    };
    let mut s = Spec::new(id, if h1 { "h1" } else { "p" }).attr("class", format!("txc {class}"));
    if h1 {
        // the screen's h1 takes focus when a different screen replaces it (see `restore_focus`)
        s = s.attr("tabindex", "-1");
    }
    s.rect = rect_of(at, size);
    s.auto_h = true;
    s.attrs.push(("data-alloc-h", Some(size.1.to_string())));
    if id == "legend" {
        // what the three swatches mean: a sample of the look and its name, as the JS app's `.legend`
        let kids = [("warn", "Spelling to check"), ("bad", "Couldn’t read"), ("ok", "You checked it")]
            .iter()
            .enumerate()
            .map(|(i, (k, label))| {
                Spec::new(&format!("legend#{i}"), "span").attr("class", "lg").kids(vec![
                    Spec::new(&format!("legend#{i}k"), "span").attr("class", format!("k {k}")).attr("aria-hidden", "true").text("word"),
                    Spec::new(&format!("legend#{i}t"), "span").text(label),
                ])
            })
            .collect();
        s.children = Some(kids);
    } else {
        s.text = Some(text.clone());
    }
    s
}

/// A node that is DOM text of the pane, with where it may be seen.
fn chrome_item(n: &Node) -> Option<(Spec, (i32, i32, i32, i32))> {
    if !matches!(n, Node::Heading { .. } | Node::Status { .. } | Node::Text { .. }) {
        return None;
    }
    let (class, v) = chrome_of(n.id())?;
    Some((chrome_spec(n, &class), (v.0, v.1, v.2 as i32, v.3 as i32)))
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
        Node::Para { id, parts, reading, at, size, flags, margin, .. } if !flags.is_empty() => dom_para(id, parts, flags, *reading, *margin, *at, *size),
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
fn region_specs(nodes: &[&Node], holes: &[(i32, i32, i32, i32)]) -> Vec<Spec> {
    let mut out: Vec<Spec> = vec![];
    let mut open: Option<(&'static str, Spec)> = None;
    // the DOM transcript lines of this region, in one box that is the part of the screen they may be seen in
    let mut tx: Option<Spec> = None;
    let flush = |open: &mut Option<(&'static str, Spec)>, out: &mut Vec<Spec>| {
        if let Some((_, g)) = open.take() {
            out.push(g);
        }
    };
    for n in nodes {
        // DOM text: the lines of the transcript and the text of the pane around them, in a box each run of them
        // shares that is the part of the screen they may be seen in
        let dom_item = match n {
            Node::Para { flags, vis, .. } if !flags.is_empty() => Some((spec_of(n), (vis.0, vis.1, vis.2 as i32, vis.3 as i32))),
            _ => chrome_item(n),
        };
        if let Some((item, v)) = dom_item {
            flush(&mut open, &mut out);
            let c = tx.get_or_insert_with(|| {
                // named by what it starts with, so it is the same element from frame to frame. A run that holds
                // lines is a box the browser can scroll (find-in-page, selection, focus); one that holds only the
                // pane's own text stays where it is (a scroller with nothing to focus in it is not reachable by
                // keyboard, WCAG 2.1.1)
                if matches!(n, Node::Para { .. }) {
                    Spec::new(&format!("tx-clip:{}", item.key), "div").attr("class", "tx-clip").kids(vec![])
                } else {
                    Spec::new(&format!("tx-still:{}", item.key), "div").attr("class", "tx-clip tx-still").kids(vec![])
                }
            });
            // a run that began with the pane's text and reached the lines is one that scrolls
            if matches!(n, Node::Para { .. }) && c.key.starts_with("tx-still:") {
                c.key = c.key.replacen("tx-still:", "tx-clip:", 1);
                c.attrs.retain(|(k, _)| *k != "class");
                c.attrs.push(("class", Some("tx-clip".to_string())));
            }
            c.rect = Some(match c.rect {
                None => v,
                Some((x, y, w, h)) => {
                    let (l, t, r, b) = (x.min(v.0), y.min(v.1), (x + w).max(v.0 + v.2), (y + h).max(v.1 + v.3));
                    (l, t, r - l, b - t)
                }
            });
            if let Some(k) = c.children.as_mut() {
                // (the lines carry their own end-of-line break; these are separate boxes as well, which a selection
                // would run together)
                let nl = (!matches!(n, Node::Para { .. })).then(|| Spec::new(&format!("{}#nl", item.key), "span").attr("class", "tx-nl").attr("aria-hidden", "true").text("\n"));
                k.push(item);
                k.extend(nl);
            }
            continue;
        }
        if let Some(c) = tx.take() {
            out.push(clip_holes(c, holes));
        }
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
    if let Some(c) = tx.take() {
        out.push(clip_holes(c, holes));
    }
    out
}

/// What is drawn over the transcript by egui (the toast, a tray that rides over the page) is cut out of the
/// box the DOM lines are seen in: a polygon with holes (even-odd), in the box's own coordinates.
fn clip_holes(mut c: Spec, holes: &[(i32, i32, i32, i32)]) -> Spec {
    let Some((cx, cy, cw, ch)) = c.rect else { return c };
    let mut hs: Vec<(i32, i32, i32, i32)> = vec![];
    for &(x, y, w, h) in holes {
        // (left, top, right, bottom) inside the box
        let (l, t, r, b) = ((x - cx).max(0), (y - cy).max(0), (x + w - cx).min(cw), (y + h - cy).min(ch));
        if r > l && b > t {
            hs.push((l, t, r, b));
        }
    }
    // holes that touch are one hole (even-odd would fill where two overlap)
    'again: loop {
        for i in 0..hs.len() {
            for j in i + 1..hs.len() {
                let (a, b) = (hs[i], hs[j]);
                if a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3 {
                    hs[i] = (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3));
                    hs.remove(j);
                    continue 'again;
                }
            }
        }
        break;
    }
    let v = if hs.is_empty() {
        None
    } else {
        let mut pts = vec![format!("0px 0px, {cw}px 0px, {cw}px {ch}px, 0px {ch}px, 0px 0px")];
        for (l, t, r, b) in hs {
            pts.push(format!("{l}px {t}px, {r}px {t}px, {r}px {b}px, {l}px {b}px, {l}px {t}px, 0px 0px"));
        }
        Some(format!("polygon(evenodd, {})", pts.join(", ")))
    };
    c.props.push(("clip-path", v));
    // content below the box's own height, so that it can rest at `TX_BIAS` when the lines are short
    let mut pad = Spec::new("tx-pad", "div").attr("aria-hidden", "true");
    pad.rect = Some((cx, cy + ch, 1, 1));
    if let Some(k) = c.children.as_mut() {
        k.push(pad);
    }
    c
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
    let holes = overlays(nodes);
    for (region, list) in &regions {
        let kids = region_specs(list, &holes);
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

/// Rectangles egui draws over the page that DOM text must not cover: the toast, and the word panel (a tray
/// that rides over the transcript on a narrow screen).
fn overlays(nodes: &[Node]) -> Vec<(i32, i32, i32, i32)> {
    let sheet = SHEET.with(|s| s.take());
    let mut out = vec![];
    let mut tray: Option<(i32, i32, i32, i32)> = None;
    for n in nodes {
        let (id, region, at, size) = match n {
            Node::Status { id, region, at, size, .. } => (id.as_str(), *region, *at, *size),
            Node::Control(c) => (c.id.as_str(), c.region, c.at, c.size),
            _ => continue,
        };
        if size.0 == 0 || size.1 == 0 {
            continue;
        }
        let r = (at.0, at.1, size.0 as i32, size.1 as i32);
        if id == "toast" {
            out.push(r);
        } else if region == crate::twin::TRAY {
            tray = Some(match tray {
                None => r,
                Some((x, y, w, h)) => {
                    let (l, t, rr, b) = (x.min(r.0), y.min(r.1), (x + w).max(r.0 + r.2), (y + h).max(r.1 + r.3));
                    (l, t, rr - l, b - t)
                }
            });
        }
    }
    // the whole sheet when the screen says where it is (it has padding and a background the controls in it
    // do not cover), else what the controls in it span
    out.extend(sheet.or(tray));
    out
}

thread_local! {
    /// Where the word panel's sheet is this frame (x, y, w, h), set by the review screen when it rides over the page.
    static SHEET: std::cell::Cell<Option<(i32, i32, i32, i32)>> = const { std::cell::Cell::new(None) };
}

pub fn set_sheet(r: Option<(i32, i32, i32, i32)>) {
    SHEET.with(|s| s.set(r));
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
fn set_style(el: &web_sys::Element, rect: Option<(i32, i32, i32, i32)>, origin: (i32, i32), auto_h: bool) {
    let Some(style) = el.dyn_ref::<web_sys::HtmlElement>().map(|h| h.style()) else { return };
    match rect {
        Some((x, y, w, h)) => {
            let (x, y) = (x - origin.0, y - origin.1);
            let mut want = vec![("position", "absolute".to_string()), ("left", format!("{x}px")), ("top", format!("{y}px")), ("width", format!("{w}px")), ("margin", "0".to_string())];
            if auto_h {
                if style.get_property_value("height").map(|v| !v.is_empty()).unwrap_or(false) {
                    let _ = style.remove_property("height");
                }
            } else {
                want.push(("height", format!("{h}px")));
            }
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
        set_style(&el, sp.rect, origin, sp.auto_h);
        for (k, v) in &sp.props {
            if let Some(style) = el.dyn_ref::<web_sys::HtmlElement>().map(|h| h.style()) {
                match v {
                    Some(v) => {
                        if style.get_property_value(k).ok().as_deref() != Some(v.as_str()) {
                            let _ = style.set_property(k, v);
                        }
                    }
                    None => {
                        if style.get_property_value(k).map(|v| !v.is_empty()).unwrap_or(false) {
                            let _ = style.remove_property(k);
                        }
                    }
                }
            }
        }
        if sp.auto_h {
            SIZES.with(|s| {
                if let Some(ro) = s.borrow().as_ref() {
                    ro.observe(&el);
                }
            });
        }
        if let Some((tex, disp)) = &sp.math {
            render_math(&el, tex, *disp);
        }
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
            let mut o = sp.rect.map_or(origin, |r| (r.0, r.1));
            let clip = sp.key.starts_with("tx-clip:");
            if clip {
                o.1 -= TX_BIAS;
            }
            reconcile(doc, &el, kids, o);
            if clip && (el.scroll_top() - BIAS_F).abs() > 0.5 {
                el.set_scroll_top(BIAS_F);
            }
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

// ---- the DOM transcript: measuring, formulas, colours

/// What the browser laid out for the transcript lines: heights by line id, and the rectangle of each flagged
/// word (x, y, w, h) relative to its line's top left corner, in egui points.
#[derive(Default, Clone)]
struct Measure {
    h: HashMap<String, f32>,
    chips: HashMap<String, [f32; 4]>,
    /// a formula changed between inline and a scrolling box while measuring: the numbers are stale
    relaid: bool,
}

thread_local! {
    static MEASURED: std::cell::RefCell<Measure> = std::cell::RefCell::new(Measure::default());
}

/// The last measurement (line heights, word rectangles).
pub fn measured() -> (HashMap<String, f32>, HashMap<String, [f32; 4]>) {
    MEASURED.with(|m| {
        let m = m.borrow();
        (m.h.clone(), m.chips.clone())
    })
}

/// Measure every transcript line. True when something is not where the last measurement had it, so the
/// screen is drawn again with the new numbers.
pub fn measure() -> bool {
    let Some(h) = host() else { return false };
    let Ok(list) = h.query_selector_all("p.tx, .txc") else { return false };
    if list.length() == 0 && MEASURED.with(|m| m.borrow().h.is_empty()) {
        return false;
    }
    // the twin may be scaled to the canvas (`place`): its boxes are then k times their layout size
    let k = match h.dyn_ref::<web_sys::HtmlElement>() {
        Some(e) if e.offset_width() > 0 => (e.get_bounding_client_rect().width() / e.offset_width() as f64).max(0.01),
        _ => 1.0,
    };
    let mut now = Measure::default();
    for i in 0..list.length() {
        let Some(p) = list.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
        let Some(id) = p.get_attribute("data-id") else { continue };
        let r = p.get_bounding_client_rect();
        // a line with words has room under it for the 44px reach of a word on its last row (index.html,
        // `p.tx:has(.w)`); the screen is spaced by the line without it, so that room lies over the next line
        let reach = web_sys::window()
            .and_then(|w| w.get_computed_style(&p).ok().flatten())
            .and_then(|cs| cs.get_property_value("padding-bottom").ok())
            .and_then(|v| v.trim_end_matches("px").parse::<f64>().ok())
            .map_or(0.0, |pb| (pb - 1.0).max(0.0));
        // margins of the line (a user stylesheet's paragraph spacing, WCAG 1.4.12) are room the screen makes: the
        // lines are placed by the screen, so the browser has no neighbour to push away
        let (mt, mb) = web_sys::window()
            .and_then(|w| w.get_computed_style(&p).ok().flatten())
            .map(|cs| {
                let px = |n: &str| cs.get_property_value(n).ok().and_then(|v| v.trim_end_matches("px").parse::<f64>().ok()).unwrap_or(0.0);
                (px("margin-top"), px("margin-bottom"))
            })
            .unwrap_or((0.0, 0.0));
        now.h.insert(id, ((r.height() / k + mt + mb) - reach).max(0.0) as f32);
        // a formula wider than the line is a box of the column's width that scrolls (WCAG 1.4.10: it is not
        // cut off); one that fits is left as inline math is laid out
        if let Ok(ms) = p.query_selector_all(".tx-math") {
            let avail = p.client_width() as f64;
            // inline math is as wide as its content however narrow the column (its own box says the column's
            // width), so the line's overflow is what tells; once it is a box, it fills the column while the
            // content needs all of it
            let overflow = p.scroll_width() as f64 > avail + 0.5;
            for j in 0..ms.length() {
                let Some(m) = ms.item(j).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
                if m.query_selector("math").ok().flatten().is_none() {
                    continue;
                }
                let has = m.has_attribute("data-wide");
                let wide = if has { avail > 0.0 && m.client_width() as f64 >= avail - 1.0 } else { overflow };
                if wide != has {
                    let _ = if wide { m.set_attribute("data-wide", "") } else { m.remove_attribute("data-wide") };
                    now.relaid = true;
                }
                // the TeX laid over the formula (what a find hit selects) is squeezed to the formula's width, so a
                // hit anywhere in it is a box on the formula, not off its end
                if let (Ok(Some(hit)), Some(style)) = (m.query_selector(".tx-hit"), m.query_selector(".tx-hit").ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()).map(|e| e.style())) {
                    let (nat, w) = (hit.scroll_width() as f64, hit.client_width() as f64);
                    let t = if nat > w && w > 0.0 { format!("scaleX({:.3})", w / nat) } else { String::new() };
                    if style.get_property_value("transform").ok().as_deref() != Some(t.as_str()) {
                        let _ = style.set_property("transform-origin", "0 0");
                        let _ = style.set_property("transform", &t);
                    }
                }
                // a box that scrolls is reached with the keyboard (WCAG 2.1.1) and says what it is
                let scrolls = m.scroll_width() > m.client_width() + 1;
                if scrolls != m.has_attribute("tabindex") {
                    let _ = if scrolls { m.set_attribute("tabindex", "0") } else { m.remove_attribute("tabindex") };
                }
            }
        }
        if let Ok(ws) = p.query_selector_all("button.w") {
            for j in 0..ws.length() {
                let Some(b) = ws.item(j).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else { continue };
                let Some(bid) = b.get_attribute("data-id") else { continue };
                let br = b.get_bounding_client_rect();
                now.chips.insert(bid, [((br.x() - r.x()) / k) as f32, ((br.y() - r.y()) / k + mt) as f32, (br.width() / k) as f32, (br.height() / k) as f32]);
            }
        }
    }
    let changed = MEASURED.with(|m| {
        let old = m.borrow();
        let same_h = old.h.len() == now.h.len() && now.h.iter().all(|(k, v)| old.h.get(k).is_some_and(|o| (o - v).abs() <= 0.5));
        let same_c = old.chips.len() == now.chips.len() && now.chips.iter().all(|(k, v)| old.chips.get(k).is_some_and(|o| o.iter().zip(v).all(|(a, b)| (a - b).abs() <= 0.5)));
        now.relaid || !(same_h && same_c)
    });
    if changed {
        MEASURED.with(|m| *m.borrow_mut() = now);
    }
    changed
}

fn katex() -> Option<js_sys::Function> {
    let w = web_sys::window()?;
    let k = js_sys::Reflect::get(&w, &JsValue::from_str("katex")).ok()?;
    if k.is_undefined() || k.is_null() {
        return None;
    }
    js_sys::Reflect::get(&k, &JsValue::from_str("renderToString")).ok()?.dyn_into::<js_sys::Function>().ok()
}

thread_local! {
    static KATEX_ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// KaTeX (the JS app's typesetter) is fetched the first time a formula is on the page, as the math font is.
fn ensure_katex() {
    if KATEX_ASKED.with(|a| a.replace(true)) {
        return;
    }
    let (Some(doc), Some(ctx)) = (document(), CTX.with(|c| c.borrow().clone())) else { return };
    let Ok(el) = doc.create_element("script") else { return };
    let _ = el.set_attribute("src", "katex.min.js");
    let _ = el.set_attribute("async", "");
    let onload = Closure::<dyn FnMut()>::new(move || {
        // typeset what is waiting, then let the page measure it
        if let Some(d) = document() {
            if let Ok(list) = d.query_selector_all("#controls [data-tex]") {
                for i in 0..list.length() {
                    if let Some(e) = list.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) {
                        let tex = e.get_attribute("data-tex").unwrap_or_default();
                        render_math(&e, &tex, e.class_list().contains("disp"));
                    }
                }
            }
        }
        ctx.request_repaint();
    });
    let _ = el.add_event_listener_with_callback("load", onload.as_ref().unchecked_ref());
    onload.forget();
    if let Some(head) = doc.head() {
        let _ = head.append_child(&el);
    }
}

/// A formula in a transcript line: KaTeX's MathML (the browser's own math layout, in the STIX Two Math face),
/// or its TeX source in monospace until KaTeX is there (plan rule 4: never nothing).
fn render_math(el: &web_sys::Element, tex: &str, display: bool) {
    let render = katex();
    let state = format!("{}{}|{}", if render.is_some() { "m" } else { "s" }, display as u8, tex);
    if el.get_attribute("data-done").as_deref() == Some(state.as_str()) {
        return;
    }
    match render {
        Some(f) => {
            let opts = js_sys::Object::new();
let _ = js_sys::Reflect::set(&opts, &"output".into(), &"mathml".into());
            let _ = js_sys::Reflect::set(&opts, &"displayMode".into(), &JsValue::from_bool(display));
            let _ = js_sys::Reflect::set(&opts, &"throwOnError".into(), &JsValue::FALSE);
            match f.call2(&JsValue::NULL, &JsValue::from_str(tex), &opts).ok().and_then(|v| v.as_string()) {
                Some(html) => {
                    el.set_inner_html(&html);
                    let _ = el.class_list().remove_1("src");
                    // its TeX annotation is a second copy of the text, a hit of `window.find` that selects nothing
                    if let Ok(Some(a)) = el.query_selector("annotation") {
                        a.remove();
                    }
                    // The typeset MathML is what assistive technology gets (its structure: fractions, roots, scripts,
                    // for math navigation and Nemeth / UEB braille), named in words; the TeX laid over it is
                    // `aria-hidden`, so a formula is still one node in the tree and spoken once
                    if let Ok(Some(k)) = el.query_selector(".katex") {
                        let _ = k.remove_attribute("aria-hidden");
                    }
                    // What find-in-page hits and a selection holds (the typeset formula is `user-select: none`:
                    // its symbols are blocks, which a selection reads one per line): the TeX, as text laid over
                    // the formula, transparent, so a hit is painted on the formula where it is
                    if let Some(hit) = document().and_then(|d| d.create_element("span").ok()) {
                        let _ = hit.set_attribute("class", "tx-hit");
                        let _ = hit.set_attribute("aria-hidden", "true");
                        let d = if display { "$$" } else { "$" };
                        hit.set_text_content(Some(&format!("{d}{tex}{d}")));
                        // before the typeset formula: `window.find` takes the first hit it can read, and the
                        // formula's own text is not selectable (it would be a hit that selects nothing)
                        let first = el.first_child();
                        let _ = el.insert_before(&hit, first.as_ref());
                    }
                }
                None => {
                    src_text(el, tex);
                }
            }
        }
        None => {
            src_text(el, tex);
            ensure_katex();
        }
    }
    let _ = el.set_attribute("data-done", &state);
}

/// The TeX as monospace text while there is no typesetter (plan rule 4), hidden from a reader: the element is
/// named from its label.
fn src_text(el: &web_sys::Element, tex: &str) {
    el.set_text_content(None);
    if let Some(span) = document().and_then(|d| d.create_element("span").ok()) {
        let _ = span.set_attribute("aria-hidden", "true");
        span.set_text_content(Some(tex));
        let _ = el.append_child(&span);
    }
    let _ = el.class_list().add_1("src");
}

fn css(c: egui::Color32) -> String {
    format!("rgba({},{},{},{})", c.r(), c.g(), c.b(), c.a() as f32 / 255.0)
}

/// The colours of the DOM transcript are the screen's tokens (theme, contrast, the system's own colours).
pub fn set_tokens(t: &crate::theme::Tokens) {
    thread_local! {
        static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    let v = [("--ink", t.ink), ("--muted", t.muted), ("--warn", t.warn), ("--bad", t.bad), ("--bad-bg", t.bad_bg), ("--ok", t.ok), ("--ok-bg", t.ok_bg), ("--hover", t.hover), ("--accent", t.accent), ("--sunken", t.sunken), ("--line2", t.line2)]
        .iter()
        .map(|(k, c)| format!("{k}:{}", css(*c)))
        .collect::<Vec<_>>()
        .join(";");
    if LAST.with(|l| *l.borrow() == v) {
        return;
    }
    if let Some(style) = host().and_then(|h| h.dyn_into::<web_sys::HtmlElement>().ok()).map(|h| h.style()) {
        for part in v.split(';') {
            if let Some((k, val)) = part.split_once(':') {
                let _ = style.set_property(k, val);
            }
        }
    }
    LAST.with(|l| *l.borrow_mut() = v);
}
