# Vendored crates

## eframe 0.28.1

Copy of the crates.io 0.28.1 release with one line changed in `src/web/mod.rs` (`set_clipboard_text`):
`window.navigator().clipboard()` returns `Clipboard` (not `Option<Clipboard>`) since web-sys 0.3.77, which
broke the build once `--cfg=web_sys_unstable_apis` is on. That cfg is what enables the web copy path
(`navigator.clipboard.writeText`); without it, copy/cut do nothing on the web build.

Wired in with `[patch.crates-io]` in the root `Cargo.toml`. Remove both when eframe is upgraded.

## latex-rust 2.1.1

Copy of the crates.io release (src, data, fonts, licences; tests, goldens and benches left out) with one change in
`Cargo.toml`: the optional `egui` dependency has `default-features = false`. With the default, any build that
enables latex-rust's `egui` feature also enables egui's `default_fonts`, which embeds 1.4 MB of faces (Hack,
Ubuntu-Light, NotoEmoji, emoji-icon-font) that the review screen does not use. Source is untouched, so the math
output is byte-identical (checked by `ui::math` tests).

Wired in with `[patch.crates-io]`. Drop it when latex-rust makes the egui features default-less upstream or when
the pin to egui 0.28 is lifted.
