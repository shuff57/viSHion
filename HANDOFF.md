# Handoff: replace the Handwriting OCR UI with the new design

You are working in the `ocr-app` repo (classic `<script defer>` files on a shared `window.OCR` namespace, no ES modules, no build step, must keep working from `file://`). A finished design for the whole UI is in `design/`. Your job is to rebuild the UI layer to match it and wire it to the existing engine. Do not rewrite the engine.

Read first: `README.md`, `index.html`, `js/main.js`, `js/state.js`, and every file in `js/ui/`. Then read the design files listed below. Make a plan before editing.

## Ground rules

1. **Keep the engine as it is:** `ns.js, state.js, ocr.js, judge.js, queue.js, image.js, export.js, fs.js, fs-projects.js, render.js`. If a change is truly needed (for example a new field in `newPage`), keep it small, additive, and list it in your final summary.
2. **Replace the UI layer:** `css/*` and `js/ui/*`, plus the shell in `index.html` (the left rail becomes the top header from the design). New views get new files in `js/ui/`.
3. **No frameworks, no bundler, no ES modules.** Plain scripts, `window.OCR.ui.*`, same init pattern as `js/main.js`.
4. **Offline-safe fonts.** The app runs from `file://`. Bundle Instrument Sans (400, 500, 600) and Source Serif 4 (400, 600, italic 400) as local `woff2` in `fonts/`, with `system-ui` and `Georgia` fallbacks. Do not link Google Fonts. Kalam appears in the design only to fake handwriting on a mock scan; do not ship it.
5. **Keep every existing capability** unless this file says otherwise: folder or browser storage, import/export, retry, stop, resume, demo mode, rotate/replace page, drag-to-reorder, keyboard support, `prefers-reduced-motion`.
6. Keyboard focus must stay visible, touch targets are at least 44px, and all text keeps 4.5:1 contrast in both themes.

## How to read the design files

Each `design/*.dc.html` is one screen written in a small template format. Read it as a spec:

- Inline `style="..."` and the `<helmet><style>` block are the real CSS. Values like `var(--ink)` are the tokens listed below.
- `{{name}}`, `<sc-for list="{{rows}}" as="r">` and `<sc-if value="{{flag}}">` are template holes, loops and conditions. `renderVals()` in the bottom `<script>` shows what data each hole expects and how styles are chosen from state.
- **Preview-only, do not ship:** the `dark` prop and `tk()` method (the app uses `data-theme` instead), the sample names and numbers, the faux scan paper (`Kalam` text on ruled paper; the real app shows the page image), the dimmed fake app behind each dialog, `data-props`, `$preview`, and the `hint-placeholder-*` attributes.
- `canvas.json` is the board layout. `Phone.dc.html` is marked "later" and is out of scope.

## Screens and where they live

| Design file | Becomes | Notes |
|---|---|---|
| `Projects.dc.html` | `#home` view (`js/ui/home.js`) | "Projects" are called **assignments** in the UI. Folder card has four states, see `States.dc.html`. |
| `Batch.dc.html` | **new** inbox view (`js/ui/inbox.js`), `S.d.view === 'inbox'` | One row per student, built by grouping pages (see data additions). |
| `Main.dc.html` | `#project` view: `panes.js`, `strip.js`, part of `actionbar.js` | The review screen. |
| `Add.dc.html` | **new** `add` view (`js/ui/add.js`) | A staging step in front of `U.upload.addFiles`. |
| `Settings.dc.html` | `U.openSettings` modal | Same fields as today, friendlier labels. |
| `Export.dc.html` | `U.openExport` modal | Same actions as today. Scope is now "Everyone" or "Only students who are ready". |
| `AssignmentSettings.dc.html` | replaces the left drawer (`drawer.js`) | Name, topic, prompt. Opened from the inbox. |
| `Glossary.dc.html` | `U.openTopics` modal | Same as today with plain labels. |
| `States.dc.html` | banners, empty states, error blocks, run bar variants | Reuse these everywhere they apply. |

View routing: extend `S.d.view` to `home | inbox | review | add`. Opening an assignment goes to `inbox`; a student row goes to `review` for that student's first page; "Add pages" goes to `add`. Keep the confirm-before-leaving behavior when a run is active.

### Vocabulary (UI word, engine word)

assignment = project, student = a group of pages, subject = topic, subject words = topic terms or glossary, instructions = prompt, transcription server = OCR server, spelling helper = judge, "Corrected / Original" = `textVersion`, "to check" = open corrections or `[illegible]`.

## Wiring, screen by screen

### Projects home
- List: `S.d.projects` (`name, slug, pages, done, updated`). Card actions call `fs.renameProject`, `fs.duplicateProject`, `fs.exportProjectZip` + `OCR.export.download`, `fs.deleteProject` (with `U.confirm`, danger).
- Header buttons: `fs.importFiles` for zip and folder, create via `U.prompt` then `fs.createProject`.
- Folder card states from `S.d.folder` (`mode`: `none | folder | idb`, `permission`: `granted | prompt`). Save-failure banner uses `S.d.banner`.

### Inbox
- Rows: group `S.pages()` by `page.student.name` (pages without one become their own row, named after the file). Row status:
  - `failed`: any page `status === 'error'`
  - `processing`: any page `transcribing`
  - `queued`: any page `pending` that belongs to the active run (check how `queue.js` exposes membership; if it does not, add a read-only getter)
  - `check`: all pages `done` and at least one open item (see "Words to check") and not `reviewed`
  - `ready`: all pages `done`, nothing open or `reviewed`
- Counts, the segmented bar and the filter tabs are computed from those groups. `S.counts()` still drives the run bar.
- Run bar: `S.d.run` (`active, done, total, etaMs, message`). Buttons: `Q.stop()`, `Q.retryFailed()`, `Q.resume()`; label "Resume (N left)" when stopped, as in `actionbar.js`. Format ETA with `OCR.fmtDur`.
- Save indicator: `S.d.save.status` (`saving | saved | error`). Server problems: `S.d.serverError` and `S.d.banner` (render with the banner pattern from `States.dc.html`).
- Row "Retry" calls `Q.start(ids, { force: true })` for that student's failed pages.
- Re-render on `pages:structure`, `page:update`, `stats`, `change` (throttle with `requestAnimationFrame`, like `actionbar.js`).

### Add pages
- Drop zone and "Choose files" reuse `U.upload.bindDrop`, `U.upload.pick`, `collect`/`walk` (folders and paste keep working). Instead of adding straight into the project, call `OCR.image.check` and show rejects in the yellow "files were skipped" notice (the design lists unsupported types: PDF and HEIC are rejected by `image.check`; keep it that way and keep the iPhone tip).
- Accept PNG, JPG, WebP, 40 MB each, as `image.check` enforces. Natural-sort by path like `addFiles` does.
- Queue list: staged files with thumbnail (`OCR.image.thumb`), file name, detected student (or the "Name not found" select), and remove.
- "Adding to" shows the current assignment (read-only).
- Subject select writes `S.updateMeta({ topic })`; options come from `S.d.settings.topics`.
- "Read the name at the top of each page" is a new setting `readNames` (default on) in `S.saveSettings`.
- CTA "Transcribe N pages": commit staged files with the existing add path (`S.newPage`, `fs.saveImage`, `S.addPages`), set `page.student`, call `Q.start(ids)`, then go to `inbox`.

### Review (the main screen)
- Header: back to inbox, prev/next student (groups, not pages), "Side by side / Text only" (new `S.d.layout`, persist in `meta.ui`), Copy text (`OCR.export.copyText` for the student's pages joined with the saved separator), "Done, next student" (sets `reviewed: true` on the student's pages, then selects the next student).
- Left pane = `#doc-pane`: the page image with the same tools as now: line boxes toggle (`S.d.showBoxes`), rotate, replace. The design's zoom control was dropped (the engine fits to width); do not add zoom.
- Page thumbnails under the pane header are the existing strip, **filtered to the current student's pages** and moved here from the bottom. Status dot colors: yellow = open items, green = done and checked, red = error, grey = pending. Keep drag-to-reorder and multi-select only if they fit; otherwise keep them in the inbox row's page list (your call, note it).
- Right pane = `#tx-pane`: `R.renderFormatted` output, with the "Corrected / Original" toggle bound to `S.d.textVersion`, the legend, a status pill ("N words to check", "All words checked"), and the stats line (`tokens in seconds`, saved state). Keep the streaming view for `transcribing` and the empty, error and retry blocks from `States.dc.html`.
- Struck text (`~~x~~`) renders as muted line-through. `Margin:` lines render as a block starting with a bold "Margin note" lead-in. Math stays KaTeX.

### Words to check (the tray)
This is the one part where the design is bigger than the old UI. It must work with the data the engine really produces: **no per-word confidence, no word boxes, no alternatives list.**

- Two kinds of flagged word:
  1. **Suggestion**: an entry in `page.corrections` (`word, term, conf, applied`). Highlighted yellow.
  2. **Illegible**: each `[illegible]` marker in `page.raw`. Highlighted red, dashed underline.
- Post-process the rendered transcript DOM: wrap each flagged word in a `<button>` (text nodes only, do not break KaTeX). Selected word gets the outline from the design.
- Tray options:
  - Suggestion: "Use <term>" (`J.toggle(id, i, true)`), "Keep <word>" (`J.toggle(id, i, false)` and mark resolved), or type your own.
  - Illegible: "Leave as [illegible]" (mark resolved) or type your own.
- "Use all N suggestions" calls `J.applyAll(id)`. Show it only when two or more suggestions are unresolved.
- Typed text: store it so it survives re-toggling. Recommended: new fields on the page, `resolved: { 'c:<index>' | 'i:<occurrence>': 'suggested' | 'written' | 'typed' | 'marked' }` and `edits: [{ key, text }]`, applied on top of `corrected` when rendering and exporting. Make `OCR.export` read the same effective text; if that needs a small change to `export.js` or `render.js`, keep it minimal and report it.
- A word is "checked" (green underline) once it has an entry in `resolved`. The pill counts unresolved words on the current student's pages.
- The "This line, as the student wrote it" crop uses the page's line `boxes` (they are line-level and approximate). Pick the box whose vertical center is closest to the word's position in the text (estimate from the word's line index; if there is no match, hide the crop and keep the options). Never show a crop you cannot place.

## Data additions (all optional fields with safe defaults, so old projects still open)

On each page: `student: { name, source: 'read' | 'chosen' | 'file', confirmed: boolean }`, `reviewed: boolean`, `resolved: {}`, `edits: []`.
On the project: nothing new. Existing `meta.topic` and `meta.prompt` are used as is.
Setting: `readNames: true`, plus `layout` in `meta.ui`.
Persist through the existing `S.updatePage`, `S.updateMeta`, `fs.saveMeta` paths.

## Reading the student's name (new, keep it isolated)

Add `js/student.js` (classic script, `OCR.student.read(blob)`). Crop the top ~14% of the prepared image, send a separate short request to the same server with a low temperature asking for the student's name only, as JSON `{ "name": "..." | null }`. Never mix this into the main transcription prompt. If the result is null or very short, the name is "not found" and the Add screen shows the select; the user can also type a new name. Fall back to the file name stem for `source: 'file'`. The inbox sorts students by name (natural compare).

Test this on real pages before building the UI around it. Report how often it is right.

## Import of old sessions

The README says importing sessions saved by the old single-file `ocr.html` is not written. The original file is `design/legacy-ocr.html`; its session format (pages with `raw`, `corrected`, `corrections`, `boxes`, and the prompts and judge logic) is the source of truth. Write `OCR.fs.importLegacy` reading its saved session JSON into a new assignment. It is a nice-to-have: do it last, after everything else is working.

## Design tokens

Put these in `css/tokens.css` as custom properties on `:root`. Dark values apply under `[data-theme="dark"]` and under `@media (prefers-color-scheme: dark)` for `[data-theme="auto"]` (the existing theme setting values are `auto | light | dark`). Replace the old token names or alias them.

| Token | Light | Dark |
|---|---|---|
| `--bg` | #EEF1F5 | #0F131C |
| `--surface` | #FFFFFF | #171C28 |
| `--sunken` | #F3F5F8 | #1D2331 |
| `--ink` | #141B2D | #E8ECF3 |
| `--on-ink` | #FFFFFF | #0F131C |
| `--muted` | #5A6477 | #9AA5B8 |
| `--faint` | #8792A6 | #6F7A90 |
| `--line` | #D3D9E2 | #2B3345 |
| `--line2` | #BCC5D2 | #3E4A63 |
| `--soft` | #E3E8EF | #222A3A |
| `--hover` | #E4E9F0 | #232B3B |
| `--track` | #E1E6EE | #2A3347 |
| `--warn` | #FFE27A | #6B5712 |
| `--warn-bg` | #FFF6CF | #3A3110 |
| `--warn-line` | #E6C84F | #7A6516 |
| `--bad` | #A5221B | #FF9C92 |
| `--bad-bg` | #FBDAD5 | #4A1F1B |
| `--ok` | #176B45 | #5FD39A |
| `--ok-bg` | #DDF0E5 | #14382A |
| `--accent` (link hover) | #26358F | #9DB0FF |
| `--scanbg` (image pane) | #C6CEDA | #0A0D14 |
| `--scanhead` | #D3DAE4 | #131824 |
| `--scanline` | #B5BECC | #252D40 |
| `--scrim` | rgba(20,27,45,.5) | rgba(5,7,11,.62) |

Fixed colors (same in both themes): inbox bar green `#2F8F62` (edge `#176B45`), yellow `#FFE27A` (edge `#D9A400`), red `#E26D5C` (edge `#A5221B`); highlight on the page image `rgba(255,226,122,.6)` yellow and `rgba(251,218,213,.85)` red.

Type: UI is Instrument Sans 14px/1.4 (weights 400, 500, 600). Page titles and the transcript are Source Serif 4: titles 32 to 36px/1.15, section titles 24px/1.2, transcript 22px/1.7 at most 31em wide. Radii: 8 (controls), 10 (cards, panels), 12 (dialogs), 999 (pills). Controls are 44px tall. Dialogs are 600px wide with a 28px inner gutter and a footer divider.

Meaning of color: yellow = a spelling suggestion to check, red with dashed underline = could not be read, green underline = the teacher checked it. Never use color alone: the legend text and the underline style carry the meaning too.

Copy rules: sentence case, plain verbs, buttons say what they do ("Transcribe 5 pages"), errors say what happened and what to do next, no apologies.

## Icons

Use the Phosphor icons the engine already loads (`OCR.icon`) where a match exists. The design uses inline stroke SVG at 20 to 22px; either is fine as long as sizes and weight look the same.

## Test against the real model before calling it done

1. Transcribe a real stack (at least 10 pages, mixed neat and messy, some with math). Check that `[illegible]` shows as red words and that glossary suggestions show as yellow ones.
2. Name reading: report the hit rate and what happens on a page with no name.
3. Line boxes on ruled paper: say how often a crop would land on the wrong line. If often, ship without the crop.
4. Stop a run halfway, reload, resume. Kill the server mid-run and confirm the server-down banner and "Retry".
5. Pick a folder, close the browser, reopen, reconnect. Try browser storage mode too.
6. Switch themes (system, light, dark) on every screen. Resize to a 900px wide window.
7. Export with a mix of resolved, typed and untouched words in each format and confirm the effective text matches what the review screen shows.

## Open questions I may need to answer (list them, do not stop for them)

- Whether `logprobs` from llama-server give usable per-word confidence for Qwen3-VL. If yes, the tray could later show alternatives again. Try it and report; do not build on it yet.
- Whether multi-page students should be grouped by name only, or also by file order.

## Definition of done

Every screen in `design/` is built and reachable, both themes work, nothing in the old engine broke, `README.md` is updated with the new views and files, and your final message lists every engine file you touched and why.
