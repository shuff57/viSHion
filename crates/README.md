# crates/

Rust rewrite workspace (plan: `docs/RUST-REWRITE-PLAN.md`). The legacy JS app at the repo root is untouched.

| crate | package | role |
|---|---|---|
| `core/` | `vishion-core` | Pure model and logic. No UI, no I/O. Must never depend on egui, web-sys or tokio (CI checks). |
| `store/` | `vishion-store` | `Store` trait (async get/put/list/delete). Native `FsStore` now; `idb` and `fsa` for web later. |
| `net/` | `vishion-net` | OCR client edge. Phase 0: endpoint type only; reqwest streaming client lands in Phase 1. |
| `ui/` | `vishion-ui` | egui app, `ctl()` control layer, web a11y twin (`web.rs`). |
| `app/` | `vishion-app` | Thin entry points: native `vishion` bin, wasm `start()`, and `web/` (index.html, build, serve, CSP check). |

egui/eframe are pinned to 0.28 (plan rule 6).

## Commands

Prerequisites: Rust stable with `rustup target add wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129
(`cargo install wasm-bindgen-cli --version 0.2.129 --locked`, must match `Cargo.lock`), Node 22+, Chrome.

```
# tests (all crates)
cargo test --workspace

# native: opens a window; --exit-after-ms closes it cleanly (exit code 0)
cargo run -p vishion-app --bin vishion --release -- --exit-after-ms 2000

# web build into crates/app/dist (index.html, wasm, JS glue, _headers with CSP + 'wasm-unsafe-eval' + hash)
cd crates/app/web
npm ci
node build.mjs            # add --debug for a debug wasm

# serve it under the production CSP with wrangler dev (http://127.0.0.1:8788/)
node serve.mjs

# CSP check: loads the app in Chrome, requires zero violations, drives the DOM twin,
# and proves enforcement with a negative control. CHROME=<path> overrides the browser.
node check.mjs

# deploy-leak check: serves the repo root through the root wrangler.jsonc + .assetsignore and requires
# that Cargo.*, crates/, target/, spikes/, docs/ are NOT served while the legacy app still is
node check-assetsignore.mjs
```

Windows default Chrome path is `C:/Program Files/Google/Chrome/Application/chrome.exe`; Linux `/usr/bin/google-chrome`.

## CSP

`build.mjs` takes the policy from the repo-root `_headers` unchanged and appends `'wasm-unsafe-eval'` and the
sha256 of every inline script in `index.html` to `script-src`. `connect-src` is untouched. The hash changes
whenever the inline bootstrap script in `crates/app/web/index.html` changes, so always build through `build.mjs`.

## Keyboard model (web)

The canvas is `tabindex=-1` and `aria-hidden`: it takes pointer focus but is not in the Tab order and is not
exposed to assistive tech. The hidden `#controls` twin buttons are the keyboard/AT surface, in on-screen
reading order. eframe would swallow Tab on the canvas, so `ui/src/web.rs` keeps Tab away from it (capture
listener) and the browser moves focus: no keyboard trap. `check.mjs` drives real Tab, Shift+Tab, Enter and
Space, never `focus()`. A future egui text field must keep this property (Tab leaves it).

## Rules to keep

- Real `project.json` has no `id` and keys are `slug/project.json`, `slug/pages/<id>.json`; `core::Project`
  keeps the whole document in file order and `FsStore` accepts validated `/` keys and writes atomically.
- Every interactive widget goes through `App::ctl()` so web gets a twin, focus ring and keyboard activation.
- Do not use `std::time::Instant` or `SystemTime` in code that runs on wasm32 (it panics); use egui time.
- `cargo test` must pass natively; wasm builds with `--target wasm32-unknown-unknown` (CI does both).
