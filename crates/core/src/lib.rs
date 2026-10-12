//! viSHion core: pure Rust, no UI, no I/O. Must never depend on egui, web-sys or tokio.
//!
//! Phase 0 holds only the document model skeleton that proves the S3 serde rules; later phases add
//! words, judge, queue, render pipeline, export, imaging and the SSE parser (see docs/RUST-REWRITE-PLAN.md).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A stored project. Unknown fields are kept in `extra` and written back in their original order (S3 rule 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Project {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Build version of the shared core.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_and_order_round_trip() {
        let src = r#"{"id":"p1","zeta":1,"name":"Quiz","alpha":{"b":2,"a":1},"big":12345678901234567890123}"#;
        let p = Project::from_json(src).unwrap();
        assert_eq!(p.id, "p1");
        assert_eq!(p.name, "Quiz");
        assert_eq!(
            p.to_json().unwrap(),
            r#"{"id":"p1","name":"Quiz","zeta":1,"alpha":{"b":2,"a":1},"big":12345678901234567890123}"#
        );
    }

    #[test]
    fn missing_name_defaults() {
        assert_eq!(Project::from_json(r#"{"id":"x"}"#).unwrap().name, "");
    }
}
