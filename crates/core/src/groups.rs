//! Port of js/groups.js: pages grouped by `page.student.name`, and the status of each group.
//! A page with no name is its own row, named after the file. Read-only over the pages.
use crate::jsx::{self, nat_compare, split_ws};
use crate::model::Page;
use crate::words;
use serde_json::{json, Value};

/// Group status, worst first: failed > processing > queued/waiting > check > ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Failed,
    Processing,
    Queued,
    Waiting,
    Check,
    Ready,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Failed => "failed",
            Status::Processing => "processing",
            Status::Queued => "queued",
            Status::Waiting => "waiting",
            Status::Check => "check",
            Status::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: String,
    pub name: String,
    /// Page ids in encounter order.
    pub pages: Vec<String>,
    pub open: usize,
    pub words: usize,
    pub failed: Vec<String>,
    pub status: Status,
    pub source: Option<Value>,
}

impl Group {
    pub fn to_value(&self) -> Value {
        json!({"key": self.key, "name": self.name, "pages": self.pages, "open": self.open,
               "words": self.words, "failed": self.failed, "status": self.status.as_str(),
               "source": self.source.clone().unwrap_or(Value::Null)})
    }
}

/// `stem`: file name without its extension, "Untitled" when missing.
fn stem(name: Option<&Value>) -> String {
    let n = if jsx::truthy(name) { jsx::to_string(name.unwrap()) } else { "Untitled".to_string() };
    // .replace(/\.[a-z0-9]+$/i, '')
    if let Some(dot) = n.rfind('.') {
        let ext = &n[dot + 1..];
        if !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return n[..dot].to_string();
        }
    }
    n
}

fn student_name(p: &Page) -> Option<&str> {
    p.student_name()
}

/// `G.nameOf`.
pub fn name_of(p: &Page) -> String {
    if let Some(n) = student_name(p) {
        let t = jsx::trim(n);
        if !t.is_empty() {
            return t.to_string();
        }
    }
    stem(p.get("name"))
}

/// `G.keyOf`.
pub fn key_of(p: &Page) -> String {
    if let Some(n) = student_name(p) {
        if !jsx::trim(n).is_empty() {
            return format!("n:{}", jsx::collapse_ws(&jsx::trim(n).to_lowercase()));
        }
    }
    format!("p:{}", p.id())
}

fn word_count(p: &Page) -> usize {
    if p.status() == "done" {
        split_ws(&words::effective(p)).len()
    } else {
        0
    }
}

fn status_of(pages: &[&Page], open: usize, is_queued: &dyn Fn(&str) -> bool) -> Status {
    if pages.iter().any(|p| p.status() == "error") {
        Status::Failed
    } else if pages.iter().any(|p| p.status() == "transcribing") {
        Status::Processing
    } else if pages.iter().any(|p| p.status() == "pending") {
        if pages.iter().any(|p| is_queued(p.id())) {
            Status::Queued
        } else {
            Status::Waiting
        }
    } else if open > 0 && !pages.iter().all(|p| p.reviewed()) {
        Status::Check
    } else {
        Status::Ready
    }
}

/// `G.list`: groups of the given pages (already in `meta.order`), sorted by natural name order.
pub fn list(pages: &[&Page], is_queued: &dyn Fn(&str) -> bool) -> Vec<Group> {
    let mut by: Vec<(String, String, Vec<&Page>)> = vec![];
    for p in pages {
        let k = key_of(p);
        match by.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.2.push(p),
            None => by.push((k, name_of(p), vec![p])),
        }
    }
    let mut out: Vec<Group> = by
        .into_iter()
        .map(|(key, name, ps)| {
            let open = ps.iter().map(|p| words::count(p)).sum();
            let status = status_of(&ps, open, is_queued);
            let source = ps[0]
                .get("student")
                .and_then(|s| s.get("source"))
                .filter(|v| jsx::truthy(Some(v)))
                .cloned();
            Group {
                key,
                name,
                open,
                words: ps.iter().map(|p| word_count(p)).sum(),
                failed: ps.iter().filter(|p| p.status() == "error").map(|p| p.id().to_string()).collect(),
                status,
                source,
                pages: ps.iter().map(|p| p.id().to_string()).collect(),
            }
        })
        .collect();
    out.sort_by(|a, b| nat_compare(&a.name, &b.name));
    out
}

/// `G.of`: the group a page belongs to.
pub fn of<'a>(p: &Page, groups: &'a [Group]) -> Option<&'a Group> {
    let k = key_of(p);
    groups.iter().find(|g| g.key == k)
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Tally {
    pub total: usize,
    pub ready: usize,
    pub check: usize,
    pub failed: usize,
    pub processing: usize,
}

/// `G.tally`: "processing" covers queued and waiting groups too.
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

/// `G.matches`: filter tab test.
pub fn matches(g: &Group, f: &str) -> bool {
    if f == "all" {
        return true;
    }
    if f == "processing" {
        return matches!(g.status, Status::Processing | Status::Queued | Status::Waiting);
    }
    g.status.as_str() == f
}

/// `G.names`: names already used in this assignment.
pub fn names(pages: &[&Page], groups: &[Group]) -> Vec<String> {
    groups
        .iter()
        .filter(|g| {
            g.pages.iter().any(|id| {
                pages.iter().any(|p| p.id() == id && jsx::truthy(p.get("student").and_then(|s| s.get("name"))))
            })
        })
        .map(|g| g.name.clone())
        .collect()
}
