# S4 - streaming fetch + SSE in Rust/WASM

**Verdict: GO.** The Rust client reproduces js/ocr.js's event sequence in 32 of 33 scenarios against the real
`js/ocr.js` (loaded unmodified), in Node and in real Chromium, and in 6000 randomised byte streams. The one
divergence is a lone-surrogate JSON escape, which is deliberate (see table). Caveats are listed at the bottom.

## Layout

| file | what |
|---|---|
| `src/sse.rs` | **pure** SSE state machine + incremental UTF-8 decoder + JS `trim()` (no web types; `cargo test` runs natively, 8 tests) |
| `src/web.rs` | thin shell: `web-sys` fetch/Request/Response/AbortSignal + `wasm-streams` `ReadableStream -> Stream` |
| `mock-server.js` | Node mock `/v/<variant>/v1/chat/completions` (25 stream variants + 5 HTTP failures) |
| `clients.js` | the real `js/ocr.js` and the Rust client behind one signature |
| `compare.js` | every variant, both clients, event-by-event comparison + server-side "did the client hang up" check |
| `fuzz.js` | differential fuzz (random SSE-ish byte streams, random split points, no network) |
| `browser.js` | same comparison inside Chromium (Playwright, real `window`, real CORS preflight) |
| `build.sh` | build steps |

Build: `rustc 1.92`, `wasm-bindgen =0.2.128` + matching CLI, `wasm-streams 0.7.0`, `serde_json` (`float_roundtrip`).
Run: `sh build.sh && node compare.js && node fuzz.js 6000 && node browser.js`.

## How comparison works

Both clients get the same body of events: `status` messages and `text(chunk, full)` callbacks, then a result
`{text, tokens, tps}` or an error `{name, kind, message, partial}`. The Rust client returns raw text and the
harness runs the real `O.clean` over it (clean is a separate port). Server side: the mock records whether the
client closed the socket before the stream finished (abort behaviour).

## Results (compare.js, Node 25 / undici)

SAME = identical events, outcome, error kind/message/partial and server-side hang-up.

| scenario | JS result | Rust | note |
|---|---|---|---|
| normal, one-packet, crlf, comments, scalar-payloads, data-after-done, timings-only, no-stats | ok | SAME | tokens fallback chain `usage -> timings.predicted_n -> chunks` verified |
| split-bytes (7-byte writes, splits emoji and lines), bom, bad-utf8 | ok | SAME | U+FFFD per maximal subpart, BOM stripped once, like `TextDecoder` |
| nbsp-nel (`U+0085`, `U+FEFF`, `U+00A0` before `data:`) | ok | SAME | needs the hand-written JS `trim` set |
| `[DONE]` with no trailing newline | error `stream` "stopped before finishing" | SAME | **bug-compat**, see below |
| `[DONE]\n` (single newline) | ok | SAME | |
| no `[DONE]` at all | error `stream` | SAME | |
| drop-mid (socket destroyed) | error `stream` + `partial` | SAME | |
| one malformed JSON chunk | error `stream` "unreadable" | SAME | whole page fails, partial kept |
| empty `data:` keep-alive | error "unreadable" | SAME | **bug-compat** |
| `data: null` | error "unreadable" | SAME | `null.choices` throws inside the JS try |
| CR-only line endings | error "unreadable" | SAME | no `\r` splitting in JS |
| HTTP 401 / 500 | error `http` | SAME | |
| HTTP 503 (+ empty body) | error `unavailable` | SAME | |
| HTTP 503 with emoji straddling char 160 | message ends in a lone surrogate | SAME | detail kept as `JsString`, `slice(0,160)` is UTF-16 |
| unreachable port | error `network` | SAME | |
| abort: pre-aborted / during headers / after 2nd chunk / timer (queue.js timeout path) | `AbortError` | SAME | connection closed server-side in both |
| abort with custom reason (`name != AbortError`) | error `stream` | SAME | JS misclassifies; reproduced |
| big (4000 chunks, 8001 events) | ok | SAME | |
| **lone-surrogate** (`\ud83d` / `\ude00` in separate chunks) | ok, text `a<emoji>b` | **DIFF**: error "unreadable" | serde_json rejects lone surrogate escapes |

Chromium run (`browser.js`): identical outcomes for the 17 scenarios it covers; same single lone-surrogate DIFF.
Fuzz: 6000 streams (2651 ok, 3349 error), 0 differences; lone surrogates excluded from the generator by design.

## Every behavioural difference, and whether it is deliberate

| # | difference | deliberate? |
|---|---|---|
| 1 | JSON string escapes that are lone surrogates (`"\ud83d"`) -> Rust marks `malformed`; JS accepts and may rejoin the pair across chunks | **Accepted divergence.** serde_json (and any Rust `String`) cannot hold lone surrogates. Real servers (llama.cpp, Ollama, OpenAI) emit valid UTF-8, so this should not occur; failure mode is a visible error, not silent corruption |
| 2 | Non-string truthy `delta.content` (number, bool, array, object): JS passes the raw value to `onText` and coerces with `+=`; Rust passes the coerced string (`12` -> `"12"`, object -> `"[object Object]"`). Accumulated text is identical | Accepted; never produced by real servers |
| 3 | `tps`: JS gives `undefined` when `timings` has no `predicted_per_second`, `null` without timings; Rust gives `null` for both | Accepted, harmless |
| 4 | `ms` from `Date.now()` instead of `performance.now()` | Accepted (1 ms resolution, wall clock) |
| 5 | On error/abort the Rust stream is dropped, which cancels the body (wasm-streams `cancel_on_drop`). JS leaves the reader open | Deliberate improvement; not observable in tests (both hung up on abort) |
| 6 | `/slots` polling (prompt-reading progress) is not ported | Out of scope for this spike; independent of streaming. JS polls every 400 ms via fetch+AbortSignal, trivial with `gloo-timers` |
| 7 | `O.clean` not ported (harness applies the real one) | Out of scope |
| 8 | `Response.body == null`: JS throws TypeError from `getReader`; Rust throws a TypeError | Same class, message differs |
| 9 | A throwing `onText`/`onStatus` callback is caught by the read-loop try/catch in JS and reported as a "connection dropped" `stream` error; Rust reproduces this | **Bug-compat** (callbacks that throw are an app bug either way) |

### Deliberate bug-compat (replicated, pinned by unit tests; flip individually when convenient)

1. Lines split on `\n` only; lone `\r` is not a terminator.
2. An unterminated final line is dropped, so `data: [DONE]` without a trailing newline is reported as
   "stopped before finishing". `SseParser.flush_tail = true` (exposed as the `flush_tail` argument) fixes it:
   verified to turn that scenario into `ok`. **Recommend enabling in the rewrite**; real servers end with `\n\n`
   so it is a latent, not live, bug.
3. `data:` with an empty payload (a keep-alive some proxies send) sets `malformed` and fails the page. Recommend
   ignoring empty payloads in the rewrite.
4. `malformed` is sticky and outranks `finished`; `[DONE]` does not stop reading; later usage/timings still win.
5. `null` payload is `malformed`; `123`, `"x"`, `[]`, `true` are silently ignored.
6. `usage`/`timings` keep the last truthy value; content only when truthy; `tokens` fallback chain.
7. Abort classification is `e.name === 'AbortError'` only, so `abort(reason)` with a custom reason is
   misreported as a `stream` error, exactly as in JS.

## Gotchas found

- `web_sys::window()` is `None` in Node and in workers. `fetch` is called through
  `js_sys::global().unchecked_into::<WorkerGlobalScope>().fetch_with_request(..)`, which works in window,
  worker and Node. Do not use `window().unwrap()`.
- **A global `window` stub in a test harness breaks `js_sys::global()`**: with `global.window = {...}` defined,
  the fetch call failed with `arg0.fetch is not a function` (js-sys resolves its global through `window`/`self`
  candidates). Harness passes `window` to the script as a parameter instead of defining a global.
- Rust `str::trim` and JS `trim` disagree: Rust trims U+0085, JS does not; JS trims U+FEFF, Rust does not.
  `js_trim` is hand-written and unit-tested.
- serde_json needs `float_roundtrip` to parse `predicted_per_second` to the same f64 as `JSON.parse`.
- `wasm-bindgen-cli 0.2.129` was not installable (cargo resolved `0.2.128`); the crate is pinned `=0.2.128` to
  match. CLI and crate versions must be identical.
- 503 body `slice(0,160)` counts UTF-16 units; slicing in Rust as `char`s/bytes would be wrong. Keep the value as
  a `JsString` until it is concatenated into the message.

## Cost

| artifact | raw | gzip |
|---|---|---|
| `vishion_s4_bg.wasm` (release, `opt-level=s`, lto, no wasm-opt) | 272 KB | 95 KB |
| `vishion_s4.js` glue (`--target web`) | 25 KB | 5.5 KB |

Most of the wasm is serde_json + web-sys glue; this will not repeat once the client lives in the app's single
wasm module. Parsing could be trimmed further by a hand-rolled extractor for the 4 fields used, if size matters.

## Caveats before relying on this

- `/slots` polling and `O.clean` still need porting (both straightforward, neither touches the state machine).
- Tested against undici (Node 25) and Chromium 1234 only; Firefox/Safari stream and abort timing not exercised.
- The mock cannot reproduce proxy-specific behaviours (buffering, HTTP/2 resets).
