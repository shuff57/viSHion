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
```

Windows default Chrome path is `C:/Program Files/Google/Chrome/Application/chrome.exe`; Linux `/usr/bin/google-chrome`.

## CSP

`build.mjs` takes the policy from the repo-root `_headers` unchanged and appends `'wasm-unsafe-eval'` and the
sha256 of every inline script in `index.html` to `script-src`. `connect-src` is untouched. The hash changes
whenever the inline bootstrap script in `crates/app/web/index.html` changes, so always build through `build.mjs`.

## Rules to keep

- Every interactive widget goes through `App::ctl()` so web gets a twin, focus ring and keyboard activation.
- Do not use `std::time::Instant` or `SystemTime` in code that runs on wasm32 (it panics); use egui time.
- `cargo test` must pass natively; wasm builds with `--target wasm32-unknown-unknown` (CI does both).
