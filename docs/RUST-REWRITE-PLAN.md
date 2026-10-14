# viSHion Rust rewrite plan

Status: draft, based on spikes S1 to S8 in `spikes/` (all committed on `rust-rewrite`).
The current JS app (`index.html`, `js/`) stays the oracle until the rewrite passes parity.

## Decision record

| Decision | Choice | Why (spike) |
|---|---|---|
| UI toolkit | **egui / eframe 0.28**, one codebase for desktop and web | Same `lib.rs` built native + wasm and rendered the review screen (S7, S8). Replaces the Tauri/Leptos track. |
| Math | `latex-rust` egui backend (STIX Two Math) | 25 of 26 typical handwriting expressions and every non-broken corpus case render (S7). |
| Inline markdown | `pulldown-cmark` (inline only) feeding egui | The JS renderer only uses `renderInline`; no HTML needed (S5, S7). |
| Accessibility on web | Hidden DOM twin of every control and the transcript | egui web has no accessibility tree in 0.28; the twin costs one call per widget (S7 mirror, S8 `ctl()`). |
| Storage | Rust data layer behind a trait: IndexedDB / File System Access on web, `std::fs` on desktop | Must stay byte-compatible with existing projects (S3 rules below). |
| Streaming | Rust SSE parser (pure) over `ureq` natively and `fetch` on the web | Matches `ocr.js` in 32 of 33 scenarios, one fix known (S4); the fix (flush the tail, ignore empty `data:`) is in. Built in phase 3. |
| Imaging | Pure Rust `imaging-core`, no wasm-specific code | Bit-exact vs JS on every fixture (S1). Resampling stays platform canvas on web, `image` crate on desktop. |
| Renderer | **glow** (OpenGL natively, WebGL2 on the web) by default; **wgpu** is an opt-in cargo feature | wgpu renders the same natively (screenshots match; `egui-wgpu` is vendored with the font-gamma knob). On the web eframe 0.28 uses wgpu only with glow off, and that build is WebGPU-only with no WebGL fallback, so web stays on glow. Build: `--no-default-features --features wgpu`, web `node build.mjs --wgpu`. |
| Hosting | Static assets on Cloudflare Workers, as today | S2: needs `'wasm-unsafe-eval'` + one bootstrap hash in `script-src`. |

**Hard consequence:** the web build cannot run from `file://` (wasm fetch needs http). The desktop build
replaces that use case. This overrides ground rule "must keep working from `file://`" in `HANDOFF.md`.

## Workspace layout

```
crates/
  core/      pure Rust, no UI, no I/O. Compiles native + wasm32.
             model (project, page, settings), words (flagged items, edits), judge, queue,
             render pipeline (units, hold_math, pieces), export, imaging-core, sse parser
  store/     trait Store { get, put, list, delete } + impls: idb (web), fsa (web folder), fs (native)
  net/       OCR client (chat completions streaming) over reqwest, wasm + native
  ui/        egui app: screens, theme, ctl() control layer, math cache, a11y twin (web only)
  app/       thin bins: native `main.rs`, web `start()` + index.html + wrangler config
```

Rule: `core` never imports egui, web-sys or tokio. Everything platform-specific enters through
`store`, `net`, and `ui`.

## Module map (JS to Rust)

| JS (lines) | Rust | Notes |
|---|---|---|
| `ns.js` 78, `state.js` 158 | `core::model`, `ui::state` | Document is `serde_json::Value` source of truth with typed accessors (S3 rule 2). |
| `words.js` 188, `judge.js` 98, `guess.js` 108, `groups.js` 64, `student.js` 67 | `core::words`, `core::judge`, `core::groups` | Port tests first (`tests/*.test.js` become Rust tests with the same inputs). |
| `ocr.js` 151, `queue.js` 105 | `net::ocr`, `core::queue` | Enable SSE `flush_tail`, ignore empty `data:` payloads (S4). |
| `image.js` 158, `deskew.js` 92 | `core::imaging` | S1 harness stays as regression gate. |
| `render.js` 117 | `ui::render` | S7 pipeline; overlay/hover-sync become egui hit-testing. |
| `fs.js` 249, `fs-projects.js` 199, `fs-legacy.js` 83 | `store::*`, `core::import` | IDB DB `ocr_fs` must open at version 1 (S3 rule 4). |
| `export.js` 135 | `core::export` | zip + markdown/text/json outputs, byte-compare against JS. |
| `models.js` 219 | `net::models` | |
| `js/ui/*` | `ui::screens::{home,inbox,add,review,settings,export,assignment}` | One screen per phase; follow `design/*.dc.html`. |

## Rules carried over from the spikes

1. **serde_json** with `preserve_order` and `arbitrary_precision`; unknown fields preserved via flattened
   `extra`; handle lone surrogates or leave the document untouched (S3).
2. **Open `ocr_fs` at version 1 exactly**; create both stores if missing (S3).
3. **Every interactive widget goes through `ctl()`** so web gets a twin, focus ring and keyboard activation
   for free; twins are keyed by id and sorted by on-screen position (S8).
4. **Math fallback**: on layout error show source in monospace red, as `render.js` does (S7).
5. **CSP**: add `'wasm-unsafe-eval'` and the bootstrap hash; keep `connect-src` as is (S2). Generate the hash
   at build (`build.mjs` pattern from S2).
6. **Pin egui/eframe to 0.28** while `latex-rust` requires it; revisit on upgrade.

## Phases

Each phase ends with a runnable build on both targets and a parity check against the JS oracle.

| # | Phase | Deliverable | Exit criteria |
|---|---|---|---|
| 0 | Workspace + CI | Cargo workspace, native + wasm builds, `wrangler dev` serving the web build with prod CSP | Both targets boot an empty app; CI runs `cargo test` and the CSP check. |
| 1 | `core` data + words | model, words, judge, groups, render pipeline, export | Ported JS tests pass; S5 corpus gives identical units/pieces; export bytes match JS on fixtures. |
| 2 | `store` | idb + fsa + native fs, legacy import | S3 `run.mjs` oracle test passes on all modes; old projects open unchanged in both directions. |
| 3 | `net` + queue | streaming client, run/stop/retry/resume | S4 scenario table passes; demo mode works end to end. **Done** for the client: mock-server tests cover streaming, partial on a dropped stream, 503, dead port and Stop; demo mode unchanged. `/slots` progress polling is in (`net::slots`, native thread and web `select`; tested against a mock). The judge-model Guess works outside demo mode (`net::complete`, `core::guess::prompt`; tested against a mock). Not done: a run of the web transport against a real server. |
| 4 | Review screen | Main screen with math, tray, strip, hidden twin | S8 test suite passes; NVDA smoke test; 44px targets. |
| 5 | Home, inbox, add | remaining screens, drag-reorder, rotate/replace page | Feature list in `README.md` screens table complete. |
| 6 | Dialogs | settings, assignment settings, export, topics | Same fields as today; export parity. |
| 7 | Polish + parity | fonts bundled, dark theme, reduced motion, contrast audit | Side-by-side walkthrough vs JS app with no feature gaps. |
| 8 | Cutover | deploy web build to the existing route, ship desktop installers | JS app kept on a `/legacy` path for one release. |

Rough order of risk: phase 4 first (earliest real validation of the UI), then 2 (data loss is the worst failure).
If staffing is one person, do 0, 1, 4, then 2, 3, 5, 6, 7.

## Test strategy

- **Oracle tests** reuse the spike harnesses: S1 `harness/run.js` (imaging), S3 `run.mjs` (storage), S4
  scenario table (streaming), S5 corpus (render pipeline). Wire each into CI as it lands.
- **Golden screenshots** of the review screen in light and dark for native and web.
- **Accessibility**: `ariaSnapshot` assertions on the twin (from `spikes/s8-review/test.mjs`), plus a manual
  NVDA and VoiceOver pass per release.
- **Migration**: a fixture project exported by the JS app must open, edit, and re-export in Rust, then open
  again in the JS app.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Real screen readers behave differently from the Chrome tree | web a11y claim fails | NVDA/VoiceOver check in phase 4, before building the other screens. Fallback: Leptos DOM UI over the same `core`. |
| Data loss from storage changes | worst case | S3 rules, oracle tests, never migrate in place; keep reading old schema. |
| Visual fidelity with default egui styling | design drift | bundle Instrument Sans and Source Serif 4; build a small theme crate from `css/tokens`. |
| `latex-rust` coverage (`\pmod`, `\bmod`, CJK in `\text`) | wrong math | upstream or patch; keep the red-source fallback. |
| wasm size (about 4 MB, 2 MB gzipped with math font) | slow first load on weak networks | lazy-load the math font, `opt-level=s`, wasm-opt, long cache headers. |
| Mobile web keyboard / IME on canvas | poor phone editing | out of scope for v1 (`Phone.dc.html` is already "later"). |
| egui and `latex-rust` version pinning | upgrade friction | isolate math behind `ui::math`, so it can be swapped. |

## Open decisions

1. Desktop distribution: plain eframe binary, or wrap in an installer (MSI/dmg)? Needs a signing story.
2. Keep `llama.cpp` as the only backend, or abstract the OCR provider in `net` now?
3. Is dropping `file://` acceptable for teachers who open `index.html` directly today?
4. Phone layout (`Phone.dc.html`): egui responsive layout, or leave for the web DOM later?

## Not in scope for v1

Phone layout, new model features, server-side storage, multi-user sync.
