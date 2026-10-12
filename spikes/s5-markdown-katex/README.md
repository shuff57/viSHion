# S5 - markdown-it + KaTeX interop from Rust/WASM

**Verdict: GO-WITH-CAVEATS for driving markdown-it 14.1.0 + KaTeX 0.16.9 from Rust via wasm-bindgen externs.
NO-GO for replacing markdown-it with pulldown-cmark.**

Rust reproduces js/render.js byte-for-byte on all 145 display units of the corpus; the cost is small. The
caveats are the UTF-16 boundary (lone surrogates), ~1.3x per-unit time, and a missing-library failure mode that
has to be handled explicitly.

## Layout

| file | what |
|---|---|
| `vendor/markdown-it-14.1.0`, `vendor/katex-0.16.9` | `npm pack` contents, pruned to `dist/*.min.js` + licence (no CDN at runtime). Both min.js files are byte-identical to the jsdelivr copies index.html loads (sha256 `38c70a1e...` / `dc84b296...`) |
| `src/pipeline.rs` | **pure** port of render.js string logic: `$$`/`$` hold, `[illegible]` marking, `⟦n⟧` restore, `R.units` (native `cargo test`) |
| `src/web.rs` | wasm-bindgen externs for `markdownit(...)`, `.renderInline`, `katex.renderToString`; `Renderer` class |
| `corpus.js` | corpus builder (uses the repo's real words.js / ocr.js for `W.marked`, `W.effective`, `O.clean`) |
| `run.js` | JS (real, unmodified `js/render.js`) vs Rust byte comparison + glue benchmarks; writes `inline-cases.json` |
| `tests/pulldown.rs` | pulldown-cmark evaluation against every string markdown-it's `renderInline` saw |

Run: `sh build.sh && node run.js && cargo test --test pulldown -- --nocapture`.

## Byte-identity results

Corpus: 64 transcripts / 145 units = 9 strings from tests/clean.test.js (raw and after `O.clean`), the inputs of
tests/words.test.js (raw, `W.marked` corrected/original with the U+E000..E002 markers, `W.effective`), the 3 DEMO
transcripts in js/ocr.js (LF and CRLF), and 30 synthetic transcripts covering `$inline$`, `$$display$$`
(single- and multi-line, unterminated), `~~strike~~`, lines starting `# `, `- `, `* `, `+ `, `1.`, `1)`, `> `, 4-space
and tab indents, fences, `---`, odd `$` counts (`$5 and $10`, `$ padded $`, `$$$`, `$$$$`), Windows newlines,
unicode/emoji/CJK, raw HTML, entities, links incl. `javascript:`, broken TeX (`\frac{1}{`, `\undefined`, an
expansion bomb), literal `⟦0⟧` placeholders in the source, markers inside text, and math inside bold/strike/link.

| check | result |
|---|---|
| `R.units` split (JS vs Rust) | 0 mismatches |
| `R.unit` HTML, byte for byte, 145 units | **0 mismatches** |
| KaTeX absent -> `<code class="math-fb">` fallback | identical |
| KaTeX parse errors (`throwOnError:false`) | identical error spans (it is the same KaTeX) |

What this does and does not prove: the markdown-it and KaTeX output is identical by construction (same
library); what was verified is the Rust port of the glue (`hold_math`, whitespace/regex semantics, restore).

### Not identical (hostile input, documented, not in the corpus)

| input | JS | Rust |
|---|---|---|
| lone surrogate in the transcript (`a\ud83d b`) | passes through to markdown-it/KaTeX unchanged | becomes U+FFFD at the Rust `&str` boundary |
| everything else tried (NUL, valid astral, emoji in `\text{}`) | same | same |

A lone surrogate cannot come from a UTF-8 network response (the decoder already produced U+FFFD), only from the
JS side (e.g. text already stored in a project, or a broken paste). Negligible, but it is a real difference.

## Glue cost (Node 25, V8, median of 5 runs)

| measure | JS direct | via Rust | delta |
|---|---|---|---|
| full pipeline, 5800 units | 54.8 ms (9.5 us/unit) | 69.3 ms (12.0 us/unit) | +26%, +2.5 us/unit |
| bare `renderInline` of a 50-char line | 2.83 us | 4.26 us | +1.4 us/call |
| bare `katex.renderToString('x^2+\frac{a}{b}')` | 31.7 us | 50.0 us | +18 us/call |

The KaTeX delta is two string copies of ~1.5 KB of HTML (JS UTF-16 -> wasm UTF-8 for the tex, then the result
UTF-8 -> JS). A page is about 20-60 units, so this is ~0.1-0.5 ms per page; imperceptible. Size:

| artifact | raw | gzip |
|---|---|---|
| `vishion_s5_bg.wasm` (release, `opt-level=s`, no wasm-opt) | 68 KB | 28 KB |
| `vishion_s5.js` glue (`--target web`) | 17 KB | 4 KB |

markdown-it (124 KB) and KaTeX (277 KB) stay as JS and must still be loaded; Rust does not shrink them.

## API gotchas

1. Both libraries are UMD **globals** (`window.markdownit`, `window.katex`), not ES modules. Externs must be
   `#[wasm_bindgen(js_name = markdownit)] fn markdownit(opts)` and `#[wasm_bindgen(js_namespace = katex, js_name =
   renderToString)]`. In Node (`--target nodejs`) the harness has to put them on `globalThis`.
2. **Missing library = ReferenceError on first call** (not a Rust error). index.html loads both from a CDN with
   `defer`; if the rewrite keeps that, the `Renderer` constructor must probe `typeof markdownit` first.
   render.js degrades to its `basic()` escape-only renderer in that case; **that fallback is not ported** here.
3. Declare externs that can throw with `catch` (`Result<String, JsValue>`), or a JS exception becomes a wasm trap
   that poisons the instance. `katex.renderToString` with `throwOnError:false` still throws for non-`ParseError`
   failures; render.js catches that and falls back to `<code class="math-fb">`, reproduced.
4. `renderInline(src, env)`: `env` is optional; do not pass `null`. The options object must be a real JS object
   (built with `Reflect::set`); `html:false, linkify:false, breaks:true` as booleans.
5. Strings are copied both ways; `Option<&str>`/`String` returns are UTF-8 validated, hence the lone surrogate case.
6. KaTeX `strict:'warn'` `console.warn`s for characters it has no font metrics for (seen with emoji inside a text-mode group). Same in JS; if
   noise matters pass `strict:'ignore'`, which would be a *behaviour change* in output for some inputs.
7. Bug-compat that is easy to miss: render.js replaces literal `⟦12⟧` in the source with math #12 or with `''`
   (silently deleting text) because the placeholder alphabet is user-reachable; `$ padded $` is *consumed* as a
   match and returned as-is, so its closing `$` cannot open the next span; `[illegible]` is replaced after
   markdown-it, case-insensitively; JS `\s`/`trim` differ from Rust `char::is_whitespace` (U+0085, U+FEFF).
   All reproduced in `pipeline.rs` and covered by the corpus.
8. `W.wrap` (words.js, DOM TreeWalker) is untouched and unaffected: HTML is identical, `.katex` ancestry too.

## pulldown-cmark as a replacement for `renderInline`?

`tests/pulldown.rs` feeds every distinct string markdown-it's `renderInline` received (98 from the corpus,
placeholders already substituted) through pulldown-cmark 0.13 with the best adapter I could write
(strikethrough on, raw HTML events turned into text, soft break -> `<br>`, `<del>` -> `<s>`, `<br />` -> `<br>`,
`<p>` stripped). Result: **71 / 98 identical (72%)**. The 27 failures:

| category | n | counter-example (markdown-it | pulldown) |
|---|---|---|---|
| list marker eaten | 12 | `1. Robotics budget due Friday` -> literal | `<ol><li>Robotics budget due Friday</li></ol>` |
| thematic break | 3 | `***` -> `***` | `<hr />` |
| ATX heading | 2 | `# Heading looking line` -> literal | `<h1>Heading looking line</h1>` |
| blockquote | 2 | `> quoted line` -> `&gt; quoted line` | `<blockquote><p>quoted line</p></blockquote>` |
| code block | 3 | `    four-space indented code` -> kept verbatim | `<pre><code>four-space indented code</code></pre>` |
| leading/trailing spaces | 2 | `hard  ` kept | `hard` |
| link URL validation | 1 | `[bad](javascript:alert(1))` stays text | `<a href="javascript:alert(1)">bad</a>` |
| `"` escaping | 1 | `&quot;quotes&quot;` | `"quotes"` (not escaped in text) |
| NUL | 1 | `a\0b` -> U+FFFD | `\0` |

Three of the twelve list cases are the app's own DEMO transcript ("1. Robotics budget due Friday" ...), i.e.
the content the app ships as sample data; the rest are synthetic but typical handwritten notes. Extra probes (not in the corpus, from `extra_block_probes`):

| input | markdown-it renderInline | pulldown-cmark |
|---|---|---|
| `[foo]: /url "title"` | `[foo]: /url &quot;title&quot;` | *empty string* (text lost: link reference definition) |
| `Term\n===` | `Term<br>\n===` | `<h1>Term</h1>` |
| `a\n---` | `a<br>\n---` | `<h2>a</h2>` |
| `~~~` | `~~~` | `<pre><code></code></pre>` |
| `* * *`, `+ plus`, `1) paren`, `  - two`, `- [ ] task` | literal | hr / lists |
| `text\n> quote` | literal `&gt;` | `</p><blockquote>...` (unbalanced after `<p>` strip) |

The root problem: markdown-it exposes an inline-only entry point (`renderInline`) and treats every unit as
paragraph *content*; pulldown-cmark has no inline-only mode, so block syntax at the start of a handwritten line
(numbered lists, dashes, `#`, `>`, indentation, `---` underlines) is interpreted, and handwriting produces
those constantly. A backslash-escaping pre-pass for line starts is possible but is hand-rolled block detection
(setext underlines, reference definitions, fences, 4-space rules), plus `<s>` vs `<del>`, quote and NUL
normalisation, URL validation (security: pulldown emits `javascript:` links). That is more code and more
risk than keeping markdown-it, and still would not be byte-identical. **Keep markdown-it.**

## Recommendation

Keep markdown-it + KaTeX as vendored JS, call through the thin `Renderer` (68 KB wasm), and port only the
`pipeline.rs` string logic. Alternative worth weighing: leave render.js as JS entirely; the Rust port gains
ownership of the pipeline but no performance or size benefit.
