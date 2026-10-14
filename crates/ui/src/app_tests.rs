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
            app.filter_tab(&mut input);
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
    assert!(h.ctl("cs-select").is_some() && h.ctl("cs-student-0").is_none(), "the list is shut until the select is opened");
    h.act("cs-select");
    assert!(h.ctl("cs-student-0").is_some());
    h.act("cs-student-0"); // Ana L.
    h.act("dialog-ok");
    assert_eq!(h.s().page("m1").unwrap().student_name(), Some("Ana L."));
    assert_eq!(h.s().page("m2").unwrap().student_name(), Some("Maya R."));
    assert!(h.sent.borrow().iter().any(|c| matches!(c, Cmd::SavePage { page, .. } if page.id() == "m1")));

    // a new student, all pages
    let mut h = H::new("m1");
    h.act("change-student");
    h.act("cs-select");
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
                Part::Math(t) | Part::Del(t) => flat.push_str(t),
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

// ---- phase 4: accessibility twin

#[test]
fn roles_are_the_right_ones_and_listitems_have_a_group() {
    let h = H::new("m1");
    let role = |id: &str| h.app.controls().iter().find(|c| c.id == id).map(|c| c.role);
    assert_eq!(role("done"), Some("button"));
    assert_eq!(role("page-p-maya-1").or(role("page-m1")), Some("listitem"));
    assert_eq!(role("alt-suggested"), Some("listitem"));
    assert_eq!(role("typed-word").or(Some("textbox")), Some("textbox"));
    // every listitem belongs to a named list, every group has a label
    for c in h.app.controls().into_iter().filter(|c| c.role == "listitem") {
        let g = crate::a11y::group_of(&c.id).unwrap_or_else(|| panic!("{} has no list", c.id));
        assert_eq!(g.1, "list", "{}", c.id);
    }
    // the screen has a banner, the two sections of main and the word panel
    let nodes: Vec<&Node> = h.app.nodes().iter().collect();
    for region in [crate::twin::HEADER, crate::twin::SCAN, crate::twin::TEXT, crate::twin::TRAY] {
        let list: Vec<&Node> = nodes.iter().copied().filter(|n| n.key().0 == region).collect();
        assert!(crate::a11y::landmark(region, &list).is_some(), "region {region} is not a landmark");
    }
}

#[test]
fn checkboxes_in_the_inbox_carry_their_state() {
    let mut h = H::new("m1");
    h.act("crumb");
    let boxes: Vec<_> = h.app.controls().into_iter().filter(|c| c.id.starts_with("sel-") && c.role == "checkbox").map(|c| (c.id.clone(), c.pressed)).collect();
    assert!(!boxes.is_empty(), "the inbox rows have select checkboxes");
    assert!(boxes.iter().all(|(_, p)| *p == Some(false)));
    // every control in the twin, wherever it is, is large enough to hit
    assert!(h.app.controls().iter().filter(|c| c.size.0 > 0).all(|c| c.size.0 >= 44 && c.size.1 >= 44));
}

#[test]
fn decisions_and_toasts_are_announced() {
    let mut h = H::new("m1");
    let before = h.app.announced().len();
    let sug = h.app.controls().into_iter().find(|c| c.id == "alt-suggested").map(|c| c.id.clone());
    let w = h.word_id("c:0");
    h.act(&w);
    if sug.is_some() || h.app.controls().iter().any(|c| c.id == "alt-suggested") {
        h.act("alt-suggested");
        let said = &h.app.announced()[before..];
        assert!(said.iter().any(|s| s.contains("suggested word")), "{said:?}");
    }
    h.act("copy");
    assert!(h.app.announced().iter().any(|s| s.contains("Copied Maya R.")), "{:?}", h.app.announced());
    // moving to another student says who it is, once the new screen has drawn
    let go = if h.app.controls().iter().any(|c| c.id == "prev-student" && !c.disabled) { "prev-student" } else { "next-student" };
    h.act(go);
    h.frames(2);
    assert!(h.app.announced().last().is_some_and(|s| s.contains("student")), "{:?}", h.app.announced());
}

#[test]
fn math_has_a_spoken_form() {
    let h = H::new("m1");
    let tex: Vec<String> = h
        .app
        .nodes()
        .iter()
        .filter_map(|n| match n {
            Node::Para { parts, .. } => Some(parts.iter().filter_map(|p| if let Part::Math(t) = p { Some(t.clone()) } else { None }).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(!tex.is_empty());
    for t in tex {
        let said = crate::a11y::tex_speech(&t);
        assert!(!said.is_empty() && !said.contains('\\') && !said.contains('{'), "{t} -> {said}");
    }
}

/// Blind-critic probes: what a screen reader says must keep the scope a sighted reader sees.
/// Each case names the spoken form that does not lose the structure of the formula.
#[test]
fn critic_math_speech_keeps_scope() {
    let cases: &[(&str, &[&str])] = &[
        // limits of a sum: "from i equals 1 to n", never "sub i equals 1 to the power of n"
        (r"\sum_{i=1}^{n} x_i", &["from", "to n"]),
        (r"\int_0^1 x\,dx", &["from 0 to 1"]),
        // a long exponent must say where it ends, or "x^{a+b} + c" reads as x^(a+b+c)
        (r"x^{a+b} + c", &["end"]),
        // a subscript with several tokens needs an end as well
        (r"a_{n+1} = 2", &["end"]),
        // absolute value is not "bar x bar"
        (r"|x - 3| < 2", &["absolute value"]),
    ];
    let mut bad = Vec::new();
    for (tex, wants) in cases {
        let said = crate::a11y::tex_speech(tex);
        if !wants.iter().all(|w| said.contains(w)) {
            bad.push(format!("{tex} -> {said:?} (wanted {wants:?})"));
        }
    }
    assert!(bad.is_empty(), "spoken math loses structure:\n{}", bad.join("\n"));
}

// ---- phase 4 a11y twin, round 2: the gaps the blind critic named

/// A control is on screen once it has focus: on a phone, in a short window and on a desktop where the
/// foot of the transcript is below the fold. The twin is `overflow: clip`, so the canvas has to scroll.
#[test]
fn a11y_focus_scrolls_the_control_into_view() {
    for (w, hgt) in [(390.0, 844.0), (640.0, 400.0), (320.0, 256.0), (1280.0, 520.0)] {
        let mut h = H::new("m1");
        h.size = (w, hgt);
        h.with_picture();
        h.frames(10);
        let ids: Vec<String> = h.app.controls().iter().filter(|c| !c.disabled && c.size.0 > 0).map(|c| c.id.clone()).collect();
        let mut off = vec![];
        for id in ids {
            h.focus(&id);
            h.frames(3);
            if let Some(c) = h.ctl(&id) {
                let (top, bottom) = (c.at.1, c.at.1 + c.size.1 as i32);
                if top < -1 || bottom > hgt as i32 + 1 {
                    off.push(format!("{id} {top}..{bottom}"));
                }
            }
        }
        assert!(off.is_empty(), "{w}x{hgt}: focus left off screen: {off:?}");
    }
}

/// The regions that take keys are laid over what is visible of them, not their whole (taller) extent.
#[test]
fn a11y_transcript_group_is_its_visible_part() {
    let mut h = H::new("m1");
    h.size = (320.0, 256.0);
    h.with_picture();
    h.frames(10);
    h.focus("transcript");
    h.frames(3);
    let g = h.app.nodes().iter().find_map(|n| match n {
        Node::Group { id, at, size, .. } if id == "transcript" => Some((*at, *size)),
        _ => None,
    });
    let (at, size) = g.expect("transcript group");
    assert!(at.1 >= 0 && at.1 + size.1 as i32 <= 257, "{at:?} {size:?}");
}

/// A Tab stop that is the transcript shows its ring when focus came by keyboard (web: `kbd:1` from
/// `:focus-visible`); a mouse click alone still shows none.
#[test]
fn a11y_transcript_ring_shows_after_tab() {
    let mut h = H::new("m1");
    h.with_picture();
    h.act("tray-close");
    h.frames(5);
    let wide = |h: &H| h.rings().iter().any(|r| r.width() > 150.0 && r.height() > 40.0);
    h.app.queue.lock().unwrap().push("kbd:1".into());
    h.focus("transcript");
    h.frames(2);
    assert!(wide(&h), "the transcript has a ring after a keyboard focus");
}

#[test]
fn a11y_page_image_zoom_and_meta_are_in_the_twin() {
    let mut h = H::new("m1");
    h.with_picture();
    h.frames(5);
    assert!(h.app.nodes().iter().any(|n| matches!(n, Node::Image { label, .. } if label == "The scanned handwritten page")));
    assert!(h.ctl("zoom-fit").unwrap().label.contains("100%"), "{:?}", h.ctl("zoom-fit").map(|c| c.label.clone()));
    let meta = h.node_text("student-meta").expect("meta line");
    assert!(meta.contains("page") && meta.contains("words"), "{meta}");
}

#[test]
fn a11y_strikethrough_is_a_deletion_in_the_twin() {
    let h = H::new("m1");
    let has = h.app.nodes().iter().any(|n| match n {
        Node::Para { parts, .. } => parts.iter().any(|p| matches!(p, Part::Del(t) if t.contains("probably"))),
        _ => false,
    });
    assert!(has, "~~probably~~ is a Del part");
}

/// After a decision the panel moved on by itself: the announcement says which word it is on now.
#[test]
fn a11y_decision_announces_the_next_word() {
    let mut h = H::new("m1");
    let w = h.word_id("c:0");
    h.act(&w);
    let before = h.app.announced().len();
    h.act("alt-suggested");
    let said = h.app.announced()[before..].join(" | ");
    assert!(said.contains("suggested word") && (said.contains("Next, word") || said.contains("All words checked")), "{said}");
    assert!(said.contains("Nothing is chosen for it yet") || said.contains("All words checked"), "{said}");
}

#[test]
fn a11y_reordering_a_page_is_announced() {
    let mut h = H::new("m1");
    h.focus("page-m1");
    h.twin_key("ArrowRight", true);
    let said = h.app.announced().join(" | ");
    assert!(said.contains("Moved to position 2 of 2"), "{said}");
}

#[test]
fn a11y_settings_dialog_starts_on_its_first_control() {
    let mut h = H::new("m1");
    h.act("settings");
    h.frames(3);
    assert_eq!(h.app.ctl.focus.as_deref(), Some("theme-auto"));
}

/// A window a few hundred points high has no room for a header that stays put: it is the top of the page.
#[test]
fn a11y_short_window_scrolls_the_header_with_the_page() {
    let mut h = H::new("m1");
    h.size = (1280.0, 256.0);
    h.frames(6);
    let done = h.ctl("done").expect("done");
    let tools = h.ctl("zoom-out").expect("page tools");
    assert!(tools.at.1 > done.at.1, "the page starts under the header, inside one scroll area");
    h.focus("remove");
    h.frames(3);
    let r = h.ctl("remove").unwrap();
    assert!(r.at.1 >= 0 && r.at.1 + r.size.1 as i32 <= 257, "{:?}", r.at);
}

// ---- phase 4 a11y twin, round 3: the native tree, announcements, title, scope in spoken math

impl H {
    /// A frame with AccessKit on (what a screen reader attaching does), and its tree update.
    fn ak(&mut self) -> Vec<(accesskit::NodeId, accesskit::Node)> {
        self.ctx.enable_accesskit();
        self.frames(2);
        self.out.platform_output.accesskit_update.clone().expect("an AccessKit update").nodes
    }
}

fn ak_find<'a>(nodes: &'a [(accesskit::NodeId, accesskit::Node)], role: accesskit::Role, name: &str) -> Option<&'a (accesskit::NodeId, accesskit::Node)> {
    nodes.iter().find(|(_, n)| n.role() == role && n.name() == Some(name))
}

/// The native window exposes the same structure as the web twin: landmarks, a level 1 heading, named
/// buttons, flagged words inline in the transcript, math with its spoken form. (Before this the Windows tree
/// was 50 unnamed custom nodes.)
#[test]
fn native_tree_names_what_the_screen_shows() {
    use accesskit::Role as R;
    let mut h = H::new("m1");
    let n = h.ak();
    let named = n.iter().filter(|(_, x)| x.name().is_some_and(|s| !s.is_empty())).count();
    assert!(named >= 20, "{named} named nodes");
    for (role, name) in [
        (R::Button, "Zoom in"),
        (R::Button, "Done, next student"),
        (R::Heading, "Maya R."),
        (R::Banner, ""),
        (R::Main, ""),
        (R::Complementary, "Word panel"),
        (R::Region, "Transcript"),
    ] {
        let hit = n.iter().any(|(_, x)| x.role() == role && (name.is_empty() || x.name() == Some(name)));
        assert!(hit, "no {role:?} {name:?}");
    }
    let (_, h1) = ak_find(&n, R::Heading, "Maya R.").unwrap();
    assert_eq!(h1.hierarchical_level(), Some(1));
    assert!(n.iter().any(|(_, x)| x.role() == R::Math && x.name() == Some("x bar") && x.value().is_some_and(|v| v.contains("bar{x}"))));
    assert!(n.iter().any(|(_, x)| x.role() == R::Button && x.name().is_some_and(|s| s.contains("confidance"))), "flagged words are buttons");
    let (_, tb) = ak_find(&n, R::TextInput, "Type what the word says").expect("typed word box");
    assert_eq!(tb.value(), Some(""));
    // the live region for announcements is there
    assert!(n.iter().any(|(_, x)| x.role() == R::Status && x.live() == Some(accesskit::Live::Polite)));
}

/// A screen reader presses a node with `Action::Default`: it must reach the widget.
#[test]
fn native_tree_activation_presses_the_control() {
    let mut h = H::new("m1");
    let n = h.ak();
    let (id, _) = ak_find(&n, accesskit::Role::Button, "Zoom in").expect("zoom in");
    let before = h.ctl("zoom-fit").unwrap().label.clone();
    h.events.push(egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
        action: accesskit::Action::Default,
        target: *id,
        data: None,
    }));
    h.frames(3);
    let after = h.ctl("zoom-fit").unwrap().label.clone();
    assert_ne!(before, after, "Zoom in from the accessibility tree changed the zoom");
}

/// The tree update must be one a platform adapter accepts: every node reachable from the root. (An update
/// with a stray node makes AccessKit panic in the window thread: the first version of the native tree did.)
#[test]
fn native_tree_updates_are_consistent_through_a_session() {
    let mut h = H::new("m1");
    h.ctx.enable_accesskit();
    h.frames(2);
    let upd = h.out.platform_output.accesskit_update.clone().expect("update");
    let mut tree = accesskit_consumer::Tree::new(upd, true);
    let feed = |h: &mut H, tree: &mut accesskit_consumer::Tree| {
        for _ in 0..3 {
            h.frames(1);
            let u = h.out.platform_output.accesskit_update.clone().expect("update");
            tree.update(u);
        }
    };
    let w = h.word_id("i:0");
    h.act(&w);
    feed(&mut h, &mut tree);
    h.app.queue.lock().unwrap().push("text:typed-word\u{1}forty".into());
    feed(&mut h, &mut tree);
    h.act("alt-marked");
    feed(&mut h, &mut tree);
    h.act("settings");
    feed(&mut h, &mut tree);
    h.act("theme-dark");
    feed(&mut h, &mut tree);
    h.app.queue.lock().unwrap().push("key:Escape".into());
    feed(&mut h, &mut tree);
    h.act("layout-text");
    feed(&mut h, &mut tree);
    h.size = (390.0, 844.0);
    feed(&mut h, &mut tree);
    let root = tree.state().root();
    assert!(root.children().count() >= 3, "the window has the banner, main and the panel");
}

/// Round 6 blocker: egui keeps a focus it was given for one frame even when nothing draws that widget (the
/// choice of the word the panel left); its update then named a focus node the tree did not contain and
/// accesskit_consumer asserted in the window thread (a UI Automation client attached, repeated Enter in the
/// word panel).
#[test]
fn native_focus_on_a_widget_nobody_drew_is_let_go_of() {
    let mut h = H::new("m1");
    h.ctx.enable_accesskit();
    h.frames(2);
    let upd = h.out.platform_output.accesskit_update.clone().expect("update");
    let mut tree = accesskit_consumer::Tree::new(upd, true);
    let w = h.word_id("i:0");
    h.act(&w);
    for round in 0..6 {
        // focus on a choice of the open word, then one that the next word does not have
        let id = if round % 2 == 0 {
            h.app.ctl.widgets.get("alt-written").or_else(|| h.app.ctl.widgets.values().next()).copied().unwrap_or(egui::Id::new("ghost"))
        } else {
            egui::Id::new(("ghost", round))
        };
        h.ctx.memory_mut(|m| m.request_focus(id));
        for _ in 0..3 {
            h.frames(1);
            let u = h.out.platform_output.accesskit_update.clone().expect("update");
            tree.update(u);
        }
        if let Some(c) = h.app.controls().iter().find(|c| c.id.starts_with("alt-")).map(|c| c.id.clone()) {
            h.app.queue.lock().unwrap().push(format!("act:{c}"));
        }
    }
    for _ in 0..3 {
        h.frames(1);
        let u = h.out.platform_output.accesskit_update.clone().expect("update");
        tree.update(u);
    }
}

/// Reading order of the tree is the visual order: banner, main (page image, transcript), word panel.
#[test]
fn native_tree_reading_order_and_groups() {
    use crate::tree::Role as R;
    let h = H::new("m1");
    let t = crate::tree::of(h.app.nodes());
    let roles: Vec<_> = t.iter().map(|n| n.role).collect();
    assert_eq!(roles, vec![R::Banner, R::Main, R::Complementary], "{roles:?}");
    let kids: Vec<_> = t[1].children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(kids, vec!["Page image", "Transcript"]);
    let zoom = t[1].find("grp-zoom").expect("zoom group");
    assert_eq!(zoom.children.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), vec!["zoom-out", "zoom-fit", "zoom-in"]);
    assert!(t[1].find("grp-pages").is_some_and(|g| g.role == R::List && g.children.iter().all(|c| c.role == R::ListItem)));
    let layout = t.iter().find_map(|n| n.find("layout-split")).expect("layout button");
    assert!(layout.checked.is_some());
}

#[test]
fn title_names_the_assignment() {
    let h = H::new("m1");
    assert!(h.app.title().ends_with(" | viSHion") && h.app.title().len() > " | viSHion".len(), "{:?}", h.app.title());
    let empty = App::new(egui::Context::default());
    assert_eq!(empty.title(), "viSHion");
}

#[test]
fn moved_to_a_name_with_a_full_stop_has_one() {
    assert_eq!(state::moved_pages(1, "Ben T."), "Moved 1 page to Ben T.");
    assert_eq!(state::moved_pages(2, "Ben"), "Moved 2 pages to Ben.");
}

/// A decision that carries the panel to a word on the other page says that the page changed.
#[test]
fn decision_across_pages_says_the_page() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0");
    h.act(&w);
    let before = h.app.announced().len();
    h.act("alt-marked");
    assert_eq!(h.s().selected.as_deref(), Some("m2"));
    let said = h.app.announced()[before..].join(" | ");
    assert!(said.contains("Now on page 2 of 2"), "{said}");
}

/// "Marked as checked." and who the next student is reach the live region as one sentence.
#[test]
fn done_says_it_in_one_utterance() {
    let mut h = H::new("a1");
    let before = h.app.announced().len();
    h.act("done");
    h.frames(3);
    let said: Vec<_> = h.app.announced()[before..].to_vec();
    assert!(said.iter().any(|s| s.starts_with("Marked as checked.") && s.contains("student")), "{said:?}");
    assert!(!said.iter().any(|s| s == "Marked as checked."), "{said:?}");
}

/// Typed text and Enter reach the app in one frame: the box the twin shows afterwards is empty, as on the
/// canvas (the twin used to keep "mean" and a screen reader read text that was not there).
#[test]
fn typed_word_box_is_empty_after_it_is_used_in_the_same_frame() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0");
    h.act(&w);
    {
        let mut q = h.app.queue.lock().unwrap();
        q.push("text:typed-word\u{1}mean".into());
        q.push("act:use-typed".into());
    }
    h.frames(1);
    h.frames(2);
    assert_eq!(h.s().page("m1").unwrap().resolved().get("i:0").and_then(|v| v.as_str()), Some("typed"));
    assert_eq!(h.ctl("typed-word").and_then(|c| c.value.clone()), Some(String::new()));
}

#[test]
fn spoken_math_says_which_part_belongs_to_which() {
    use crate::a11y::tex_speech as t;
    assert_eq!(t(r"\frac{\frac{1}{2}}{3}"), "the fraction the fraction 1 over 2 end fraction over 3 end fraction");
    assert_eq!(t(r"\frac{1}{\frac{2}{3}}"), "the fraction 1 over the fraction 2 over 3 end fraction end fraction");
    assert_eq!(t(r"\frac{1}{2}x"), "the fraction 1 over 2 end fraction x");
    assert_eq!(t(r"\frac{1}{2} + 3"), "the fraction 1 over 2 end fraction plus 3");
    assert_eq!(t(r"\begin{pmatrix}a&b\\c&d\end{pmatrix}"), "the 2 by 2 matrix, row 1: a, b; row 2: c, d; end matrix");
    assert_eq!(t(r"\begin{vmatrix} 1 & 2 \\ 3 & 4 \end{vmatrix} = -2"), "the 2 by 2 determinant, row 1: 1, 2; row 2: 3, 4; end determinant equals minus 2");
    assert_eq!(t(r"\binom{n}{k}"), "n choose k");
    assert_eq!(t(r"\binom{n}{k} x^k"), "the binomial n choose k end binomial x to the power of k");
    // truncated environments do not panic
    let _ = t(r"\begin{pmatrix} a & b \\");
    let _ = t(r"\begin{");
}

// ---- phase 4 a11y twin, round 4

fn tab_key(shift: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: Some(egui::Key::Tab),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers { shift, ..Default::default() },
    }
}

/// Native Tab walks the twin's reading order (header, tools, page, transcript, then the word panel), not the
/// order egui drew the widgets in (the docked panel first), and never stops on an unnamed egui widget.
#[test]
fn native_tab_follows_reading_order_and_visits_only_named_stops() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0");
    h.act(&w);
    let stops: Vec<String> = twin::tab_stops(h.app.nodes()).into_iter().map(String::from).collect();
    let iw = stops.iter().position(|s| s.starts_with("word-")).expect("a flagged word");
    let it = stops.iter().position(|s| s == "typed-word").expect("the panel's text box");
    assert!(iw < it, "the transcript comes before the word panel: {iw} {it}");
    h.app.prev_focus = None;
    let mut walked = vec![];
    for _ in 0..stops.len() {
        h.events.push(tab_key(false));
        h.frames(2);
        walked.push(h.app.prev_focus.clone().unwrap_or_default());
    }
    assert_eq!(walked, stops, "Tab visits each stop once, in reading order");
    // Shift+Tab goes back one
    h.events.push(tab_key(true));
    h.frames(2);
    assert_eq!(h.app.prev_focus.as_deref(), Some(stops[stops.len() - 2].as_str()));
    // every stop has a name
    let named: std::collections::HashSet<&str> = twin::controls(h.app.nodes()).iter().filter(|c| !c.label.is_empty()).map(|c| c.id.as_str()).collect();
    for s in &stops {
        let group = h.app.nodes().iter().any(|n| matches!(n, Node::Group { id, label, .. } if id == s && !label.is_empty()));
        assert!(named.contains(s.as_str()) || group, "unnamed stop {s}");
    }
}

/// The same words twice are two announcements: a live region only reads what changed, so the toast carries a
/// serial (the JS toast adds a node every time).
#[test]
fn repeating_an_action_announces_it_again() {
    let mut h = H::new("m1");
    let n = |h: &H| h.app.said_log.iter().filter(|s| s.starts_with("Copied")).count();
    h.act("copy");
    h.frames(3);
    assert_eq!(n(&h), 1, "{:?}", h.app.said_log);
    h.act("copy");
    h.frames(3);
    assert_eq!(n(&h), 2, "{:?}", h.app.said_log);
}

/// At 400% on a phone the wrapped header is taller than half the screen: it moves into the page rather than
/// being clipped in a panel (Settings was drawn under the page pane, WCAG 1.4.10). A desktop keeps its panel.
#[test]
fn tall_wrapped_header_is_not_clipped() {
    let mut h = H::new("m1");
    h.size = (320.0, 420.0);
    h.frames(6);
    assert!(h.app.ctl.header_inline, "header height {}", h.app.ctl.header_h);
    let g = h.ctl("settings").expect("settings");
    assert!(g.at.1 >= 0 && g.at.1 + g.size.1 as i32 <= 420, "Settings at {:?} is on the screen", g.at);
    let b = h.ctl("brand").expect("brand");
    assert!(b.at.1 < g.at.1, "the header reads top to bottom");
    h.size = (1280.0, 800.0);
    h.frames(6);
    assert!(!h.app.ctl.header_inline);
}

/// js puts a `title` on the "Which text to show" group while nothing has been corrected: the twin gives the
/// group and the dimmed button the same reason, and the native tree says it in the button's name.
#[test]
fn disabled_original_toggle_says_why() {
    let h = H::new("m1");
    let c = h.ctl("ver-original").expect("original toggle").clone();
    assert!(c.disabled, "nothing is corrected in the fixture");
    let why = a11y::disabled_reason("ver-original").unwrap();
    assert!(why.starts_with("Nothing has been corrected"));
    let t = crate::tree::of(h.app.nodes());
    let g = t.iter().find_map(|n| n.find("grp-version")).expect("version group");
    assert_eq!(g.description.as_deref(), Some(why));
    let b = g.find("ver-original").unwrap();
    assert_eq!(b.description.as_deref(), Some(why));
    assert!(native::platform_name(b, true).contains("Original, unavailable: Nothing has been corrected"));
}

// ---------------------------------------------------------------------------------------------------------
// Blind critic round 5.
// ---------------------------------------------------------------------------------------------------------

#[test]
fn round5_counts_and_save_state_are_not_live_regions() {
    let h = H::new("m1");
    let t = crate::tree::of(h.app.nodes());
    for id in ["student-status", "tray-count", "save-state"] {
        let n = t.iter().find_map(|n| n.find(id)).unwrap_or_else(|| panic!("{id} in the tree"));
        assert_ne!(n.role, crate::tree::Role::Status, "{id} is read in place, not announced on its own");
    }
}

#[test]
fn round5_arrows_and_page_keys_pan_the_zoomed_page() {
    let mut h = H::new("m1");
    h.with_picture();
    h.focus("stage");
    for _ in 0..4 {
        h.twin_key("+", false);
    }
    h.frames(6);
    let before = h.app.stage.off;
    h.twin_key("ArrowDown", false);
    h.frames(4);
    assert!(h.app.stage.off.y > before.y, "ArrowDown pans down: {:?} -> {:?}", before, h.app.stage.off);
    let y = h.app.stage.off.y;
    h.twin_key("PageDown", false);
    h.frames(4);
    assert!(h.app.stage.off.y > y + 100.0, "PageDown pans by most of the view");
    h.twin_key("ArrowRight", false);
    h.frames(4);
    assert!(h.app.stage.off.x > 0.0, "ArrowRight pans right");
    h.twin_key("Home", false);
    h.frames(4);
    assert_eq!(h.app.stage.off.y, 0.0, "Home goes to the top");
}

#[test]
fn round5_change_student_says_why_and_leads_to_the_text_box() {
    let mut h = H::new("m1");
    h.act("change-student");
    let n = h.app.announced().len();
    h.act("dialog-ok");
    let said = h.app.announced()[n..].join(" ");
    assert!(said.contains("Choose a student"), "{said:?}");
    assert_eq!(h.app.ctl.focus.as_deref(), Some("cs-select"), "focus goes to the select, as in js");
    let n = h.app.announced().len();
    h.act("cs-select");
    h.act("cs-new");
    h.frames(2);
    assert_eq!(h.app.ctl.focus.as_deref(), Some("cs-name"));
    assert!(h.app.announced()[n..].join(" ").contains("text box"), "{:?}", h.app.announced());
    // a new student with no name is refused the same way
    h.act("dialog-ok");
    assert_eq!(h.app.ctl.focus.as_deref(), Some("cs-name"));
}

#[test]
fn round5_next_word_says_which_word_and_the_page_when_it_changes() {
    let mut h = H::new("m1");
    let n = h.app.announced().len();
    h.act("next-word");
    let said = h.app.announced()[n..].join(" ");
    assert!(said.starts_with("Word 2 of "), "{said:?}");
    assert!(said.contains("spelling to check") || said.contains("could not be read"), "{said:?}");
    assert!(!said.contains("page 2"), "{said:?}");
    // walk until the page changes
    for _ in 0..4 {
        let n = h.app.announced().len();
        let was = h.s().selected.clone();
        h.act("next-word");
        if h.s().selected != was {
            assert!(h.app.announced()[n..].join(" ").contains("Now on page 2"), "{:?}", h.app.announced());
            return;
        }
    }
    panic!("the page never changed");
}

#[test]
fn round5_settings_reach_model_course_words_and_export() {
    let mut h = H::new("m1");
    h.act("settings");
    for id in ["st-provider", "st-ocr", "st-key", "st-model", "st-words", "st-fmt", "st-sep", "st-txt", "st-jmode", "st-orig", "st-clear-fixes", "dialog-x"] {
        assert!(h.ctl(id).is_some(), "{id} is in the Settings dialog");
    }
    // a select: the list opens under it, then an option is picked
    h.act("st-fmt");
    h.frames(2);
    h.act("st-fmt-docx");
    assert_eq!(h.s().settings.get("exportFormat").and_then(|v| v.as_str()), Some("docx"));
    assert!(h.app.announced().last().is_some_and(|s| s.contains("Combined file")));
    h.app.queue.lock().unwrap().push("text:st-words\u{1}alpha, beta".into());
    h.frames(2);
    h.act("dialog-close");
    let topic = h.s().a.meta.topic().to_string();
    assert_eq!(h.s().settings.terms(&topic), vec!["alpha".to_string(), "beta".to_string()]);
}

#[test]
fn round5_text_hint_and_stats_are_in_the_twin() {
    let h = H::new("m1");
    assert!(h.node_text("legend").is_some_and(|t| t.contains("Spelling to check")));
    assert!(h.node_text("foot-hint").is_some_and(|t| t.starts_with("Click a line")));
}

#[test]
fn round5_boxes_button_names_end_with_the_instruction_in_every_state() {
    let mut h = H::new("m1");
    for (mode, want) in [
        (state::BoxMode::Off, "Marks on the page: off. Click to change."),
        (state::BoxMode::Flagged, "Marks on the page: underlines. Click to change."),
        (state::BoxMode::All, "Marks on the page: underlines and a box on every line. Click to change."),
    ] {
        h.sess().boxes = mode;
        h.frames(2);
        assert_eq!(h.ctl("boxes").unwrap().label, want);
        let t = crate::tree::of(h.app.nodes());
        assert_eq!(t.iter().find_map(|n| n.find("boxes")).unwrap().name, want);
    }
}

// ---- round 6 ----

/// Text nodes are laid over the canvas where they are drawn (touch exploration, magnifiers and the highlight of
/// a screen reader follow the rectangle), not stacked at the top left.
#[test]
fn round6_text_nodes_have_the_rectangle_they_are_drawn_in() {
    let h = H::new("m1");
    let mut seen = 0;
    for n in h.app.nodes() {
        let (id, at, size) = match n {
            Node::Heading { id, at, size, .. } | Node::Text { id, at, size, .. } | Node::Status { id, at, size, .. } | Node::Para { id, at, size, .. } => (id, at, size),
            _ => continue,
        };
        if id == "toast" {
            continue;
        }
        seen += 1;
        assert!(size.0 > 0 && size.1 > 0, "{id} has no rectangle");
        assert!(at.0 >= 0 && at.0 + size.0 as i32 <= 1280 + 2, "{id} runs off the window: {at:?} {size:?}");
    }
    assert!(seen > 8, "headings, lines and counts: {seen}");
    let t = crate::tree::of(h.app.nodes());
    let name = t.iter().find_map(|n| n.find("student-name")).unwrap();
    let (x0, _, x1, _) = name.at.unwrap();
    let tx = t.iter().find_map(|n| n.find("transcript")).and_then(|n| n.at).unwrap();
    assert!(x0 >= tx.0 - 4 && x1 <= tx.2 + 4, "the name {x0}..{x1} is in the text pane {tx:?}");
    // the line is inside the transcript pane, and the word buttons inside the line's row
    let line_id = h.app.nodes().iter().find_map(|n| match n { Node::Para { id, .. } if id.starts_with("line-") => Some(id.clone()), _ => None }).unwrap();
    let line = t.iter().find_map(|n| n.find(&line_id)).unwrap();
    let l = line.at.unwrap();
    assert!(l.0 >= tx.0 - 4 && l.2 <= tx.2 + 8, "line {l:?} in pane {tx:?}");
}

/// Shift+Enter and Ctrl+Enter on a page thumbnail (a click that carries the modifiers) select pages as the
/// click with the same keys does.
#[test]
fn round6_modifier_clicks_from_the_twin_select_pages() {
    let mut h = H::new("m1");
    let pages: Vec<String> = h.app.controls().iter().filter(|c| c.id.starts_with("page-")).map(|c| c.id.clone()).collect();
    assert!(pages.len() >= 2, "{pages:?}");
    h.app.queue.lock().unwrap().push(format!("act:{}\u{1}s", pages[0]));
    h.frames(3);
    h.app.queue.lock().unwrap().push(format!("act:{}\u{1}s", pages[1]));
    h.frames(3);
    assert_eq!(h.s().multi.len(), 2, "{:?}", h.s().multi);
    assert!(h.app.controls().iter().any(|c| c.label == "Export 2 selected"));
    h.act("strip-clear");
    assert!(h.s().multi.is_empty());
    h.app.queue.lock().unwrap().push(format!("act:{}\u{1}c", pages[1]));
    h.frames(3);
    assert_eq!(h.s().multi.len(), 2, "ctrl toggles the page and keeps the one that was open: {:?}", h.s().multi);
}

/// The start and the end of a transcription are spoken, and a transcript replaced in one step as well.
#[test]
fn round6_transcribing_again_says_that_it_started_and_finished() {
    let mut h = H::new("m1");
    h.frames(2);
    let n0 = h.app.announced().len();
    h.sess().a.pages.get_mut("m1").unwrap().set("status", serde_json::json!("transcribing"));
    h.frames(2);
    assert!(h.app.announced()[n0..].iter().any(|s| s.starts_with("Reading this page")), "{:?}", &h.app.announced()[n0..]);
    let n1 = h.app.announced().len();
    {
        let p = h.sess().a.pages.get_mut("m1").unwrap();
        p.set("raw", serde_json::json!("Lesson 4: Quadratics\nx = 1"));
        p.set("status", serde_json::json!("done"));
    }
    h.frames(2);
    let said = h.app.announced()[n1..].join(" | ");
    assert!(said.contains("The page was transcribed"), "{said}");
    // a transcript that changes under a page already done
    let n2 = h.app.announced().len();
    h.sess().a.pages.get_mut("m1").unwrap().set("raw", serde_json::json!("Lesson 5: Roots\nx = 2"));
    h.frames(2);
    assert!(h.app.announced()[n2..].iter().any(|s| s.contains("transcribed")), "{:?}", &h.app.announced()[n2..]);
    let n3 = h.app.announced().len();
    {
        let p = h.sess().a.pages.get_mut("m1").unwrap();
        p.set("status", serde_json::json!("error"));
        p.set("error", serde_json::json!("The model did not answer."));
    }
    h.frames(2);
    assert!(h.app.announced()[n3..].iter().any(|s| s.contains("could not be transcribed") && s.contains("did not answer")));
}

/// A decision hands focus to the word the panel moved to, never to the same place in the next word's list of
/// choices (a second Enter there would pick another kind of answer unheard).
#[test]
fn round6_focus_after_a_decision_is_on_the_next_word_not_on_its_choices() {
    let mut h = H::new("m1");
    let w = h.word_id("i:0");
    h.act(&w);
    let alt = h.app.controls().iter().find(|c| c.id.starts_with("alt-")).expect("a choice").id.clone();
    h.act(&alt);
    let f = h.app.ctl.focused.as_ref().map(|f| f.0.clone()).unwrap_or_default();
    assert!(f.starts_with("word-"), "focus is on {f:?}, not on the word the panel moved to");
}

/// Round 7 blocker: a dialog opened from the keyboard (Enter on a focused button) with a UI Automation client
/// attached killed the window: the update named a focus the tree did not contain (accesskit_consumer tree.rs:35).
/// Feeds every update of the same key sequence to a consumer, as the platform adapter does.
#[test]
fn native_dialog_opened_from_the_keyboard_keeps_focus_in_the_tree() {
    for (opener, first) in [("settings", "theme-auto"), ("remove", "dialog-cancel"), ("change-student", "cs-select")] {
        let mut h = H::new("m1");
        h.ctx.enable_accesskit();
        h.frames(2);
        let upd = h.out.platform_output.accesskit_update.clone().expect("update");
        let mut tree = accesskit_consumer::Tree::new(upd, true);
        for _ in 0..80 {
            if h.app.prev_focus.as_deref() == Some(opener) {
                break;
            }
            h.events.push(tab_key(false));
            h.frames(2);
        }
        assert_eq!(h.app.prev_focus.as_deref(), Some(opener), "Tab reaches it");
        let mut feed = |h: &mut H, n: usize| {
            for _ in 0..n {
                h.frames(1);
                let u = h.out.platform_output.accesskit_update.clone().expect("update");
                tree.update(u);
            }
        };
        feed(&mut h, 2);
        for k in [egui::Key::Enter, egui::Key::Escape] {
            for pressed in [true, false] {
                h.events.push(egui::Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: Default::default() });
                feed(&mut h, 1);
            }
            feed(&mut h, 4);
            if k == egui::Key::Enter {
                assert!(h.app.dialog.is_some(), "{opener}: dialog open");
                assert!(h.ctl(first).is_some(), "{opener}: {first} present");
            }
        }
    }
}

/// Native, round 8: arrow keys inside a modal dialog stay inside it (egui looked for the nearest widget in that
/// direction among all it drew, so ArrowLeft/ArrowUp from the first control landed on an unnamed widget behind
/// the dialog), and a dialog opened with the mouse hands focus back to the button that opened it.
#[test]
fn native_arrow_keys_stay_in_the_dialog() {
    for opener in ["settings", "remove", "change-student"] {
        let mut h = H::new("m1");
        h.act(opener);
        h.frames(3);
        // egui's own focus (what AccessKit reports to the client) is a widget of the dialog, not one behind it
        let inside = |h: &H| {
            let f = h.ctx.memory(|m| m.focused());
            h.app.ctl.widgets.iter().any(|(id, w)| Some(*w) == f && h.ctl(id).is_some_and(|c| c.region == crate::twin::DIALOG))
        };
        for k in [egui::Key::ArrowLeft, egui::Key::ArrowUp, egui::Key::ArrowRight, egui::Key::ArrowDown, egui::Key::ArrowLeft] {
            for pressed in [true, false] {
                h.events.push(egui::Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: Default::default() });
                h.frames(1);
            }
            h.frames(3);
            assert!(inside(&h), "{opener}: {k:?} leaves focus in the dialog, got {:?}", h.ctx.memory(|m| m.focused()));
        }
    }
}

#[test]
fn native_dialog_opened_with_the_mouse_returns_focus_to_its_opener() {
    for opener in ["settings", "remove", "change-student"] {
        let mut h = H::new("m1");
        let c = h.ctl(opener).expect("opener").clone();
        h.click(egui::pos2(c.at.0 as f32 + c.size.0 as f32 / 2.0, c.at.1 as f32 + c.size.1 as f32 / 2.0));
        assert!(h.app.dialog.is_some(), "{opener}: dialog open");
        h.key(egui::Key::Escape);
        h.frames(3);
        assert!(h.app.dialog.is_none(), "{opener}: closed");
        assert_eq!(h.app.prev_focus.as_deref(), Some(opener), "{opener}: focus back on the opener");
    }
}

/// Native, round 9: Enter in the new-student name box keeps focus in the box (egui's single-line text box
/// surrenders it; a js input does not), and Escape on a plain control with nothing open keeps focus there.
#[test]
fn native_enter_in_the_name_box_keeps_focus() {
    let mut h = H::new("m1");
    h.act("change-student");
    h.act("cs-select");
    h.act("cs-new");
    h.frames(3);
    let name = *h.app.ctl.widgets.get("cs-name").expect("name box widget");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(name), "name box has egui focus");
    h.events.push(egui::Event::Text("Zed".into()));
    h.frames(2);
    for pressed in [true, false] {
        h.events.push(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
        h.frames(1);
    }
    h.frames(3);
    assert!(h.app.dialog.is_some(), "Enter does not close the dialog");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(name), "focus stays in the name box after Enter");
    assert_eq!(h.app.prev_focus.as_deref(), Some("cs-name"));
}

#[test]
fn native_escape_on_a_plain_control_keeps_focus() {
    let mut h = H::new("m1");
    for id in ["zoom-in", "settings"] {
        if h.ctl(id).is_none() {
            continue;
        }
        h.app.ctl.want_focus = Some(id.into());
        h.frames(3);
        assert_eq!(h.app.prev_focus.as_deref(), Some(id));
        let before = h.ctx.memory(|m| m.focused());
        assert!(before.is_some());
        for pressed in [true, false] {
            h.events.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
            h.frames(1);
        }
        h.frames(3);
        assert_eq!(h.ctx.memory(|m| m.focused()), before, "{id}: Escape leaves egui focus on it");
        assert_eq!(h.app.prev_focus.as_deref(), Some(id));
    }
    // and Escape still closes a dialog, from a control and from the name box
    h.act("change-student");
    h.act("cs-select");
    h.act("cs-new");
    h.frames(3);
    for pressed in [true, false] {
        h.events.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
        h.frames(1);
    }
    h.frames(3);
    assert!(h.app.dialog.is_none(), "Escape closes the dialog from the name box");
}

/// Native: the AccessKit tree is a tree. A node with two parents (or a cycle) shows up to a UI Automation client
/// as the same subtree many times over (FindAll returned ~4500 nodes with Settings open).
#[test]
fn native_tree_has_one_parent_per_node_and_no_cycles() {
    for opener in [None, Some("settings"), Some("change-student"), Some("remove")] {
        let mut h = H::new("m1");
        h.ctx.enable_accesskit();
        h.frames(2);
        if let Some(o) = opener {
            h.act(o);
        }
        h.frames(3);
        let upd = h.out.platform_output.accesskit_update.clone().expect("update");
        let mut tree = accesskit_consumer::Tree::new(upd, true);
        for _ in 0..3 {
            h.frames(1);
            tree.update(h.out.platform_output.accesskit_update.clone().expect("update"));
        }
        let state = tree.state();
        let mut parents: std::collections::HashMap<accesskit::NodeId, Vec<accesskit::NodeId>> = Default::default();
        let mut stack = vec![state.root().id()];
        let mut seen = std::collections::HashSet::new();
        let mut visits = 0usize;
        while let Some(id) = stack.pop() {
            visits += 1;
            assert!(visits < 20000, "{opener:?}: the tree walk does not end (cycle)");
            if !seen.insert(id) {
                continue;
            }
            let n = state.node_by_id(id).unwrap();
            for c in n.child_ids() {
                parents.entry(c).or_default().push(id);
                stack.push(c);
            }
        }
        let multi: Vec<_> = parents.iter().filter(|(_, p)| p.len() > 1).map(|(c, p)| (c.0, p.iter().map(|x| x.0).collect::<Vec<_>>())).collect();
        let describe = |id: u64| {
            let n = state.node_by_id(accesskit::NodeId(id)).unwrap();
            format!("{:?} {:?}", n.role(), n.name())
        };
        let shown: Vec<String> = multi
            .iter()
            .take(5)
            .map(|(c, p)| format!("{} <- {}", describe(*c), p.iter().map(|x| describe(*x)).collect::<Vec<_>>().join(" | ")))
            .collect();
        assert!(multi.is_empty(), "{opener:?}: nodes with several parents: {shown:?}");
    }
}

// ---- native round 10 (dialog stack, a long dialog's tab order, focus after a dialog's action)

fn esc_key() -> egui::Event {
    egui::Event::Key { key: egui::Key::Escape, physical_key: Some(egui::Key::Escape), pressed: true, repeat: false, modifiers: Default::default() }
}

fn focused(h: &H) -> Option<String> {
    h.app.ctl.focused.as_ref().map(|f| f.0.clone())
}

/// modal.js stacks the confirmation on Settings: it opens with focus on Cancel, Cancel and Escape return to
/// Settings with focus on "Clear remembered fixes", accepting keeps Settings open. Every update goes to a
/// consumer, as the platform adapter does (no focus the tree lacks, whatever the order of the nodes).
#[test]
fn native_clear_fixes_confirmation_stacks_on_settings() {
    for escape in [true, false] {
        let mut h = H::new("m1");
        h.ctx.enable_accesskit();
        h.sess().settings.set("reference", serde_json::json!([{"wrong": "intgral", "right": "integral"}]));
        h.act("settings");
        h.frames(3);
        let upd = h.out.platform_output.accesskit_update.clone().expect("update");
        let mut tree = accesskit_consumer::Tree::new(upd, true);
        let mut feed = |h: &mut H| {
            for _ in 0..3 {
                h.frames(1);
                tree.update(h.out.platform_output.accesskit_update.clone().expect("update"));
            }
        };
        h.act("st-clear-fixes");
        feed(&mut h);
        assert!(matches!(h.app.dialog, Some(dialog::Dialog::Confirm { .. })));
        assert_eq!(focused(&h).as_deref(), Some("dialog-cancel"), "the confirmation opens on Cancel");
        if escape {
            h.events.push(esc_key());
            h.frames(1);
        } else {
            h.act("dialog-cancel");
        }
        feed(&mut h);
        assert!(matches!(h.app.dialog, Some(dialog::Dialog::Settings { .. })), "back on Settings");
        assert_eq!(focused(&h).as_deref(), Some("st-clear-fixes"));
        h.act("st-clear-fixes");
        h.act("dialog-ok");
        feed(&mut h);
        assert!(matches!(h.app.dialog, Some(dialog::Dialog::Settings { .. })), "accepting keeps Settings open");
        assert_eq!(h.s().settings.get("reference").and_then(|v| v.as_array()).map(|a| a.len()), Some(0));
        assert_eq!(focused(&h).as_deref(), Some("dialog-close"));
        // closing Settings still goes back to the gear
        h.act("dialog-close");
        feed(&mut h);
        assert!(h.app.dialog.is_none());
        assert_eq!(focused(&h).as_deref(), Some("settings"));
    }
}

/// A long dialog is one lap of Tab with the footer button once, last: it is not sorted by where it is on the
/// screen against controls that scroll past it. Controls below the fold are not reported inside the window.
#[test]
fn native_settings_tab_order_is_drawing_order_and_clipped_controls_have_no_rect() {
    let mut h = H::new("m1");
    h.size = (1280.0, 520.0);
    h.act("settings");
    h.frames(3);
    let stops: Vec<String> = twin::tab_stops(h.app.nodes()).into_iter().map(String::from).collect();
    assert_eq!(stops.iter().filter(|s| *s == "dialog-close").count(), 1);
    assert_eq!(stops.last().map(String::as_str), Some("dialog-close"), "{stops:?}");
    let at = |id: &str| stops.iter().position(|s| s == id).unwrap_or_else(|| panic!("{id} is a stop"));
    let order = ["theme-auto", "st-ocr", "st-demo", "st-jmode", "st-words", "st-guess", "st-fmt", "st-orig"];
    for w in order.windows(2) {
        assert!(at(w[0]) < at(w[1]), "{} before {}", w[0], w[1]);
    }
    // shift+tab from the first stop goes round to the footer, and no stop repeats in a lap
    let mut seen = std::collections::HashSet::new();
    let mut cur = Some(stops[0].clone());
    for _ in 0..stops.len() {
        let next = twin::next_stop(h.app.nodes(), cur.as_deref(), false);
        assert!(seen.insert(next.clone().unwrap()), "{next:?} visited twice in one lap");
        cur = next;
    }
    // the window is 520 high: Keep untouched originals is far below it
    let r = h.ctl("st-orig").unwrap();
    assert!(r.size.0 == 0 || r.size.1 == 0 || (r.at.1 as f32) < 520.0, "st-orig reported at y={} size {:?}", r.at.1, r.size);
    let done = h.ctl("dialog-close").unwrap();
    assert!(done.size.1 > 0 && (done.at.1 as f32) < 520.0);
}

/// Focus is on a named control after the control that had it is gone (Review again accepted, the last page of
/// a student removed, Back to the overview), never on the window.
#[test]
fn native_focus_lands_on_a_control_when_the_focused_one_is_gone() {
    // Review again, accepted: the strip's button disappears with the selection
    let mut h = H::new("m1");
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    h.sess().multi_click("m2", false);
    h.frames(2);
    h.act("strip-review-again");
    assert_eq!(focused(&h).as_deref(), Some("dialog-cancel"));
    h.act("dialog-ok");
    h.frames(3);
    assert!(h.ctl("strip-review-again").is_none());
    assert!(focused(&h).is_some(), "focus is on a control after Review again");
    // Back to the overview from the crumb
    let mut h = H::new("m1");
    h.focus("crumb");
    h.act("crumb");
    h.frames(3);
    assert!(focused(&h).is_some(), "focus is on a control on the overview");
    // removing the pages of a student one after the other
    let mut h = H::new("m1");
    for _ in 0..2 {
        if h.ctl("remove").is_none() {
            break;
        }
        h.act("remove");
        if h.app.dialog.is_some() {
            h.act("dialog-ok");
        }
        h.frames(3);
        assert!(focused(&h).is_some(), "focus is on a control after a page was removed");
    }
}

/// A control that is unavailable while the work it started runs does not leave focus on the window.
#[test]
fn native_focus_survives_transcribe_again_on_an_untouched_page() {
    let mut h = H::new("m1");
    h.focus("transcribe-again");
    h.act("transcribe-again");
    h.sess().a.pages.get_mut("m1").unwrap().set("status", serde_json::json!("transcribing"));
    h.frames(4);
    assert!(focused(&h).is_some(), "focus is on a control while the page is read");
}

/// Round 11: Enter on a plain action button (no dialog) on the real window dropped the platform focus to the
/// Window element. Same key sequence as the window gets (Tab to the button, Enter down and up), every update
/// handed to a consumer; the focus of every update after the key is a node of the tree other than the root.
#[test]
fn native_focus_stays_on_a_control_after_enter_on_a_plain_action() {
    for (target, by_tab) in [("text-view", true), ("transcribe-again", true), ("text-view", false), ("transcribe-again", false)] {
        let mut h = H::new("m1");
        h.ctx.enable_accesskit();
        h.frames(2);
        let upd = h.out.platform_output.accesskit_update.clone().expect("update");
        let mut tree = accesskit_consumer::Tree::new(upd, true);
        if by_tab {
            for _ in 0..120 {
                if h.app.prev_focus.as_deref() == Some(target) {
                    break;
                }
                h.events.push(tab_key(false));
                h.frames(2);
            }
        } else {
            // a UI Automation client's SetFocus (what the critic script does before the key)
            let w = h.app.ctl.widgets[target];
            h.events.push(egui::Event::AccessKitActionRequest(accesskit::ActionRequest {
                action: accesskit::Action::Focus,
                target: accesskit::NodeId(w.value()),
                data: None,
            }));
            h.frames(3);
        }
        assert_eq!(h.app.prev_focus.as_deref(), Some(target), "Tab reaches it");
        let root = egui::accesskit_root_id();
        for pressed in [true, false] {
            h.events.push(egui::Event::Key { key: egui::Key::Enter, physical_key: Some(egui::Key::Enter), pressed, repeat: false, modifiers: Default::default() });
            for _ in 0..1 {
                h.frames(1);
                let u = h.out.platform_output.accesskit_update.clone().expect("update");
                tree.update(u);
            }
        }
        for i in 0..8 {
            h.frames(1);
            let u = h.out.platform_output.accesskit_update.clone().expect("update");
            let f = u.focus;
            tree.update(u);
            assert!(f.0 != root.value(), "{target}: frame {i} after Enter: platform focus is the window ({f:?})");
            assert!(tree.state().focus().is_some(), "{target}: focus is a node");
        }
    }
}

// ---- the transcript as DOM text (web): `Ctl::dom_text` makes the lines text for the page to lay out

fn para<'a>(nodes: &'a [Node], id: &str) -> &'a Node {
    nodes.iter().find(|n| matches!(n, Node::Para { id: i, .. } if i == id)).expect("line")
}

#[test]
fn dom_lines_carry_their_looks_and_words_their_text() {
    use crate::twin::look;
    let mut h = H::new("m1");
    h.app.ctl.dom_text = true;
    h.frames(3);
    let nodes = h.app.nodes();
    let lines: Vec<&Node> = nodes.iter().filter(|n| matches!(n, Node::Para { id, .. } if id.starts_with("line-"))).collect();
    assert!(lines.len() >= 3);
    for n in &lines {
        let Node::Para { parts, flags, vis, .. } = n else { unreachable!() };
        assert_eq!(parts.len(), flags.len(), "one look per part");
        assert!(vis.2 > 0 && vis.3 > 0, "the part of the screen the line may be seen in is known");
    }
    // the word is a button that shows the word, marked as a spelling to check
    let Node::Para { parts, flags, .. } = para(nodes, "line-m1-0") else { unreachable!() };
    let i = parts.iter().position(|p| matches!(p, Part::Control(c) if c.id.ends_with("-c:0"))).expect("word");
    let Part::Control(c) = &parts[i] else { unreachable!() };
    assert_eq!(c.shown.as_deref(), Some("confidance"));
    assert!(flags[i] & look::WARN != 0, "{:?}", flags[i]);
    // the egui layout is not used: native keeps its own (no looks)
    let n = H::new("m1");
    assert!(n.app.nodes().iter().all(|x| !matches!(x, Node::Para { flags, .. } if !flags.is_empty())));
}

#[test]
fn dom_line_takes_the_height_the_page_measured_and_the_rest_follows() {
    let mut h = H::new("m1");
    h.app.ctl.dom_text = true;
    h.frames(3);
    let y = |h: &H, id: &str| match para(h.app.nodes(), id) {
        Node::Para { at, size, .. } => (at.1, size.1),
        _ => unreachable!(),
    };
    let (y0, h0) = y(&h, "line-m1-0");
    let (y1, _) = y(&h, "line-m1-1");
    assert_eq!(y1, y0 + h0 as i32, "lines are contiguous");
    // a user stylesheet made the first line three times as tall
    h.app.ctl.dom_h.insert("line-m1-0".into(), 240.0);
    h.frames(3);
    let (y0b, h0b) = y(&h, "line-m1-0");
    let (y1b, _) = y(&h, "line-m1-1");
    assert_eq!((y0b, h0b), (y0, 240));
    assert_eq!(y1b, y0 + 240, "the next line starts under the taller one");
}

#[test]
fn dom_click_on_a_line_underlines_it_and_a_word_keeps_its_measured_rectangle() {
    let mut h = H::new("m1");
    h.app.ctl.dom_text = true;
    h.frames(3);
    h.app.queue.lock().unwrap().push("act:line-m1-1".into());
    h.frames(3);
    assert_eq!(h.s().read_line.get("m1"), Some(&1), "the click on the text moved the reading underline");
    assert!(h.app.nodes().iter().any(|n| matches!(n, Node::Para { id, reading: true, .. } if id == "line-m1-1")));
    // the page says where the word landed in its line; the control follows
    let w = h.word_id("c:0");
    h.app.ctl.dom_chips.insert(w.clone(), [100.0, 4.0, 150.0, 30.0]);
    h.frames(3);
    let c = h.app.controls().into_iter().find(|c| c.id == w).unwrap().clone();
    assert_eq!(c.size, (150, 30));
}

/// Blind critic, DOM transcript round 4: while a page is being read its text streams in; that text is painted by
/// the canvas only (`css_text` in the `transcribing` arm of `transcript_pane` registers no twin node), so it is
/// not DOM text on the web (no find, selection, user stylesheet) and a screen reader never gets it.
#[test]
fn critic4_streamed_transcript_is_a_node_of_the_twin() {
    let mut h = H::new("m1");
    h.app.ctl.dom_text = true;
    h.sess().a.pages.get_mut("m1").unwrap().set("status", serde_json::json!("transcribing"));
    h.sess().a.pages.get_mut("m1").unwrap().set("raw", serde_json::json!("The mean of the sample streams in here"));
    h.frames(4);
    let has = h.app.nodes().iter().any(|n| match n {
        Node::Text { text, .. } | Node::Status { text, .. } | Node::Heading { text, .. } => text.contains("streams in here"),
        Node::Para { parts, .. } => parts.iter().any(|p| matches!(p, Part::Text(t) if t.contains("streams in here"))),
        _ => false,
    });
    assert!(has, "the text being read is in the twin");
}

/// DOM transcript round 4: on the web the name, counts, legend, pill, stat and hint are DOM text of the pane, said
/// to the twin per node (class and where it may be seen), and egui paints nothing of them.
#[test]
fn round4_pane_text_is_dom_text_on_the_web() {
    let mut h = H::new("m1");
    h.app.ctl.dom_text = true;
    h.frames(4);
    let chrome = &h.app.last_chrome;
    for id in ["student-name", "student-meta", "legend", "student-status"] {
        let c = chrome.get(id).unwrap_or_else(|| panic!("{id} is DOM text: {:?}", chrome.keys().collect::<Vec<_>>()));
        assert!(c.1 .2 > 0 && c.1 .3 > 0, "{id} says where it may be seen: {c:?}");
    }
    assert!(chrome.get("student-status").is_some_and(|c| c.0.starts_with("pill ")));
    // the nodes are the same as ever (native tree, tests, a screen reader's headings)
    assert!(h.app.nodes().iter().any(|n| matches!(n, Node::Heading { id, level: 1, .. } if id == "student-name")));
}

#[test]
fn round4_streamed_text_is_dom_text_on_the_web() {
    let mut h = H::new("m1");
    h.app.ctl.dom_text = true;
    h.sess().a.pages.get_mut("m1").unwrap().set("status", serde_json::json!("transcribing"));
    h.sess().a.pages.get_mut("m1").unwrap().set("raw", serde_json::json!("The mean of the sample streams in here"));
    h.frames(4);
    let chrome = &h.app.last_chrome;
    assert!(chrome.get("stream").is_some_and(|c| c.0 == "stream"), "{:?}", chrome.keys().collect::<Vec<_>>());
    assert!(chrome.contains_key("stream-note"));
    assert!(h.app.nodes().iter().any(|n| matches!(n, Node::Text { id, text, .. } if id == "stream" && text.contains("streams in here"))));
}

// ---- a11y critic round 9: silent controls

#[test]
fn a11y_layout_and_panel_close_are_announced() {
    let mut h = H::new("m1");
    h.act("layout-text");
    assert!(h.app.announced().join(" | ").contains("text only"), "{:?}", h.app.announced());
    h.act("layout-split");
    assert!(h.app.announced().join(" | ").contains("side by side"), "{:?}", h.app.announced());
    let w = h.word_id("c:0");
    h.act(&w);
    let before = h.app.announced().len();
    h.act("tray-close");
    let said = h.app.announced()[before..].join(" | ");
    assert!(said.contains("Word panel closed"), "{said}");
}

#[test]
fn a11y_use_typed_with_nothing_typed_says_so_and_focuses_the_box() {
    let mut h = H::new("m1");
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("use-typed");
    let said = h.app.announced().join(" | ");
    assert!(said.contains("Type the word first"), "{said}");
    h.frames(3);
    assert_eq!(h.app.ctl.focus.as_deref(), Some("typed-word"));
}

#[test]
fn a11y_moving_the_underline_says_the_line() {
    let mut h = H::new("m1");
    h.focus("transcript");
    let before = h.app.announced().len();
    h.twin_key("ArrowDown", false);
    let said = h.app.announced()[before..].join(" | ");
    assert!(said.contains("Line 1 of"), "{said}");
}

#[test]
fn a11y_a_dimmed_control_says_why() {
    let mut h = H::new("m1");
    let zo = h.ctl("zoom-out").expect("zoom out");
    assert!(zo.disabled, "100% is the smallest zoom here");
    assert!(crate::a11y::disabled_reason("zoom-out").is_some());
    let before = h.app.announced().len();
    h.act("zoom-out");
    let said = h.app.announced()[before..].join(" | ");
    assert!(said.contains("smallest"), "{said}");
}

/// Round 11: rotate says the resulting orientation (not the same words every press), Home and End in the
/// transcript move the reading underline to the first and last line, and a dimmed control that has a reason
/// (Zoom out at fit) is a Tab stop natively as it is on the web.
#[test]
fn rotate_says_orientation_home_end_move_and_dimmed_zoom_out_is_a_stop() {
    let mut h = H::new("m1");
    h.act("rotate");
    h.frames(3);
    assert!(h.app.said_log.iter().any(|s| s.contains("90 degrees clockwise")), "{:?}", h.app.said_log);
    h.act("rotate");
    h.frames(3);
    assert!(h.app.said_log.iter().any(|s| s.contains("upside down")), "{:?}", h.app.said_log);
    h.focus("transcript");
    h.twin_key("End", false);
    let last = *h.s().read_line.get("m1").expect("End moved the underline");
    assert!(last > 0, "{last}");
    h.twin_key("Home", false);
    assert_eq!(h.s().read_line.get("m1"), Some(&0));
    assert!(h.ctl("zoom-out").unwrap().disabled);
    assert!(twin::tab_stops(h.app.nodes()).contains(&"zoom-out"));
}

#[test]
fn ctrl_z_takes_back_the_last_choice_and_says_so() {
    let mut h = H::new("m1");
    let w = h.word_id("c:0");
    h.act(&w);
    h.act("alt-suggested");
    assert!(h.s().page("m1").unwrap().corrected().contains("confidence"));
    h.app.queue.lock().unwrap().push("key:undo".into());
    h.frames(3);
    assert!(!h.s().page("m1").unwrap().corrected().contains("confidence"));
    assert_eq!(h.s().sel_word, Some(("m1".into(), "c:0".into())));
    assert!(h.app.announced().iter().any(|a| a.starts_with("Undid the last choice.")), "{:?}", h.app.announced());
    // nothing left: it says so rather than staying silent
    h.app.queue.lock().unwrap().push("key:undo".into());
    h.frames(3);
    assert!(h.app.announced().iter().any(|a| a.starts_with("Nothing to undo.")), "{:?}", h.app.announced());
}

/// Visual parity (h): with the transcript scrolled, accepting a suggestion (focus moves to the word) and
/// toggling Original leave it where it is; only the reading line moves it.
#[test]
fn parity_accept_and_original_do_not_move_a_scrolled_transcript() {
    let mut h = H::new("m1");
    h.with_picture();
    h.frames(10);
    h.size = (1280.0, 520.0);
    h.frames(5);
    h.wheel(160.0);
    h.frames(10);
    let y0 = blind_node_y(&h, "student-name").expect("name");
    h.act("alt-suggested");
    h.frames(30);
    assert_eq!(blind_node_y(&h, "student-name"), Some(y0), "accepting a suggestion moved the transcript");
    h.act("ver-original");
    h.frames(30);
    assert_eq!(blind_node_y(&h, "student-name"), Some(y0), "Original moved the transcript");
    h.act("ver-corrected");
    h.frames(30);
    assert_eq!(blind_node_y(&h, "student-name"), Some(y0), "Corrected moved the transcript");
}

/// Visual parity (i): `meta.ui.scroll` and `scrollId` are written by a scroll event of the pane only. Opening
/// another page after a decision (focus falls back to the transcript) scrolls nothing and writes nothing.
#[test]
fn parity_scroll_keys_are_only_written_by_a_scroll_event() {
    let mut h = H::new("m1");
    h.with_picture();
    h.frames(10);
    h.act("alt-suggested");
    h.frames(10);
    h.act("page-m2");
    h.frames(60);
    let ui = h.s().a.meta.get("ui").cloned().unwrap_or_default();
    assert!(ui.get("scrollId").is_none() && ui.get("scroll").is_none(), "written without a scroll event: {ui}");
}
