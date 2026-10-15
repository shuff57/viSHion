//! Review-screen state and the flows around it (js/ui/review.js + header.js + words.js glue). Pure Rust over
//! `vishion-core`: no egui, no I/O, so every rule here is unit-tested natively and runs the same on wasm.
//!
//! Persistence is by dirty flags: a flow marks what changed, the app drains `take_dirty()` after the frame
//! and sends one save per changed document to the host.
use std::collections::{BTreeSet, HashMap};
use serde_json::{json, Value};
use vishion_core::export::{self, Opts};
use vishion_core::groups::{self, Group, Status};
use vishion_core::model::{Assignment, Page, Settings};
use vishion_core::words::{self, Item};
use vishion_core::{jsx, render};

pub use crate::bands::Band;

/// What the page overlay shows: nothing, the lines that hold a flagged word, or every line as well.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoxMode {
    Off,
    Flagged,
    All,
}

impl BoxMode {
    pub fn next(self) -> BoxMode {
        match self {
            BoxMode::Off => BoxMode::Flagged,
            BoxMode::Flagged => BoxMode::All,
            BoxMode::All => BoxMode::Off,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Warn,
    Bad,
    Ok,
}

/// The look of a status pill (`.pill.*` in css/base.css).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PillKind {
    /// Words to check: the yellow pill.
    Warn,
    Ok,
    Bad,
    /// Transcribing: a neutral filled pill.
    Busy,
    /// Waiting for its turn: neutral and outlined, not a problem state.
    Queued,
}

/// The inbox filter tabs (`G.matches`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    All,
    Ready,
    Check,
    Processing,
    Failed,
}

impl Filter {
    pub const ALL: [Filter; 5] = [Filter::All, Filter::Ready, Filter::Check, Filter::Processing, Filter::Failed];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Ready => "Ready to read",
            Filter::Check => "Needs a check",
            Filter::Processing => "Transcribing",
            Filter::Failed => "Failed",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Ready => "ready",
            Filter::Check => "check",
            Filter::Processing => "processing",
            Filter::Failed => "failed",
        }
    }

    /// `G.matches`: "processing" covers waiting and queued students too.
    pub fn matches(self, st: Status) -> bool {
        match self {
            Filter::All => true,
            Filter::Ready => st == Status::Ready,
            Filter::Check => st == Status::Check,
            Filter::Failed => st == Status::Failed,
            Filter::Processing => matches!(st, Status::Processing | Status::Queued | Status::Waiting),
        }
    }
}

/// `G.tally`: counts for the segmented bar and the tabs.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tally {
    pub total: usize,
    pub ready: usize,
    pub check: usize,
    pub failed: usize,
    pub processing: usize,
}

/// One row of the tray: a flagged word of the current student, on some page.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub page: String,
    pub item: Item,
}

/// One choice offered for the open word.
#[derive(Clone, Debug, PartialEq)]
pub struct Alt {
    pub label: String,
    pub hint: String,
    pub via: &'static str,
    /// Control id in the twin: unique per option (every model guess has `via` "typed").
    pub id: String,
    /// Text handed to `choose` (typed guesses only).
    pub text: String,
    pub on: bool,
}

#[derive(Default, Debug)]
pub struct Dirty {
    pub pages: BTreeSet<String>,
    pub meta: bool,
    pub settings: bool,
    /// Pages taken out of the assignment: the host deletes their file and image.
    pub removed: Vec<String>,
    /// Pages whose image must be turned a quarter turn (the page JSON is already updated).
    pub rotated: Vec<String>,
}

impl Dirty {
    pub fn any(&self) -> bool {
        !self.pages.is_empty() || self.meta || self.settings || !self.removed.is_empty() || !self.rotated.is_empty()
    }
}

/// Zoom steps of the page pane (js/ui/review.js `ZOOMS`).
pub const ZOOMS: [f32; 5] = [1.0, 1.5, 2.0, 3.0, 4.0];

/// Which screen is showing. The inbox is the assignment overview the header crumb returns to; the full
/// inbox arrives with phase 5.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Review,
    Inbox,
    /// js/ui/add.js: the staging step in front of adding pages.
    Add,
    /// `design/Phone.dc.html`: the camera.
    Capture,
}

/// The camera screen's state.
#[derive(Debug, Default)]
pub struct CaptureState {
    /// The latest frame, kept for the shutter.
    pub frame: Option<(u32, u32, Vec<u8>)>,
    pub found: Option<vishion_core::capture::Found>,
    pub steady: vishion_core::capture::Steady,
    /// Take the photo by itself once the page holds still.
    pub auto: bool,
    /// Photos taken in this visit.
    pub shots: usize,
    pub error: Option<String>,
    /// Frames to ignore after a shot, so one page is not taken twice.
    pub cooldown: u32,
}

/// One picture waiting in the Add screen.
#[derive(Clone, Debug, PartialEq)]
pub struct StagedRow {
    pub key: u64,
    pub item: crate::host::StagedItem,
    /// The "who is this" chooser is open for this row.
    pub picking: bool,
    /// What is typed in its name box.
    pub draft: String,
}

/// The Add screen's state (`staged`, `skipped`, `nameDown`, `preset` of js/ui/add.js).
#[derive(Default, Debug)]
pub struct AddState {
    pub rows: Vec<StagedRow>,
    pub skipped: Vec<(String, String)>,
    pub names_down: bool,
    /// Pictures are being prepared and read for names.
    pub working: bool,
    /// A student chosen beforehand ("Add pages for Maya").
    pub preset: Option<String>,
    pub seq: u64,
    /// The pages just committed are to be read when the host answers.
    pub starting: bool,
}

pub struct Session {
    pub slug: String,
    pub a: Assignment,
    pub settings: Settings,
    pub selected: Option<String>,
    /// The word open in the tray: (page id, item key).
    pub sel_word: Option<(String, String)>,
    pub original: bool,
    pub split: bool,
    pub boxes: BoxMode,
    pub tray_open: bool,
    tray_group: Option<String>,
    /// Transcript line being read, per page (the underline that follows the reader).
    pub read_line: HashMap<String, usize>,
    pub dirty: Dirty,
    pub now: i64,
    pub view: View,
    pub add: AddState,
    /// The result line under Settings' Test connection.
    pub conn_test: Option<(bool, String)>,
    /// What Find models found: (id, label, takes images), and the note under the model picker.
    pub found_models: Vec<(String, String, Option<bool>)>,
    pub models_note: String,
    pub capture: CaptureState,
    /// `S.d.textView === 'raw'`: the transcript as plain monospace text.
    pub text_raw: bool,
    /// Zoom of the page pane, one of `ZOOMS`.
    pub zoom: f32,
    /// Pages ticked in the strip (ctrl/shift click).
    pub multi: BTreeSet<String>,
    /// The inbox filter tab.
    pub filter: Filter,
    /// Where the transcript pane should scroll to now (`meta.ui.scroll` when it belongs to the selected
    /// page, else the top). The counter changes with every request so the screen applies each once.
    pub scroll_req: Option<(u32, f32)>,
    /// The last word decision, so Ctrl+Z can take it back: (page id, the page before, the settings before, the word).
    pub undo: Option<(String, Page, Settings, String)>,
}

fn ui_str<'a>(a: &'a Assignment, k: &str) -> Option<&'a str> {
    a.meta.get("ui").and_then(|u| u.get(k)).and_then(Value::as_str)
}

pub fn shown_str(it: &Item) -> String {
    match &it.shown {
        Value::String(s) => s.clone(),
        Value::Null => it.base.clone().unwrap_or_default(),
        v => jsx::to_string(v),
    }
}

fn word_str(it: &Item) -> String {
    it.word.as_ref().map(jsx::to_string).unwrap_or_default()
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// "Moved 1 page to Ben T." A name that ends in a full stop is not given a second one.
pub fn moved_pages(n: usize, name: &str) -> String {
    let stop = if name.ends_with('.') { "" } else { "." };
    format!("Moved {} to {name}{stop}", plural(n, "page", "pages"))
}

impl Session {
    pub fn new(slug: String, a: Assignment, settings: Settings, now: i64) -> Session {
        let layout_text = ui_str(&a, "layout") == Some("text");
        let selected = a
            .meta
            .get("ui")
            .and_then(|u| u.get("selectedId"))
            .and_then(Value::as_str)
            .filter(|id| a.pages.contains_key(*id))
            .map(String::from)
            .or_else(|| a.meta.order().into_iter().find(|id| a.pages.contains_key(id)));
        let mut s = Session {
            slug,
            a,
            settings,
            selected,
            sel_word: None,
            original: false,
            split: !layout_text,
            boxes: BoxMode::Flagged,
            tray_open: true,
            tray_group: None,
            read_line: HashMap::new(),
            dirty: Dirty::default(),
            now,
            view: View::Review,
            add: AddState::default(),
            conn_test: None,
            found_models: vec![],
            models_note: String::new(),
            capture: CaptureState { auto: true, ..Default::default() },
            text_raw: false,
            zoom: 1.0,
            multi: BTreeSet::new(),
            filter: Filter::All,
            scroll_req: None,
            undo: None,
        };
        s.request_scroll();
        s.sync_tray();
        s
    }

    // ------------------------------------------------------------------ reads

    pub fn pages(&self) -> Vec<&Page> {
        self.a.ordered()
    }

    pub fn groups(&self) -> Vec<Group> {
        groups::list(&self.pages(), &|_| false)
    }

    pub fn page(&self, id: &str) -> Option<&Page> {
        self.a.page(id)
    }

    pub fn current(&self) -> Option<&Page> {
        self.selected.as_deref().and_then(|id| self.a.page(id))
    }

    pub fn group(&self) -> Option<Group> {
        let p = self.current()?;
        let gs = self.groups();
        groups::of(p, &gs).cloned()
    }

    /// (index, count) of the current student among all students.
    pub fn student_pos(&self) -> Option<(usize, usize)> {
        let g = self.group()?;
        let gs = self.groups();
        Some((gs.iter().position(|x| x.key == g.key)?, gs.len()))
    }

    pub fn entries(&self, g: &Group) -> Vec<Entry> {
        let mut out = vec![];
        for id in &g.pages {
            if let Some(p) = self.a.page(id) {
                for item in words::items(p) {
                    out.push(Entry { page: id.clone(), item });
                }
            }
        }
        out
    }

    /// Text shown for a flagged word in the transcript (`original` shows what was written).
    pub fn word_text(&self, it: &Item) -> String {
        if self.original {
            if it.kind == "illegible" { "[illegible]".into() } else { word_str(it) }
        } else {
            shown_str(it)
        }
    }

    pub fn word_kind(&self, it: &Item) -> (Kind, bool) {
        let checked = it.via.is_some() && !self.original;
        let kind = if checked {
            Kind::Ok
        } else if it.kind == "illegible" {
            Kind::Bad
        } else {
            Kind::Warn
        };
        (kind, checked)
    }

    /// "Nothing corrected yet": both versions read the same, so Original is disabled.
    pub fn versions_same(&self) -> bool {
        match self.current() {
            Some(p) => p.raw().is_empty() || words::effective(p) == p.raw(),
            None => true,
        }
    }

    /// Text of the page for the transcript, with word markers.
    pub fn marked(&self, p: &Page) -> String {
        words::marked(p, self.original && !self.versions_same())
    }

    pub fn pill(&self, g: &Group) -> (PillKind, String) {
        match g.status {
            Status::Failed => (PillKind::Bad, "Couldn\u{2019}t transcribe".into()),
            Status::Processing => (PillKind::Busy, "Transcribing".into()),
            Status::Queued | Status::Waiting => (PillKind::Queued, "Waiting".into()),
            _ => {
                let n: usize = g.pages.iter().filter_map(|id| self.a.page(id)).map(words::count).sum();
                if n > 0 {
                    return (PillKind::Warn, format!("{} to check", plural(n, "word", "words")));
                }
                let any = g.pages.iter().filter_map(|id| self.a.page(id)).any(|p| !words::items(p).is_empty());
                (PillKind::Ok, if any { "All words checked" } else { "No words to check" }.into())
            }
        }
    }

    /// The chip of an inbox row (`chip(g)` of js/ui/inbox.js): the group's own status, so a student that was
    /// marked done reads "Ready to read" even with words left in it.
    pub fn chip(&self, g: &Group) -> (PillKind, String) {
        match g.status {
            Status::Ready => (PillKind::Ok, "Ready to read".into()),
            Status::Check => (PillKind::Warn, plural(g.open, "word to check", "words to check")),
            Status::Processing => (PillKind::Busy, "Transcribing".into()),
            Status::Queued | Status::Waiting => (PillKind::Queued, "Waiting".into()),
            Status::Failed => (PillKind::Bad, "Couldn’t transcribe".into()),
        }
    }

    pub fn tally(groups: &[Group]) -> Tally {
        let mut t = Tally { total: groups.len(), ..Default::default() };
        for g in groups {
            match g.status {
                Status::Ready => t.ready += 1,
                Status::Check => t.check += 1,
                Status::Failed => t.failed += 1,
                _ => t.processing += 1,
            }
        }
        t
    }

    /// Pages `Q.start()` runs by default: every pending page, in order.
    pub fn pending_ids(&self) -> Vec<String> {
        self.pages().into_iter().filter(|p| p.status() == "pending").map(|p| p.id().to_string()).collect()
    }

    /// Pages `Q.retryFailed()` runs: every page whose last read failed.
    pub fn failed_ids(&self) -> Vec<String> {
        self.pages().into_iter().filter(|p| p.status() == "error").map(|p| p.id().to_string()).collect()
    }

    pub fn copy_text(&self, g: &Group) -> String {
        let list: Vec<Page> = g
            .pages
            .iter()
            .filter_map(|id| self.a.page(id))
            .enumerate()
            .map(|(i, p)| {
                let mut c = p.clone();
                c.set_carried("order", json!(i));
                c
            })
            .collect();
        let refs: Vec<&Page> = list.iter().collect();
        let sep = self.settings.get("exportSeparator").and_then(Value::as_str).unwrap_or("label");
        let mode = if self.original { "raw" } else { "corrected" };
        export::combined(&refs, &Opts::new(mode, sep, "txt"))
    }

    /// The tray choices for an entry (js/ui/review.js `opts`).
    pub fn alts(&self, e: &Entry) -> Vec<Alt> {
        let it = &e.item;
        if it.kind == "suggest" {
            let conf = it.conf.as_ref().map(jsx::number).unwrap_or(0.0);
            let term = it.term.as_ref().map(jsx::to_string).unwrap_or_default();
            return vec![
                Alt {
                    label: term.clone(),
                    hint: format!("Course word, {}% match", jsx::round(conf * 100.0)),
                    via: "suggested",
                    id: "alt-suggested".into(),
                    text: String::new(),
                    on: it.via.as_deref() == Some("suggested"),
                },
                Alt {
                    label: word_str(it),
                    hint: "Keep it as read from the page".into(),
                    via: "written",
                    id: "alt-written".into(),
                    text: String::new(),
                    on: it.via.as_deref() == Some("written"),
                },
            ];
        }
        let mut out = vec![];
        let guesses = self.a.page(&e.page).and_then(|p| p.get("guesses")).and_then(|g| g.get(&it.key)).and_then(Value::as_array);
        for (n, g) in guesses.map(|v| v.as_slice()).unwrap_or(&[]).iter().enumerate() {
            let text = g.get("text").and_then(Value::as_str).unwrap_or("").to_string();
            if text.is_empty() {
                continue;
            }
            let pct = g.get("percent").map(jsx::number).unwrap_or(0.0);
            out.push(Alt {
                on: it.via.as_deref() == Some("typed") && shown_str(it) == text,
                label: text.clone(),
                hint: format!("{}% likely", jsx::round(pct)),
                via: "typed",
                id: format!("alt-guess-{n}"),
                text,
            });
        }
        out.push(Alt {
            label: "[illegible]".into(),
            hint: "Keep the marker so you can find it later".into(),
            via: "marked",
            id: "alt-marked".into(),
            text: String::new(),
            on: it.via.as_deref() == Some("marked"),
        });
        out
    }

    pub fn note(&self, e: &Entry) -> String {
        let it = &e.item;
        if self.original {
            return "Showing the original transcript. Switch to Corrected to see your choices.".into();
        }
        match (it.via.as_deref(), it.kind) {
            (Some("typed"), _) => format!("You typed \u{201C}{}\u{201D}. Choose another option to change it.", shown_str(it)),
            (Some(_), _) => format!("You chose \u{201C}{}\u{201D}. Choose another option to change it.", shown_str(it)),
            (None, "illegible") => "The model could not read this word. Pick one of its best guesses, type it yourself, or keep the marker.".into(),
            _ => format!("The model wrote \u{201C}{}\u{201D}, which is close to a course word.", word_str(it)),
        }
    }

    /// "Now on page 2 of 2." for the page the panel is on (a decision that crossed to the next page).
    pub fn page_phrase(&self) -> String {
        let Some(g) = self.group() else { return String::new() };
        match self.selected.as_ref().and_then(|id| g.pages.iter().position(|p| p == id)) {
            Some(i) => format!("Now on page {} of {}.", i + 1, g.pages.len()),
            None => String::new(),
        }
    }

    /// What a listener needs to hear once a decision moved the open word on: the word now open, its place and
    /// whether anything is chosen for it yet (focus lands on its first option, which names only itself).
    pub fn next_word_phrase(&self) -> String {
        let Some(g) = self.group() else { return String::new() };
        let list = self.entries(&g);
        if !list.iter().any(|e| e.item.via.is_none()) {
            return "All words checked.".into();
        }
        let Some((e, i, n)) = self.open_entry() else {
            let left = list.iter().filter(|e| e.item.via.is_none()).count();
            return format!("{} left to check.", plural(left, "word", "words"));
        };
        format!("Next, word {} of {}: {}", i + 1, n, Self::kind_phrase(&e))
    }

    /// "a word that could not be read. Nothing is chosen for it yet." for one entry of the tray.
    fn kind_phrase(e: &Entry) -> String {
        let what = match (e.item.via.as_deref(), e.item.kind) {
            (Some(_), _) => format!("a word you checked, {}", shown_str(&e.item)),
            (None, "illegible") => "a word that could not be read".to_string(),
            _ => format!("spelling to check, {}", word_str(&e.item)),
        };
        let state = if e.item.via.is_some() { "Already chosen." } else { "Nothing is chosen for it yet." };
        format!("{what}. {state}")
    }

    /// What Next and Previous word to check say: which word the panel is on now, what kind of problem it is and,
    /// when the step crossed to another page, which page the transcript shows.
    pub fn step_phrase(&self, page_changed: bool) -> String {
        let Some((e, i, n)) = self.open_entry() else { return String::new() };
        let page = if page_changed { format!(" {}", self.page_phrase()) } else { String::new() };
        format!("Word {} of {}: {}{page}", i + 1, n, Self::kind_phrase(&e))
    }

    /// Transcript line (unit index) of every flagged word on a page, with its strongest kind:
    /// `flaggedLines` of review.js. Also the unit of the open word.
    pub fn flagged_lines(&self, p: &Page) -> (HashMap<usize, Kind>, Option<usize>) {
        let units = render::units(p.raw());
        let mut kinds: HashMap<usize, Kind> = HashMap::new();
        let mut sel_unit = None;
        let rank = |k: Kind| match k {
            Kind::Ok => 1,
            Kind::Warn => 2,
            Kind::Bad => 3,
        };
        let stripped: Vec<String> = units.iter().map(|u| strip_math(u)).collect();
        let selected = self.sel_word.as_ref().filter(|(id, _)| id == p.id());
        for it in words::items(p) {
            let kind = if it.via.is_some() {
                Kind::Ok
            } else if it.kind == "illegible" {
                Kind::Bad
            } else {
                Kind::Warn
            };
            let mut put = |i: usize| {
                if kinds.get(&i).map_or(true, |k| rank(kind) > rank(*k)) {
                    kinds.insert(i, kind);
                }
                if selected.is_some_and(|(_, k)| *k == it.key) {
                    sel_unit = Some(i);
                }
            };
            if it.kind == "illegible" {
                let mut n = 0;
                for (i, u) in stripped.iter().enumerate() {
                    let c = count_illegible(u);
                    if it.index < n + c {
                        put(i);
                        break;
                    }
                    n += c;
                }
            } else {
                let w = word_str(&it);
                for (i, u) in stripped.iter().enumerate() {
                    if has_word(u, &w) {
                        put(i);
                    }
                }
            }
        }
        (kinds, sel_unit)
    }

    // ------------------------------------------------------------------ tray

    /// A new student opens the tray only when words are waiting; the open word defaults to the first
    /// undecided one (on this page first).
    pub fn sync_tray(&mut self) {
        let Some(g) = self.group() else {
            self.sel_word = None;
            return;
        };
        let list = self.entries(&g);
        if list.is_empty() {
            self.sel_word = None;
            return;
        }
        if self.tray_group.as_deref() != Some(g.key.as_str()) {
            self.tray_group = Some(g.key.clone());
            self.tray_open = list.iter().any(|e| e.item.via.is_none());
        }
        let valid = self
            .sel_word
            .as_ref()
            .is_some_and(|(id, k)| list.iter().any(|e| &e.page == id && &e.item.key == k));
        if !valid {
            let cur = self.selected.clone();
            let on_page = list.iter().position(|e| Some(&e.page) == cur.as_ref() && e.item.via.is_none());
            let first_open = list.iter().position(|e| e.item.via.is_none());
            let on_any = list.iter().position(|e| Some(&e.page) == cur.as_ref());
            let i = on_page.or(first_open).or(on_any).unwrap_or(0);
            self.sel_word = Some((list[i].page.clone(), list[i].item.key.clone()));
        }
    }

    /// The entry open in the tray plus its index and the list length.
    pub fn open_entry(&self) -> Option<(Entry, usize, usize)> {
        let g = self.group()?;
        let list = self.entries(&g);
        let (id, key) = self.sel_word.as_ref()?;
        let i = list.iter().position(|e| &e.page == id && &e.item.key == key)?;
        Some((list[i].clone(), i, list.len()))
    }

    /// A click on a flagged word: opens it in the tray and, as review.js does, moves the reading underline
    /// to the line it sits on.
    pub fn pick_word(&mut self, page: &str, key: &str) {
        self.sel_word = Some((page.to_string(), key.to_string()));
        self.tray_open = true;
        if self.selected.as_deref() != Some(page) {
            self.select(page);
        }
        let unit = self
            .a
            .page(page)
            .and_then(|p| words::items(p).into_iter().find(|i| i.key == key).and_then(|it| unit_of(p, &it)));
        if let Some(u) = unit {
            self.read_line.insert(page.to_string(), u);
        }
    }

    /// `renderWho`: Original is not available where nothing was corrected, so the flag cannot stay set.
    pub fn normalize(&mut self) {
        if self.original && self.versions_same() {
            self.original = false;
        }
    }

    /// Previous / next word of this student, wrapping.
    pub fn step_word(&mut self, d: i32) {
        let Some((_, i, n)) = self.open_entry() else { return };
        let Some(g) = self.group() else { return };
        let list = self.entries(&g);
        let t = &list[(i as i64 + d as i64).rem_euclid(n as i64) as usize];
        let (page, key) = (t.page.clone(), t.item.key.clone());
        self.sel_word = Some((page.clone(), key));
        if self.selected.as_deref() != Some(&page) {
            self.select(&page);
        }
    }

    /// The next undecided word after `from`, wrapping; `from` itself when nothing else is open.
    fn advance(&self, g: &Group, from: &(String, String)) -> (String, String) {
        let list = self.entries(g);
        let i = list.iter().position(|e| e.page == from.0 && e.item.key == from.1).unwrap_or(0);
        for k in 1..=list.len() {
            let c = &list[(i + k) % list.len()];
            if c.item.via.is_none() {
                return (c.page.clone(), c.item.key.clone());
            }
        }
        from.clone()
    }

    // ------------------------------------------------------------------ flows

    /// Record a decision for the open word and move to the next undecided one.
    pub fn choose_open(&mut self, via: &str, text: &str) -> bool {
        let Some((id, key)) = self.sel_word.clone() else { return false };
        let Some(g) = self.group() else { return false };
        let next = self.advance(&g, &(id.clone(), key.clone()));
        if !self.choose(&id, &key, via, text) {
            return false;
        }
        self.sel_word = Some(next.clone());
        if self.selected.as_deref() != Some(next.0.as_str()) {
            self.select(&next.0);
        }
        true
    }

    /// `W.choose` plus bookkeeping: false (nothing touched) when a typed fix is empty after cleaning.
    pub fn choose(&mut self, page: &str, key: &str, via: &str, text: &str) -> bool {
        let topic = self.a.meta.topic().to_string();
        let before = self.settings.get("reference").cloned();
        let snap = self.a.pages.get(page).map(|p| (page.to_string(), p.clone(), self.settings.clone(), key.to_string()));
        let Some(p) = self.a.pages.get_mut(page) else { return false };
        if !words::choose(p, key, via, text, &mut self.settings, &topic, self.now) {
            return false;
        }
        self.undo = snap;
        self.dirty.pages.insert(page.to_string());
        if self.settings.get("reference") != before.as_ref() {
            self.dirty.settings = true;
        }
        true
    }

    /// Take back the last word decision (the page and the settings as they were), reopening that word.
    /// `None`: there is nothing to undo.
    pub fn undo_choice(&mut self) -> Option<()> {
        let (id, page, settings, key) = self.undo.take()?;
        self.a.pages.insert(id.clone(), page);
        if self.settings != settings {
            self.settings = settings;
            self.dirty.settings = true;
        }
        self.dirty.pages.insert(id.clone());
        if self.selected.as_deref() != Some(id.as_str()) {
            self.select(&id);
        }
        self.sel_word = Some((id, key));
        self.tray_open = true;
        self.sync_tray();
        Some(())
    }

    /// `Use all N suggestions` for the current student.
    pub fn use_all(&mut self) {
        self.undo = None;
        let Some(g) = self.group() else { return };
        let topic = self.a.meta.topic().to_string();
        for id in &g.pages {
            if let Some(p) = self.a.pages.get_mut(id) {
                if words::open(p).iter().any(|i| i.is_suggest()) {
                    words::use_all(p, &mut self.settings, &topic, self.now);
                    self.dirty.pages.insert(id.clone());
                }
            }
        }
        self.sel_word = None;
        self.sync_tray();
    }

    pub fn unresolved_suggestions(&self) -> usize {
        self.group()
            .map(|g| self.entries(&g).iter().filter(|e| e.item.is_suggest() && e.item.via.is_none()).count())
            .unwrap_or(0)
    }

    /// `change:selectedId`: `pane.scrollTop = ui.scrollId === selectedId ? ui.scroll || 0 : 0`.
    fn request_scroll(&mut self) {
        let ui = self.a.meta.get("ui");
        let saved = ui.and_then(|u| u.get("scrollId")).and_then(Value::as_str);
        let off = if saved.is_some() && saved == self.selected.as_deref() {
            ui.and_then(|u| u.get("scroll")).map(jsx::number).filter(|v| v.is_finite()).unwrap_or(0.0)
        } else {
            0.0
        };
        let n = self.scroll_req.map_or(0, |(n, _)| n) + 1;
        self.scroll_req = Some((n, off.max(0.0) as f32));
    }

    /// The transcript pane stopped scrolling at `y` (js: debounced `scroll` handler of `#tx-pane`):
    /// `meta.ui.scroll` and `meta.ui.scrollId` follow, and the project is saved.
    pub fn set_scroll(&mut self, y: f32) {
        let Some(id) = self.selected.clone() else { return };
        let mut ui = self.a.meta.get("ui").and_then(Value::as_object).cloned().unwrap_or_default();
        ui.insert("scroll".into(), jsx::num_value(y as f64));
        ui.insert("scrollId".into(), json!(id));
        self.a.meta.set_carried("ui", Value::Object(ui));
        self.dirty.meta = true;
    }

    pub fn select(&mut self, id: &str) {
        if !self.a.pages.contains_key(id) || self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_string());
        self.request_scroll();
        let mut ui = self.a.meta.get("ui").and_then(Value::as_object).cloned().unwrap_or_default();
        ui.insert("selectedId".into(), json!(id));
        self.a.meta.set_carried("ui", Value::Object(ui));
        self.dirty.meta = true;
        if !self.sel_word.as_ref().is_some_and(|(p, _)| p == id) {
            self.sel_word = None;
        }
        self.sync_tray();
    }

    pub fn set_split(&mut self, split: bool) {
        if self.split == split {
            return;
        }
        self.split = split;
        let mut ui = self.a.meta.get("ui").and_then(Value::as_object).cloned().unwrap_or_default();
        ui.insert("layout".into(), json!(if split { "split" } else { "text" }));
        self.a.meta.set_carried("ui", Value::Object(ui));
        self.dirty.meta = true;
    }

    /// Move to the first page of the previous / next student.
    pub fn step_student(&mut self, d: i32) -> bool {
        let gs = self.groups();
        let Some((i, _)) = self.student_pos() else { return false };
        let t = i as i64 + d as i64;
        if t < 0 || t as usize >= gs.len() {
            return false;
        }
        let first = gs[t as usize].pages[0].clone();
        self.select(&first);
        true
    }

    /// `Done, next student`: marks the student's pages reviewed and moves on. False on the last student.
    pub fn done_next(&mut self) -> bool {
        let Some(g) = self.group() else { return false };
        for id in &g.pages {
            if let Some(p) = self.a.pages.get_mut(id) {
                if !p.reviewed() {
                    p.set_carried("reviewed", Value::Bool(true));
                    self.dirty.pages.insert(id.clone());
                }
            }
        }
        self.step_student(1)
    }

    pub fn set_read_line(&mut self, i: Option<usize>) {
        if let Some(id) = self.selected.clone() {
            match i {
                Some(i) => self.read_line.insert(id, i),
                None => self.read_line.remove(&id),
            };
        }
    }

    // ------------------------------------------------------------------ page tools

    pub fn set_zoom(&mut self, z: f32) {
        self.zoom = z.clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
    }

    /// `stepZoom`: the next preset above or below the current zoom.
    pub fn step_zoom(&mut self, d: i32) {
        let i = ZOOMS.iter().position(|z| *z >= self.zoom - 0.001).unwrap_or(0) as i32;
        let j = (i + d).clamp(0, ZOOMS.len() as i32 - 1) as usize;
        self.set_zoom(ZOOMS[j]);
    }

    /// Turn the current page a quarter turn. The host turns the image; boxes are dropped (they no longer
    /// match). Not while the page is being read.
    pub fn rotate_current(&mut self) -> bool {
        let Some(id) = self.selected.clone() else { return false };
        let Some(p) = self.a.pages.get_mut(&id) else { return false };
        if p.status() == "transcribing" {
            return false;
        }
        let rot = p.get("rotation").map(jsx::number).unwrap_or(0.0) as i64;
        p.set("rotation", json!((rot + 90).rem_euclid(360)));
        p.set("boxes", json!([]));
        self.dirty.pages.insert(id.clone());
        self.dirty.rotated.push(id);
        true
    }

    /// `S.removePage`: a neighbour of the same student becomes the selection; with none left the inbox shows.
    pub fn remove_page(&mut self, id: &str) {
        if !self.a.pages.contains_key(id) {
            return;
        }
        let sibling = self.group().and_then(|g| g.pages.iter().find(|x| x.as_str() != id).cloned());
        let i = self.a.index_of(id).unwrap_or(0);
        let was_selected = self.selected.as_deref() == Some(id);
        self.a.remove_page(id);
        self.multi.remove(id);
        self.read_line.remove(id);
        if self.sel_word.as_ref().is_some_and(|(p, _)| p == id) {
            self.sel_word = None;
        }
        self.dirty.pages.remove(id);
        self.dirty.rotated.retain(|x| x != id);
        self.dirty.removed.push(id.to_string());
        self.dirty.meta = true;
        if was_selected {
            self.selected = None;
            let order = self.a.meta.order();
            match sibling {
                Some(n) if self.a.pages.contains_key(&n) => self.select(&n),
                _ => {
                    // review.js: no page left for this student, so the inbox shows; the selection stays valid
                    self.view = View::Inbox;
                    if let Some(n) = order.get(i.min(order.len().saturating_sub(1))).cloned() {
                        self.select(&n);
                    }
                }
            }
        }
        self.sync_tray();
    }

    /// Alt+Arrow in the strip: move a page one place within the student's pages (`S.movePage`).
    pub fn move_page(&mut self, id: &str, d: i32) -> bool {
        let Some(g) = self.group() else { return false };
        let Some(i) = g.pages.iter().position(|p| p == id) else { return false };
        let j = i as i32 + d;
        if j < 0 || j as usize >= g.pages.len() {
            return false;
        }
        let (Some(a), Some(b)) = (self.a.index_of(id), self.a.index_of(&g.pages[j as usize])) else { return false };
        self.a.move_page(a as i64, b as i64);
        // `S.movePage` reindexes in memory and saves only the project: untouched pages keep the `order`
        // they were stored with until something else saves them
        self.dirty.meta = true;
        true
    }

    /// Select the page `d` places away within the student, clamped. Returns the page now selected.
    pub fn step_page(&mut self, from: &str, d: i32) -> Option<String> {
        let g = self.group()?;
        let i = g.pages.iter().position(|p| p == from)? as i32;
        let t = g.pages[(i + d).clamp(0, g.pages.len() as i32 - 1) as usize].clone();
        self.select(&t);
        Some(t)
    }

    pub fn edge_page(&mut self, last: bool) -> Option<String> {
        let g = self.group()?;
        let t = if last { g.pages.last()? } else { g.pages.first()? }.clone();
        self.select(&t);
        Some(t)
    }

    /// Ctrl/Cmd click toggles a page in the selection, Shift click ranges from the current page.
    pub fn multi_click(&mut self, id: &str, shift: bool) {
        let Some(g) = self.group() else { return };
        if shift && self.selected.is_some() {
            let a = g.pages.iter().position(|x| Some(x) == self.selected.as_ref()).unwrap_or(0);
            let b = g.pages.iter().position(|x| x == id).unwrap_or(0);
            for x in &g.pages[a.min(b)..=a.max(b)] {
                self.multi.insert(x.clone());
            }
        } else {
            if self.multi.is_empty() {
                if let Some(c) = self.selected.clone() {
                    self.multi.insert(c);
                }
            }
            if !self.multi.remove(id) {
                self.multi.insert(id.to_string());
            }
        }
    }

    /// Names a page can be moved to with "Change student": every other student.
    pub fn other_students(&self) -> Vec<String> {
        let cur = self.group().map(|g| g.name).unwrap_or_default();
        let mut v: Vec<String> = self.groups().into_iter().map(|g| g.name).filter(|n| *n != cur).collect();
        v.dedup();
        v
    }

    /// "Change student": `student = {name, source: 'chosen', confirmed: true}` on this page or on all of the
    /// student's pages. Returns the number of pages changed.
    pub fn set_student(&mut self, name: &str, all: bool) -> usize {
        let name = name.trim();
        if name.is_empty() {
            return 0;
        }
        let ids: Vec<String> = if all {
            self.group().map(|g| g.pages).unwrap_or_default()
        } else {
            self.selected.clone().into_iter().collect()
        };
        for id in &ids {
            if let Some(p) = self.a.pages.get_mut(id) {
                p.set("student", json!({"name": name, "source": "chosen", "confirmed": true}));
                self.dirty.pages.insert(id.clone());
            }
        }
        self.sync_tray();
        ids.len()
    }

    /// `U.reReview`: decisions cleared and suggestions un-applied; the transcripts stay. Returns pages touched.
    pub fn review_again(&mut self, ids: &[String]) -> usize {
        let mut n = 0;
        for id in ids {
            let Some(p) = self.a.pages.get_mut(id) else { continue };
            if p.status() != "done" {
                continue;
            }
            let applied: Vec<usize> = p
                .corrections()
                .iter()
                .enumerate()
                .filter(|(_, c)| c.get("applied").and_then(Value::as_bool) == Some(true))
                .map(|(i, _)| i)
                .collect();
            for i in applied {
                vishion_core::judge::toggle(p, i as f64, false);
            }
            p.set("resolved", json!({}));
            p.set("edits", json!([]));
            p.set("reviewed", Value::Bool(false));
            self.dirty.pages.insert(id.clone());
            n += 1;
        }
        self.multi.clear();
        self.sel_word = None;
        self.sync_tray();
        n
    }

    /// `U.reReview` from the inbox: the pages go back to "needs a check", the selection clears, and the review
    /// screen opens on the first of those students. From the review strip it stays where it is.
    pub fn review_again_and_open(&mut self, ids: &[String]) -> usize {
        let first = if self.view == View::Inbox {
            self.groups().into_iter().find(|g| g.pages.iter().any(|p| ids.contains(p))).and_then(|g| g.pages.first().cloned())
        } else {
            None
        };
        let n = self.review_again(ids);
        if let Some(f) = first {
            self.select(&f);
            self.view = View::Review;
        }
        n
    }

    /// The page got a new picture (js/ui/upload.js `doReplace`): everything read from the old one is reset.
    pub fn page_replaced(&mut self, id: &str, name: &str) {
        let Some(p) = self.a.pages.get_mut(id) else { return };
        p.set("name", json!(name));
        p.set("status", json!("pending"));
        for (k, v) in [
            ("raw", json!("")),
            ("corrected", json!("")),
            ("corrections", json!([])),
            ("boxes", json!([])),
            ("timings", json!({"ms": 0, "tokens": 0})),
            ("error", json!("")),
            ("rotation", json!(0)),
            ("reviewed", json!(false)),
            ("resolved", json!({})),
            ("edits", json!([])),
        ] {
            p.set(k, v);
        }
        self.a.sync_stats();
        self.read_line.remove(id);
        self.dirty.pages.insert(id.to_string());
        self.dirty.meta = true;
        self.sync_tray();
    }

    /// `openExport`'s `chosen()` with `flatten`: page copies named after the student (`<name> page N` when a
    /// student has several), ordered within the student. `scope`: `all` | `ready` | `selected`.
    pub fn export_pages(&self, scope: &str) -> Vec<Page> {
        let mut out = vec![];
        for g in self.groups() {
            if scope == "ready" && g.status != groups::Status::Ready {
                continue;
            }
            let ids: Vec<&String> = g.pages.iter().filter(|id| scope != "selected" || self.multi.contains(*id)).collect();
            let n = ids.len();
            for (i, id) in ids.into_iter().enumerate() {
                let Some(p) = self.a.page(id) else { continue };
                let mut c = p.clone();
                c.set("order", json!(i));
                c.set("name", json!(if n > 1 { format!("{} page {}", g.name, i + 1) } else { g.name.clone() }));
                let mut st = c.get("student").and_then(Value::as_object).cloned().unwrap_or_default();
                st.insert("name".into(), json!(g.name));
                c.set("student", Value::Object(st));
                out.push(c);
            }
        }
        out
    }

    /// `S.updateMeta({ key })`: one field of the assignment's `project.json`.
    pub fn set_meta(&mut self, key: &str, v: Value) {
        self.a.meta.set(key, v);
        self.dirty.meta = true;
    }

    /// Pages the host created for a student (picture already stored). They are saved like any edit.
    pub fn pages_added(&mut self, list: Vec<Page>) {
        let ids: Vec<String> = list.iter().map(|p| p.id().to_string()).collect();
        self.a.add_pages(list);
        // `S.addPages` saves the new pages only; the others are renumbered in memory
        for id in ids {
            self.dirty.pages.insert(id);
        }
        self.dirty.meta = true;
    }

    /// A page was read (`Q.runOne` `finish`, then `J.run`, then the automatic guess): status done, the
    /// transcript, its line boxes when the ink scan agrees with the line count, the glossary suggestions
    /// from the offline judge, and guesses for unreadable words when the host made them. Decisions already
    /// on the page (`resolved`, `edits`) are left as they are, exactly as the JS queue leaves them.
    pub fn page_read(&mut self, id: &str, raw: &str, tokens: u32, ms: u32, guesses: Option<serde_json::Map<String, Value>>, bands: &[Band]) {
        let topic = self.a.meta.topic().to_string();
        let terms = self.settings.terms(&topic);
        let guess_on = self.settings.get("guessIllegible").and_then(Value::as_bool) != Some(false);
        let boxes = compute_boxes(raw, bands);
        let Some(p) = self.a.pages.get_mut(id) else { return };
        let raw = raw.trim_end_matches(jsx::is_ws).to_string();
        let was_done = p.status() == "done";
        // `autoGuess`: a page that starts reading drops old guesses (only when it had any)
        let had_guesses = jsx::truthy(p.get("guessed")) || p.get("guesses").and_then(Value::as_object).is_some_and(|o| !o.is_empty());
        if had_guesses {
            p.set("guesses", json!({}));
            p.set("guessed", json!(false));
        }
        p.set("status", json!("done"));
        p.set("raw", json!(raw));
        p.set("corrected", json!(raw));
        p.set("corrections", json!([]));
        p.set("boxes", Value::Array(boxes));
        p.set("error", json!(""));
        p.set("timings", json!({"ms": ms, "tokens": tokens}));
        if !raw.is_empty() && !terms.is_empty() {
            let found = vishion_core::judge::heuristic(&raw, &terms);
            vishion_core::judge::apply_found(p, &found);
        }
        if guess_on && !jsx::truthy(p.get("guessed")) && has_illegible(&raw) {
            if let Some(g) = guesses {
                p.set("guesses", Value::Object(g));
                p.set("guessed", json!(true));
            }
        }
        self.read_line.remove(id);
        self.dirty.pages.insert(id.to_string());
        if !was_done {
            self.a.sync_stats();
            self.dirty.meta = true;
        }
        self.sync_tray();
    }

    /// `levelPage` tail: `boxes = computeBoxes(image, raw)` for a transcribed page, `[]` otherwise. Returns the
    /// number of boxes.
    pub fn set_boxes_from_ink(&mut self, id: &str, bands: &[Band]) -> usize {
        let Some(p) = self.a.pages.get_mut(id) else { return 0 };
        let boxes = if p.status() == "done" && !p.raw().is_empty() { compute_boxes(p.raw(), bands) } else { vec![] };
        let n = boxes.len();
        p.set("boxes", Value::Array(boxes));
        self.dirty.pages.insert(id.to_string());
        n
    }

    /// The teacher's earlier resolutions for this topic, as the guess prompt quotes them (`W.referenceExamples`).
    pub fn reference_examples(&self) -> Vec<String> {
        vishion_core::words::reference_examples(&self.settings, self.a.meta.topic(), Some(10))
    }

    /// How to reach the OCR server for a run (`S.d.settings`, `O.buildPrompt`).
    pub fn read_conn(&self) -> crate::host::Conn {
        let st = &self.settings;
        let text = |k: &str| st.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        crate::host::Conn {
            url: text("ocrUrl").trim_end_matches('/').to_string(),
            api_key: text("apiKey"),
            model: text("model"),
            timeout_secs: st.get("timeoutSec").and_then(Value::as_u64).filter(|n| *n > 0).unwrap_or(180),
            prompt: vishion_core::words::build_prompt(st, &self.a.meta),
            demo: st.get("demo").and_then(Value::as_bool) == Some(true),
            provider: text("provider"),
            guess: st.get("guessIllegible").and_then(Value::as_bool) != Some(false),
            examples: self.reference_examples(),
        }
    }

    /// The text of a page being read so far (`queue.js` `runOne` start and `page:stream`): status
    /// transcribing, nothing decided yet. Not saved until the page is done.
    pub fn page_reading(&mut self, id: &str, text: &str) {
        let Some(p) = self.a.pages.get_mut(id) else { return };
        let started = p.status() != "transcribing";
        if started {
            p.set("status", json!("transcribing"));
            p.set("corrected", json!(""));
            p.set("corrections", json!([]));
            p.set("boxes", json!([]));
            p.set("error", json!(""));
        }
        p.set("raw", json!(text));
        if started {
            self.a.sync_stats();
            self.sync_tray();
        }
    }

    /// Stop or an unreachable server: the page goes back to pending with nothing on it (`queue.js`).
    pub fn page_unread(&mut self, id: &str) {
        let Some(p) = self.a.pages.get_mut(id) else { return };
        if p.status() != "transcribing" {
            return;
        }
        p.set("status", json!("pending"));
        p.set("raw", json!(""));
        p.set("corrected", json!(""));
        self.a.sync_stats();
        self.sync_tray();
    }

    /// The page could not be read: status error with the message (`queue.js` catch).
    pub fn page_failed(&mut self, id: &str, msg: &str) {
        let Some(p) = self.a.pages.get_mut(id) else { return };
        let changed = p.status() != "error";
        p.set("status", json!("error"));
        let partial = p.raw().to_string();
        p.set("corrected", json!(partial));
        p.set("error", json!(if msg.is_empty() { "Transcription failed." } else { msg }));
        self.dirty.pages.insert(id.to_string());
        if changed {
            self.a.sync_stats();
            self.dirty.meta = true;
        }
        self.sync_tray();
    }

    /// `W.guess` answered: `guesses = found`, `guessed = true`.
    pub fn page_guessed(&mut self, id: &str, found: serde_json::Map<String, Value>) {
        let Some(p) = self.a.pages.get_mut(id) else { return };
        p.set("guesses", Value::Object(found));
        p.set("guessed", json!(true));
        self.dirty.pages.insert(id.to_string());
    }

    /// True when the page carries decisions that "Transcribe again" would replace.
    pub fn has_decisions(&self, p: &Page) -> bool {
        !p.edits().is_empty() || !p.resolved().is_empty()
    }

    /// The text of the transcript as plain text (`textView === 'raw'`): no word markers.
    pub fn plain_text(&self, p: &Page) -> String {
        let t = if self.original { p.raw().to_string() } else { words::effective(p) };
        t.chars().filter(|c| !('\u{E000}'..='\u{E002}').contains(c)).collect()
    }

    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }
}

/// `/\[illegible\]/i`
pub fn has_illegible(t: &str) -> bool {
    let b = t.as_bytes();
    (0..b.len().saturating_sub(10)).any(|i| b[i] == b'[' && b[i..i + 11].eq_ignore_ascii_case(b"[illegible]"))
}

/// `R.computeBoxes`: the ink bands fitted to the transcript lines, or nothing when they are not reliable.
pub fn compute_boxes(raw: &str, bands: &[Band]) -> Vec<Value> {
    let n = render::units(raw).len();
    if n == 0 || !render::bands_reliable(bands.len(), n) {
        return vec![];
    }
    let vals: Vec<Value> = bands.iter().map(|b| json!({"x": b.x, "y": b.y, "w": b.w, "h": b.h, "r": 0, "s": b.s})).collect();
    render::map_bands(&vals, n)
}

/// A transcript line's box on the page, as fractions of the image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where each of the `n` transcript lines sits on the page, as review.js `geometry` decides it: the stored
/// boxes when there are exactly `n` and they are spread over the page; else the ink bands scanned from the
/// picture (`bands`) when there are `n`, or a few strays more (the weakest are dropped); else the lines are
/// spread evenly over the area where writing was found (`approx = true`), so a page always gets its underline.
pub fn geometry(p: &Page, n: usize, bands: &[Band]) -> Option<(Vec<BBox>, bool)> {
    if n == 0 || p.status() != "done" {
        return None;
    }
    let boxes = stored_boxes(p);
    let spread = |b: &[BBox]| {
        if b.len() < 2 {
            return b.len() == 1;
        }
        let top = b.iter().map(|x| x.y).fold(f32::MAX, f32::min);
        let bottom = b.iter().map(|x| x.y + x.h).fold(f32::MIN, f32::max);
        bottom - top >= 0.12 || (b.len() < 3 && bottom - top >= 0.04)
    };
    if boxes.len() == n && spread(&boxes) {
        return Some((boxes, false));
    }
    let scanned: Vec<BBox> = bands.iter().map(|b| BBox { x: b.x, y: b.y, w: b.w, h: b.h }).collect();
    if scanned.len() == n && spread(&scanned) {
        return Some((scanned, false));
    }
    let tol = 2usize.max(jsx::round(n as f64 * 0.35) as usize);
    if scanned.len() > n && scanned.len() - n <= tol && spread(&scanned) {
        // drop the strays: map_bands removes the weakest band until there are n
        let vals: Vec<Value> = bands.iter().map(|b| json!({"x": b.x, "y": b.y, "w": b.w, "h": b.h, "r": 0, "s": b.s})).collect();
        let f = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let out: Vec<BBox> = render::map_bands(&vals, n)
            .iter()
            .map(|v| BBox { x: f(v, "x"), y: f(v, "y"), w: f(v, "w"), h: f(v, "h") })
            .collect();
        return Some((out, false));
    }
    let (top, bottom, left, right) = if scanned.len() >= 2 {
        (
            scanned.iter().map(|b| b.y).fold(f32::MAX, f32::min),
            scanned.iter().map(|b| b.y + b.h).fold(f32::MIN, f32::max),
            scanned.iter().map(|b| b.x).fold(f32::MAX, f32::min),
            scanned.iter().map(|b| b.x + b.w).fold(f32::MIN, f32::max),
        )
    } else {
        (0.08, 0.92, 0.08, 0.92)
    };
    let step = (bottom - top) / n as f32;
    Some((
        (0..n)
            .map(|i| BBox { x: left, y: top + i as f32 * step + step * 0.1, w: right - left, h: step * 0.8 })
            .collect(),
        true,
    ))
}

/// The line boxes stored on the page (`p.boxes`).
pub fn stored_boxes(p: &Page) -> Vec<BBox> {
    match p.get("boxes") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|b| {
                let f = |k: &str| b.get(k).and_then(Value::as_f64).map(|v| v as f32);
                Some(BBox { x: f("x")?, y: f("y")?, w: f("w")?, h: f("h")? })
            })
            .collect(),
        _ => vec![],
    }
}

/// The line (transcript unit) a flagged word sits on, for cropping the handwriting into the tray.
pub fn unit_of(p: &Page, it: &Item) -> Option<usize> {
    let units = render::units(p.raw());
    let stripped: Vec<String> = units.iter().map(|u| strip_math(u)).collect();
    if it.kind == "illegible" {
        let mut n = 0;
        for (i, u) in stripped.iter().enumerate() {
            let c = count_illegible(u);
            if it.index < n + c {
                return Some(i);
            }
            n += c;
        }
        None
    } else {
        let w = word_str(it);
        stripped.iter().position(|u| has_word(u, &w))
    }
}

/// Text outside `$..$` / `$$..$$` (math is never searched for words).
fn strip_math(t: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (a, b) in words::math_spans(t) {
        out.push_str(&t[last..a]);
        last = b;
    }
    out.push_str(&t[last..]);
    out
}

fn count_illegible(t: &str) -> usize {
    let b = t.as_bytes();
    let n = "[illegible]".len();
    let (mut i, mut c) = (0, 0);
    while i + n <= b.len() {
        if b[i] == b'[' && b[i..i + n].eq_ignore_ascii_case(b"[illegible]") {
            c += 1;
            i += n;
        } else {
            i += 1;
        }
    }
    c
}

/// `new RegExp('\\b' + esc(w) + '\\b').test(t)`.
fn has_word(t: &str, w: &str) -> bool {
    if w.is_empty() {
        return false;
    }
    let ch: Vec<char> = t.chars().collect();
    let wc: Vec<char> = w.chars().collect();
    if wc.len() > ch.len() {
        return false;
    }
    (0..=ch.len() - wc.len()).any(|i| ch[i..i + wc.len()] == wc[..] && jsx::boundary(&ch, i) && jsx::boundary(&ch, i + wc.len()))
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use vishion_core::model::Project;

    /// Two students, three pages; Maya has a suggestion, an unreadable word and a struck word.
    pub fn session() -> Session {
        let meta = Project::from_json(r#"{"name":"Unit 5","topic":"Math/Education","order":[],"ui":{}}"#).unwrap();
        let mut a = Assignment::new(meta);
        let mk = |id: &str, name: &str, who: &str, raw: &str, corr: Value| {
            let mut p = Assignment::new_page(id, name);
            p.set("status", json!("done"));
            p.set("raw", json!(raw));
            p.set("corrected", json!(raw));
            p.set("corrections", corr);
            p.set("student", json!({"name": who, "source": "page", "confirmed": true}));
            p
        };
        let c = json!([{"word": "confidance", "term": "confidence", "conf": 0.91, "applied": false}]);
        let pages = vec![
            mk("a1", "a1.png", "Ana L.", "Plain text with nothing wrong.", json!([])),
            mk(
                "m1",
                "m1.png",
                "Maya R.",
                "The 95% confidance interval is $\\bar{x}$ here.\nIt took [illegible] minutes. ~~probably~~ ok\nMargin: check my units!",
                c,
            ),
            mk("m2", "m2.png", "Maya R.", "A second page with a standerd word.", json!([{"word": "standerd", "term": "standard", "conf": 0.86, "applied": false}])),
        ];
        a.add_pages(pages);
        Session::new("unit-5".into(), a, Settings::defaults(), 1_700_000_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::session;
    use super::*;

    fn to_maya(s: &mut Session) {
        s.select("m1");
    }

    #[test]
    fn groups_pill_and_tray_defaults() {
        let mut s = session();
        assert_eq!(s.selected.as_deref(), Some("a1"));
        to_maya(&mut s);
        let g = s.group().unwrap();
        assert_eq!(g.name, "Maya R.");
        assert_eq!(s.student_pos(), Some((1, 2)));
        let (k, label) = s.pill(&g);
        assert_eq!((k, label.as_str()), (PillKind::Warn, "3 words to check"));
        // first undecided word on the page that is open
        let (e, i, n) = s.open_entry().unwrap();
        assert_eq!((i, n), (0, 3));
        assert_eq!(e.item.key, "c:0");
        assert!(s.tray_open);
    }

    #[test]
    fn choosing_suggestion_updates_page_and_advances() {
        let mut s = session();
        to_maya(&mut s);
        assert!(s.choose_open("suggested", ""));
        assert_eq!(s.dirty.pages.iter().collect::<Vec<_>>(), vec!["m1"]);
        let p = s.page("m1").unwrap();
        assert!(p.corrected().contains("confidence"));
        assert_eq!(s.sel_word.as_ref().unwrap().1, "i:0");
        // typed fix on the illegible word, remembered into settings.reference
        assert!(s.choose_open("typed", "  forty "));
        assert_eq!(s.dirty.settings, true);
        let (e, _, _) = s.open_entry().unwrap();
        assert_eq!(e.item.key, "c:0", "wraps to the next undecided word");
        let g = s.group().unwrap();
        assert!(s.entries(&g).iter().any(|x| x.item.via.as_deref() == Some("typed") && shown_str(&x.item) == "forty"));
    }

    #[test]
    fn empty_typed_fix_is_refused_and_touches_nothing() {
        let mut s = session();
        to_maya(&mut s);
        s.sel_word = Some(("m1".into(), "i:0".into()));
        s.take_dirty();
        assert!(!s.choose_open("typed", " $ "));
        assert!(!s.dirty.any());
        assert_eq!(s.page("m1").unwrap().edits().len(), 0);
    }

    #[test]
    fn use_all_then_keep_the_written_word() {
        let mut s = session();
        to_maya(&mut s);
        assert_eq!(s.unresolved_suggestions(), 2);
        s.use_all();
        assert_eq!(s.unresolved_suggestions(), 0);
        assert!(s.page("m2").unwrap().corrected().contains("standard"));
        // choosing "keep what was written" takes the suggestion back out of the corrected text
        s.sel_word = Some(("m1".into(), "c:0".into()));
        assert!(s.choose_open("written", ""));
        assert!(!s.page("m1").unwrap().corrected().contains("confidence"));
        assert_eq!(s.unresolved_suggestions(), 0);
    }

    #[test]
    fn done_marks_reviewed_and_moves_on() {
        let mut s = session();
        assert!(s.done_next());
        assert_eq!(s.selected.as_deref(), Some("m1"));
        assert!(s.page("a1").unwrap().reviewed());
        assert!(s.dirty.pages.contains("a1") && s.dirty.meta);
        assert!(!s.done_next(), "Maya is the last student");
        assert!(s.page("m1").unwrap().reviewed() && s.page("m2").unwrap().reviewed());
    }

    #[test]
    fn flagged_lines_map_words_to_units() {
        let mut s = session();
        to_maya(&mut s);
        let (k, _) = s.flagged_lines(s.page("m1").unwrap());
        assert_eq!(k.get(&0), Some(&Kind::Warn));
        assert_eq!(k.get(&1), Some(&Kind::Bad));
        assert_eq!(k.get(&2), None);
    }

    #[test]
    fn copy_text_uses_labels_and_the_shown_version() {
        let mut s = session();
        to_maya(&mut s);
        let g = s.group().unwrap();
        let t = s.copy_text(&g);
        assert!(t.starts_with("--- Maya R., Page 1 ---\n"));
        assert!(t.contains("--- Maya R., Page 2 ---"));
        s.original = true;
        assert!(s.copy_text(&g).contains("confidance"));
    }

    #[test]
    fn layout_and_selection_persist_in_meta_ui() {
        let mut s = session();
        s.set_split(false);
        to_maya(&mut s);
        let ui = s.a.meta.get("ui").unwrap();
        assert_eq!(ui["layout"], "text");
        assert_eq!(ui["selectedId"], "m1");
        assert!(s.dirty.meta);
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::fixture::session;
    use super::*;

    fn band(y: f32, s: f32) -> Band {
        Band { x: 0.1, y, w: 0.7, h: 0.04, s }
    }

    fn page_without_boxes() -> Session {
        let mut s = session();
        s.a.pages.get_mut("m1").unwrap().set("boxes", json!([]));
        s
    }

    #[test]
    fn scanned_bands_stand_in_for_missing_boxes() {
        let s = page_without_boxes();
        let p = s.page("m1").unwrap();
        let bands = [band(0.10, 5.0), band(0.40, 5.0), band(0.70, 5.0)];
        let (b, approx) = geometry(p, 3, &bands).unwrap();
        assert!(!approx);
        assert_eq!(b.iter().map(|x| x.y).collect::<Vec<_>>(), vec![0.10, 0.40, 0.70]);
    }

    #[test]
    fn a_few_stray_bands_are_dropped_weakest_first() {
        let s = page_without_boxes();
        let p = s.page("m1").unwrap();
        let bands = [band(0.02, 0.5), band(0.10, 5.0), band(0.40, 6.0), band(0.70, 5.0)];
        let (b, approx) = geometry(p, 3, &bands).unwrap();
        assert!(!approx);
        assert_eq!(b.iter().map(|x| x.y).collect::<Vec<_>>(), vec![0.10, 0.40, 0.70]);
    }

    #[test]
    fn otherwise_lines_spread_over_the_ink_extent_not_the_whole_page() {
        let s = page_without_boxes();
        let p = s.page("m1").unwrap();
        // two bands for three lines: spread over where the writing was found
        let bands = [band(0.20, 5.0), band(0.60, 5.0)];
        let (b, approx) = geometry(p, 3, &bands).unwrap();
        assert!(approx);
        assert!(b[0].y >= 0.20 && b[2].y + b[2].h <= 0.64 + 1e-4, "{b:?}");
        // nothing found at all: the fixed margins
        let (b, approx) = geometry(p, 3, &[]).unwrap();
        assert!(approx);
        assert!((b[0].x - 0.08).abs() < 1e-6);
    }
}
