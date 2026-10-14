//! S4 spike: streaming chat-completions client in Rust/WASM. `sse` is pure; `web` is the thin shell.
pub mod sse;
#[cfg(target_arch = "wasm32")]
mod web;
