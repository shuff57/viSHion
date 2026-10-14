//! viSHion core: pure Rust, no UI, no I/O. Must never depend on egui, web-sys or tokio.
//!
//! Phase 1 port of the JS engine's pure logic (see docs/RUST-REWRITE-PLAN.md and DEVIATIONS.md):
//! `model` (project/page/settings as JSON documents), `words`, `judge`, `groups`, `student`, `guess`,
//! `clean` (model-wrapper stripping), `render` (units, math hold, pieces), `export`, `idle`.
//! `jsx` reproduces the JavaScript semantics the ports depend on.
pub mod clean;
pub mod demo;
pub mod export;
pub mod groups;
pub mod guess;
pub mod idle;
pub mod import;
pub mod judge;
mod hanfold;
pub mod jsx;
pub mod lenient;
pub mod model;
pub mod render;
pub mod v8sort;
pub mod student;
pub mod words;

pub use model::{Assignment, Page, Project, Settings};

/// Build version of the shared core.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
