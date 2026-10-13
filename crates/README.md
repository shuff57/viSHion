# crates/

Rust rewrite workspace (plan: `docs/RUST-REWRITE-PLAN.md`). The legacy JS app at the repo root is untouched.

| crate | package | role |
|---|---|---|
| `core/` | `vishion-core` | Pure model and logic. No UI, no I/O. Must never depend on egui, web-sys or tokio (CI checks). |
| `store/` | `vishion-store` | `Store` trait (async get/put/list/delete). Native `FsStore` now; `idb` and `fsa` for web later. |
| `net/` | `vishion-net` | OCR client edge. Phase 0: endpoint type only; reqwest streaming client lands in Phase 1. |
| `ui/` | `vishion-ui` | The review screen (Phase 4): `state` flows over core, `transcript`, `math` (latex-rust isolation), `theme` (tokens + bundled fonts), `ctl()` control layer, `twin` + web a11y twin (`web.rs`), `host` command/event seam. |
| `app/` | `vishion-app` | Thin entry points and the storage wiring: native `vishion` bin (folder via `FsStore` on a worker thread), wasm `start()` (`IdbStore`), the `Backend` that answers the UI's commands, a `demo` assignment, and `web/` (index.html, build, serve, CSP check, native/web parity check). |

egui/eframe are pinned to 0.28 (plan rule 6).

## Commands

Prerequisites: Rust stable with `rustup target add wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129
(`cargo install wasm-bindgen-cli --version 0.2.129 --locked`, must match `Cargo.lock`), Node 22+, Chrome.

```
# tests (all crates)
cargo test --workspace

# native: opens the most recent assignment in ~/Documents/viSHion (or --root DIR / $VISHION_DIR)
cargo run -p vishion-app --bin vishion --release
#   --demo                 sample assignment from memory (nothing is stored)
#   --seed-demo-to DIR     write the sample assignment into DIR and open it (edits persist there)
#   --project SLUG         open that assignment instead of the most recent one
#   --theme light|dark     override settings.theme
#   --exit-after-ms N      close the window cleanly after N ms (smoke tests; exit code 0)
#   --screenshot FILE.png  write the first settled frame and close
#   --dump-controls        print the control list (id, role, label, size) as JSON lines on close

# web build into crates/app/dist (index.html, wasm, JS glue, _headers with CSP + 'wasm-unsafe-eval' + hash).
# Web URL options: ?demo=1 (sample assignment, nothing stored), ?project=<slug>, ?theme=light|dark
cd crates/app/web
npm ci
node build.mjs            # add --debug for a debug wasm

# serve it under the production CSP with wrangler dev (http://127.0.0.1:8788/)
node serve.mjs

# CSP check: loads the app in Chrome, requires zero violations, drives the DOM twin,
# and proves enforcement with a negative control. CHROME=<path> overrides the browser.
node check.mjs

# native vs web parity: same demo assignment, same control list in the same order, every target >= 44pt,
# light and dark; screenshots go to crates/app/dist/parity-{native,web}-{light,dark}.png
cargo build -p vishion-app && node parity.mjs

# deploy-leak check: serves the repo root through the root wrangler.jsonc + .assetsignore and requires
# that Cargo.*, crates/, target/, spikes/, docs/ are NOT served while the legacy app still is
node check-assetsignore.mjs
```

Windows default Chrome path is `C:/Program Files/Google/Chrome/Application/chrome.exe`; Linux `/usr/bin/google-chrome`.

## CSP

`build.mjs` takes the policy from the repo-root `_headers` unchanged and appends `'wasm-unsafe-eval'` and the
sha256 of every inline script in `index.html` to `script-src`. `connect-src` is untouched. The hash changes
whenever the inline bootstrap script in `crates/app/web/index.html` changes, so always build through `build.mjs`.

## Review screen (Phase 4)

`ui::review` draws design/Main.dc.html: header (student nav, layout, Copy, Done), page pane (image, line marks,
strip), transcript (units, typeset math, flagged words inline as controls), and the "Check this word" tray.
Flows are `ui::state::Session` over `core::words` (`choose`, `use_all`, which call `core::judge::toggle`),
`core::groups`, `core::export` (Copy). The screen never touches storage: it sends `host::Cmd`s
(`LoadImage`, `SavePage`, `SaveMeta`, `SaveSettings`) and receives `host::Event`s; `app::backend::Backend`
answers them over `Projects<AnyStore>` (`FsStore` native, `IdbStore` web, `MemStore` demo).

- Targets: every control is at least 44 x 44 pt (flagged words keep the design's line rhythm and extend
  their hit area); checked by `ui` frame tests and `parity.mjs`.
- Theme: `theme::LIGHT` / `DARK` mirror css/tokens.css; `settings.theme` auto follows the system.
  Fonts: Instrument Sans 400/500/600 and Source Serif 4 400/600/italic as TrueType in `ui/assets/fonts`.
- Math: `ui::math` is the only user of latex-rust; a layout error shows the TeX in red monospace.
- Not in this phase: rotate/replace/straighten, drag-reorder, "Suggest likely words" (needs `net`),
  Settings/Export dialogs, the inbox (back link is a label until Phase 5).

## Keyboard model (web)

The canvas is `tabindex=-1` and `aria-hidden`: it takes pointer focus but is not in the Tab order and is not
exposed to assistive tech. The hidden `#controls` twin buttons are the keyboard/AT surface, in on-screen
reading order (region, then 16px row band, then x): header, page pane, transcript, tray. The transcript is
mirrored as headings, a `role=status` live region and paragraphs with the flagged words as inline buttons and
math as `role=math`. eframe would swallow Tab on the canvas, so `ui/src/web.rs` keeps Tab away from it (capture
listener) and the browser moves focus: no keyboard trap. `check.mjs` drives real Tab, Shift+Tab, Enter and
Space, never `focus()`. A future egui text field must keep this property (Tab leaves it).

## Rules to keep

- Real `project.json` has no `id` and keys are `slug/project.json`, `slug/pages/<id>.json`; `core::Project`
  keeps the whole document in file order and `FsStore` accepts validated `/` keys and writes atomically.
- Every interactive widget goes through `App::ctl()` so web gets a twin, focus ring and keyboard activation.
- Do not use `std::time::Instant` or `SystemTime` in code that runs on wasm32 (it panics); use egui time.
- `cargo test` must pass natively; wasm builds with `--target wasm32-unknown-unknown` (CI does both).
