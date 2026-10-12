# Parity oracle for `crates/core`

```
node tools/parity/run.js            # regenerate fixture.json from the real js/*.js, then cargo test -p vishion-core
node tools/parity/run.js --no-cargo # regenerate only
cargo test -p vishion-core          # replay the committed fixture (no node needed)
```

`run.js` loads the unmodified `js/ns.js`, `state.js`, `ocr.js`, `judge.js`, `words.js`, `guess.js`,
`groups.js`, `student.js`, `export.js` and `render.js` with a stub `window` (the pattern of
`tests/*.test.js` and `spikes/s5-markdown-katex/run.js`), runs a seeded set of inputs through them, and writes
`fixture.json`: input plus what the JS returned. `crates/core/tests/parity.rs` replays every case through the
Rust crate and compares the serialised results as strings, so key order and number text count.

Covered:

| kind | what is compared |
|---|---|
| `page` | `effective`, `marked` (corrected / original), `items` (key, kind, index, word, term, conf, base, via, shown), `count`, `contextFor`, and the page + `settings.reference` after each of `choose`, `reopen`, `useAll`, `judge.toggle`, `judge.applyAll`. Inputs: the `tests/words.test.js` scenarios, 260 seeded fuzz pages, and every S5 corpus transcript. |
| `heuristic`, `accept`, `remote` | `J.heuristic`, `J.accept`, `J.remote` through a stubbed `fetch` (accepted suggestions, strict mode, errors) |
| `student_reply`, `student_demo` | `ST.read` with a stubbed `fetch`/canvas (name cleaning and reply parsing) and demo mode names |
| `guesses`, `numbered` | `W.parseGuesses`; the numbered transcript in the request `W.guess` really sends |
| `groups` | `G.list` (keys, names, open, words, failed, status, source), `tally`, `matches`, `of`, `names`, `nameOf`, `keyOf` |
| `clean` | `O.clean` on the `tests/clean.test.js` inputs plus 500 generated wrappers |
| `render`, `render_libs`, `bands` | `R.units` and `R.unit` HTML (no libraries); with markdown-it 14.1.0 + KaTeX 0.16.9 recorded and replayed through the Rust glue; `R.mapBands` |
| `export` | `E.pageText`, `E.label`, `E.combined` (27 mode/sep/fmt combinations), `E.csv`, `E.json`, `E.perPage`, `E.docx` and `E.run` for every kind (names, MIME, SHA-256 of the bytes) |
| `settings`, `new_page`, `assignment` | `loadSettings` defaults and merge, `newPage`, `addPages`/`removePage`/`movePage`/`updatePage`/`syncStats` |
| `nat`, `slug`, `number_str`, `to_fixed`, `csv_secs` | `OCR.natCompare` (51 626 pairs: ASCII all-pairs, words, every BMP Latin/Nd/No code point vs probes; `natfold.js` regenerates `crates/core/src/natfold.rs`), `OCR.slug`, `Number()`/`String()` coercion, `toFixed(1)` ties |

`render_libs` needs `spikes/s5-markdown-katex/vendor/*/dist/*.min.js` (gitignored there). When they are missing
the script keeps the `render_libs` section already in `fixture.json` and says so.
