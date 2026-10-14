# Spike S8: the review screen (design/Main.dc.html) in egui, with a hidden-DOM control layer on web

Verdict: **GO-WITH-CAVEATS.** The screen is buildable, and the control layer that makes the web build keyboard
and screen-reader usable costs about one call per widget. Fidelity to the design (type, spacing) is the
larger cost, not accessibility plumbing.

## What it is
`src/lib.rs`, one file, builds native (`cargo run --release --bin s8`) and wasm. Header (student nav, layout
segmented control, Copy, Done), scan pane (placeholder), transcript with flagged words as buttons in three
states (warn / illegible / checked-underline), Corrected/Original toggle, and the "Check this word" tray with
prev/next, "Use suggestion", "Keep what was written", and a typed-fix `TextEdit` for `[illegible]`.

## The control layer (`App::ctl`)
Every interactive widget goes through one funnel: `ctl(ui, &response, id, role, label, pressed) -> bool`.
- reports activation from egui **or** from the DOM twin (so keyboard/AT work with no second code path),
- registers `{id, role, label, pressed, pos}`; the list is sorted by on-screen position (row band, then x) and
  pushed to a hidden `<div id=controls>` only when it changed, reconciled by `data-id` so focus survives,
- draws a 2px ring on the canvas when the twin has DOM focus.
Two delegated listeners (`click`, `focusin`) feed a queue that wakes egui with `request_repaint`.

## Measured (Chrome, SwiftShader, `test.mjs`)
- 15 twin controls at start, correct roles/labels, `aria-pressed` on the toggles; twins appear/disappear
  with state (e.g. `undo` after a fix, `type-word` + `save-typed` for an illegible word).
- Focus on a twin then Enter activates the egui action; screenshot shows the focus ring on the canvas.
- Typing into the egui `TextEdit` (click, type, Enter) works on web and resolves the item.
- Tab order matches visual order after the position sort (it was registration order before: tray first).
- Size: 2.8 MB wasm, 1.24 MB gzipped (no math font; S7's STIX + latex-rust added about 1.4 MB).

## Costs and gaps seen
- **Fidelity.** Only egui's default fonts were used. Instrument Sans / Source Serif 4 need bundling, and the
  design's spacing and rounded segmented controls are hand-built (about 15 lines each).
- **Targets.** Inline word buttons are about 28px tall, under the 44px rule (the design's line-height 1.7 at
  22px has the same problem). Header controls were forced to 44px.
- **Twin is a mirror, not a tree.** Reading order for the transcript text itself still needs S7's
  `a11y_mirror`; no landmarks/headings yet; no `aria-live` for status ("2 words to check").
- **Not verified:** screen readers (NVDA/VoiceOver), mobile soft keyboard, IME composition, clipboard on web
  (`copy_text` needs `--cfg=web_sys_unstable_apis`, eframe warns), native UI Automation tree.
- Headless Chrome needed pointer nudges to trigger egui's on-demand repaint; real browsers do not.

Run: `cargo build --release --target wasm32-unknown-unknown --lib && wasm-bindgen --target web --no-typescript
--out-dir dist target/wasm32-unknown-unknown/release/vishion_s8.wasm` (needs a `dist/index.html` with
`<canvas id=c>` and `<div id=controls class=sr>`), then `node test.mjs`.
