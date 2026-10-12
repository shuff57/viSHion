# Spike S7: egui (eframe 0.28) for desktop + web from one codebase

Verdict: **GO-WITH-CAVEATS on rendering, OPEN RISK on accessibility.** One `src/lib.rs` builds native
(`cargo run --release --bin s7`) and wasm (`wasm32-unknown-unknown` + `wasm-bindgen --target web`) and renders
the S5 transcript corpus with typeset math. Screen-reader access on the web build is not there.

## What was measured
- **Pipeline** mirrors `js/render.js`: `units()`, `$$`/`$` held as placeholders, inline markdown via
  pulldown-cmark (bold, italic, strike, code), `[illegible]` styled, math via `latex-rust` 2.1.1 (egui
  feature, STIX Two Math, pinned to egui 0.28) cached per (tex, display, size).
- **Corpus** (`corpus.json`, from S5): 64 transcripts, 145 units, 38 math expressions. 30 render. The 8
  failures: 6 are also KaTeX errors (broken/undefined TeX, `\def` loop, `x&y`), so they fall back to source
  text exactly as render.js does. Real gaps: bare `\` and CJK inside `\text{}` (STIX has no CJK glyphs).
- **Handwriting-style set** (`math.txt`, 26 expressions: integrals, sums, limits, roots, pmatrix/bmatrix,
  aligned, cases, accents, binom, \boxed, \xrightarrow...): 25 render. **`\pmod` and `\bmod` are
  unsupported** (KaTeX has both).
- **Web build**: 4.25 MB wasm, 2.1 MB gzipped (S5's JS-glue route was 68 KB wasm plus vendored JS). Boots in
  Chrome (SwiftShader WebGL2), screenshots in `out-*.png`.
- **Accessibility probe** (`shot.mjs`): the page has 9 DOM nodes, 0 `role`/`aria-label`, one hidden `<input>`
  (eframe's text agent). Everything else is canvas pixels. A screen reader sees an empty page.

## Not verified
- Native AccessKit output (feature is on, never exercised).
- Word-level correction highlights (U+E000..E002 markers from `words.js`) with click targets.
- Baseline alignment of inline math (it is vertically centred, visibly a little off).
- IME, mobile soft keyboard, text selection/copy of rendered math, Safari/Firefox WebGL.
- egui repaints on demand: headless Chrome needed synthetic mouse moves before the first frame showed.

## Gotchas
- `latex-rust` ties you to egui 0.28 (optional dep), so eframe 0.28 too.
- eframe 0.28 `WebRunner::start` takes the canvas id string.
- Name the bin differently from the lib or cargo warns about output filename collisions.

Run: `cargo run --release --bin s7 -- --report`, `... -- --tex math.txt`, then
`cargo build --release --target wasm32-unknown-unknown --lib && wasm-bindgen --target web --no-typescript --out-dir dist target/wasm32-unknown-unknown/release/vishion_s7.wasm && node shot.mjs`
