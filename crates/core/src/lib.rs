//! viSHion core: pure Rust, no UI, no I/O. Must never depend on egui, web-sys or tokio.
//!
//! Phase 0 holds only the document model skeleton that proves the S3 serde rules; later phases add
//! words, judge, queue, render pipeline, export, imaging and the SSE parser (see docs/RUST-REWRITE-PLAN.md).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A stored project (`<slug>/project.json`). The document is the source of truth (S3 rule 2): the whole
/// JSON object is kept in file order, so unknown fields, key order and number text round-trip untouched.
/// Typed accessors cover only what the UI reads. Real files carry no `id` (the folder slug is the identity),
/// so nothing here is required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Project {
    doc: Map<String, Value>,
}

impl Project {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// Compact form (what pages use).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// 2-space pretty form, no trailing newline: byte-identical to JS `JSON.stringify(meta, null, 2)`.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Display name; empty when absent or not a string.
    pub fn name(&self) -> &str {
        self.doc.get("name").and_then(Value::as_str).unwrap_or("")
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
}

/// Build version of the shared core.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
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
    fn explicit_id_is_still_read() {
        assert_eq!(Project::from_json(r#"{"id":"p1"}"#).unwrap().id(), Some("p1"));
    }
}
