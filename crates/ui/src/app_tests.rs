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
    /// Screen size in points (1280 x 800 unless a test asks for a phone).
    size: (f32, f32),
    /// Raw events for the next frame (mouse wheel).
    events: Vec<egui::Event>,
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
        let mut h = H { app, ctx, sent, t: 0.0, out: Default::default(), copied: String::new(), size: (1280.0, 800.0), events: vec![] };
        h.frames(3);
        h
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.t += 0.1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size.0, self.size.1))),
                time: Some(self.t),
                ..Default::default()
            };
            input.events.append(&mut self.events);
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

    /// Wheel over the transcript pane by `dy` points (positive scrolls down), then settle.
    fn wheel(&mut self, dy: f32) {
        self.events.push(egui::Event::PointerMoved(egui::pos2(900.0, 110.0)));
        self.frames(1);
        self.events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -dy), modifiers: Default::default() });
        self.frames(30);
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

// ---------------------------------------------------------------------------------------------------------
// Blind critic (phase 4 review screen): behaviour of js/ui/review.js + header.js that the screen lacks.
// Each test names the JS behaviour it is checked against and is expected to FAIL until fixed.
// ---------------------------------------------------------------------------------------------------------

impl H {
    fn key(&mut self, k: egui::Key) {
        self.t += 0.1;
        let ev = |pressed| egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size.0, self.size.1))),
            time: Some(self.t),
            events: vec![ev(true), ev(false)],
            ..Default::default()
        };
        let app = &mut self.app;
        self.out = self.ctx.run(input, |c| app.step(c, false));
        self.frames(2);
    }

    fn has(&self, label: &str) -> bool {
        self.app.controls().iter().any(|c| c.label.contains(label))
    }
}

/// review.js: `.tray` keydown Escape -> close() (the panel closes, focus returns to a word).
#[test]
fn critic_escape_in_the_tray_closes_it() {
    let mut h = H::new("m1");
    assert!(h.s().tray_open);
    h.app.queue.lock().unwrap().push("focus:alt-suggested".into());
    h.frames(2);
    h.key(egui::Key::Escape);
    assert!(!h.s().tray_open, "Escape with focus in the tray should close it, as js/ui/review.js does");
}

/// review.js: any click inside `.w` also calls setReadLine(line), so choosing a word underlines its line.
#[test]
fn critic_activating_a_flagged_word_moves_the_reading_underline_to_its_line() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0"); // [illegible] is on transcript line 1
    h.act(&w);
    assert_eq!(h.s().read_line.get("m1").copied(), Some(1), "review.js sets the read line when a flagged word is clicked");
}

/// review.js renderWho(): `if (same && textVersion === 'original') textVersion = 'corrected'` on every render.
#[test]
fn critic_original_flag_is_reset_on_a_page_where_both_versions_read_the_same() {
    let mut h = H::new("m1");
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    h.act("ver-original");
    assert!(h.s().original);
    h.act("page-m2"); // nothing corrected on m2: Original is disabled there
    let note = h
        .app
        .nodes()
        .iter()
        .find_map(|n| match n {
            Node::Text { id, text, .. } if id == "tray-note" => Some(text.clone()),
            _ => None,
        })
        .expect("tray note");
    assert!(!note.contains("Showing the original"), "Corrected is the pressed toggle, yet the tray says: {note}");
}

/// design/Main.dc.html + header.js: brand, back to the assignment, Settings.
#[test]
fn critic_header_has_back_to_assignment_and_settings() {
    let h = H::new("m1");
    assert!(h.has("Settings"), "no Settings control in the header: {:?}", h.app.controls().iter().map(|c| &c.label).collect::<Vec<_>>());
    assert!(
        h.app.controls().iter().any(|c| c.region == crate::twin::HEADER && c.label.contains("Unit 5")),
        "the assignment name is not a control: header.js `crumb` navigates back to the inbox"
    );
}

/// review.js scan pane tools: zoom, level, rotate, replace, remove; strip: Add page.
#[test]
fn critic_page_pane_tools_exist() {
    let h = H::new("m1");
    for l in ["Zoom in", "Zoom out", "Rotate page", "Replace this page", "Remove this page", "Straighten", "Add a page for"] {
        assert!(h.has(l), "page pane lacks {l:?}");
    }
}

/// review.js: Change student; Show plain text; Transcribe this page again.
#[test]
fn critic_transcript_tools_exist() {
    let h = H::new("m1");
    for l in ["Change student", "plain text", "again"] {
        assert!(h.has(l), "transcript pane lacks {l:?}");
    }
}

/// design flex-wrap + review.css: on a phone the header wraps and everything stays inside the screen,
/// and the page image is still reachable (scan pane stacks above the transcript).
#[test]
fn critic_phone_width_keeps_every_control_on_screen_and_keeps_the_page() {
    let mut h = H::new("m1");
    h.size = (390.0, 844.0);
    h.frames(3);
    let off: Vec<_> = h
        .app
        .controls()
        .into_iter()
        .filter(|c| c.pos.1 < 0 || c.pos.1 + c.size.0 as i32 > 390)
        .map(|c| (c.id.clone(), c.pos.1, c.size.0))
        .collect();
    assert!(off.is_empty(), "controls outside a 390pt screen: {off:?}");
    assert!(h.app.controls().iter().any(|c| c.id == "page-m1"), "no page strip / image at phone width");
}


// ---------------------------------------------------------------------------------------------------------
// Review-screen tools and parity (phase 4 builder round 2): every gap the critic named has a test here.
// ---------------------------------------------------------------------------------------------------------

impl H {
    /// A key forwarded by the DOM twin (`key:<name>`), `alt` for Alt+key.
    fn twin_key(&mut self, key: &str, alt: bool) {
        let m = if alt { format!("key:{key}\u{1}alt") } else { format!("key:{key}") };
        self.app.queue.lock().unwrap().push(m);
        self.frames(3);
    }

    fn focus(&mut self, id: &str) {
        self.app.queue.lock().unwrap().push(format!("focus:{id}"));
        self.frames(2);
    }

    fn sess(&mut self) -> &mut state::Session {
        self.app.session.as_mut().unwrap()
    }

    fn take_cmds(&self) -> Vec<Cmd> {
        std::mem::take(&mut *self.sent.borrow_mut())
    }

    fn node_text(&self, id: &str) -> Option<String> {
        self.app.nodes().iter().find_map(|n| match n {
            Node::Text { id: i, text, .. } | Node::Status { id: i, text, .. } | Node::Heading { id: i, text, .. } if i == id => {
                Some(text.clone())
            }
            _ => None,
        })
    }

    fn ctl(&self, id: &str) -> Option<&Reg> {
        self.app.controls().into_iter().find(|c| c.id == id)
    }
}

#[test]
fn scan_title_is_page_n_of_m() {
    let h = H::new("m1");
    assert_eq!(h.node_text("scan-title").as_deref(), Some("Page 1 of 2"));
    let mut h = H::new("m2");
    assert_eq!(h.node_text("scan-title").as_deref(), Some("Page 2 of 2"));
    h.act("page-m1");
    assert_eq!(h.node_text("scan-title").as_deref(), Some("Page 1 of 2"));
}

#[test]
fn phone_header_wraps_without_overlap_and_keeps_the_tools_on_screen() {
    let mut h = H::new("m1");
    h.size = (390.0, 844.0);
    h.frames(3);
    // nothing past the right edge, in any region (header, scan tools, strip, transcript, tray)
    let wide: Vec<_> = h
        .app
        .controls()
        .into_iter()
        .filter(|c| c.pos.1 < 0 || c.pos.1 + c.size.0 as i32 > 390)
        .map(|c| c.id.clone())
        .collect();
    assert!(wide.is_empty(), "past the right edge: {wide:?}");
    // header controls never sit on top of each other
    let hdr: Vec<&Reg> = h.app.controls().into_iter().filter(|c| c.region == crate::twin::HEADER).collect();
    for (i, a) in hdr.iter().enumerate() {
        for b in &hdr[i + 1..] {
            let same_band = a.pos.0 == b.pos.0 || (a.pos.0 - b.pos.0).abs() < 3;
            let overlap_x = a.pos.1 < b.pos.1 + b.size.0 as i32 && b.pos.1 < a.pos.1 + a.size.0 as i32;
            assert!(!(same_band && overlap_x && a.pos.0 == b.pos.0), "{} overlaps {}", a.id, b.id);
        }
    }
    // the page pane is above the transcript and its tools are all there
    for id in ["zoom-out", "zoom-in", "zoom-fit", "boxes", "level", "rotate", "replace", "remove", "add-page", "page-m1", "page-m2"] {
        assert!(h.ctl(id).is_some(), "{id} missing at phone width");
    }
    let scan_row = h.ctl("page-m1").unwrap().pos.0;
    let name_row = h.app.nodes().iter().find_map(|n| match n {
        Node::Heading { id, pos, .. } if id == "student-name" => Some(pos.0),
        _ => None,
    });
    assert!(scan_row < name_row.unwrap(), "the page pane stacks above the transcript");
    // Side by side still says pressed because the page really is there
    assert_eq!(h.ctl("layout-split").unwrap().pressed, Some(true));
}

#[test]
fn zoom_buttons_and_keys_follow_review_js() {
    let mut h = H::new("m1");
    assert_eq!(h.s().zoom, 1.0);
    assert!(h.ctl("zoom-out").unwrap().disabled, "cannot zoom out below fit");
    h.act("zoom-in");
    assert_eq!(h.s().zoom, 1.5);
    assert!(h.ctl("zoom-fit").unwrap().label.contains("Fit"));
    h.act("zoom-in");
    h.act("zoom-in");
    h.act("zoom-in");
    assert_eq!(h.s().zoom, 4.0);
    assert!(h.ctl("zoom-in").unwrap().disabled);
    h.act("zoom-out");
    assert_eq!(h.s().zoom, 3.0);
    h.act("zoom-fit");
    assert_eq!(h.s().zoom, 1.0);
    // keys only while the page stage has focus
    h.twin_key("+", false);
    assert_eq!(h.s().zoom, 1.0, "+ elsewhere does nothing");
    h.focus("stage");
    h.twin_key("+", false);
    assert_eq!(h.s().zoom, 1.5);
    h.twin_key("=", false);
    assert_eq!(h.s().zoom, 2.0);
    h.twin_key("-", false);
    assert_eq!(h.s().zoom, 1.5);
    h.twin_key("0", false);
    assert_eq!(h.s().zoom, 1.0);
}

#[test]
fn rotate_turns_the_page_and_asks_the_host_to_turn_the_image() {
    let mut h = H::new("m1");
    h.take_cmds();
    h.act("rotate");
    let p = h.s().page("m1").unwrap();
    assert_eq!(p.get("rotation").and_then(|v| v.as_i64()), Some(90));
    assert_eq!(p.get("boxes").and_then(|v| v.as_array()).map(|a| a.len()), Some(0));
    let cmds = h.take_cmds();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::RotateImage { id, .. } if id == "m1")), "{cmds:?}");
    assert!(cmds.iter().any(|c| matches!(c, Cmd::SavePage { page, .. } if page.id() == "m1")));
}

#[test]
fn replace_add_and_straighten_go_to_the_host() {
    let mut h = H::new("m1");
    h.take_cmds();
    h.act("replace");
    h.act("add-page");
    h.act("level");
    let cmds = h.take_cmds();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ReplaceImage { id, .. } if id == "m1")));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::AddPages { student, .. } if student == "Maya R.")));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Straighten { id, .. } if id == "m1")));
    assert_eq!(h.ctl("add-page").unwrap().label, "Add a page for Maya R.");
}

#[test]
fn a_replaced_page_resets_like_upload_js_and_new_pages_join_the_student() {
    let mut h = H::new("m1");
    h.app.host.as_ref().unwrap().1.push(Event::PageReplaced { id: "m1".into(), name: "new.jpg".into(), bytes: vec![] });
    h.frames(3);
    let p = h.s().page("m1").unwrap();
    assert_eq!((p.status(), p.raw(), p.s("name")), ("pending", "", "new.jpg"));
    assert!(p.edits().is_empty() && !p.reviewed());
    // a pending page offers to be read and says it is waiting
    assert!(h.ctl("transcribe-page").is_some());
    assert_eq!(h.node_text("student-status").as_deref(), Some("Waiting"));

    let mut np = vishion_core::model::Assignment::new_page("new1", "extra.jpg");
    np.set("student", serde_json::json!({"name": "Maya R.", "source": "chosen", "confirmed": true}));
    h.app.host.as_ref().unwrap().1.push(Event::PagesAdded { pages: vec![(np, vec![])] });
    h.frames(3);
    assert_eq!(h.s().group().unwrap().pages.len(), 3);
    assert_eq!(h.s().selected.as_deref(), Some("new1"));
    assert!(h.ctl("page-new1").is_some());
}

#[test]
fn removing_a_page_confirms_when_it_has_a_transcript_then_selects_a_neighbour() {
    let mut h = H::new("m1");
    h.take_cmds();
    h.act("remove");
    // a confirmation is the only thing the twin offers while it is open
    assert!(h.app.dialog.is_some());
    assert!(h.ctl("dialog-ok").is_some() && h.ctl("dialog-cancel").is_some());
    assert!(h.ctl("done").is_none(), "the twin hides the screen behind a modal");
    h.act("dialog-cancel");
    assert!(h.s().page("m1").is_some() && h.app.dialog.is_none());
    h.act("remove");
    h.act("dialog-ok");
    assert!(h.s().page("m1").is_none());
    assert_eq!(h.s().selected.as_deref(), Some("m2"));
    let cmds = h.take_cmds();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::RemovePage { id, .. } if id == "m1")), "{cmds:?}");
    assert!(cmds.iter().any(|c| matches!(c, Cmd::SaveMeta { meta, .. } if !meta.order().contains(&"m1".to_string()))));
}

#[test]
fn removing_the_only_page_of_a_student_goes_to_the_inbox() {
    let mut h = H::new("a1");
    h.act("remove");
    h.act("dialog-ok");
    assert_eq!(h.s().view, state::View::Inbox);
    assert!(h.node_text("inbox-title").is_some());
}

#[test]
fn change_student_moves_this_page_or_all_pages() {
    let mut h = H::new("m1");
    h.act("change-student");
    assert!(h.ctl("cs-student-0").is_some());
    h.act("cs-student-0"); // Ana L.
    h.act("dialog-ok");
    assert_eq!(h.s().page("m1").unwrap().student_name(), Some("Ana L."));
    assert_eq!(h.s().page("m2").unwrap().student_name(), Some("Maya R."));
    assert!(h.sent.borrow().iter().any(|c| matches!(c, Cmd::SavePage { page, .. } if page.id() == "m1")));

    // a new student, all pages
    let mut h = H::new("m1");
    h.act("change-student");
    h.act("cs-new");
    h.app.queue.lock().unwrap().push("text:cs-name\u{1}Zed Q.".into());
    h.frames(2);
    h.act("cs-scope-all");
    h.act("dialog-ok");
    assert_eq!(h.s().page("m1").unwrap().student_name(), Some("Zed Q."));
    assert_eq!(h.s().page("m2").unwrap().student_name(), Some("Zed Q."));
    assert_eq!(h.s().page("m1").unwrap().get("student").unwrap()["source"], "chosen");
}

#[test]
fn change_student_refuses_an_empty_choice_and_escape_closes_it() {
    let mut h = H::new("m1");
    h.act("change-student");
    h.act("dialog-ok");
    assert!(h.app.dialog.is_some(), "stays open");
    assert!(h.node_text("cs-error").is_some());
    h.twin_key("Escape", false);
    assert!(h.app.dialog.is_none());
}

#[test]
fn plain_and_formatted_text_toggle() {
    let mut h = H::new("m1");
    assert_eq!(h.ctl("text-view").unwrap().label, "Show plain text");
    h.act("text-view");
    assert!(h.s().text_raw);
    assert_eq!(h.ctl("text-view").unwrap().label, "Show formatted text");
    let plain = h.node_text("plain-m1").expect("plain transcript text");
    assert!(plain.contains("confidence") || plain.contains("confidance"));
    assert!(!plain.contains('\u{E000}'), "no word markers in plain text");
    assert!(h.ctl("word-m1-c:0").is_none(), "no word buttons in plain view");
    h.act("text-view");
    assert!(h.ctl("word-m1-c:0").is_some());
}

#[test]
fn transcribe_again_confirms_only_when_decisions_would_be_lost() {
    let mut h = H::new("m1");
    h.take_cmds();
    h.act("transcribe-again");
    // nothing decided yet: goes straight to the host
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::Transcribe { ids, .. } if ids == &vec!["m1".to_string()])));
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    h.act("page-m1");
    h.take_cmds();
    h.act("transcribe-again");
    assert!(h.app.dialog.is_some(), "decisions exist: ask first");
    assert!(h.take_cmds().iter().all(|c| !matches!(c, Cmd::Transcribe { .. })));
    h.act("dialog-ok");
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::Transcribe { .. })));
}

#[test]
fn error_page_has_an_alert_and_retry_and_pending_page_has_transcribe() {
    let mut h = H::new("m1");
    {
        let p = h.sess().a.pages.get_mut("m1").unwrap();
        p.set("status", serde_json::json!("error"));
        p.set("error", serde_json::json!("The model did not answer."));
        p.set("raw", serde_json::json!(""));
    }
    h.frames(2);
    assert_eq!(h.node_text("page-error").as_deref(), Some("The model did not answer."));
    h.take_cmds();
    h.act("retry-page");
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::Transcribe { ids, .. } if ids == &vec!["m1".to_string()])));
    assert_eq!(h.node_text("student-status").as_deref(), Some("Couldn\u{2019}t transcribe"));
    {
        let p = h.sess().a.pages.get_mut("m1").unwrap();
        p.set("status", serde_json::json!("pending"));
    }
    h.frames(2);
    assert!(h.ctl("transcribe-page").is_some());
    assert!(h.node_text("state-title").unwrap().contains("no transcript yet"));
    h.take_cmds();
    h.act("transcribe-page");
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::Transcribe { .. })));
}

#[test]
fn header_has_brand_crumb_and_settings_and_the_last_student_goes_to_the_inbox() {
    let mut h = H::new("m1");
    assert!(h.ctl("brand").is_some());
    assert_eq!(h.ctl("crumb").unwrap().label, "Back to Unit 5");
    // Maya is the last student: Done marks her reviewed, toasts, and shows the inbox
    h.act("done");
    assert_eq!(h.s().view, state::View::Inbox);
    assert!(h.s().page("m1").unwrap().reviewed() && h.s().page("m2").unwrap().reviewed());
    assert!(h.node_text("toast").unwrap().contains("last student"));
    assert!(h.node_text("inbox-title").is_some());
    // the inbox lists the students and opens one
    assert!(h.ctl("row-ana-l").is_some() || h.app.controls().iter().any(|c| c.id.starts_with("row-")));
    let row = h.app.controls().iter().find(|c| c.id.starts_with("row-") && c.label.contains("Ana")).unwrap().id.clone();
    h.act(&row);
    assert_eq!(h.s().view, state::View::Review);
    assert_eq!(h.s().selected.as_deref(), Some("a1"));
    // the crumb and the brand go back
    h.act("crumb");
    assert_eq!(h.s().view, state::View::Inbox);
}

#[test]
fn settings_gear_opens_a_dialog_that_changes_the_theme_and_escape_closes_it() {
    let mut h = H::new("m1");
    h.take_cmds();
    h.act("settings");
    assert!(h.ctl("theme-dark").is_some());
    h.act("theme-dark");
    assert_eq!(h.s().settings.get("theme").and_then(|v| v.as_str()), Some("dark"));
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::SaveSettings { .. })));
    assert_eq!(h.app.dark_applied, Some(true));
    h.twin_key("Escape", false);
    assert!(h.app.dialog.is_none());
    assert!(h.ctl("done").is_some());
}

#[test]
fn escape_only_closes_the_tray_when_focus_is_in_it() {
    let mut h = H::new("m1");
    assert!(h.s().tray_open);
    h.focus("done");
    h.twin_key("Escape", false);
    assert!(h.s().tray_open, "Escape from the header must not close the tray");
    h.focus("alt-suggested");
    h.twin_key("Escape", false);
    assert!(!h.s().tray_open);
    // focus goes back to a word, as close() does in review.js
    assert!(h.app.ctl.focus.as_deref().is_some_and(|f| f.starts_with("word-")), "{:?}", h.app.ctl.focus);
}

#[test]
fn arrow_keys_move_the_reading_underline_only_inside_the_transcript() {
    let mut h = H::new("m1");
    h.focus("done");
    h.twin_key("ArrowDown", false);
    assert_eq!(h.s().read_line.get("m1"), None, "header focus: arrows do nothing to the transcript");
    h.focus("page-m1");
    h.twin_key("ArrowDown", false);
    assert_eq!(h.s().read_line.get("m1"), None);
    h.focus("transcript");
    h.twin_key("ArrowDown", false);
    assert_eq!(h.s().read_line.get("m1"), Some(&0));
    h.twin_key("ArrowDown", false);
    assert_eq!(h.s().read_line.get("m1"), Some(&1));
    h.twin_key("ArrowUp", false);
    assert_eq!(h.s().read_line.get("m1"), Some(&0));
    h.twin_key("Escape", false);
    assert_eq!(h.s().read_line.get("m1"), None);
    assert!(h.s().tray_open, "the first Escape clears the underline, not the tray");
}

#[test]
fn strip_keys_step_jump_delete_and_reorder() {
    let mut h = H::new("m1");
    h.focus("page-m1");
    h.twin_key("ArrowRight", false);
    assert_eq!(h.s().selected.as_deref(), Some("m2"));
    assert_eq!(h.app.ctl.focus.as_deref(), Some("page-m2"), "focus follows the selection");
    h.twin_key("ArrowRight", false);
    assert_eq!(h.s().selected.as_deref(), Some("m2"), "clamped at the end");
    h.twin_key("Home", false);
    assert_eq!(h.s().selected.as_deref(), Some("m1"));
    h.twin_key("End", false);
    assert_eq!(h.s().selected.as_deref(), Some("m2"));
    h.twin_key("ArrowLeft", false);
    assert_eq!(h.s().selected.as_deref(), Some("m1"));
    // Alt+Arrow reorders within the student and saves the order
    h.take_cmds();
    h.focus("page-m1");
    h.twin_key("ArrowRight", true);
    let order = h.s().a.meta.order();
    let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
    assert!(pos("m2") < pos("m1"), "{order:?}");
    let cmds = h.take_cmds();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::SaveMeta { .. })));
    // js writes the project only: no page file is rewritten for a reorder (blind critic round 3)
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::SavePage { .. })), "{cmds:?}");
    assert_eq!(h.s().group().unwrap().pages, vec!["m2".to_string(), "m1".to_string()]);
    // Delete asks first
    h.focus("page-m1");
    h.twin_key("Delete", false);
    assert!(h.app.dialog.is_some());
    h.act("dialog-ok");
    assert!(h.s().page("m1").is_none());
}

#[test]
fn strip_selection_offers_export_review_again_and_clear() {
    let mut h = H::new("m1");
    assert!(h.ctl("strip-export").is_none(), "nothing selected, no bar");
    h.sess().multi_click("m2", false); // ctrl-click: current page joins, m2 toggles in
    h.frames(2);
    assert_eq!(h.s().multi.len(), 2);
    assert_eq!(h.ctl("strip-export").unwrap().label, "Export 2 selected");
    // review again clears decisions after a confirmation
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    assert!(h.s().page("m1").unwrap().corrected().contains("confidence"));
    h.act("strip-review-again");
    assert!(h.app.dialog.is_some());
    h.act("dialog-ok");
    assert!(!h.s().page("m1").unwrap().corrected().contains("confidence"), "suggestion un-applied");
    assert!(h.s().page("m1").unwrap().resolved().is_empty());
    assert!(h.s().multi.is_empty());
    // export writes a file through the host
    h.sess().multi_click("m2", false);
    h.frames(2);
    h.take_cmds();
    h.act("strip-export");
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::Export { name, bytes, .. } if name.ends_with(".txt") && !bytes.is_empty())));
    h.act("strip-clear");
    assert!(h.s().multi.is_empty());
}

#[test]
fn transcript_column_is_31em_and_chips_keep_the_line_pitch() {
    let mut h = H::new("m1");
    h.size = (1600.0, 900.0);
    h.frames(3);
    let left = h
        .app
        .nodes()
        .iter()
        .find_map(|n| match n {
            Node::Heading { id, pos, .. } if id == "student-name" => Some(pos.1),
            _ => None,
        })
        .unwrap();
    // 31em of 22px text = 682; chips are at least 44 wide but the column itself ends at 682
    let over: Vec<_> = h
        .app
        .controls()
        .into_iter()
        .filter(|c| c.id.starts_with("word-") && c.pos.1 - left > 682)
        .map(|c| c.id.clone())
        .collect();
    assert!(over.is_empty(), "chips start past the 31em column: {over:?}");
    // consecutive transcript lines are 37.4 apart (22 * 1.7) whether or not they hold a chip
    let rows: Vec<i32> = h
        .app
        .nodes()
        .iter()
        .filter_map(|n| match n {
            Node::Para { id, pos, .. } if id.starts_with("line-m1-") => Some(pos.0),
            _ => None,
        })
        .collect();
    assert!(rows.len() >= 3);
    for w in rows.windows(2) {
        let d = (w[1] - w[0]) * 16;
        assert!((16..=80).contains(&d), "line spacing in 16px bands: {rows:?}");
    }
}

#[test]
fn word_chips_reach_44_pt_but_paint_at_the_design_height() {
    let h = H::new("m1");
    for c in h.app.controls().into_iter().filter(|c| c.id.starts_with("word-")) {
        assert!(c.size.0 >= 44 && c.size.1 >= 44, "{}: {:?}", c.id, c.size);
    }
}

#[test]
fn page_replace_cancelled_or_failed_shows_a_notice_not_a_change() {
    let mut h = H::new("m1");
    h.app.host.as_ref().unwrap().1.push(Event::Notice { text: "No picture chosen. The page is unchanged.".into(), error: false });
    h.frames(2);
    assert!(h.node_text("toast").unwrap().contains("unchanged"));
    assert_eq!(h.s().page("m1").unwrap().status(), "done");
}

// ---------------------------------------------------------------------------------------------------------
// Blind critic (phase 4 review screen, round 2). Each test names the js/ui behaviour it is checked against and
// is expected to FAIL until the product code is fixed.
// ---------------------------------------------------------------------------------------------------------

/// review.js renderTray: every guess is its own `.alt` button. Here the twin id is `alt-{via}`, and every guess has
/// via "typed", so two guesses share one id and the twin (keyed by id) exposes only the last one to keyboard and
/// screen readers on the web: "intervals, 62% likely" cannot be reached.
#[test]
fn critic_each_illegible_guess_has_its_own_twin_control() {
    let mut h = H::new("m1");
    h.sess().a.pages.get_mut("m1").unwrap().set(
        "guesses",
        serde_json::json!({"i:0": [{"text": "intervals", "percent": 62}, {"text": "intervening", "percent": 21}]}),
    );
    h.frames(2);
    let w = h.word_id("i:0");
    h.act(&w);
    let alts: Vec<String> = h.app.controls().iter().filter(|c| c.id.starts_with("alt-")).map(|c| c.id.clone()).collect();
    let mut uniq = alts.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(alts.len(), 3, "two guesses and the marker must be three controls: {alts:?}");
    assert_eq!(uniq.len(), 3, "control ids must be unique (the twin is keyed by id): {alts:?}");
}

/// review.js syncSel/paintReading: the selected word's line is scrolled into view (`scrollIntoView`), so the word
/// being decided is never left behind the tray. Here the transcript keeps its offset.
#[test]
fn critic_selected_word_is_not_left_under_the_tray() {
    let mut h = H::new("m1");
    h.size = (1280.0, 520.0);
    h.frames(3);
    let w = h.word_id("i:0");
    h.act(&w);
    h.frames(4);
    let tray_top = h
        .app
        .nodes()
        .iter()
        .find_map(|n| match n {
            Node::Heading { id, pos, .. } if id == "tray-title" => Some(pos.0 * 16),
            _ => None,
        })
        .expect("tray title");
    let word = h.ctl(&w).expect("word control");
    let bottom = word.pos.0 * 16 + 22;
    assert!(bottom <= tray_top, "the selected word ends at {bottom} but the tray starts at {tray_top}");
}

/// review.js renders the page's own text in any script (system fonts): CJK and emoji are real student input.
/// The bundled fonts have no CJK fallback, so Chrome shows tofu boxes.
#[test]
fn critic_cjk_text_has_a_fallback_font() {
    let h = H::new("m1");
    let ok = h.ctx.fonts(|f| f.has_glyph(&theme::serif(22.0), '\u{65e5}'));
    assert!(ok, "no glyph for U+65E5 in the serif family: CJK transcripts render as boxes");
}

// ---------------------------------------------------------------------------------------------------------
// Blind critic round 2 fixes: tray layout, activations that arrive together, plain text, footer, fonts.
// ---------------------------------------------------------------------------------------------------------

/// A PNG page with grain and dark writing on the given pixel rows (what `lineBands` is meant to find).
fn page_png(w: u32, h: u32, rows: &[(u32, u32)]) -> Vec<u8> {
    let mut im = image::RgbImage::from_pixel(w, h, image::Rgb([235, 235, 232]));
    for y in 0..h {
        for x in 0..w {
            if (x * 7 + y * 13) % 29 == 0 {
                im.put_pixel(x, y, image::Rgb([120, 120, 120]));
            }
        }
    }
    for &(a, b) in rows {
        for y in a..b {
            for x in 80..(w - 120) {
                if (x / 8) % 5 == 0 {
                    im.put_pixel(x, y, image::Rgb([40, 40, 60]));
                }
            }
        }
    }
    let mut out = std::io::Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(im).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

impl H {
    /// Give page `m1` a picture with three lines of writing and the boxes of those lines.
    fn with_picture(&mut self) {
        let png = page_png(840, 1120, &[(160, 220), (460, 520), (760, 820)]);
        let boxes = serde_json::json!([
            {"x": 0.09, "y": 0.13, "w": 0.75, "h": 0.05, "r": 0},
            {"x": 0.09, "y": 0.40, "w": 0.75, "h": 0.05, "r": 0},
            {"x": 0.09, "y": 0.67, "w": 0.75, "h": 0.05, "r": 0},
        ]);
        self.sess().a.pages.get_mut("m1").unwrap().set("boxes", boxes);
        self.app.host.as_ref().unwrap().1.push(Event::Image { id: "m1".into(), bytes: Some(png) });
        self.frames(3);
    }
}

/// css/components.css `.tray-body { flex-wrap: wrap }` with `.crop { flex: 0 1 280px }` and `.alts { flex: 1 1 300px }`:
/// side by side only when 280 + 20 + 300 fit, otherwise the options take the whole row under the crop.
#[test]
fn critic_tray_body_wraps_like_the_design() {
    let mut h = H::new("m1");
    h.with_picture();
    let w = h.word_id("i:0");
    h.act(&w);
    // 1280 split: the text pane is 640 wide with a 1px border, so the tray content is 559: stacked, options
    // full width (js: `button.alt` 559 wide)
    let alt = h.ctl("alt-marked").unwrap().size.0;
    assert_eq!(alt, 559, "options use the whole row when the crop wraps above them");
    let typed = h.ctl("typed-word").unwrap().size.0;
    assert!(typed >= 400, "typed input keeps its width ({typed})");
    // text only: 680 inside the tray, 280 + 20 for the crop leaves 380 for the options
    h.act("layout-text");
    let alt = h.ctl("alt-marked").unwrap().size.0;
    assert_eq!(alt, 380, "crop beside the options");
}

/// review.js: every option card's text stays inside the card, whatever its width.
#[test]
fn critic_option_hint_stays_inside_its_card() {
    let mut h = H::new("m1");
    h.size = (390.0, 800.0);
    h.frames(3);
    let w = h.word_id("i:0");
    h.act(&w);
    let c = h.ctl("alt-marked").unwrap();
    assert!(c.size.0 <= 390, "{:?}", c.size);
    assert!(c.size.1 >= 44);
}

/// Several activations of one control that reach the app in the same frame are each applied (JS: three clicks
/// on Zoom in give 300%), as is a held key repeating Enter or Space on a focused button.
#[test]
fn critic_activations_that_arrive_together_are_not_coalesced() {
    let mut h = H::new("m1");
    for _ in 0..3 {
        h.app.queue.lock().unwrap().push("act:zoom-in".into());
    }
    h.frames(8);
    assert_eq!(h.s().zoom, 3.0);
}

/// js/ui/review.js pillFor: a queued group is the neutral outlined `.pill.queued`, not the yellow warning.
#[test]
fn critic_waiting_is_a_quiet_pill_not_a_warning() {
    let mut s = state::fixture::session();
    s.select("m2");
    s.a.pages.get_mut("m2").unwrap().set("status", serde_json::json!("pending"));
    let g = s.group().unwrap();
    let (kind, label) = s.pill(&g);
    assert_eq!(label, "Waiting");
    assert_eq!(kind, state::PillKind::Queued);
    s.a.pages.get_mut("m2").unwrap().set("status", serde_json::json!("transcribing"));
    let g = s.group().unwrap();
    assert_eq!(s.pill(&g).0, state::PillKind::Busy);
}

/// `.tx-text.plain`: left aligned, in the same column as the heading.
#[test]
fn critic_plain_text_is_left_aligned_in_the_column() {
    let mut h = H::new("m1");
    h.act("text-view");
    let name_x = h.app.nodes().iter().find_map(|n| match n {
        Node::Heading { id, pos, .. } if id == "student-name" => Some(pos.1),
        _ => None,
    });
    let plain_x = h.app.nodes().iter().find_map(|n| match n {
        Node::Text { id, pos, .. } if id.starts_with("plain-") => Some(pos.1),
        _ => None,
    });
    assert!(name_x.is_some() && plain_x.is_some());
    assert_eq!(name_x, plain_x, "the plain text starts at the column's left edge");
}

/// `.tx-foot`: the buttons are left aligned under the transcript, in the column of the heading.
#[test]
fn critic_footer_is_left_aligned_and_stacked() {
    let h = H::new("m1");
    let x = |id: &str| h.ctl(id).map(|c| c.pos.1);
    let name = h.app.nodes().iter().find_map(|n| match n {
        Node::Heading { id, pos, .. } if id == "student-name" => Some(pos.1),
        _ => None,
    });
    assert_eq!(x("text-view"), name, "Show plain text starts at the column edge");
    // Transcribe again follows it on the same row (flex-wrap)
    assert_eq!(h.ctl("transcribe-again").map(|c| c.pos.0), h.ctl("text-view").map(|c| c.pos.0));
    // Change student sits 16px left of the name so its text lines up with it
    let cs = x("change-student").unwrap();
    assert!(cs < name.unwrap(), "{cs} {name:?}");
}

/// css/layout.css `.brand` holds the 119 x 32 logo svg.
#[test]
fn critic_wordmark_has_the_logo_width() {
    let h = H::new("m1");
    assert_eq!(h.ctl("brand").unwrap().size.0, 119);
}

/// Student text can be in any script. The web build fetches the fallback font when the open page has a
/// character the bundled faces lack, and installs it when it arrives.
#[test]
fn critic_fallback_font_is_requested_on_demand_and_covers_cjk() {
    let mut h = H::new("m1");
    // as the web build starts: no fallback yet
    h.ctx.set_fonts(theme::fonts(None));
    h.app.fallback_loaded = false;
    h.app.fallback_requested = false;
    h.app.fallback_seen = None;
    h.frames(2);
    assert!(!h.ctx.fonts(|f| f.has_glyph(&theme::serif(22.0), '\u{65e5}')));
    h.sess().a.pages.get_mut("m1").unwrap().set(
        "raw",
        serde_json::json!("The 95% interval.\n\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8} \u{d55c}\u{ad6d}\u{c5b4} \u{1f600}"),
    );
    h.take_cmds();
    h.frames(3);
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::LoadFallbackFont)), "asked for the fallback font");
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/fallback-cjk-emoji.ttf")).unwrap();
    h.app.host.as_ref().unwrap().1.push(Event::FallbackFont(bytes));
    h.frames(3);
    for c in ['\u{65e5}', '\u{672c}', '\u{30c6}', '\u{d55c}', '\u{4e2d}', '\u{1f600}'] {
        assert!(h.ctx.fonts(|f| f.has_glyph(&theme::serif(22.0), c)), "no glyph for {c:?}");
    }
}

/// A page without usable stored boxes gets its marks where the ink is (image.js lineBands), not spread
/// over a fixed 8%..92%.
#[test]
fn critic_marks_land_on_the_ink_when_boxes_are_missing() {
    let mut h = H::new("m1");
    h.sess().a.pages.get_mut("m1").unwrap().set("boxes", serde_json::json!([]));
    h.app.host.as_ref().unwrap().1.push(Event::Image {
        id: "m1".into(),
        bytes: Some(page_png(840, 1120, &[(160, 220), (460, 520), (760, 820)])),
    });
    h.frames(3);
    let Some(Slot::Ready(im)) = h.app.images.get("m1") else { panic!("image not decoded") };
    assert_eq!(im.bands.len(), 3, "{:?}", im.bands);
    let p = h.s().page("m1").unwrap();
    let (boxes, approx) = state::geometry(p, 3, &im.bands).unwrap();
    assert!(!approx);
    for (b, y) in boxes.iter().zip([190.0, 490.0, 790.0]) {
        let mid = (b.y + b.h / 2.0) * 1120.0;
        assert!((mid - y).abs() < 25.0, "mark at {mid}, ink at {y}");
    }
}

/// The tray grows when the page picture arrives (its crop appears): the word being decided is brought back
/// above it instead of staying behind it.
#[test]
fn critic_word_stays_above_the_tray_when_the_crop_arrives() {
    let mut h = H::new("m1");
    h.size = (1280.0, 460.0);
    h.frames(3);
    let w = h.word_id("i:0");
    h.act(&w);
    h.frames(10);
    h.with_picture();
    h.frames(10);
    // the tray's first row (previous, next, close) starts 12 below its top edge
    let tray_top = h.ctl("prev-word").expect("tray controls").at.1 - 12;
    let word = h.ctl(&w).expect("word control");
    // the target is 44 high, centred on the painted word (29.7 high)
    let chip_bottom = word.at.1 + 22 + 15;
    assert!(chip_bottom <= tray_top, "the word ends at {chip_bottom} but the tray starts at {tray_top}");
}

fn blind_node_y(h: &H, id: &str) -> Option<i32> {
    h.app.nodes().iter().find_map(|n| match n {
        Node::Heading { id: i, pos, .. } if i == id => Some(pos.0 * 16),
        _ => None,
    })
}

/// js/ui/review.js never scrolls the transcript on open when the selected word already fits above the tray
/// (1280 x 800): the student name and the status pill stay visible. Here the name heading is scrolled up
/// under the top bar.
#[test]
fn blind3_desktop_open_keeps_student_name_and_pill_on_screen() {
    let mut h = H::new("m1");
    h.with_picture();
    h.frames(15);
    // the top bar ends 8 + 1 below its lowest control (padding and the 1px border)
    let bar = h.app.controls().iter().filter(|c| c.region == crate::twin::HEADER).map(|c| c.at.1 + c.size.1 as i32).max().unwrap() + 9;
    let name_y = blind_node_y(&h, "student-name").expect("student name heading");
    assert!(name_y >= bar, "student name sits at {name_y}: scrolled under the {bar}px top bar");
    // exactly one 32px of padding under the bar, as in js (unscrolled)
    let c = h.ctl("change-student").unwrap();
    assert!(c.at.1 >= bar + 32 + 36 + 6 + 19 + 4 - 2, "text pane was scrolled: Change student at {}", c.at.1);
    // and the word being decided is above the tray without moving the pane
    let w = h.ctl(&h.word_id("c:0")).unwrap().at.1;
    let tray = blind_node_y(&h, "tray-title").unwrap();
    assert!(w + 44 <= tray + 24, "word target at {w}, tray title at {tray}");
}

/// Blind critic round 3: js writes `meta.ui.scroll` and `meta.ui.scrollId` when the transcript pane scrolls
/// (debounced 600 ms) and restores them when that page is selected again.
#[test]
fn blind3_transcript_scroll_position_is_persisted_and_restored() {
    let mut h = H::new("m1");
    h.with_picture();
    h.frames(10);
    h.take_cmds();
    h.size = (1280.0, 520.0);
    h.frames(5);
    h.wheel(160.0);
    h.frames(15);
    let ui = h.s().a.meta.get("ui").cloned().unwrap();
    let y = ui.get("scroll").and_then(|v| v.as_f64()).unwrap_or(-1.0);
    assert!(y > 20.0, "scroll was not written: {ui}");
    assert_eq!(ui.get("scrollId").and_then(|v| v.as_str()), Some("m1"));
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::SaveMeta { .. })), "the project is saved");
    // other fields of meta.ui stay (selectedId, layout)
    // leave the page and come back: the saved offset is asked for again
    h.act("page-m2");
    assert_eq!(h.s().scroll_req.map(|(_, o)| o), Some(0.0), "another page starts at the top");
    h.act("page-m1");
    let back = h.s().scroll_req.map(|(_, o)| o).unwrap();
    assert!((back as f64 - y).abs() < 1.0, "restored {back}, saved {y}");
}

/// Blind critic round 3, phone 390 x 844 in a real browser: the picture, the crop and the math font arrive
/// after the word was selected and move the page. js never scrolls to the word (the page opens at the top,
/// on the page image), so the page pane stays under the top bar instead of being scrolled away.
#[test]
fn blind3_phone_page_stays_at_the_top_while_pictures_arrive() {
    let mut h = H::new("m1");
    h.size = (390.0, 844.0);
    h.frames(8);
    h.with_picture();
    h.frames(12);
    h.app.host.as_ref().unwrap().1.push(Event::MathFont(vec![]));
    h.frames(12);
    let bar = h.app.controls().iter().filter(|c| c.region == crate::twin::HEADER).map(|c| c.at.1 + c.size.1 as i32).max().unwrap() + 9;
    let tools = h.ctl("zoom-out").expect("page tools").at.1;
    assert!(tools >= bar && tools < bar + 40, "page tools at {tools}, top bar ends {bar}");
}

/// Blind critic round 3: U+1F600 drew as a monochrome outline. The transcript shows emoji as colour pictures
/// (a COLR face rasterised in `emoji.rs`), between the words, and the twin still carries them as text.
#[test]
fn blind3_emoji_are_drawn_in_colour_between_the_words() {
    let mut h = H::new("m1");
    h.sess().a.pages.get_mut("m1").unwrap().set("raw", serde_json::json!("Last line with \u{65E5}\u{672C}\u{8A9E} \u{1F600}\nand a flag \u{1F1EF}\u{1F1F5} and \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} here"));
    h.sess().a.pages.get_mut("m1").unwrap().set("corrected", serde_json::json!(""));
    h.sess().a.pages.get_mut("m1").unwrap().set("corrections", serde_json::json!([]));
    h.frames(6);
    assert!(h.app.emoji.has_font(), "the face is linked into the native app");
    assert_eq!(h.app.emoji.drawn(), 3, "smile, flag and family are three pictures");
    // the paragraph keeps the characters for assistive tech
    let text: String = h
        .app
        .nodes()
        .iter()
        .filter_map(|n| match n {
            Node::Para { parts, .. } => Some(parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<String>()),
            _ => None,
        })
        .collect();
    assert!(text.contains('\u{1F600}'), "{text:?}");
}

// ---------------------------------------------------------------------------------------------------------
// Phase 4 round 4: transcript keyboard after a click, no transcript jumps, phone math wrap, metrics.
// ---------------------------------------------------------------------------------------------------------

impl H {
    /// A real pointer click on the canvas at `p` (press, then release on the next frame).
    fn click(&mut self, p: egui::Pos2) {
        self.events.push(egui::Event::PointerMoved(p));
        self.frames(1);
        self.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        self.frames(1);
        self.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        self.frames(3);
    }

    /// Top left of a transcript paragraph (`line-<page>-<n>`), in points.
    fn line_pos(&self, id: &str) -> egui::Pos2 {
        self.app
            .nodes()
            .iter()
            .find_map(|n| match n {
                Node::Para { id: i, pos, .. } if i == id => Some(egui::pos2(pos.1 as f32, pos.0 as f32 * 16.0)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no paragraph {id}"))
    }

    /// Stroked rectangles of width 2 in the last frame (focus rings), as rects.
    fn rings(&self) -> Vec<egui::Rect> {
        let mut v = vec![];
        for cs in &self.out.shapes {
            if let egui::epaint::Shape::Rect(r) = &cs.shape {
                if r.stroke.width == 2.0 && r.fill == egui::Color32::TRANSPARENT {
                    v.push(r.rect);
                }
            }
        }
        v
    }
}

/// js: `txText` has tabindex 0, so a click on a line focuses the transcript. Arrow keys then move the underline
/// (page image too), Escape clears it, and the focus ring shows once the keyboard is used.
#[test]
fn round4_click_on_a_line_then_arrows_and_escape_drive_the_underline() {
    let mut h = H::new("m1");
    h.with_picture();
    h.act("tray-close");
    h.frames(5);
    let p = h.line_pos("line-m1-0");
    h.click(p + egui::vec2(30.0, 12.0));
    assert_eq!(h.s().read_line.get("m1"), Some(&0), "click underlines the line");
    assert_eq!(h.app.ctl.focus.as_deref(), Some("transcript"), "the click focuses the transcript");
    let wide = |h: &H| h.rings().iter().any(|r| r.width() > 300.0 && r.height() > 100.0);
    assert!(!wide(&h), "a mouse click alone shows no focus ring (:focus-visible)");
    h.twin_key("ArrowDown", false);
    h.twin_key("ArrowDown", false);
    assert_eq!(h.s().read_line.get("m1"), Some(&2));
    h.frames(2);
    assert!(wide(&h), "after a key the transcript shows its ring");
    h.twin_key("ArrowUp", false);
    assert_eq!(h.s().read_line.get("m1"), Some(&1));
    h.twin_key("Escape", false);
    assert_eq!(h.s().read_line.get("m1"), None, "Escape clears the underline");
}

/// js leaves `tx-pane.scrollTop` at 0 (or the saved offset) when a page opens or a word is accepted: the name,
/// pill, legend and Corrected/Original stay in view.
#[test]
fn round4_selecting_a_page_or_accepting_a_word_does_not_scroll_the_transcript() {
    let mut h = H::new("m1");
    h.with_picture();
    h.frames(10);
    let at_open = blind_node_y(&h, "student-name").expect("name");
    h.act("page-m2");
    h.frames(20);
    assert_eq!(blind_node_y(&h, "student-name"), Some(at_open), "opening page 2 moved the transcript");
    h.act("alt-suggested");
    h.frames(20);
    assert_eq!(blind_node_y(&h, "student-name"), Some(at_open), "accepting a suggestion moved the transcript");
    // and on a phone the page pane stays first
    let mut h = H::new("m1");
    h.size = (390.0, 844.0);
    h.with_picture();
    h.frames(20);
    let bar = h.app.controls().iter().filter(|c| c.region == crate::twin::HEADER).map(|c| c.at.1 + c.size.1 as i32).max().unwrap() + 9;
    let title = h.ctl("zoom-out").expect("page tools").at.1;
    assert!(title >= bar && title < bar + 40, "the phone opened scrolled: page tools at {title}, top bar ends {bar}");
}

/// js: KaTeX wraps an inline formula after a binary operator or relation when it is wider than the column;
/// the 390pt phone showed `9.2 / sqrt(40)` cut off at the right edge.
#[test]
fn round4_inline_math_wider_than_the_column_wraps_after_an_operator() {
    let tex = r"$\bar{x} \pm 1.96 \cdot \frac{s}{\sqrt{n}} = 21.25 \pm 1.96 \cdot \frac{9.2}{\sqrt{40}}$";
    let setup = |h: &mut H| {
        let p = h.sess().a.pages.get_mut("m1").unwrap();
        p.set("raw", serde_json::json!(format!("{tex}\nnext line\nthird")));
        p.set("corrected", serde_json::json!(""));
        p.set("corrections", serde_json::json!([]));
        h.frames(6);
    };
    let pitch = |h: &H| h.line_pos("line-m1-1").y - h.line_pos("line-m1-0").y;
    let mut wide = H::new("m1");
    wide.size = (2200.0, 900.0);
    wide.frames(3);
    setup(&mut wide);
    let one = pitch(&wide);
    let mut phone = H::new("m1");
    phone.size = (390.0, 844.0);
    phone.frames(3);
    setup(&mut phone);
    let many = pitch(&phone);
    assert!(many > one + 30.0, "one line {one}pt, phone {many}pt: the formula did not wrap");
    // the picture of each piece stays inside the 390pt screen
    let right = phone.out.shapes.iter().map(|s| s.shape.visual_bounding_rect().right()).fold(0.0f32, f32::max);
    assert!(right <= 390.5, "something is painted at x = {right}");
}

/// A browser sets text with unrounded advances and the font's GPOS kerning; egui rounded every advance to a
/// pixel and ignored GPOS, so labels came out up to 5% off and every button, chip and legend entry with them.
/// The widths are what Chrome reported for the same strings (button width minus padding and border).
#[test]
fn round4_text_is_as_wide_as_the_browser_sets_it() {
    let h = H::new("m1");
    let w = |txt: &str, f: egui::FontId| h.ctx.fonts(|fo| fo.layout_no_wrap(txt.to_string(), f, egui::Color32::BLACK).size().x);
    for (txt, f, js) in [
        ("Copy text", theme::sans_semi(14.0), 64.2),
        ("Done, next student", theme::sans_semi(14.0), 125.5),
        ("Saved in this browser", theme::sans(14.0), 135.4),
        ("Unit 5 written response", theme::sans_medium(14.0), 150.9),
        ("Maya R.", theme::sans_semi(14.0), 50.4),
        ("Text only", theme::sans_semi(14.0), 59.8),
    ] {
        let got = w(txt, f);
        assert!((got - js).abs() < 0.6, "{txt:?} is {got}, the browser sets it {js}");
    }
}

/// The transcript pane shows no scroll bar (the browser's headless Chrome shows none; egui's floating bar
/// appeared on the right edge in the light theme once the pane was hovered or scrolled).
#[test]
fn round4_transcript_pane_draws_no_scroll_bar() {
    let mut h = H::new("m1");
    h.act("tray-close");
    h.size = (1280.0, 400.0);
    h.frames(3);
    h.wheel(120.0);
    h.frames(5);
    let bars: Vec<_> = h
        .out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) if r.rect.width() <= 16.0 && r.rect.height() > 40.0 && r.rect.right() > 1230.0 && r.fill.a() > 0 => {
                Some(r.rect)
            }
            _ => None,
        })
        .collect();
    assert!(bars.is_empty(), "a scroll bar is painted: {bars:?}");
}

/// `.crop-img { border-radius: 6px; background: var(--paper); box-shadow: 0 1px 2px rgba(20,27,45,.25) }`
/// and the field's placeholder is Chrome's grey (117), not full ink.
#[test]
fn round4_crop_has_a_shadow_and_the_placeholder_is_grey() {
    let mut h = H::new("m1");
    h.with_picture();
    let w = h.word_id("c:0");
    h.act(&w);
    h.frames(6);
    let rects: Vec<_> = h
        .out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) => Some((r.rect, r.fill, r.rounding)),
            _ => None,
        })
        .collect();
    let paper = crate::theme::LIGHT.paper;
    let crop = rects.iter().find(|(r, f, _)| *f == paper && r.width() == 280.0).expect("crop background");
    assert_eq!(crop.2.nw, 6.0, "crop corners are rounded by 6");
    let shadow = rects.iter().find(|(r, f, _)| (f.a() == 60 || f.a() == 26) && (r.width() - 280.0).abs() < 3.0 && r.top() >= crop.0.top());
    assert!(shadow.is_some(), "no drop shadow under the crop");
    // the placeholder of the typed field: text galleys in the tray with the grey of `::placeholder`
    let mut greys = 0;
    for cs in &h.out.shapes {
        if let egui::epaint::Shape::Text(t) = &cs.shape {
            if t.galley.text() == "Type what it says" && t.override_text_color.is_none() {
                let c = t.galley.job.sections[0].format.color;
                if c == egui::Color32::from_rgb(117, 117, 117) {
                    greys += 1;
                }
            }
        }
    }
    assert_eq!(greys, 1, "placeholder is not the browser's grey");
}

/// Widths and heights Chrome reported for the same screen (js/ui at 1280 x 800 and 390 x 844, demo data).
#[test]
fn round4_metrics_match_the_browser() {
    let mut h = H::new("m1");
    h.act("tray-close");
    h.frames(3);
    let w = |id: &str| h.ctl(id).unwrap_or_else(|| panic!("no {id}")).size.0 as f32;
    // the ghost buttons under the transcript are 13px text with 16px padding and a 1px border
    assert!((w("text-view") - 128.0).abs() <= 1.0, "Show plain text is {} wide", w("text-view"));
    assert!((w("transcribe-again") - 194.0).abs() <= 1.0, "Transcribe again is {} wide", w("transcribe-again"));
    let (a, b) = (h.ctl("text-view").unwrap(), h.ctl("transcribe-again").unwrap());
    let gap = b.at.0 - (a.at.0 + a.size.0 as i32);
    assert!((gap - 16).abs() <= 1, "the buttons are {gap} apart, the row's gap is 16");
    // header buttons
    assert!((w("copy") - 98.0).abs() <= 1.0 && (w("done") - 160.0).abs() <= 1.0, "copy {} done {}", w("copy"), w("done"));
    let (nav, who_next) = (h.ctl("prev-student").unwrap().at.0, h.ctl("next-student").unwrap().at.0);
    assert_eq!(who_next - nav, 148, "previous and next are 4 + 96 + 4 + 44 apart");

    // the tray's typed row: the field takes what "Use this" (52 + 34) leaves of the 559pt column
    let mut h = H::new("m1");
    h.frames(3);
    assert!((h.ctl("use-typed").unwrap().size.0 as f32 - 86.0).abs() <= 1.0, "Use this is {}", h.ctl("use-typed").unwrap().size.0);
    assert!((h.ctl("typed-word").unwrap().size.0 as f32 - 465.0).abs() <= 1.0, "field is {}", h.ctl("typed-word").unwrap().size.0);
}

/// js: the 390pt top bar is 236.6 high (save state is one 19.6 line, rows 8 apart).
#[test]
fn round4_phone_top_bar_is_as_tall_as_the_browsers() {
    let mut h = H::new("m1");
    h.size = (390.0, 844.0);
    h.frames(5);
    let bottom = h.app.controls().iter().filter(|c| c.region == crate::twin::HEADER).map(|c| c.at.1 + c.size.1 as i32).max().unwrap();
    // js: Done and Settings end at 227.6, the bar's padding and border make 236.6
    assert!((bottom - 228).abs() <= 1, "header controls end at {bottom}");
    let split = h.ctl("layout-split").unwrap().at.1;
    assert!((split - 132).abs() <= 1, "layout toggle at {split} (js 131.6)");
}

#[test]
fn round5_flagged_words_read_in_place_inside_the_sentence() {
    let h = H::new("m1");
    for n in h.app.nodes() {
        let Node::Para { id, parts, .. } = n else { continue };
        if !id.starts_with("line-m1-") {
            continue;
        }
        // text after a word starts where the word was, so no part sits next to another text part and the
        // sentence has no hole where a word was cut out
        let mut flat = String::new();
        for p in parts {
            match p {
                Part::Text(t) => flat.push_str(t),
                Part::Math(t) => flat.push_str(t),
                Part::Control(c) => flat.push_str(&format!("[{}]", c.id)),
            }
        }
        assert!(!flat.contains("  "), "double space in {id}: {flat:?}");
        if let Some(ci) = parts.iter().position(|p| matches!(p, Part::Control(_))) {
            // a word is never the first or last thing unless the line starts or ends with it
            let before: String = parts[..ci].iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect();
            if id.ends_with("-0") {
                assert!(before.starts_with("The 95% "), "{id}: {parts:?}");
                assert!(matches!(&parts[ci + 1], Part::Text(t) if t.starts_with("interval") || t.starts_with(" interval")), "{parts:?}");
            }
        }
    }
}

#[test]
fn round5_tray_position_and_student_position_are_in_the_twin() {
    let mut h = H::new("m1");
    h.act(&h.word_id("c:0"));
    let n = h.s().open_entry().map(|(_, _, n)| n).unwrap();
    let t = h.node_text("tray-count").expect("tray count node");
    assert_eq!(t, format!("Word 1 of {n}"));
    let nodes = h.app.nodes();
    let at = |id: &str| nodes.iter().position(|x| x.id() == id).unwrap();
    // read between the previous and next buttons
    assert!(at("tray-count") > at("prev-word") && at("tray-count") < at("next-word"));
    h.act("next-word");
    assert_eq!(h.node_text("tray-count").unwrap(), format!("Word 2 of {n}"));
    assert!(h.node_text("student-pos").unwrap().contains(" of "), "student position");
}

#[test]
fn round5_phone_page_pane_tools_stay_on_one_row_like_the_browser() {
    let mut h = H::new("m1");
    h.size = (390.0, 844.0);
    h.frames(3);
    let ids = ["zoom-out", "zoom-fit", "zoom-in", "boxes", "level", "rotate", "replace", "remove"];
    let y = h.ctl("zoom-out").unwrap().at.1;
    for id in ids {
        let c = h.ctl(id).unwrap();
        assert_eq!(c.at.1, y, "{id} dropped to another row");
        assert!(c.at.0 >= 0 && c.at.0 + c.size.0 as i32 <= 390, "{id} off screen: {:?}", c.at);
    }
    // js: tools row 44, 4 padding + 1 border + 10 strip padding, then the first thumbnail
    let thumb = h.ctl("page-m1").unwrap().at.1;
    assert!((thumb - y - 59).abs() <= 1, "strip starts {} below the tools, js 59", thumb - y);
}
