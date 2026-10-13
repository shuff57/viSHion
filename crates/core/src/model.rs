//! Document model. The JSON document is the source of truth (S3 rule 2): every type here wraps a
//! `serde_json` map in file order, so unknown fields, key order and number text round-trip untouched,
//! and typed accessors cover only what the logic reads. Mirrors js/state.js and the page shape in js/ns.js.
use crate::{jsx, lenient};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// UTF-16 code units of `t` as strings (a lone surrogate is a unit of its own, spelled with `marker`).
fn string_units(t: &str, marker: char) -> Vec<String> {
    let mut out = vec![];
    let cs: Vec<char> = t.chars().collect();
    let hex = |i: usize| cs.get(i + 1..i + 5).filter(|h| h.iter().all(char::is_ascii_hexdigit));
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == marker {
            if let Some(h) = hex(i) {
                let s: String = h.iter().collect();
                if u32::from_str_radix(&s, 16).is_ok_and(|v| (0xD800..0xE000).contains(&v)) {
                    out.push(format!("{marker}{s}"));
                    i += 5;
                    continue;
                }
            }
        }
        let mut b = [0u16; 2];
        for u in c.encode_utf16(&mut b).iter() {
            out.push(if c.len_utf16() == 2 { format!("{marker}{u:04x}") } else { c.to_string() });
        }
        i += 1;
    }
    out
}

fn s_of<'a>(m: &'a Map<String, Value>, k: &str) -> &'a str {
    m.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The lone-surrogate marker character of one document (see [`lenient::pick_marker`]). Never serialised
/// and ignored by `==`.
#[derive(Debug, Clone, Copy)]
pub struct Marker(pub char);

impl Default for Marker {
    fn default() -> Self {
        Marker(lenient::SENT)
    }
}

/// A stored project (`<slug>/project.json`), the JS `project.meta`. Real files carry no `id`
/// (the folder slug is the identity), so nothing here is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Project {
    doc: Map<String, Value>,
    #[serde(skip)]
    marker: Marker,
}

impl PartialEq for Project {
    fn eq(&self, o: &Self) -> bool {
        self.doc == o.doc
    }
}

impl Project {
    /// Lenient parse: lone-surrogate escapes are accepted (see [`crate::lenient`]).
    ///
    /// Any valid JSON document is accepted, as JS `Object.assign({defaults}, JSON.parse(text))` does: an
    /// object is the document, `null` / numbers / booleans contribute no keys, an array or a string
    /// contributes its index keys.
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let (v, m): (Value, char) = lenient::parse_marked(s)?;
        let doc = match v {
            Value::Object(m) => m,
            Value::Array(a) => a.into_iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
            Value::String(t) => string_units(&t, m).into_iter().enumerate().map(|(i, u)| (i.to_string(), Value::String(u))).collect(),
            _ => Map::new(),
        };
        Ok(Project { doc, marker: Marker(m) })
    }
    /// Compact form (what pages use).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        lenient::to_string_with(self, false, self.marker.0)
    }
    /// 2-space pretty form, no trailing newline: byte-identical to JS `JSON.stringify(meta, null, 2)`.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        lenient::to_string_with(self, true, self.marker.0)
    }
    /// The lone-surrogate marker of this document.
    pub fn marker(&self) -> char {
        self.marker.0
    }
    /// JS `meta.order.length`: array length, string length in UTF-16 units, `None` (undefined) otherwise.
    pub fn order_len(&self) -> Option<usize> {
        match self.doc.get("order") {
            Some(Value::Array(a)) => Some(a.len()),
            Some(Value::String(t)) => Some(string_units(t, self.marker.0).len()),
            _ => None,
        }
    }
    /// `normalize` of js/fs-projects.js: `Object.assign({defaults}, meta)`. Default keys come first in
    /// default order, then the file's other keys in file order. `now` fills `created` / `updated`.
    pub fn normalized(&self, now: i64) -> Project {
        let mut doc = Map::new();
        doc.insert("schemaVersion".into(), json!(1));
        doc.insert("name".into(), json!("Untitled"));
        doc.insert("topic".into(), json!("General"));
        doc.insert("prompt".into(), json!(""));
        doc.insert("order".into(), json!([]));
        doc.insert("created".into(), json!(now));
        doc.insert("updated".into(), json!(now));
        doc.insert("ui".into(), json!({}));
        doc.insert("stats".into(), Value::Null);
        for (k, v) in &self.doc {
            doc.insert(k.clone(), v.clone());
        }
        Project { doc, marker: self.marker }
    }
    /// New empty document (`{}`).
    pub fn empty() -> Project {
        Project { doc: Map::new(), marker: Marker::default() }
    }
    /// Display name; empty when absent or not a string.
    pub fn name(&self) -> &str {
        s_of(&self.doc, "name")
    }
    pub fn set_name(&mut self, name: &str) {
        self.doc.insert("name".into(), Value::String(name.into()));
    }
    /// Optional `id`; legacy files never have one.
    pub fn id(&self) -> Option<&str> {
        self.doc.get("id").and_then(Value::as_str)
    }
    /// Any field by name.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.doc.get(key)
    }
    pub fn topic(&self) -> &str {
        s_of(&self.doc, "topic")
    }
    pub fn prompt(&self) -> &str {
        s_of(&self.doc, "prompt")
    }
    /// Page ids in display order (`meta.order`), non-strings skipped.
    pub fn order(&self) -> Vec<String> {
        match self.doc.get("order") {
            Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => vec![],
        }
    }
    pub fn set_order(&mut self, ids: &[String]) {
        let arr = ids.iter().cloned().map(Value::String).collect();
        self.doc.insert("order".into(), Value::Array(arr));
    }
    pub fn set(&mut self, key: &str, v: Value) {
        self.doc.insert(key.into(), v);
    }
    pub fn doc(&self) -> &Map<String, Value> {
        &self.doc
    }
}

/// One page record (`pages/<id>.json`).
///
/// `.1` is the storage key the page was loaded from (the `meta.order` entry), never serialised and ignored
/// by `==`. JS saves a page under that key, not under the page's own `id` field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Page(pub Map<String, Value>, #[serde(skip)] pub Option<String>, #[serde(skip)] pub Marker);

impl PartialEq for Page {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}

impl Page {
    /// Lenient parse (lone-surrogate escapes accepted).
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let (mut p, m): (Page, char) = lenient::parse_marked(s)?;
        p.2 = Marker(m);
        Ok(p)
    }
    /// Compact form, byte-identical to JS `JSON.stringify(page)`.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        lenient::to_string_with(self, false, self.2 .0)
    }
    pub fn from_value(v: Value) -> Option<Self> {
        match v {
            Value::Object(m) => Some(Page(m, None, Marker::default())),
            _ => None,
        }
    }
    pub fn to_value(&self) -> Value {
        Value::Object(self.0.clone())
    }
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k)
    }
    /// String field; "" when absent or not a string (JS treats those as falsy in every use here).
    pub fn s(&self, k: &str) -> &str {
        s_of(&self.0, k)
    }
    pub fn set(&mut self, k: &str, v: Value) {
        self.0.insert(k.into(), v);
    }
    pub fn id(&self) -> &str {
        self.s("id")
    }
    /// File key (without `.json`) this page is saved under: the key it was loaded from, else its `id`.
    pub fn file_key(&self) -> &str {
        self.1.as_deref().unwrap_or_else(|| self.id())
    }
    pub fn name(&self) -> &str {
        self.s("name")
    }
    pub fn status(&self) -> &str {
        self.s("status")
    }
    pub fn raw(&self) -> &str {
        self.s("raw")
    }
    pub fn corrected(&self) -> &str {
        self.s("corrected")
    }
    /// `p.corrected || p.raw`.
    pub fn base(&self) -> &str {
        let c = self.corrected();
        if c.is_empty() {
            self.raw()
        } else {
            c
        }
    }
    pub fn reviewed(&self) -> bool {
        jsx::truthy(self.0.get("reviewed"))
    }
    pub fn corrections(&self) -> &[Value] {
        match self.0.get("corrections") {
            Some(Value::Array(a)) => a,
            _ => &[],
        }
    }
    pub fn edits(&self) -> &[Value] {
        match self.0.get("edits") {
            Some(Value::Array(a)) => a,
            _ => &[],
        }
    }
    /// `p.resolved || {}`.
    pub fn resolved(&self) -> Map<String, Value> {
        match self.0.get("resolved") {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        }
    }
    /// `p.student.name` when it is a non-empty string.
    pub fn student_name(&self) -> Option<&str> {
        self.0
            .get("student")
            .and_then(|s| s.get("name"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    }
}

/// App settings (`settings.json` / `ocr_settings_v1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Settings(pub Map<String, Value>);

pub const DEFAULT_PROMPT: &str = concat!(
    "Transcribe the handwriting on this page. Output only the words written on the page, exactly as written, one line of handwriting per line of output.\n",
    "Rules:\n",
    "- Copy spelling, abbreviations, numbers, symbols and punctuation exactly. Do not fix spelling or grammar. Do not reword.\n",
    "- Keep the line breaks, and keep bullets and dashes as written. Write arrows and other symbols as the characters you see (for example \u{2192}), never as LaTeX.\n",
    "- Do not add a title, summary, introduction, explanation, heading or code fence. Do not describe the page or the handwriting.\n",
    "- Do not add any word that is not on the page. Do not repeat a line.\n",
    "- If a word is truly unreadable, write [illegible]. Do not invent a word to fill the gap.\n",
    "- Write ~~word~~ for struck-through text.\n",
    "- Put a margin note on its own line starting with \"Margin:\".\n",
    "- Use LaTeX inside $...$ or $$...$$ only for mathematical formulas.\n",
    "Start with the first line of handwriting and stop after the last."
);

impl Settings {
    /// `DEFAULTS` of js/state.js, key order included.
    pub fn defaults() -> Settings {
        let topics = json!({
            "General": {"description": "Everyday handwriting with no special vocabulary.", "terms": []},
            "Medical": {"description": "Clinical notes, charts and medication names.", "terms": ["hypertension", "tachycardia", "bradycardia", "dyspnea", "edema", "acetaminophen", "ibuprofen", "metformin", "lisinopril", "atorvastatin", "auscultation", "prn", "bid", "tid", "qid"]},
            "Math/Education": {"description": "Lesson plans, student work and mathematics.", "terms": ["derivative", "integral", "quadratic", "polynomial", "coefficient", "theorem", "hypotenuse", "logarithm", "asymptote", "regression", "hypothesis", "rubric", "formative", "summative", "scaffold", "differentiation"]}
        });
        let v = json!({
            "theme": "auto", "ocrUrl": "http://localhost:8188", "judgeUrl": "http://localhost:8189", "judgeMode": "auto",
            "apiKey": "", "demo": false, "storage": "auto", "keepOriginals": false, "timeoutSec": 180,
            "exportFormat": "md", "exportSeparator": "label", "exportText": "corrected",
            "topics": topics, "defaultPrompt": DEFAULT_PROMPT, "noticeSeen": false, "readNames": true, "straighten": true,
            "provider": "local", "model": "", "profiles": {}, "idleUnloadMin": 10, "guessIllegible": true,
            "rememberFixes": true, "reference": []
        });
        match v {
            Value::Object(m) => Settings(m),
            _ => unreachable!(),
        }
    }

    /// `S.loadSettings`: `Object.assign({}, DEFAULTS, saved)`, then `defaultPrompt` is always the current one.
    pub fn load(saved: &Value) -> Settings {
        let mut s = Settings::defaults();
        if let Value::Object(m) = saved {
            for (k, v) in m {
                s.0.insert(k.clone(), v.clone());
            }
        }
        s.0.insert("defaultPrompt".into(), Value::String(DEFAULT_PROMPT.into()));
        s
    }

    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k)
    }
    pub fn set(&mut self, k: &str, v: Value) {
        self.0.insert(k.into(), v);
    }
    /// `settings.rememberFixes !== false`.
    pub fn remember_fixes(&self) -> bool {
        self.0.get("rememberFixes") != Some(&Value::Bool(false))
    }
    /// `settings.reference || []`.
    pub fn reference(&self) -> Vec<Value> {
        match self.0.get("reference") {
            Some(Value::Array(a)) => a.clone(),
            _ => vec![],
        }
    }
    /// Glossary of a topic: `(settings.topics || {})[topic].terms`, `[]` when missing.
    pub fn terms(&self, topic: &str) -> Vec<String> {
        self.0
            .get("topics")
            .and_then(|t| t.get(topic))
            .and_then(|t| t.get("terms"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }
}

/// Page status counts (`S.counts`).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Counts {
    pub total: u32,
    pub done: u32,
    pub error: u32,
    pub pending: u32,
    pub transcribing: u32,
}

/// An open assignment: project meta plus its pages (`S.d.project`).
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub meta: Project,
    pub pages: BTreeMap<String, Page>,
}

impl Assignment {
    pub fn new(meta: Project) -> Self {
        Assignment { meta, pages: BTreeMap::new() }
    }

    /// `S.pages()`: pages in `meta.order`, missing ids skipped.
    pub fn ordered(&self) -> Vec<&Page> {
        self.meta.order().iter().filter_map(|id| self.pages.get(id)).collect()
    }
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.get(id)
    }
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.meta.order().iter().position(|x| x == id)
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for p in self.ordered() {
            c.total += 1;
            match p.status() {
                "done" => c.done += 1,
                "error" => c.error += 1,
                "pending" => c.pending += 1,
                "transcribing" => c.transcribing += 1,
                _ => {}
            }
        }
        c
    }

    /// `S.newPage`.
    pub fn new_page(id: &str, name: &str) -> Page {
        let v = json!({
            "id": id, "name": name, "order": 0, "status": "pending", "raw": "", "corrected": "", "corrections": [],
            "boxes": [], "timings": {"ms": 0, "tokens": 0}, "error": "", "rotation": 0,
            "student": null, "reviewed": false, "resolved": {}, "edits": [], "guesses": {}
        });
        Page::from_value(v).unwrap()
    }

    /// `S.reindex`: `page.order` follows `meta.order`.
    pub fn reindex(&mut self) {
        for (i, id) in self.meta.order().into_iter().enumerate() {
            if let Some(p) = self.pages.get_mut(&id) {
                p.set("order", Value::from(i as i64));
            }
        }
    }

    /// `S.syncStats`: `meta.stats = { pages, done }`.
    pub fn sync_stats(&mut self) {
        let c = self.counts();
        self.meta.set("stats", json!({"pages": c.total, "done": c.done}));
    }

    /// `S.updatePage` without events/saving: assigns the patch; re-syncs stats when `status` changed.
    pub fn update_page(&mut self, id: &str, patch: Map<String, Value>) {
        let Some(p) = self.pages.get_mut(id) else { return };
        let changed = patch.get("status").map_or(false, |s| Some(s) != p.get("status"));
        for (k, v) in patch {
            p.0.insert(k, v);
        }
        if changed {
            self.sync_stats();
        }
    }

    /// `S.addPages` (selection is UI state and not modelled here).
    pub fn add_pages(&mut self, list: Vec<Page>) {
        let mut order = self.meta.order();
        for p in list {
            order.push(p.id().to_string());
            self.pages.insert(p.id().to_string(), p);
        }
        self.meta.set_order(&order);
        self.reindex();
        self.sync_stats();
    }

    /// `S.removePage`.
    pub fn remove_page(&mut self, id: &str) {
        let Some(i) = self.index_of(id) else { return };
        let mut order = self.meta.order();
        order.remove(i);
        self.pages.remove(id);
        self.meta.set_order(&order);
        self.reindex();
        self.sync_stats();
    }

    /// `S.movePage`.
    pub fn move_page(&mut self, from: i64, to: i64) {
        let mut o = self.meta.order();
        let n = o.len() as i64;
        if from == to || from < 0 || to < 0 || from >= n || to >= n {
            return;
        }
        let x = o.remove(from as usize);
        o.insert(to as usize, x);
        self.meta.set_order(&o);
        self.reindex();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape written by the JS app's createProject + a few saves: no id, name not first, nested stats.
    const LEGACY: &str = r#"{
  "prompt": "",
  "name": "Period 3 Quiz",
  "created": 1760000000000,
  "updated": 1760000123456,
  "stats": {
    "pages": 2,
    "done": 1
  },
  "order": [
    "a1",
    "b2"
  ],
  "futureField": {
    "z": 1,
    "a": 0.30000000000000004
  },
  "big": 12345678901234567890123
}"#;

    #[test]
    fn legacy_project_without_id_parses_and_round_trips_byte_identical() {
        let p = Project::from_json(LEGACY).expect("legacy project.json has no id");
        assert_eq!(p.id(), None);
        assert_eq!(p.name(), "Period 3 Quiz");
        assert_eq!(p.order(), vec!["a1", "b2"]);
        assert_eq!(p.to_json_pretty().unwrap(), LEGACY);
    }

    #[test]
    fn compact_round_trip_keeps_order_and_big_numbers() {
        let src = r#"{"zeta":1,"name":"Quiz","alpha":{"b":2,"a":1},"big":12345678901234567890123}"#;
        let p = Project::from_json(src).unwrap();
        assert_eq!(p.to_json().unwrap(), src);
    }

    #[test]
    fn missing_or_non_string_name_is_empty() {
        assert_eq!(Project::from_json("{}").unwrap().name(), "");
        assert_eq!(Project::from_json(r#"{"name":5}"#).unwrap().name(), "");
    }

    #[test]
    fn set_name_keeps_position() {
        let mut p = Project::from_json(r#"{"a":1,"name":"x","b":2}"#).unwrap();
        p.set_name("y");
        assert_eq!(p.to_json().unwrap(), r#"{"a":1,"name":"y","b":2}"#);
    }

    #[test]
    fn lone_surrogate_documents_load_and_write_back_identically() {
        let page = r#"{"id":"p","raw":"a\ud800b","n":1e+21}"#;
        let mut p = Page::from_json(page).expect("lone surrogate page must load");
        assert_eq!(p.to_json().unwrap(), page);
        p.set("reviewed", json!(true));
        assert_eq!(p.to_json().unwrap(), r#"{"id":"p","raw":"a\ud800b","n":1e+21,"reviewed":true}"#);
        let meta = "{\n  \"name\": \"n\\udfff\"\n}";
        assert_eq!(Project::from_json(meta).unwrap().to_json_pretty().unwrap(), meta);
    }

    #[test]
    fn normalized_puts_defaults_first_like_js() {
        let p = Project::from_json(r#"{"name":"h","order":["a"],"extra":1,"stats":{"pages":1}}"#).unwrap();
        assert_eq!(
            p.normalized(7).to_json().unwrap(),
            r#"{"schemaVersion":1,"name":"h","topic":"General","prompt":"","order":["a"],"created":7,"updated":7,"ui":{},"stats":{"pages":1},"extra":1}"#
        );
    }

    #[test]
    fn explicit_id_is_still_read() {
        assert_eq!(Project::from_json(r#"{"id":"p1"}"#).unwrap().id(), Some("p1"));
    }

    #[test]
    fn unknown_page_fields_survive_a_patch() {
        let mut a = Assignment::new(Project::from_json(r#"{"name":"A","order":[]}"#).unwrap());
        let mut p = Assignment::new_page("p1", "one.png");
        p.set("futureField", json!({"z": 1}));
        a.add_pages(vec![p]);
        let mut patch = Map::new();
        patch.insert("status".into(), json!("done"));
        a.update_page("p1", patch);
        assert_eq!(a.meta.get("stats"), Some(&json!({"pages": 1, "done": 1})));
        assert_eq!(a.page("p1").unwrap().get("futureField"), Some(&json!({"z": 1})));
    }
}
