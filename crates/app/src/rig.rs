//! Test rig: the real screen over the real `Backend` (demo store), no window. Commands the UI sends are
//! answered synchronously through `block_on`, exactly as the native worker thread answers them, so a test
//! drives "click, host answers, screen updates" end to end. Used by the review-screen parity tests.
use crate::backend::{block_on, Backend};
use eframe::egui;
use std::rc::Rc;
use vishion_ui::{App, Cmd, Inbox};

pub struct Rig {
    pub app: App,
    pub ctx: egui::Context,
    pub out: egui::FullOutput,
    pub t: f64,
    pub size: (f32, f32),
    pub be: Rc<Backend>,
    pub slug: String,
}

impl Rig {
    /// The demo assignment (maya, ana, ben, omar), `theme` "light" or "dark".
    pub fn demo() -> Rig {
        Self::sized(1280.0, 800.0)
    }

    pub fn sized(w: f32, h: f32) -> Rig {
        let (store, slug) = block_on(crate::demo::seed(1_700_000_000_000)).expect("seed");
        let be = Rc::new(Backend::new_demo(store, "Demo assignment (not saved)"));
        let ctx = egui::Context::default();
        let inbox = Inbox::new(ctx.clone());
        let (b2, i2) = (be.clone(), inbox.clone());
        let dispatch: vishion_ui::Dispatch = Box::new(move |c: Cmd| {
            // picture dialogs and font fetches are not part of the rig
            if matches!(c, Cmd::LoadMathFont | Cmd::LoadFallbackFont | Cmd::ReplaceImage { .. } | Cmd::AddPages { .. }) {
                return;
            }
            i2.push(block_on(b2.handle(c, 1_700_000_000_000)));
        });
        let app = App::new(ctx.clone()).with_host(dispatch, inbox.clone()).theme("light");
        inbox.push(block_on(be.open(None, 1_700_000_000_000)));
        let mut r = Rig { app, ctx, out: Default::default(), t: 0.0, size: (w, h), be, slug };
        r.frames(8);
        r
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.t += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.size.0, self.size.1))),
                time: Some(self.t),
                ..Default::default()
            };
            let app = &mut self.app;
            self.out = self.ctx.run(input, |c| app.run_frame(c));
        }
    }

    pub fn act(&mut self, id: &str) {
        assert!(self.app.controls().iter().any(|c| c.id == id), "no control {id}: {:?}", self.app.controls().iter().map(|c| &c.id).collect::<Vec<_>>());
        self.app.twin_act(id);
        self.frames(6);
    }

    pub fn has(&self, id: &str) -> bool {
        self.app.controls().iter().any(|c| c.id == id)
    }

    /// Every text drawn this frame with its top-left corner, in paint order.
    pub fn texts(&self) -> Vec<(String, f32, f32)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, f32, f32)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos.x, t.pos.y)),
                egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        for cs in &self.out.shapes {
            walk(&cs.shape, &mut out);
        }
        out
    }

    pub fn y_of(&self, text: &str) -> Option<f32> {
        self.texts().into_iter().find(|(t, _, _)| t == text).map(|(_, _, y)| y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints where everything sits on the demo's first screen (run with `--nocapture --ignored`).
    #[test]
    #[ignore]
    fn dump_layout() {
        let w: f32 = std::env::var("W").ok().and_then(|v| v.parse().ok()).unwrap_or(1280.0);
        let h: f32 = std::env::var("H").ok().and_then(|v| v.parse().ok()).unwrap_or(800.0);
        let mut r = Rig::sized(w, h);
        r.frames(20);
        for (t, x, y) in r.texts() {
            println!("{y:7.1} {x:7.1}  {t:?}");
        }
        for c in r.app.controls() {
            println!("ctl {} at={:?} size={:?}", c.id, c.at, c.size);
        }
    }

    impl Rig {
        /// Through the assignment overview to a student's first page, as a click path.
        fn open_student(&mut self, key: &str) {
            if self.app.session().unwrap().view == vishion_ui::state::View::Review {
                self.act("crumb");
            }
            self.act(&format!("row-{key}"));
        }

        fn page(&self, id: &str) -> vishion_core::model::Page {
            self.app.session().unwrap().page(id).unwrap().clone()
        }
    }

    /// Blind critic round 3: "Retry this page" and "Transcribe this page" did nothing (the host answered
    /// "needs the OCR connection"). In demo mode the JS app reads sample text, runs the judge and ends with
    /// status done and corrections.
    #[test]
    fn blind3_retry_on_an_error_page_reads_it_in_demo_mode() {
        let mut r = Rig::demo();
        r.open_student("n:omar k.");
        assert_eq!(r.page("p-omar-1").status(), "error");
        assert!(r.has("retry-page"));
        r.act("retry-page");
        let p = r.page("p-omar-1");
        assert_eq!(p.status(), "done");
        assert!(p.raw().starts_with("Lesson 4: Quadratics"), "{}", p.raw());
        assert!(p.s("error").is_empty());
        assert!(!p.corrections().is_empty(), "the judge ran: {:?}", p.corrections());
        assert!(p.get("guesses").and_then(|g| g.get("i:0")).is_some(), "demo guesses for the unreadable word");
        assert!(!r.has("retry-page"), "the error block is gone");
        assert!(r.has("transcribe-again"));
        // stored, with the stats of the assignment following
        let vishion_ui::Event::Opened { assignment, .. } = block_on(r.be.open(Some(&r.slug), 5)) else { panic!("reopen") };
        assert_eq!(assignment.page("p-omar-1").unwrap().status(), "done");
        assert_eq!(assignment.meta.get("stats").unwrap()["done"], 4);
    }

    #[test]
    fn blind3_transcribe_this_page_reads_a_pending_page() {
        let mut r = Rig::demo();
        r.open_student("n:ben t.");
        assert_eq!(r.page("p-ben-1").status(), "pending");
        assert!(r.has("transcribe-page"));
        r.act("transcribe-page");
        let p = r.page("p-ben-1");
        assert_eq!(p.status(), "done");
        assert!(p.raw().starts_with("Lesson 4: Quadratics"));
        assert!(p.get("timings").unwrap()["tokens"].as_u64().unwrap() > 10);
        // and again: one more read replaces it (the second sample is only for the second page of a run)
        r.act("transcribe-again");
        assert_eq!(r.page("p-ben-1").status(), "done");
    }

    /// The review screen opens unscrolled at 1280 x 800, as the JS app does: the name and the status pill
    /// are on screen and the word being decided sits just above the tray.
    #[test]
    fn blind3_demo_opens_unscrolled_with_name_pill_and_word_in_view() {
        let mut r = Rig::demo();
        r.frames(20);
        let name = r.texts().into_iter().find(|(t, _, y)| t == "Maya R." && *y > 100.0).map(|(_, _, y)| y.round());
        // the 32px line box starts at 141 (109 + 32, as in js); its glyph row is centred in it
        assert!(matches!(name, Some(y) if (137.0..=142.0).contains(&y)), "heading at {name:?}: scrolled or off the js rhythm");
        assert!(r.y_of("4 words to check").is_some_and(|y| y < 160.0));
        let word = r.app.controls().into_iter().find(|c| c.id.contains("c:0")).unwrap().at.1;
        let tray = r.y_of("Check this word").unwrap();
        assert!(word as f32 + 44.0 <= tray + 30.0, "word target at {word}, tray heading at {tray}");
    }
}
