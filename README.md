# viSHion

Handwriting OCR for stacks of student pages, backed by a local llama.cpp vision server. A plain-script web app: no build step, no framework, runs from `file://`.

## Run it

1. `llama-server -m model.gguf --mmproj mmproj.gguf -ngl 99 -c 8192 --port 8188 --cors`
   Or serve every model folder at once with `powershell -File tools\serve-models.ps1` (one llama-server in router mode; picking another model in Settings unloads the previous one and loads the next).
2. Open `index.html` in a browser (Chrome or Edge to save to a folder), or run `python -m http.server` here.

**No model server yet?** Settings, Demo mode streams sample text so every screen can be tried.

## The screens

Words in the interface differ from the engine's: an *assignment* is a project, a *student* is a group of pages, a *subject* is a topic, *instructions* are the prompt.

| Screen | Where | What it does |
|---|---|---|
| Assignments | `js/ui/home.js` | Where work is saved, the list of assignments, import zip/folder, import old single-file sessions |
| Inbox | `js/ui/inbox.js` | One row per student, run bar (stop, retry, resume), status bar and filters |
| Add pages | `js/ui/add.js`, `upload.js` | Stage files, read the name at the top of each page, then transcribe |
| Review | `js/ui/review.js` | Page image and the student's page strip, transcript, words-to-check tray |
| Dialogs | `settings.js`, `assignment.js`, `export-panel.js`, `modal.js` | Settings, assignment settings, subjects and words, export |

Words to check come from what the engine really produces: each glossary suggestion in `page.corrections` (yellow) and each `[illegible]` marker in `page.raw` (red, dashed). A word the teacher resolved gets a green underline. Decisions are stored in two optional page fields, `resolved` and `edits`, so old projects open unchanged. `OCR.words.effective(page)` is the text the review screen and every export show.

## Layout

```
index.html
css/        tokens (light + dark), base, layout, components, motion
fonts/      Instrument Sans, Source Serif 4 (woff2, SIL OFL)
js/         engine: ns, state, fs, fs-projects, image, ocr, judge, queue, render, export
            added:  words (words to check), groups (students), student (name reader), fs-legacy
js/ui/      icons, modal, banner, header, home, inbox, add, upload, review, assignment, settings, export-panel
js/main.js  bootstrap, loaded last
design/     the design files this UI was built from, and legacy-ocr.html (the old app)
tests/      node tests/words.test.js
HANDOFF.md  the brief for this rebuild
bakeoff/    model comparison script (below)
```

On disk, per assignment: `<folder>/<slug>/project.json`, `pages/<id>.json`, `images/<id>.jpg`, `originals/`, `exports/`, plus `<folder>/settings.json`. The API key lives in `localStorage` only.

## Things to know

- **Line boxes are estimated.** The model returns text only, so boxes come from an ink-row scan fitted to the transcript's line count. The "this line, as the student wrote it" crop in the tray is shown only when the scan found exactly as many lines as the transcript has; otherwise it is hidden rather than guessed.
- **Name reading** (`js/student.js`) is a separate short request that sends the top 14% of the page and asks for `{ "name": ... }`. A page with no readable name gets a "Name not found" choice; if left unchosen the student is named after the file.
- **Pages are grouped by name only.** Two pages from the same student with different spellings of the name become two rows. Use Change student on the review screen to merge them.
- **Hosted models.** OpenRouter works straight from the page. `ollama.com` does not allow web pages to call it (no CORS), so Ollama Cloud is used through the local Ollama app: run `ollama signin`, pick the Ollama Cloud provider, and choose a `:cloud` model. To use a raw Ollama API key instead, run `UPSTREAM=https://ollama.com UPSTREAM_KEY=<key> node tools/relay.js` and point the Local server provider at `http://localhost:8188`.
- **One model.** The model that reads the page also checks spelling (same server, same key) and only ever suggests glossary words that are a small edit away. If it fails, or Settings says built-in matching only, a simple edit-distance matcher is used. A failure never blocks a transcript.
- **Old sessions.** If this browser holds sessions saved by `design/legacy-ocr.html`, the Assignments screen offers Import old sessions. It only sees data from the same browser origin.
- Keyboard: arrow keys, Home and End move through a student's page strip, Alt+Arrow reorders, Delete removes.
- Pinned CDN assets (KaTeX 0.16.9, markdown-it 14.1.0, fflate 0.8.2) each have a fallback if they fail to load.

## Deploy (Cloudflare Workers static assets)

The site is the Rust web build. `wrangler.jsonc` serves `crates/app/dist` as assets; the build writes `dist/_headers` (the CSP with the hash of the inline script, cache rules, camera allowed for this origin) and `dist/.assetsignore` (keeps check screenshots out). The plain-script JS app in the repo root is no longer deployed; it stays in the repo as the oracle for the Rust tests.

```
cd crates/app/web && npm ci && node build.mjs   # builds crates/app/dist
node check-assetsignore.mjs                     # optional: proves only the Rust app is served
cd ../../.. && npx wrangler login               # once
npx wrangler deploy                             # uploads dist and attaches vishion.lefthanddev.com
```

`lefthanddev.com` must be a zone in the same Cloudflare account. There is no backend and no secret on the server: each visitor enters their own OpenRouter key (kept in their browser) or points Settings at a local server (`--cors`, http://localhost:8188). Data saved by the old JS app under `file://` or localhost does not carry over to the new origin; export a zip there and import it here.

The CSP is built from the root `_headers` (the policy) plus the hashes of the inline scripts in `crates/app/web/index.html`; edit the policy in the root `_headers`.

## Bake-off

`bakeoff/run.py` computes character error rate for five models on your own photos. Each goes in its own folder under `bakeoff/models/` as `model.gguf` and `mmproj.gguf`, all downloaded from Hugging Face:

| Folder | Source |
|---|---|
| `qwen3vl` | Qwen3-VL-8B-Instruct Q8_0 |
| `qwen25vl` | ggml-org Qwen2.5-VL-7B-Instruct Q8_0 |
| `lumichats` | adityakum667388 LumiChats v1.2 Q8_0, with the Qwen2.5-VL projector |
| `nanonets` | mradermacher Nanonets-OCR2-3B Q8_0 |
| `paddle` | PaddleOCR-VL-1.6-GGUF |

Results so far, on 6 handwritten pages (RTX 4070 12 GB), hand-made ground truth, so differences of a point or two are noise:

| Model | Mean CER | Notes |
|---|---|---|
| Nanonets-OCR2-3B (Q8) | 7.5% | best on multi-column math layout |
| Qwen3-VL-8B (Q8) | 13.2% | best on faint/messy handwriting |
| PaddleOCR-VL-1.6 | 22.4% | fastest (~1 s/page), drops words |

Put `bin/llama/llama-server.exe` and `models/{qwen3vl,qwen25vl,lumichats,nanonets,paddle}/{model,mmproj}.gguf` under `bakeoff/`, add `truth.json`, set `OCR_PHOTOS`, then `python run.py` (all five) or `python run.py qwen3vl nanonets` (some).
