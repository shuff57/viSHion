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
    /// Text handed to `choose` (typed guesses only).
    pub text: String,
    pub on: bool,
}

#[derive(Default, Debug)]
pub struct Dirty {
    pub pages: BTreeSet<String>,
    pub meta: bool,
    pub settings: bool,
}

impl Dirty {
    pub fn any(&self) -> bool {
        !self.pages.is_empty() || self.meta || self.settings
    }
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
        };
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

    pub fn pill(&self, g: &Group) -> (Kind, String) {
        match g.status {
            Status::Failed => (Kind::Bad, "Couldn\u{2019}t transcribe".into()),
            Status::Processing => (Kind::Warn, "Transcribing".into()),
            Status::Queued | Status::Waiting => (Kind::Warn, "Waiting".into()),
            _ => {
                let n: usize = g.pages.iter().filter_map(|id| self.a.page(id)).map(words::count).sum();
                if n > 0 {
                    return (Kind::Warn, format!("{} to check", plural(n, "word", "words")));
                }
                let any = g.pages.iter().filter_map(|id| self.a.page(id)).any(|p| !words::items(p).is_empty());
                (Kind::Ok, if any { "All words checked" } else { "No words to check" }.into())
            }
        }
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
                    text: String::new(),
                    on: it.via.as_deref() == Some("suggested"),
                },
                Alt {
                    label: word_str(it),
                    hint: "Keep it as read from the page".into(),
                    via: "written",
                    text: String::new(),
                    on: it.via.as_deref() == Some("written"),
                },
            ];
        }
        let mut out = vec![];
        let guesses = self.a.page(&e.page).and_then(|p| p.get("guesses")).and_then(|g| g.get(&it.key)).and_then(Value::as_array);
        for g in guesses.map(|v| v.as_slice()).unwrap_or(&[]) {
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
                text,
            });
        }
        out.push(Alt {
            label: "[illegible]".into(),
            hint: "Keep the marker so you can find it later".into(),
            via: "marked",
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

    pub fn pick_word(&mut self, page: &str, key: &str) {
        self.sel_word = Some((page.to_string(), key.to_string()));
        self.tray_open = true;
        if self.selected.as_deref() != Some(page) {
            self.select(page);
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
        let Some(p) = self.a.pages.get_mut(page) else { return false };
        if !words::choose(p, key, via, text, &mut self.settings, &topic, self.now) {
            return false;
        }
        self.dirty.pages.insert(page.to_string());
        if self.settings.get("reference") != before.as_ref() {
            self.dirty.settings = true;
        }
        true
    }

    /// `Use all N suggestions` for the current student.
    pub fn use_all(&mut self) {
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

    pub fn select(&mut self, id: &str) {
        if !self.a.pages.contains_key(id) || self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_string());
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

    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::take(&mut self.dirty)
    }
}

/// A transcript line's box on the page, as fractions of the image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where each of the `n` transcript lines sits on the page. Exact when the scan found as many lines as the
/// transcript has (`approx = false`); otherwise the lines are spread evenly over the writing area so a page
/// always gets its underline (`approx = true`), as review.js `geometry` does when the ink scan is unusable.
pub fn geometry(p: &Page, n: usize) -> Option<(Vec<BBox>, bool)> {
    if n == 0 || p.status() != "done" {
        return None;
    }
    let boxes: Vec<BBox> = match p.get("boxes") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|b| {
                let f = |k: &str| b.get(k).and_then(Value::as_f64).map(|v| v as f32);
                Some(BBox { x: f("x")?, y: f("y")?, w: f("w")?, h: f("h")? })
            })
            .collect(),
        _ => vec![],
    };
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
    let (top, bottom, left, right) = (0.08f32, 0.92f32, 0.08f32, 0.92f32);
    let step = (bottom - top) / n as f32;
    Some((
        (0..n)
            .map(|i| BBox { x: left, y: top + i as f32 * step + step * 0.1, w: right - left, h: step * 0.8 })
            .collect(),
        true,
    ))
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
        assert_eq!((k, label.as_str()), (Kind::Warn, "3 words to check"));
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
