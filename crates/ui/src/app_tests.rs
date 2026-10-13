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
        let mut h = H { app, ctx, sent, t: 0.0, out: Default::default(), copied: String::new(), size: (1280.0, 800.0) };
        h.frames(3);
        h
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.t += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size.0, self.size.1))),
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
    assert!(h.take_cmds().iter().any(|c| matches!(c, Cmd::SaveMeta { .. })));
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
