# Spike S2: Leptos 0.8 CSR + Trunk under the real CSP on Workers static assets

Verdict: **GO-WITH-CAVEATS**. Works under the production CSP with exactly two additions to `script-src`.
The caveats are all build or tooling details listed under "Gotchas", none blocking.

## Run it

```
cargo install trunk --locked        # trunk 0.21.14 used
npm i                               # playwright only (node_modules is gitignored)
node build.mjs                      # trunk build --release, writes dist/_headers, stages dist-legacy/
N=10 node test.mjs                  # starts 2x `wrangler dev --local`, drives chromium, writes results.json
```

Only local `wrangler dev` is used (`wrangler.jsonc`: `assets.directory ./dist`, no routes, no account). Nothing was deployed.

Files: `src/main.rs` (signal + button + `localStorage` via web-sys), `index.html` (Trunk entry, copy-dir of
`../../css` and `../../fonts`, plain `<link rel=stylesheet href=css/tokens.css>`), `build.mjs` (build + CSP hash
generation), `test.mjs` (Playwright), `results.json` (raw output of the last run).

## CSP that worked

Repo `_headers` CSP, with `script-src` extended (everything else byte-identical):

```
default-src 'self'; script-src 'self' https://cdn.jsdelivr.net 'sha256-1E5ezg6KKp4xOwY/5fOsJBORK7Re+Fka9Lsd/x7zRq4=' 'wasm-unsafe-eval' 'sha256-<hash of Trunk bootstrap>'; style-src 'self' https://cdn.jsdelivr.net 'unsafe-inline'; font-src 'self' https://cdn.jsdelivr.net; img-src 'self' blob: data:; connect-src 'self' https: http://localhost:* http://127.0.0.1:*; object-src 'none'; base-uri 'none'; frame-ancestors 'none'
```

- `'wasm-unsafe-eval'` is required; without it `WebAssembly.instantiateStreaming` is blocked (`script-src wasm-eval`).
  It does NOT enable JS `eval`. No `'unsafe-eval'` needed.
- Trunk emits one inline `<script type="module">` bootstrap (`import init ... await init(...)`). Its sha256 changes
  every build because the content-hashed filenames are embedded in it. `build.mjs` recomputes it from the built
  `dist/index.html` and writes `dist/_headers`. Verified: dropping the hash gives `script-src-elem inline` and the app does not mount.
- The existing theme-bootstrap hash (`1E5ez...`) is kept for the real index.html's inline script.
- No `connect-src` change needed: same-origin wasm fetch is covered by `'self'`.
- Violations observed in the happy path: none (console + `securitypolicyviolation` events).

`dist/_headers` also adds `Cache-Control: public, max-age=31536000, immutable` for `/*.wasm`, `/*.js` (hashed names) and
`/fonts/*`, and `max-age=0, must-revalidate` for `/index.html`. Wrangler reported "Parsed 5 valid header rules" and the
responses carried them.

## Measured (local, Windows 11, chromium 1234 via Playwright, wrangler dev, localhost, no throttling)

Functional assertions: signal incremented to 3 after 3 clicks, `localStorage` read returned the seeded value,
Caveat woff2 loaded from `/fonts/` (font face status `loaded`), zero CSP violations.

Served headers: `.wasm` is `content-type: application/wasm`, `cache-control: public, max-age=31536000, immutable`;
JS is `text/javascript`, same immutable cache; woff2 `font/woff2` immutable; HTML and CSS revalidate.
(`wrangler dev` gzips wasm/js; production Cloudflare will serve brotli if the client accepts it.)

Negative controls (CSP header rewritten in flight, to prove enforcement is real):

| Variant | Result |
|---|---|
| no `'wasm-unsafe-eval'` | not mounted, `script-src wasm-eval` violation |
| no bootstrap hash | not mounted, `script-src-elem inline` violation |
| `style-src` without `'unsafe-inline'` | mounted, but Leptos' static `style="..."` attribute is blocked (`style-src-attr`) |

Sizes (hello-world Leptos app, release, `opt-level=z`, fat LTO, cgu=1, panic=abort, strip, wasm-opt -Oz):

| File | raw | gzip -9 | brotli -11 |
|---|---|---|---|
| `*_bg.wasm` | 69,161 | 29,011 | 24,495 |
| `*.js` glue | 25,399 | 5,614 | 4,903 |

Time to interactive (ms from navigation start until the interactive element exists in the DOM; median of 10):

| App | cold (empty cache) | warm (cache primed, reload) | bytes cold / warm |
|---|---|---|---|
| Leptos/WASM | 37 | 27 | 37.6 KB / 0.6 KB |
| Current JS app (CDN scripts, online) | 240 | 113 | 307 KB / 11 KB |

Raw runs are in `results.json`. For legacy, "ready" = `#topbar` populated by `js/main.js`; for Leptos = `#inc` present.
The legacy run was online (jsDelivr reachable), so its number includes CDN fetches; offline it would not boot those
libs. Leptos has no CDN dependency.

Read these numbers carefully: the Leptos app is a hello-world, so this is the floor of startup cost (fetch + compile
+ instantiate ~69 KB of wasm), not a projection for the full app. Real app size will grow with leptos features, serde,
etc. The valid conclusion is that the wasm startup mechanism itself costs ~10s of ms and is not a risk, not that the
rewrite will be 6x faster.

## Gotchas

1. **wasm-opt vs rustc 1.92**: Trunk's bundled wasm-opt (version_123) fails with `memory.copy operations require bulk
   memory operations`. Fix is in the Trunk link tag: `data-wasm-opt-params="--enable-bulk-memory
   --enable-nontrapping-float-to-int --enable-sign-ext --enable-reference-types --enable-multivalue --enable-mutable-globals"`.
   Trunk downloads wasm-opt on first build (first release build took ~2m20s including that).
2. **Windows: Trunk fails to move `dist/.stage/css` -> `dist/css` ("Access is denied")** when `dist` is inside this
   repo. Building with `--dist <os tmpdir>` works; `build.mjs` does that then copies to `./dist`. Not diagnosed further
   (suspect an indexer/AV handle); CI on Linux is unlikely to hit it.
3. **SRI / integrity**: Trunk adds `integrity="sha384-..."` to the `modulepreload` and wasm `preload` links. Chrome
   logs a warning that integrity is ignored for `as=fetch` preloads. `data-integrity="none"` on the rust link
   removes it (done). SRI itself is not blocked by CSP and works same-origin.
4. **Hashed filenames**: `app-<hash>.js` and `app-<hash>_bg.wasm` are content hashes (changed when source changed),
   so `immutable` caching is safe. `index.html` must stay `must-revalidate` or users pin stale hashes.
5. **Inline bootstrap**: the module script's sha256 changes every build, so `_headers` must be generated post-build
   (done in `build.mjs`). Alternative to avoid per-build CSP churn: not available in stock Trunk (no external-bootstrap
   option that I verified); a post-build step that moves the bootstrap to an external `boot.js` and drops the hash is a
   small follow-up if wanted.
6. **copy-dir** copies everything, including `fonts/_workspace/` (dev scratch); `build.mjs` deletes it from dist.
   Better: keep scratch out of `fonts/` in the real app. copy-dir preserves `dist/css` + `dist/fonts` so
   `url("../fonts/...")` in tokens.css resolves unchanged. Unhashed (not `data-trunk`) stylesheet link is required to
   keep that relative path; Trunk's `rel=css` would hash and relocate it.
7. **Inline style attributes**: current CSP keeps `style-src 'unsafe-inline'`. If the rewrite later wants to drop it,
   Leptos static `style="..."` attrs (and `style:prop=` reactive bindings should be re-tested) are blocked; use classes.
8. wrangler 4.59.2's runtime supports compatibility_date only up to 2026-01-14; the spike config uses that to avoid
   a fallback warning. The repo's real `wrangler.jsonc` says 2026-10-01 (works, just warns locally).
9. Favicon 404 in console is just the spike having no favicon; unrelated to CSP.
10. `.assetsignore` for the real deploy should exclude `target/`, `node_modules/`, `spikes/` if the asset directory
   stays at repo root; with Trunk the asset dir becomes `dist/`, which sidesteps this.
