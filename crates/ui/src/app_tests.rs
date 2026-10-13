//! Frame-level tests: the real screen, driven through the control layer exactly as the DOM twin drives it
//! on the web (`act:<id>` messages), with the host replaced by a recorder.
use super::*;
use crate::twin::{Part, TEXT, TRAY};
use std::cell::RefCell;
use std::rc::Rc;

struct H {
    app: App,
    ctx: egui::Context,
    sent: Rc<RefCell<Vec<Cmd>>>,
    t: f64,
    out: egui::FullOutput,
    /// Text copied during any frame since the last `act` (clipboard output is per frame).
    copied: String,
}

impl H {
    fn new(select: &str) -> H {
        let ctx = egui::Context::default();
        let sent: Rc<RefCell<Vec<Cmd>>> = Rc::default();
        let s2 = sent.clone();
        let mut app = App::new(ctx.clone()).with_host(Box::new(move |c| s2.borrow_mut().push(c)), Inbox::new(ctx.clone()));
        let mut s = state::fixture::session();
        s.select(select);
        s.take_dirty();
        app.open_session(s, "Saved to your folder");
        let mut h = H { app, ctx, sent, t: 0.0, out: Default::default(), copied: String::new() };
        h.frames(3);
        h
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.t += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                time: Some(self.t),
                ..Default::default()
            };
            let app = &mut self.app;
            self.out = self.ctx.run(input, |c| app.step(c, false));
            if !self.out.platform_output.copied_text.is_empty() {
                self.copied = self.out.platform_output.copied_text.clone();
            }
        }
    }

    fn act(&mut self, id: &str) {
        assert!(self.app.controls().iter().any(|c| c.id == id), "no control {id}");
        self.app.queue.lock().unwrap().push(format!("act:{id}"));
        self.frames(3);
    }

    fn s(&self) -> &state::Session {
        self.app.session().unwrap()
    }

    fn word_id(&self, key: &str) -> String {
        self.app
            .controls()
            .iter()
            .find(|c| c.id.starts_with("word-") && c.id.ends_with(&format!("-{key}")))
            .expect("word control")
            .id
            .clone()
    }
}

#[test]
fn every_target_is_at_least_44_points() {
    let h = H::new("m1");
    let small: Vec<_> =
        h.app.controls().into_iter().filter(|c| c.size.0 < 44 || c.size.1 < 44).map(|c| (c.id.clone(), c.size)).collect();
    assert!(small.is_empty(), "targets under 44px: {small:?}");
    assert!(h.app.controls().iter().filter(|c| c.id.starts_with("word-")).count() >= 2, "flagged words are inline controls");
}

#[test]
fn twin_is_in_reading_order_with_words_inline_in_the_transcript() {
    let h = H::new("m1");
    let nodes = h.app.nodes();
    let pos = |pred: &dyn Fn(&Node) -> bool| nodes.iter().position(pred).unwrap();
    let name = pos(&|n| matches!(n, Node::Heading { id, level: 1, .. } if id == "student-name"));
    let para = pos(&|n| {
        matches!(n, Node::Para { region, parts, .. }
            if *region == TEXT && parts.iter().any(|p| matches!(p, Part::Control(c) if c.id.contains("c:0"))))
    });
    let tray = pos(&|n| matches!(n, Node::Heading { id, .. } if id == "tray-title"));
    let hdr = pos(&|n| matches!(n, Node::Control(c) if c.id == "done"));
    assert!(hdr < name && name < para && para < tray, "{hdr} {name} {para} {tray}");
    // the paragraph keeps the sentence around the control
    let Node::Para { parts, .. } = &nodes[para] else { unreachable!() };
    assert!(matches!(&parts[0], Part::Text(t) if t.starts_with("The 95% ")), "{parts:?}");
    // math is exposed as math, with its TeX
    assert!(nodes
        .iter()
        .any(|n| matches!(n, Node::Para { parts, .. } if parts.iter().any(|p| matches!(p, Part::Math(t) if t.contains("bar{x}"))))));
    // the status is a live region node
    assert!(nodes
        .iter()
        .any(|n| matches!(n, Node::Status { id, text, .. } if id == "student-status" && text == "3 words to check")));
    assert!(h.app.controls().iter().any(|c| c.region == TRAY && c.id == "alt-suggested"));
}

#[test]
fn pick_judge_accept_flow_from_the_twin_saves_the_page() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0");
    h.act(&w);
    assert_eq!(h.s().sel_word, Some(("m1".into(), "i:0".into())));
    // the illegible word offers its marker (guesses are empty in the fixture)
    h.act("alt-marked");
    assert_eq!(h.s().page("m1").unwrap().resolved().get("i:0").and_then(|v| v.as_str()), Some("marked"));
    assert!(h.sent.borrow().iter().any(|c| matches!(c, Cmd::SavePage { page, .. } if page.id() == "m1")));
    // advancing goes to the next undecided word of this student, which is on page 2
    assert_eq!(h.s().selected.as_deref(), Some("m2"));
    assert_eq!(h.s().sel_word, Some(("m2".into(), "c:0".into())));
    h.act("alt-suggested");
    assert!(h.s().page("m2").unwrap().corrected().contains("standard"));
    // back on page 1: accept the suggestion, then undo it through the same funnel
    h.act("page-m1");
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    assert!(h.s().page("m1").unwrap().corrected().contains("confidence"));
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-written");
    assert!(!h.s().page("m1").unwrap().corrected().contains("confidence"));
}

#[test]
fn typed_fix_through_the_twin_textbox() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0");
    h.act(&w);
    h.app.queue.lock().unwrap().push("text:typed-word\u{1}forty".into());
    h.frames(2);
    assert_eq!(h.app.typed, "forty");
    h.act("use-typed");
    let p = h.s().page("m1").unwrap();
    assert_eq!(p.resolved().get("i:0").and_then(|v| v.as_str()), Some("typed"));
    assert!(vishion_core::words::effective(p).contains("forty"));
}

#[test]
fn done_moves_to_the_next_student_and_saves_page_and_meta() {
    let mut h = H::new("a1");
    h.sent.borrow_mut().clear();
    h.act("done");
    assert_eq!(h.s().selected.as_deref(), Some("m1"));
    let sent = h.sent.borrow();
    assert!(sent.iter().any(|c| matches!(c, Cmd::SavePage { page, .. } if page.id() == "a1" && page.reviewed())));
    assert!(sent.iter().any(|c| matches!(c, Cmd::SaveMeta { .. })));
}

#[test]
fn copy_puts_the_students_text_on_the_clipboard() {
    let mut h = H::new("m1");
    h.act("copy");
    let copied = h.copied.clone();
    assert!(copied.contains("--- Maya R., Page 1 ---") && copied.contains("A second page"), "{copied:?}");
    assert!(h
        .app
        .nodes()
        .iter()
        .any(|n| matches!(n, Node::Status { id, text, .. } if id == "toast" && text.contains("Copied Maya R."))));
}

#[test]
fn layout_and_version_toggles_update_pressed_state() {
    let mut h = H::new("m1");
    h.act("layout-text");
    let c = h.app.controls().into_iter().find(|c| c.id == "layout-text").unwrap();
    assert_eq!(c.pressed, Some(true));
    assert!(h.app.controls().iter().all(|c| c.id != "boxes"), "no page pane in text-only layout");
    // Original is disabled while nothing is corrected (both versions read the same)
    let orig = h.app.controls().into_iter().find(|c| c.id == "ver-original").unwrap();
    assert!(orig.disabled);
    h.act("layout-split");
    h.act("page-m1");
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    h.act("ver-original");
    assert!(h.s().original);
    // original text shows what was written, so the word reads "confidance" again
    let w = h.app.controls().into_iter().find(|c| c.id.ends_with("c:0")).unwrap();
    assert!(w.label.contains("confidance"), "{}", w.label);
}

#[test]
fn dark_and_light_both_render() {
    let mut h = H::new("m1");
    h.app.theme_override = Some("dark".into());
    h.frames(2);
    assert_eq!(h.app.dark_applied, Some(true));
    h.app.theme_override = Some("light".into());
    h.frames(2);
    assert_eq!(h.app.dark_applied, Some(false));
}

#[test]
fn empty_store_and_failure_have_a_screen() {
    let ctx = egui::Context::default();
    let inbox = Inbox::new(ctx.clone());
    let mut app = App::new(ctx.clone()).with_host(Box::new(|_| {}), inbox.clone());
    inbox.push(Event::Empty { storage: "Saved to your folder".into() });
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
        ..Default::default()
    };
    let _ = ctx.run(input.clone(), |c| app.step(c, false));
    assert!(app.nodes().iter().any(|n| matches!(n, Node::Heading { text, .. } if text == "No assignments yet")));
    inbox.push(Event::Failed("boom".into()));
    let _ = ctx.run(input, |c| app.step(c, false));
    assert!(app.nodes().iter().any(|n| matches!(n, Node::Status { text, .. } if text == "boom")));
}
