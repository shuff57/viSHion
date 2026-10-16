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
| 3 | `net` + queue | streaming client, run/stop/retry/resume | S4 scenario table passes; demo mode works end to end. **Done** for the client: mock-server tests cover streaming, partial on a dropped stream, 503, dead port and Stop; demo mode unchanged. `/slots` progress polling is in (`net::slots`, native thread and web `select`; tested against a mock). The judge-model Guess works outside demo mode (`net::complete`, `core::guess::prompt`; tested against a mock). The automatic guess after a read (`autoGuess`) is in. Not done: a run of the web transport against a real server. |
| 4 | Review screen | Main screen with math, tray, strip, hidden twin | S8 test suite passes; NVDA smoke test; 44px targets. |
| 5 | Home, inbox, add | remaining screens, drag-reorder, rotate/replace page | Feature list in `README.md` screens table complete. **In progress**: inbox and Home (list, open, create, rename, duplicate, delete) are in. Zip and folder import and Download zip are in. Home's folder card (File System Access folders on the web) is in. Old-session import (web storage, native JSON file) is in. Drop and the photo preview on the Add screen are in (11.9); paste was dropped and removed (11.10). The Add staging screen (choose, name reading, chooser, commit) and Assignment settings entry are in. |
| 6 | Dialogs | settings, assignment settings, export, topics | Same fields as today; export parity. **Done** (Settings, Assignment settings, Export, Subjects and words). |
| 7 | Polish + parity | fonts bundled, dark theme, reduced motion, contrast audit | Side-by-side walkthrough vs JS app with no feature gaps. **In progress**: a token-level contrast test (4.5:1 for text pairs, both themes and both high-contrast variants) is in; placeholder text in the dark theme was 3.7:1 and now uses `muted`. Reduced motion is in on the web (no easing, a still spinner; native follows `VISHION_REDUCE_MOTION=1` and, on Linux, the desktop's animations-off switch via `gsettings`; the flag parser is tested, the gsettings read is not run in CI; Windows and macOS have no query yet). Not done: the side-by-side walkthrough against the JS app. |
| 8 | Provider abstraction | `net::Provider` trait, OpenAI-compatible impl, settings pick by id | Existing `net` tests pass through the trait; a second (mock) provider is selectable. **Done**: `net::Provider` (OpenAI-compatible and Mock), picked by `settings.provider`; reading, name reading and guesses go through it. |
| 9a | Phone fit | Home, inbox, review, Add and the dialogs at 390 x 844 | Every target 44 x 44, nothing past the right edge. **Done**: a rig test walks all of them; the inbox header drops its right group to its own row like the review header does. |
| 9b | Capture on phone | `design/Phone.dc.html` is a camera capture screen (viewfinder, page-found detection, shutter, captured-pages thumbnail, auto toggle), not a phone version of the other screens | Web: getUserMedia viewfinder and a page-corner detector feed the Add staging step; auto-capture toggle. **Built**: `core::capture` (detector, steady counter, tested), `View::Capture`, `app::camera` (web). Camera access itself is untested here: needs a browser with a camera. |
| 10 | Cutover | deploy web build to the existing route, ship the plain desktop binaries | **Decided 2026-10-16: Rust at `/`, no `/legacy`.** The root `wrangler.jsonc` now serves `crates/app/dist` (build writes `_headers` and `.assetsignore`); the JS app stays in the repo as the test oracle but is not deployed. The build now sends `Permissions-Policy: camera=(self)` (it was `camera=()`, which would have blocked camera capture). `check-assetsignore.mjs` proves only the Rust app is served. **Not done:** `wrangler deploy` (yours to run), the desktop binaries, and tell users to export a zip from the old site first (a new origin does not see old browser storage; the README says so). |

Rough order of risk: phase 4 first (earliest real validation of the UI), then 2 (data loss is the worst failure).
If staffing is one person, do 0, 1, 4, then 2, 3, 5, 6, 7.

## Phase 11: gaps against the JS app

Found by comparing `js/` with the Rust build after phases 3 to 10. Each slice is small, ends with `cargo test`
and the wasm32 build green, and gets its own commit. Order matters: 11.1 and 11.2 make the others testable
against a real model server.

| # | Slice | JS source (oracle) | Done when |
|---|---|---|---|
| 11.1 | Settings: **Test connection** | `js/ui/settings.js` `test`, `js/ocr.js` `ping`, `models.js` `list` | A button under the server address says "Connected. N models available" or why not; demo mode says no server is needed. **Done**: `net::get`/`Reply`, `Provider::get`, `net::models::parse_rows`, `Cmd::TestConnection`, a button and result line in Settings. Tested against mock servers (models, 401, dead port) and the mock provider; not run against a live llama-server or Ollama. |
| 11.2 | Settings: **Find models** and the vision flag | `models.js` `list`, `pretty`, `shownUrl`; `settings.js` `findModels` | Lists `/v1/models` (and Ollama `/api/tags` capabilities), fills the picker, "N found, M that can see images", picks the only one. **Done**: `net::models::{pretty, assemble}`, `net::post_json`, `Provider::list_models` (GET `/v1/models`, then `/api/show` per model for a local Ollama), `Cmd::FindModels`/`Event::ModelsFound`, a Find models button and note, the picker filled from the result, and a lookup when Settings opens on a local provider. Tested against a mock Ollama (vision filter, hosted models out of the local picker, working list, errors). Not run against a live server. |
| 11.3 | Settings: **Check which models work** and Show every model again | `models.js` `probe`, `setWorking`; `settings.js` `checkBtn`, `showAll` | Tiny test image per vision model with progress; only those that answer stay listed; stored in `profiles`. **Done**: `Provider::probe` (one tiny picture and a one-word question per model; sequential, where the JS runs three at once), `Backend::check_models` with progress events, `dialog::set_working` (keeps the other profile fields), Check which models work and Show every model again buttons with a wrapped note. Tested against a mock server (a model refused with 403, all refused with 401). Not run against a live server. |
| 11.4 | **Server-down banner** with a health ping and Retry | `js/ui/banner.js`, `U.resumeAll`, `ocr.js` `ping` | An unreachable run shows a banner under the header with the address; Retry pings, then resumes every page without a transcript. **Done**: `Session::server_error` set by an unreachable read (a banner under the header instead of a toast, with the JS wording and Retry), `Cmd::Ping`/`Backend::ping` (`/health` for a local server; demo and hosted endpoints always up), Retry clears it and reads every page without a transcript. Tested with a rig run (banner stays until Retry succeeds) and a ping test against a live socket and a dead port. |
| 11.5 | **Storage banners** (folder access lost, couldn't save) | `banner.js`, `fs.js` `reconnect`, `retry`, `beforeunload` | Banner with Reconnect folder / Try again; closing the tab while a save has failed asks first (web). **Done**: a banner under the header when a save fails ("Couldn't save to <folder>" with **Try again**, which writes every page, the project and the settings once more) or the folder permission was taken back ("Folder access was lost" with **Reconnect folder**, which asks the browser again without reloading, so unsaved work stays in the tab); on the web, closing the tab while a save has failed asks first (`beforeunload`). Tested in the rig; the reconnect and the leave-page prompt need a real Chrome or Edge session to confirm. |
| 11.6 | **Model spelling check** (`judgeMode: auto`) and `judgeUrl` | `js/judge.js` `J.run`, `core::judge::parse_remote` | After a read, glossary suggestions come from the model when the mode asks for it, built-in matching on any failure; the judge address setting is honoured. **Done**: after a live read the backend asks the model to check the transcript against the subject's words (`Backend::judge_remote`, the JS prompt in `core::judge::remote_prompt`, answer filtered by `parse_remote`; `typesafe` rejects the whole answer on one bad entry) and the suggestions replace the built-in matching; any failure, the `heuristic` mode, or no words leaves the built-in matching. Note: the JS app sends this to `ocrUrl` (the one model does both jobs) and never reads `judgeUrl`, so neither does Rust. Tested against mock servers; not run against a live model. |
| 11.7 | **Idle unload** and manual unload | `models.js` `unload`, `idleCheck`, `core::idle` | After `idleUnloadMin` idle minutes (no run in flight) the model is unloaded (llama.cpp router `/models/unload`, Ollama `keep_alive: 0`); a Settings button unloads now. **Done**: `Provider::unload` (llama.cpp router `POST /models/unload` per loaded model, else Ollama `/api/ps` then `keep_alive: 0`), `Cmd::UnloadModel`/`Event::Unloaded`, an **Unload now** button and note in Settings (local provider), and an idle timer on the app (`core::idle` rule, checked every 20 s, busy while a page is reading, used by any model request event) that asks the host after `idleUnloadMin` minutes and toasts "Unloaded X after N minutes idle.". Hosted providers are never asked. Tested against mock router and Ollama servers and a timer test with a tiny interval; not run against a live model. |
| 11.8 | **Keep originals** | `fs.js` `saveImage` with `keepOriginals`, `store::save_original` | With the setting on, each added or replaced picture is also stored as `originals/<id>.<ext>`. **Done**: with `keepOriginals` on, staged pictures (Add screen) and pictures added from the strip are also stored as `originals/<id>.<ext>` (extension of the picked file, lower case, `img` when none); Replace page does not, as in the JS. A failure to keep an original never loses the page. Tested with the settings off and on. |
| 11.9 | Add screen: **Turn 90 / Straighten** per photo and the **photo preview** with detected lines | `js/ui/add.js` `fit`, `preview` | A staged row can turn and level its picture; a preview shows the sheet with numbered line boxes and a summary. **Done**: a staged picture keeps its shrunk copy (`StagedItem::base`); **Turn 90°** and **Straighten** (default from the setting) redo it with `Cmd::RefitStaged`/`Event::Refit` (`backend::fit_picture`: quarter turns, then deskew), and **Look** shows the photo large with the ink-scan lines boxed and numbered (faint ones thin) and the JS sentences ("Straightened by N°", "already level", "Straightening is off", "N lines found"). The copy for Turn/Straighten is dropped at commit. Tested with a turn that swaps the sides, the toggle and the preview sentence; the line boxes themselves were not checked against a real photo. |
| 11.10 | Add screen: **paste** (web) | `js/ui/upload.js` paste handler | **Dropped and removed** (2026-10-16): not wanted, and it did not stage a picture in a manual try. The window `paste` listener, `App::paste_files` and their test are deleted; pictures come in through Choose files and drop. Text paste into fields is eframe's own and unchanged. |

Rules for every slice: follow the surrounding code and the "Rules to keep" in `crates/README.md`; keep decisions
that are already recorded (above); no push, no PR; update this table's "Done when" with what was built and
what could not be verified without a live server or browser.

## Home: wordmark and Settings

Added after the cutover decision. The home screen has a top bar like the JS app's: the wordmark (its scan-in plays once
on load and now always runs to its end; a pointer crossing the header no longer restarts it) and a Settings button.
Settings has no assignment to live in on Home, so `Cmd::LoadSettings` / `Event::SettingsLoaded` fetch them into a
settings-only session (`App::home_settings`) that the dialog edits, saves through `SaveSettings`, and drops when it
closes. With an assignment still open behind Home, its own session is used. Tested in the rig and in Chrome; not
walked against the JS Home screen's Settings beyond opening it.

## Loading splash

`index.html` shows the wordmark animation full screen while the app loads, as the sibling apps do: the handwriting is
scanned into type once (the header wordmark's motion, 1.8 s), and the page stays up until that has finished and the app
has left its loading state (`data-booted` on the twin), then fades out (0.35 s). Before this the page removed its boot
logo the moment `start()` resolved, so a fast load cut the animation off partway, and it was an endless alternating
loop; the app's own header wordmark then started again. The header's copy now starts settled when a splash was shown
(`ui::widgets::skip_intro`). Reduced motion shows the settled word for half a second. A browser driven by a test
skips the splash (`?splash=1` forces it, `?splash=0` skips it); `check.mjs` covers it.

## Needs you

Left after the loop of 2026-10-16. Nothing here can be finished or checked from this machine unattended.

- **Browser checks need real Node.** `node` on this box is a Bun shim (kept for another tool), so the scripts in
  `crates/app/web` fail there. Node 22 is unpacked in `~/.local/share/node22`; run them with
  `PATH=$HOME/.local/share/node22/bin:$PATH`, and `CHROME=/usr/bin/google-chrome` for `visual-compare.mjs`.
  Run 2026-10-16 on that setup: `check.mjs`, `a11y.mjs`, `parity.mjs` (36 controls, 44px, light, dark, phone) and
  `check-assetsignore.mjs` pass. `check.mjs` had two stale tests, fixed: the ring baseline raced the wordmark
  scan-in (now waits for two matching frames), and the empty-store wait used the review screen's `empty-title`
  instead of Home's `home-empty`. `visual-compare.mjs` had raw newlines in two string literals; fixed.
  Side-by-side screenshots (JS vs Rust web, 1280x800 and 390x844, light and dark) show the same layout.
  Only the wordmark differs between the two frames (the scan-in animation is mid-play in a still); no other
  gaps were found, so no new 11.x slices. Not walked: the other screens (Home, Add, dialogs) in the JS app.
- **Phase 4:** NVDA and VoiceOver passes.
- **Phase 10 deploy:** run it by hand: `node crates/app/web/build.mjs`, then `npx wrangler deploy` from the repo root. Browser storage on the old origin is not visible to the new one, so anyone with work in the old site exports a zip first.
- **Live checks:** Test connection, Find models, Check which models work, the web transport, Keep originals and
  the idle unload against a real llama-server or Ollama.
- **Browser hardware:** camera capture (9b), the storage banners'
  reconnect and leave-page paths (11.5).

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

## Decisions

1. **Web storage**: add File System Access folders (Chrome/Edge) beside IndexedDB, with the choose / reconnect
   card on Home. Other browsers stay on IndexedDB.
2. **Old single-file sessions**: import on web (the old app's browser storage) and on native (an exported
   legacy session file).
3. **Desktop distribution**: plain eframe binary per platform; no installer or signing for v1.
4. **`file://`**: dropped. The wasm build needs a server; the README says so.
5. **OCR provider**: abstract it in `net` now (a trait over "read a page" and "complete a prompt"), with the
   OpenAI-compatible client as the first implementation.
6. **Phone layout**: build it in egui now, following `design/Phone.dc.html`, for Home, inbox, review and Add.

## Not in scope for v1

New model features, server-side storage, multi-user sync.
