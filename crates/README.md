# crates/

Rust rewrite workspace (plan: `docs/RUST-REWRITE-PLAN.md`). The legacy JS app at the repo root is untouched.

| crate | package | role |
|---|---|---|
| `core/` | `vishion-core` | Pure model and logic. No UI, no I/O. Must never depend on egui, web-sys or tokio (CI checks). |
| `store/` | `vishion-store` | `Store` trait (async get/put/list/delete). Native `FsStore` now; `idb` and `fsa` for web later. |
| `net/` | `vishion-net` | OCR client (`O.transcribe`): pure `sse` parser, `ureq` transport natively, `fetch` on the web, one `async fn transcribe`, `Cancel` for Stop. |
| `ui/` | `vishion-ui` | The review screen (Phase 4): `state` flows over core, `transcript`, `flow` (inline line layout), `dialog`, `math` (latex-rust isolation), `theme` (tokens + bundled fonts), `ctl()` control layer, `twin` + web a11y twin (`web.rs`), `host` command/event seam. |
| `app/` | `vishion-app` | Thin entry points and the storage wiring: native `vishion` bin (folder via `FsStore` on a worker thread), wasm `start()` (`IdbStore`), the `Backend` that answers the UI's commands, a `demo` assignment, and `web/` (index.html, build, serve, CSP check, native/web parity check). |

egui/eframe are pinned to 0.28 (plan rule 6).

## Renderer

glow (OpenGL, WebGL2 on the web) is the default. `cargo run -p vishion-app --no-default-features --features wgpu`
renders natively with wgpu (Vulkan, DX12, Metal). On the web wgpu is WebGPU only, no WebGL fallback, so use
`node build.mjs --wgpu` only where WebGPU is available.

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
#   --size WxH             window size, e.g. 390x844 for the phone layout
#   --inbox                open on the assignment overview (tabs, run bar, rows) instead of the review screen
#   --page ID              open with that page selected (e.g. p-omar-1 in the demo)
#   --no-tray              open with the word panel closed
#   --dump-controls        print the control list (id, role, label, size, position) as JSON lines on close,
#                          and the transcript lines' rects as `#line` lines

# web build into crates/app/dist (index.html, wasm, JS glue, _headers with CSP + 'wasm-unsafe-eval' + hash).
# Web URL options: ?demo=1 (sample assignment, nothing stored), ?project=<slug>, ?theme=light|dark,
# ?view=inbox (assignment overview), ?page=<page id>, ?tray=0 (word panel closed)
cd crates/app/web
npm ci
node build.mjs            # add --debug for a debug wasm, --no-opt to skip wasm-opt, --wgpu for WebGPU instead of WebGL2
#   wasm-opt -Oz runs from the `wasm-opt` npm package; the STIX math font, the CJK fallback face and the
#   Twemoji colour emoji face are copied next to the wasm and fetched the first time a page needs them
#   (the wasm is about 4.1 MB, 1.8 MB gzipped, without them)

# serve it under the production CSP with wrangler dev (http://127.0.0.1:8788/)
node serve.mjs

# CSP check: loads the app in Chrome, requires zero violations, drives the DOM twin,
# and proves enforcement with a negative control. CHROME=<path> overrides the browser.
node check.mjs

# native vs web parity: same demo assignment, same control list in the same order, every target >= 44pt,
# light, dark and phone width; screenshots go to crates/app/dist/parity-{native,web}-{light,dark,phone}.png
cargo build -p vishion-app && node parity.mjs

# accessibility: ariaSnapshot assertions (landmarks, headings, groups, lists, roles, math), axe-core in six
# states, focus restore, announcements, 44px targets, reduced motion, forced colours, 200% / 400% reflow
node a11y.mjs

# deploy-leak check: serves the repo root through the root wrangler.jsonc + .assetsignore and requires
# that Cargo.*, crates/, target/, spikes/, docs/ are NOT served while the legacy app still is
node check-assetsignore.mjs
```

Windows default Chrome path is `C:/Program Files/Google/Chrome/Application/chrome.exe`; Linux `/usr/bin/google-chrome`.

## Capture on phone (Phase 9b)

`View::Capture` is `design/Phone.dc.html`: the Add screen's "Use the camera" (web only) opens the viewfinder.
The browser side (`app::camera`) sends `Event::CameraFrame`s (RGBA, at most 960 wide, about 6 a second); the
page is found by `core::capture::detect_page` (a bright, even region covering a quarter of the frame or
more), and `Steady` fires auto-capture after the page holds still for 12 frames. The shutter (or auto) sends
`Cmd::CaptureShot`, which stages the frame like a picked file (name reading included); Done returns to the
Add screen with the photos in the list. The page detector is a heuristic, not a corner finder: it frames the
bright region's box and does not straighten (the staging step's Straighten still applies).

## Opening the web build

The web build is a wasm module and needs a web server: it does not run from a `file://` page, so opening
`index.html` from disk does not work (decision recorded in `docs/RUST-REWRITE-PLAN.md`). Serve the output
folder with `wrangler dev`, or any static host that sends the CSP below (it needs `'wasm-unsafe-eval'`).
Folder saving in the browser needs Chrome or Edge (File System Access); other browsers keep work in
IndexedDB, and Home says so.

## CSP

`build.mjs` takes the policy from the repo-root `_headers` unchanged and appends `'wasm-unsafe-eval'` and the
sha256 of every inline script in `index.html` to `script-src`. `connect-src` is untouched. The hash changes
whenever the inline bootstrap script in `crates/app/web/index.html` changes, so always build through `build.mjs`.

## Review screen (Phase 4)

`ui::review` draws design/Main.dc.html and the behaviour of js/ui/review.js + header.js:

- header: wordmark, back-to-assignment crumb, student navigation, save state, layout, Copy text, Done, Settings.
  It wraps like the design's flex-wrap (the right group drops to its own row; on a phone every item flows).
- page pane: `Page i of n`, zoom out / fit / in (buttons, +, -, 0 while the stage has focus, Ctrl+wheel, double
  click), marks, Straighten, Rotate, Replace, Remove, strip with keyboard (arrows, Home, End, Delete,
  Alt+arrows to reorder, drag to reorder, Ctrl/Shift-click to select, Export N / Review again / Clear) and Add page.
  Below 900 pt the panes stack, as css/layout.css does.
- transcript: `flow.rs` lays inline text out at the design's 31em column and 1.7 line pitch with chips that add
  no gap and no height; Change student, Show plain/formatted text, Transcribe this page again, pending
  and error states (Transcribe / Retry), underline that follows the reader (click, arrows inside the transcript,
  activating a flagged word).
- tray: choices, typed fix, step, close (Escape inside it closes it and returns focus to a word).
- dialogs: Change student, confirmations, Settings (theme). Modal: only its controls are in the twin.
- inbox: the assignment overview the crumb and the last student's Done return to (tabs, run bar, rows).
- assignment settings (Phase 6): the inbox's "Assignment settings" opens a dialog for the name, subject (select over
  `settings.topics`) and instructions (multi-line, "Reset to standard"); name and instructions are saved when it
  closes, the subject at once (`dialog::Dialog::Assignment`, `Out::Meta`).
- export (Phase 6): the inbox's Export opens `Dialog::Export` (scope everyone / ready / selected, the three export
  selects saved to settings, a summary of empty pages, one button per kind and Download all). Empty pages ask
  first (Export anyway); files land in `exports/` through `Cmd::Export`, "Copy all text" uses the clipboard.
- subjects and words (Phase 6): "Edit subjects and their words" in Assignment settings opens `Dialog::Topics`
  (stacked on it): pick, description, words (one per line or commas), New subject, Delete subject (General stays;
  confirmed), Restore defaults (confirmed). Typed text is saved on every change of subject and on Done.
- add pages (Phase 5): the inbox's "Add pages" opens `View::Add`: Choose files (`Cmd::StagePictures`; each file
  checked like `image.check`, shrunk, levelled when the setting says so, and its name read from the top 14% by
  the model, or from the sample names in demo mode), staged rows with a student chooser (known names or a typed
  one) and Remove, skipped-file and names-down notices, the subject, "Read the name" switch, and Transcribe N
  pages (`Cmd::CommitStaged`, then the run starts). Files dropped on the window are staged too (`Cmd::StageFiles`; a native folder is walked). Not yet: paste
  (egui has no image paste) and the full-size look at a photo with its lines.
- home (`ui::home`, Phase 5): the wordmark on the inbox goes there, and an empty store starts there. Lists the
  assignments (`Cmd::ListProjects` -> `Event::Projects`), opens one, creates, renames, duplicates and deletes
  (inline fields and an inline confirm instead of dialogs, so it works with no session). Import zip (`core::export::read_zip`, stored and deflated
  entries) and Download zip (writes `exports/<slug>.zip`, `Cmd::DownloadZip`) are in. Import folder
  (`Cmd::ImportFolder`: a native folder dialog, `webkitdirectory` on the web) is in too. The storage card has the JS app's four states on the web (choose a folder, reconnect, saving in
  this browser, saving to a folder): `Cmd::Folder(choose|browser|reconnect)` stores or clears the handle as
  `kv/root` in `ocr_fs` and reloads, and boot uses the folder when its permission is still granted
  (`AnyStore::Fsa`). "Import old sessions" (`Cmd::ImportLegacy`) reads the old app's `ocr_sessions_v2` and `ocr_images` in place on
  the web, and an exported JSON file natively (`[sessions]` or `{ sessions, images }`); pictures are shrunk, applied
  corrections stay resolved, line boxes come from the ink scan when the page opens.

Flows are `ui::state::Session` over `core::words` (`choose`, `use_all`, which call `core::judge::toggle`),
`core::groups`, `core::export`. The screen never touches storage: it sends `host::Cmd`s (`LoadImage`,
`SavePage`, `SaveMeta`, `SaveSettings`, `RemovePage`, `RotateImage`, `ReplaceImage`, `AddPages`, `Export`,
`Straighten`, `Transcribe`, `LoadMathFont`) and receives `host::Event`s; `app::backend::Backend` answers them over
`Projects<AnyStore>` (`FsStore` native, `IdbStore` web, `MemStore` demo). Pictures are prepared in pure Rust
(`app::pictures`: longest side 1600, JPEG 90). Native uses `rfd` for the file dialog, the browser an
`<input type=file>`.

- Targets: every control is at least 44 x 44 pt (flagged words are painted at the design's 30pt chip height and
  get a 44pt hit area centred on it); checked by `ui` frame tests and `parity.mjs`.
- Theme: `theme::LIGHT` / `DARK` mirror css/tokens.css; `settings.theme` auto follows the system.
  Fonts: Instrument Sans 400/500/600, Source Serif 4 400/600/italic and a Latin+Greek subset of Hack as
  TrueType in `ui/assets/fonts`; egui's own faces are not linked (`latex-rust` is vendored to keep them out).
- Math: `ui::math` is the only user of latex-rust; a layout error shows the TeX in red monospace. On the web the
  font is fetched on first use and the TeX shows muted until it arrives.
- Honest gaps: none known in this screen beyond the open items in the plan. Transcribe / Retry go through `net` and
  "Suggest likely words" asks the model (also automatically after a read, when `guessIllegible` is not off).
- wasm: about 4.0 MB (1.7 MB gzip) from 7.3 MB (3.2 MB gzip): no egui default fonts (-1.4 MB), math font
  out of the wasm (-0.84 MB), `opt-level=z`, `strip`, `panic=abort`, wasm-opt -Oz.

## Keyboard model (web)

The canvas is `tabindex=-1` and `aria-hidden`: it takes pointer focus but is not in the Tab order and is not
exposed to assistive tech. The hidden `#controls` twin buttons are the keyboard/AT surface, in on-screen
reading order (region, then 16px row band, then x): header, page pane, transcript, tray. The transcript is
mirrored as headings, a `role=status` live region and paragraphs with the flagged words as inline buttons and
math as `role=math`; each flagged word is reserved in its sentence at layout time (`Ctl::para_slot`), so it reads
where it stands ("The 95% [Spelling to check: confidance...] interval ..."). The tray's position is a
`role=status` node ("Word 2 of 4") between Previous and Next, and the header's student position a text node.
eframe would swallow Tab on the canvas, so `ui/src/web.rs` keeps Tab away from it (capture
listener) and the browser moves focus: no keyboard trap. `check.mjs` drives real Tab, Shift+Tab, Enter and
Space, never `focus()`. A future egui text field must keep this property (Tab leaves it).

Keys that belong to a region are forwarded from the twin as `key:<name>` and routed by where focus is: arrows in
the transcript region (`transcript`, a focusable `role=group`, or any flagged word) move the reading underline,
the strip takes arrows/Home/End/Delete/Alt+arrows, the page stage (`stage`) takes + - = 0, and Escape closes the
dialog, else the tray when focus is in it, else clears the underline. Opening a dialog moves focus into it and
closing it returns focus to what opened it. The line being read is `data-reading` on its paragraph.

## Rules to keep

- Real `project.json` has no `id` and keys are `slug/project.json`, `slug/pages/<id>.json`; `core::Project`
  keeps the whole document in file order and `FsStore` accepts validated `/` keys and writes atomically.
- Every interactive widget goes through `App::ctl()` so web gets a twin, focus ring and keyboard activation.
- Do not use `std::time::Instant` or `SystemTime` in code that runs on wasm32 (it panics); use egui time.
- `cargo test` must pass natively; wasm builds with `--target wasm32-unknown-unknown` (CI does both).

## Providers

`vishion_net::Provider` is the seam to a model API (`transcribe` streams a page, `complete` returns one answer).
`Provider::for_setting(settings.provider)` picks it: every provider the Settings select lists (local, Ollama
Cloud, OpenRouter, custom) is `OpenAiCompatible`; `mock` answers from memory (tests, trying the seam). The
backend reads, reads names and guesses only through it, so another API is one more variant.

## Settings: connection

"Test connection" (`Cmd::TestConnection`, `Backend::test_connection`) asks `GET <address>/v1/models` with the key as
typed and says "Connected. N models available", that the key was refused (401/403), or, when there is no
list, falls back to `/health` ("Connected.") and then "Can’t reach ...". Demo mode says no server is
needed. `net::models::parse_rows` reads the listing (OpenAI shape, OpenRouter modalities, llama.cpp router).

## Settings: finding models

"Find models" (`Cmd::FindModels`, `Backend::find_models`, `Provider::list_models`) reads `GET /v1/models`; for the
local provider it asks `POST /api/show` per model for `capabilities` (a llama.cpp router has no `/api/show`).
`net::models::assemble` drops hosted (`:cloud`, `-cloud`) models from the local picker and text-only models from
every picker, narrows a cloud picker to `profiles[provider].working` unless asked for all, and sorts vision
first; `pretty` tidies the ids ("gemma4:31b-cloud" is "Gemma 4 31B"). The picker lists the result; a local
provider is asked when Settings opens.

### Check which models work

`Cmd::CheckModels` lists every model that takes images, sends each a tiny picture and a one-word question
(`Provider::probe`), and reports `Event::ModelsProgress` as they answer. The ids that answered are stored as
`profiles[provider].working` and narrow a cloud picker (`assemble`); none answering keeps the full list and says
whether the key was refused. "Show every model again" clears `working`. `dialog::remember` merges into the
profile, so `working` survives a provider switch.

## Unloading the model

`Provider::unload` frees a local model: llama.cpp in router mode gets `POST /models/unload` for each row whose
`status.value` is `loaded`; otherwise Ollama's `/api/ps` is read and each model gets
`POST /api/generate { keep_alive: 0 }`. Settings' **Unload now** sends `Cmd::UnloadModel`. The idle timer
(`App::idle_tick`) uses `core::idle::Idle`: any model-touching event marks use, a page being read counts as
busy, and every 20 s `idleUnloadMin` minutes of quiet sends the same command (local provider, not demo).

## Model spelling check

After a live read, `Backend::judge_remote` (js/judge.js `J.remote`) sends the transcript and the subject's words to
the same model at `settings.ocrUrl` (text only, 20 s) and keeps the suggestions that are glossary terms close to
the written word. `settings.judgeMode` is `auto` (ask the model, fall back), `heuristic` (never ask) or
`typesafe` (any bad entry rejects the answer). The result rides on `ReadOutcome::Done { found }`; `None` means the
screen applies the built-in matching, as the JS does when the judge fails.

## Storage banners

A failed write (`Event::SaveFailed`) shows a banner under the header: "Couldn\u2019t save to \u201Cfolder\u201D." with **Try
again** (`Session::mark_all_dirty`, then the usual flush), or, when the message says the permission was refused,
"Folder access was lost." with **Reconnect folder** (`Cmd::Folder("regrant")`: `requestPermission` on the stored
handle, then `Event::Reconnected` saves everything again; no reload, so unsaved work stays). While a save has
failed the web build asks before the tab closes (`ui::web::set_unsaved`).

## Server-down banner

A read that cannot reach the server (`ReadOutcome::Unreachable`) sets `Session::server_error`: a banner under the
header (`banner.js`) with the address and **Retry**, instead of a toast. A page that reads clears it. Retry sends
`Cmd::Ping` (`GET /health` for a local server; demo mode and hosted endpoints always answer) and, when it
answers, reads every page that still has no transcript (`U.resumeAll`).

## Demo mode and reading pages

`--demo`, `?demo=1` and the `demo` setting answer Retry, Transcribe, Transcribe again and the inbox's Resume
with the sample transcripts of js/ocr.js (`core::demo`), run the offline judge over them, and add the sample
guesses for unreadable words, exactly as the JS app's demo mode does. Without demo mode, reading goes to the server (`net`) and Guess asks the same model for likely words
(`net::complete`, js/guess.js prompt, `core::guess::parse_guesses`). Straighten is a port of js/deskew.js
(`app::pictures::deskew`), also applied to pictures picked with Replace and Add page.

## Review-screen tests

* `crates/ui/src/app_tests.rs`: frame-level tests of the screen over a recorder host.
* `crates/app/src/rig.rs`: the same screen over the real `Backend` and the demo assignment, so a click goes to
  the host and the answer comes back. `cargo test -p vishion-app dump_layout -- --ignored --nocapture` prints
  where every text and control sits (`VIEW=inbox`, `W=390 H=844` change the screen).
