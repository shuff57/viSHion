# vishion-core: deviations from the JS engine

Parity is checked by `node tools/parity/run.js` (see `tools/parity/README.md`). Everything below is a place
where the Rust crate intentionally or unavoidably differs from `js/*.js`, or where a JS behaviour was left to a
later phase. Quirks of the JS (for example `$&` in a glossary term being expanded by `String#replace`, or
`Number(x.conf) || 0.5` turning a 0 confidence into 0.5) are reproduced, not fixed.

## Intentional

| Area | JS | Rust | Why |
|---|---|---|---|
| Side effects | `S.updatePage` saves, emits events, `S.addPages` selects the first page (writes `meta.ui.selectedId`) | `Assignment` ops mutate the model only; selection is not modelled | UI state, not model. The oracle pre-sets `selectedId` so `select()` stays out of the comparison. |
| Clock | `Date.now()`, `new Date().toISOString()` inside `W.remember` / `E.json` | `now: i64` / `exported: &str` parameters | `core` has no I/O or clock; the caller supplies them. |
| Queue | `OCR.queue.isQueued(id)` | `is_queued: &dyn Fn(&str) -> bool` argument to `groups::list` | Same reason. |
| Memo | `W.items` caches per page object in a `WeakMap` | no cache | Pure function; same result. |
| Assignment pages | `project.pages` is an insertion-ordered object | `BTreeMap` (sorted by id); display order always comes from `meta.order` | Nothing reads page map order. |
| Network and canvas | `J.remote`, `J.run`, `W.guess`/`ask`, `ST.read` (fetch, canvas, timers) | not ported; the pure parts are: `judge::parse_remote`, `judge::apply_found`, `guess::numbered`, `guess::parse_guesses`, `student::parse_reply`, `student::demo_name`, `student::clean` | I/O belongs to `net`/`ui` (plan phases 3 to 5). The oracle drives the real JS network paths with a stubbed `fetch` and compares the pure tails. |
| Idle unload | `M.idleCheck` mixes the rule with `fetch` and a timer | `idle::Idle` holds only the decision rule | `tests/idle.test.js` is ported against that rule; the unload request itself is `net`. |
| `Piece::Math` | not applicable (JS emits HTML) | carries no bold/italic flags | egui draws the formula as one block. |
| Inline pieces | markdown-it `renderInline` | pulldown-cmark with a U+1680 line-start marker so block syntax (`1.`, `#`, `-`, `>`, fences, indented code) stays literal text, as `renderInline` does; links with a scheme markdown-it rejects keep their source text | The JS never runs the block parser. The pieces carry the same characters and bold/italic/strike/code flags as markdown-it for all 272 inline strings of the S5 corpus (test `pieces_match_markdown_it_inline`). Links are drawn as their text; `<br>` is a space. |

## Unavoidable

* **Lone surrogates.** JS strings can hold them; Rust `String` cannot. A project document containing a lone
  `\udXXX` escape does not parse as a `Page`; the storage layer must leave such documents untouched (S3 rule).
* **Number text.** Numbers that come from stored JSON are kept verbatim (`arbitrary_precision`). Numbers the
  core computes (`conf`, `percent`, band geometry) print with the shortest round-trip digits, like JS, but
  exponent forms differ outside `1e-6 <= |x| < 1e21` (`1e21` vs JS `1e+21`); none of the computed values
  reach that range.
* **`localeCompare`.** `jsx::nat_compare` reproduces ICU `numeric: true, sensitivity: 'base'` for Latin text
  (space < punctuation < symbols < digits < other numbers < letters, case and accents ignored, digit runs by
  value). Characters whose base weight NFKD cannot give (`Đ`=`d`, `Ł`=`l`, `Ø`=`o`, `ß`=`ss`, `æ`=`ae`,
  `ı`/`þ`/`ƒ` between letters, non-ASCII decimal digits, `①`/`²`/Roman numerals after digits) come from
  `src/natfold.rs`, generated from node's ICU by `tools/parity/natfold.js`. The fixture compares every
  Latin-script, Nd and No code point of the BMP against 14 probe strings, plus a 144-name all-pairs sample
  and word pairs (51 626 comparisons), all equal to node. Not covered: non-Latin scripts (Greek, Cyrillic, CJK
  compare by code point of the lowercased char, which is right for the order Greek < Cyrillic but not
  guaranteed for every letter), supplementary-plane letters beyond Latin/Nd/No, and ICU contractions in
  other locales (JS uses the host locale; the fixture is node's default).
* **Case folding.** `to_lowercase` uses Rust's Unicode tables, V8 uses ICU's; equal for current Unicode data.
* **Non-string values in string fields.** `page.corrected` as a number or `edits[].text` missing are treated as
  empty by the typed accessors; JS would coerce or throw. Real projects never contain these.
* **JSON number text on re-serialise.** `export::json` prints numbers from stored text, JS prints its
  canonical form; identical for data the JS app wrote.

## Not done in this round

* **fflate zips.** `index.html` loads fflate, so the production JS app writes deflated zips (level 6).
  `export::store_zip` is the store-only writer the JS falls back to when fflate is absent, and that is what
  the oracle compares (byte for byte, docx and per-page zips included). A deflate stream identical to
  fflate's needs a port of its LZ77/Huffman encoder; the extracted files are identical either way.
* `models.js` (provider/profile logic), `ocr.js` beyond `clean`, `queue.js`, `image.js`, `deskew.js`, `fs*.js`
  belong to later phases.
* Typed accessors cover only what the ported logic reads; everything else stays in the `Map`.
