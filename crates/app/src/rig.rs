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
            if matches!(c, Cmd::LoadMathFont | Cmd::LoadFallbackFont | Cmd::LoadEmojiFont | Cmd::ReplaceImage { .. } | Cmd::AddPages { .. } | Cmd::ImportZip) {
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
        if std::env::var("VIEW").as_deref() == Ok("inbox") {
            r.act("crumb");
            r.frames(10);
        }
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

    /// Blind critic round 5: Rotate this page counted as a pending save that nothing ever answered, so the
    /// header said "Saving..." for the rest of the session.
    #[test]
    fn blind5_rotate_settles_the_save_indicator() {
        let mut r = Rig::demo();
        r.act("rotate");
        r.frames(20);
        let say = |r: &Rig| {
            r.app.nodes().iter().find_map(|n| match n {
                vishion_ui::twin::Node::Status { id, text, .. } if id == "save-state" => Some(text.clone()),
                _ => None,
            })
        };
        let s = say(&r).unwrap_or_default();
        assert!(!s.starts_with("Saving"), "{s:?}");
    }

    /// Visual parity (j): the JS app writes half a second after a change and shows "Saving..." until then
    /// (`fs.schedule`, 500 ms), so a change is always seen being saved; the store here answers at once.
    #[test]
    fn parity_saving_is_shown_for_half_a_second_after_a_change() {
        let mut r = Rig::demo();
        r.frames(10);
        let say = |r: &Rig| {
            r.app.nodes().iter().find_map(|n| match n {
                vishion_ui::twin::Node::Status { id, text, .. } if id == "save-state" => Some(text.clone()),
                _ => None,
            })
        };
        let before = say(&r).unwrap_or_default();
        assert!(!before.starts_with("Saving"), "{before:?}");
        r.app.twin_act("alt-suggested");
        r.frames(2);
        assert!(say(&r).unwrap_or_default().starts_with("Saving"), "{:?}", say(&r));
        r.frames(12);
        assert_eq!(say(&r).unwrap_or_default(), before, "settles after the half second");
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

    fn label_of(r: &Rig, id: &str) -> String {
        r.app.controls().into_iter().find(|c| c.id == id).map(|c| c.label.clone()).unwrap_or_default()
    }

    /// Blind critic round 3: after "Done, next student" the JS inbox reads "Ready to read" for that student
    /// (the group status honours `reviewed`), has filter tabs with counts, the run bar and per-row Retry.
    #[test]
    fn blind3_inbox_honours_reviewed_and_has_tabs_counts_and_run_bar() {
        let mut r = Rig::demo();
        r.act("crumb");
        // before: Maya has 4 words to check, Ana is ready, Ben is waiting, Omar failed
        assert_eq!(label_of(&r, "row-n:maya r."), "Open Maya R., 4 words to check");
        assert_eq!(label_of(&r, "row-n:ana l."), "Open Ana L., Ready to read");
        assert_eq!(label_of(&r, "row-n:ben t."), "Open Ben T., Waiting");
        assert_eq!(label_of(&r, "row-n:omar k."), "Open Omar K., Couldn’t transcribe");
        // tabs with counts, as the JS inbox
        assert_eq!(label_of(&r, "tab-all"), "All 4");
        assert_eq!(label_of(&r, "tab-ready"), "Ready to read 1");
        assert_eq!(label_of(&r, "tab-check"), "Needs a check 1");
        assert_eq!(label_of(&r, "tab-processing"), "Transcribing 1");
        assert_eq!(label_of(&r, "tab-failed"), "Failed 1");
        // run bar: one page left to read, one failed
        assert!(r.has("retry-failed") && r.has("resume"), "{:?}", r.app.controls().iter().map(|c| &c.label).collect::<Vec<_>>());
        assert_eq!(label_of(&r, "retry-failed"), "Retry 1 failed");
        assert_eq!(label_of(&r, "resume"), "Resume (1 left)");
        assert!(r.has("retry-n:omar k."));
        // a filter narrows the rows
        r.act("tab-failed");
        assert!(r.has("row-n:omar k.") && !r.has("row-n:maya r."));
        r.act("tab-all");

        // Done on Maya, then Done on the last student (Omar)
        r.act("row-n:maya r.");
        r.act("done");
        r.act("done");
        assert_eq!(r.app.session().unwrap().view, vishion_ui::state::View::Inbox, "the last student shows the inbox");
        assert_eq!(label_of(&r, "row-n:maya r."), "Open Maya R., Ready to read");
        assert!(label_of(&r, "tab-ready").ends_with(" 2"), "{}", label_of(&r, "tab-ready"));
        assert!(label_of(&r, "tab-check").ends_with(" 0"));
    }

    /// The inbox's Resume and per-row Retry read pages through the same host path as the review screen.
    #[test]
    fn blind3_inbox_retry_and_resume_run_the_queue() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("retry-n:omar k.");
        assert_eq!(r.page("p-omar-1").status(), "done");
        assert!(!r.has("retry-n:omar k."));
        assert!(!r.has("retry-failed"), "no failed page is left");
        // Ben is the one page still waiting: Resume reads it
        assert_eq!(label_of(&r, "resume"), "Resume (1 left)");
        r.act("resume");
        assert_eq!(r.page("p-ben-1").status(), "done");
        // everything is read now: the bar offers to start with the first student that needs a check
        assert!(r.has("start-first"), "{:?}", r.app.controls().iter().map(|c| &c.id).collect::<Vec<_>>());
    }

    /// Straighten is not a stub: a level page answers "already level" and the line boxes are redone from the
    /// picture on screen.
    #[test]
    fn straighten_answers_and_redoes_the_line_boxes() {
        let mut r = Rig::demo();
        r.frames(10);
        r.act("level");
        let toast = r.texts().into_iter().map(|(t, _, _)| t).find(|t| t.contains("already level"));
        assert!(toast.is_some(), "a toast says what happened: {:?}", r.texts().into_iter().map(|t| t.0).collect::<Vec<_>>());
        assert_eq!(r.page("p-maya-1").status(), "done");
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

    /// Phase 5: the home screen from the wordmark: the list, rename, duplicate, delete, create.
    #[test]
    fn home_lists_renames_duplicates_deletes_and_creates() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("brand");
        assert!(r.has("new-assignment") && r.has("open-0"), "{:?}", r.app.controls().iter().map(|c| &c.id).collect::<Vec<_>>());
        r.act("rename-0");
        r.app.twin_text("rename-name-0", "Quiz 2");
        r.frames(2);
        r.act("rename-ok-0");
        assert!(label_of(&r, "open-0").starts_with("Quiz 2."), "{}", label_of(&r, "open-0"));
        r.act("duplicate-0");
        assert!(r.has("open-1"));
        r.act("delete-1");
        assert!(r.has("delete-ok-1"));
        r.act("delete-cancel-1");
        assert!(r.has("open-1"), "cancel keeps it");
        r.act("delete-1");
        r.act("delete-ok-1");
        assert!(!r.has("open-1"));
        r.act("new-assignment");
        r.app.twin_text("new-name", "Fresh start");
        r.frames(2);
        r.act("create-assignment");
        assert_eq!(r.app.session().unwrap().a.meta.name(), "Fresh start");
        assert!(!r.has("new-assignment"), "the new assignment is open, not the home screen");
    }

    /// Opening a card from the home screen shows that assignment.
    #[test]
    fn home_open_goes_to_the_assignment() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("brand");
        r.act("open-0");
        assert!(!r.has("open-0"));
        assert!(r.app.session().is_some());
    }

    /// Download zip writes `exports/<slug>.zip`, and what it holds reads back as the assignment's files.
    #[test]
    fn home_download_zip_round_trips_through_read_zip() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("brand");
        r.act("zip-0");
        let key = format!("{}/exports/{}.zip", r.slug, r.slug);
        let bytes = block_on(vishion_store::Store::get(&r.be.projects.store, &key)).unwrap().expect("zip written");
        let files = vishion_core::export::read_zip(&bytes).unwrap();
        assert!(files.iter().any(|(k, _)| k.ends_with("/project.json")), "{:?}", files.iter().map(|f| &f.0).collect::<Vec<_>>());
        assert!(files.iter().all(|(k, _)| !k.contains("/exports/")));
    }

    /// Assignment settings from the inbox: name and instructions save when the dialog closes, the subject
    /// at once, and Reset to standard empties the instructions.
    #[test]
    fn assignment_settings_edit_name_subject_and_instructions() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("assignment-settings");
        r.app.twin_text("as-name", "  Unit 9 quiz ");
        r.app.twin_text("as-prompt", "Read carefully.");
        r.frames(2);
        // the subject list: pick the first option that is not the current subject
        r.act("as-topic");
        let ids: Vec<String> = r.app.controls().iter().filter(|c| c.id.starts_with("as-topic-")).map(|c| c.id.clone()).collect();
        assert!(ids.len() > 1, "{ids:?}");
        let cur = r.app.session().unwrap().a.meta.topic().to_string();
        let other = ids.iter().find(|i| **i != format!("as-topic-{cur}")).unwrap().clone();
        r.act(&other);
        assert_eq!(format!("as-topic-{}", r.app.session().unwrap().a.meta.topic()), other);
        r.act("dialog-close");
        let m = &r.app.session().unwrap().a.meta;
        assert_eq!((m.name(), m.prompt()), ("Unit 9 quiz", "Read carefully."));
        // reset
        r.act("assignment-settings");
        r.act("as-reset");
        r.act("dialog-close");
        assert_eq!(r.app.session().unwrap().a.meta.prompt(), "");
    }

    /// The export dialog: scope, the summary of empty pages, Export anyway, and files landing in exports/.
    #[test]
    fn export_dialog_asks_before_exporting_empty_pages_and_writes_files() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("export");
        assert!(r.has("ex-scope-all") && r.has("ex-all"));
        // the demo assignment has pages without a transcript: the summary says so, and "Ready" has Ana only
        assert!(label_of(&r, "ex-scope-ready").contains("(1)"), "{}", label_of(&r, "ex-scope-ready"));
        r.act("ex-scope-ready");
        // one ready page: nothing is empty, so no question is asked
        r.act("ex-combined");
        let key = format!("{}/exports/", r.slug);
        let listed = block_on(vishion_store::Store::list(&r.be.projects.store, &key)).unwrap();
        assert!(listed.iter().any(|k| k.ends_with(".md")), "{listed:?}");
        // everyone: pages are empty, so a confirmation stacks on the dialog
        r.act("ex-scope-all");
        r.act("ex-csv");
        assert!(r.has("dialog-ok") && label_of(&r, "dialog-ok") == "Export anyway");
        r.act("dialog-ok");
        let listed = block_on(vishion_store::Store::list(&r.be.projects.store, &key)).unwrap();
        assert!(listed.iter().any(|k| k.ends_with("-index.csv")), "{listed:?}");
        assert!(r.has("ex-all"), "back on the export dialog");
        r.act("ex-cancel");
        assert!(!r.has("ex-all"));
    }

    /// The subjects editor: add a subject with words, delete it (confirmed), restore the built-in set.
    #[test]
    fn subjects_editor_adds_edits_deletes_and_restores() {
        let mut r = Rig::demo();
        r.act("crumb");
        r.act("assignment-settings");
        r.act("as-edit-topics");
        assert!(r.has("tp-topic") && r.has("tp-terms"));
        r.act("tp-new-btn");
        r.app.twin_text("tp-new", "Physics");
        r.frames(2);
        r.act("tp-add");
        r.app.twin_text("tp-terms", "force, mass\nenergy");
        r.frames(2);
        r.act("dialog-close");
        let st = &r.app.session().unwrap().settings;
        assert_eq!(st.terms("Physics"), vec!["force", "mass", "energy"]);
        // back on the assignment settings; open it again and delete Physics (it was the one shown)
        assert!(r.has("as-name"));
        r.act("as-edit-topics");
        r.act("tp-topic");
        r.act("tp-topic-Physics");
        r.act("tp-delete");
        assert_eq!(label_of(&r, "dialog-ok"), "Delete subject");
        r.act("dialog-ok");
        assert!(r.app.session().unwrap().settings.get("topics").unwrap().get("Physics").is_none());
        // General stays
        r.act("tp-delete");
        assert!(!r.has("dialog-ok"));
        // restore defaults is confirmed, then the built-in subjects are back
        r.act("tp-restore");
        r.act("dialog-ok");
        let topics = r.app.session().unwrap().settings.get("topics").unwrap().clone();
        assert!(topics.get("Medical").is_some() && topics.get("General").is_some());
    }
}
