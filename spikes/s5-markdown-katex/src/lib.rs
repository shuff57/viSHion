//! S5 spike: markdown-it 14.1.0 + KaTeX 0.16.9 driven from Rust/WASM via wasm-bindgen externs.
pub mod pipeline;
#[cfg(target_arch = "wasm32")]
mod web;
