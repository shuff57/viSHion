# Vendored crates

## eframe 0.28.1

Copy of the crates.io 0.28.1 release with one line changed in `src/web/mod.rs` (`set_clipboard_text`):
`window.navigator().clipboard()` returns `Clipboard` (not `Option<Clipboard>`) since web-sys 0.3.77, which
broke the build once `--cfg=web_sys_unstable_apis` is on. That cfg is what enables the web copy path
(`navigator.clipboard.writeText`); without it, copy/cut do nothing on the web build.

Two more changes in `src/web/events.rs`: the document `copy` listener returns early (no `stop_propagation`, no
`preventDefault`) when the page's DOM transcript (`.tx-clip`) holds a non-empty selection, so the browser copies
it (the listener used to swallow every copy, and Ctrl+C on selected transcript text did nothing); and the
`wheel` listener returns early on `ctrl_key` (browser page zoom, WCAG 1.4.4, and a trackpad pinch) instead of
turning it into an egui zoom and calling `preventDefault`.

Wired in with `[patch.crates-io]` in the root `Cargo.toml`. Remove both when eframe is upgraded.

## latex-rust 2.1.1

Copy of the crates.io release (src, data, fonts, licences; tests, goldens and benches left out) with one change in
`Cargo.toml`: the optional `egui` dependency has `default-features = false`. With the default, any build that
enables latex-rust's `egui` feature also enables egui's `default_fonts`, which embeds 1.4 MB of faces (Hack,
Ubuntu-Light, NotoEmoji, emoji-icon-font) that the review screen does not use. Source is untouched, so the math
output is byte-identical (checked by `ui::math` tests).

Wired in with `[patch.crates-io]`. Drop it when latex-rust makes the egui features default-less upstream or when
the pin to egui 0.28 is lifted.

## egui_glow 0.28.1

Copy of the crates.io release (src, README, `Cargo.toml` without the examples) with one addition in
`src/painter.rs`: the font atlas is converted with a coverage gamma the app sets (`egui_glow::set_font_gamma`,
default 1.0) instead of egui's fixed 0.55. At 0.55 dark text on the light theme is visibly heavier than the
browser's rendering of the same face at the same size (the review screen's transcript is the compared
surface), while light text on the dark theme wants a boost, so `ui::theme` sets the gamma per theme and sets
the fonts again to rebuild the atlas. Wired in with `[patch.crates-io]`. Drop it when egui exposes the gamma.

## epaint 0.28.1

Copy of the crates.io release (src, README, `Cargo.toml`; the font files are only used by `default_fonts`, which
the app does not enable) with two changes. `LayoutJob::default()` has `round_output_size_to_nearest_ui_point:
false` (a galley is as wide as its text, not rounded to a whole point). And one line is removed in
`src/text/text_layout.rs`: `layout_section` rounded the
cursor to a whole pixel after every glyph, which makes each glyph advance an integer (a 14px face comes out up
to 5% wider or narrower than a browser sets it, so buttons, chips and the header land a few pixels off the
design). Advances now stay fractional and each glyph is still snapped to a pixel when the row is placed, so
widths match the browser's. The size handed to the font is also not rounded when advances are computed (the glyph
bitmaps still are), and `src/text/kern.rs` (new) reads GPOS pair kerning (`kern` feature, via `owned_ttf_parser`),
which epaint ignores (it only reads the legacy `kern` table); the two Latin families only have GPOS kerning.
Together the widths of Instrument Sans and Source Serif 4 strings are within 0.6 px of what Chrome reports
(`app_tests::round4_text_is_as_wide_as_the_browser_sets_it`). Wired in with `[patch.crates-io]`. Drop it when
egui exposes the setting.

Sub-pixel text: every glyph is rasterised four times, shifted by 0, 1/4, 1/2 and 3/4 of a pixel (`GlyphInfo::sub_uv`, `Glyph::sub_uv`), and a row snaps each glyph to the nearest quarter pixel (`text_layout.rs`, `halign_and_justify_row`) instead of the nearest whole pixel, as a browser positions text. Whole-pixel snapping moved letters by up to half a pixel and closed or opened the gaps between words ("Transcribe this page again" read as "Transcribethis"). The atlas holds four times as many glyph bitmaps.

## accesskit_consumer 0.16.1

Copy of the crates.io release (src, README, `Cargo.toml`) with one block of `tree.rs` (`State::update`) replaced.
When a node changes parent in an update, the old parent's dropped child was marked an orphan and the whole subtree
under every orphan was removed afterwards, even when another parent of the same update had taken the child over.
egui builds the update from a hash map, so whether the child survived depended on the order of the nodes. The
native build moves nodes between parents all the time (a button under the banner goes under a hidden container
when a dialog opens), and when the moved node was the focus the consumer asserted
(`self.nodes.contains_key(&self.focus)`, `tree.rs:35`) in the window thread: the window died as soon as a UI
Automation client (Narrator, NVDA) was attached and a dialog was opened from the keyboard. Now what is removed is
what the root no longer reaches. `app_tests::native_dialog_opened_from_the_keyboard_keeps_focus_in_the_tree`
feeds the same updates to a consumer. Wired in with `[patch.crates-io]` (accesskit_windows, accesskit_winit and the
test consumer all take it). Drop it when accesskit is upgraded past the fix (egui pin, plan rule 6).
