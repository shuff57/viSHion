# Vendored crates

## eframe 0.28.1

Copy of the crates.io 0.28.1 release with one line changed in `src/web/mod.rs` (`set_clipboard_text`):
`window.navigator().clipboard()` returns `Clipboard` (not `Option<Clipboard>`) since web-sys 0.3.77, which
broke the build once `--cfg=web_sys_unstable_apis` is on. That cfg is what enables the web copy path
(`navigator.clipboard.writeText`); without it, copy/cut do nothing on the web build.

Wired in with `[patch.crates-io]` in the root `Cargo.toml`. Remove both when eframe is upgraded.
