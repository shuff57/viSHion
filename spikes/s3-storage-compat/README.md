# Spike S3: Rust/WASM storage compatibility with the JS app

Question: can Rust/WASM at the SAME origin as the current JS app read what the JS app wrote with no
loss, and write it back in a format the JS app still opens?

## Verdict: GO-WITH-CAVEATS

Reading, rewriting and creating data works in both IndexedDB mode and folder mode, and the real JS app
(the oracle) loads the result with deep-equal state. It only holds with a specific serde configuration;
the default serde_json configuration silently alters numbers and cannot read pages containing lone
surrogates. Mandatory settings are listed under "Rules for the Rust data layer".

## What was run

`node run.mjs` (serves the repo root on 127.0.0.1:8791, drives chromium 148 / Playwright 1.60). Everything
below was executed, results are in `out/results.json` (+ `out/snapshot-*.json`, screenshots).

1. Seeds the real oracle app (index.html, Demo mode, real image pipeline, real queue/judge/guess code):
   2 assignments, 7 generated-image pages, `keepOriginals` for one assignment (so `originals/*.png`), a
   corrected+reviewed page, an unknown meta field, `exports/*` as Blob / Uint8Array / ArrayBuffer, an
   old-schema page (missing optional fields, odd key order), a page with every nasty string/number class,
   a page with lone surrogates, legacy `ocr_sessions_v2` + legacy IDB `ocr_images/images`.
   Run twice: **IDB mode** (`ocr_fs/files`) and **folder mode** (see "Folder mode" for how a handle was
   obtained headless). 27 stored entries each, 13 of them JSON documents.
2. `rust/src/lib.rs` (web-sys + handwritten externs, serde/serde_json `preserve_order`) opens `ocr_fs` v1,
   lists `files`, classifies value types, parses every project.json / .bak / pages/*.json / settings.json,
   re-serialises with three model variants and writes back to the same key with the same JS value type.
   Also localStorage `ocr_settings_v1`, `ocr_sessions_v2` and legacy `ocr_images/images`.
3. After each write-back the oracle is reloaded and every assignment is opened through `OCR.fs.openProject`
   and all images loaded; `deepStrictEqual` against the pre-Rust dump (meta, pages, image sha256).
4. Byte-level comparison of JS `JSON.stringify` vs serde_json output over a 60+ case battery.

Pinned: wasm-bindgen 0.2.129 (matches the wasm-bindgen-cli already installed; nothing was installed),
serde_json 1.0.154, web-sys 0.3.106. No `trunk`. Release wasm is ~490 KB before wasm-opt.

## Results

| Check | IDB mode | Folder mode |
| --- | --- | --- |
| Rust lists exactly the same keys/paths as JS | yes (27/27) | yes (27/27) |
| Value types seen in `files` | string (json), Blob (images, originals, notes.md), Uint8Array, ArrayBuffer | File via handle |
| Rust reads a JSON doc written by JS | 12/13; 13/13 with surrogate escape handling | same |
| `value` model, `float_roundtrip` (frt) | 11 byte-identical, 1 differs (number notation) | same |
| `value` model, `arbitrary_precision` (arb) | 12/12 parseable byte-identical | same |
| typed model (struct + flatten extra), arb | 10 identical, 2 differ (key order only) | same |
| typed model, frt | 9 identical, 3 differ (key order x2, number notation x1) | same |
| plain serde_json (no feature) `value` write-back | **5 of 12 docs altered, oracle state no longer deep-equal** | same |
| frt / arb / +sur write-back, oracle reload | deep-equal, all assignments, pages, transcripts, images identical | same |
| naive f64 typed write-back | 11 docs byte-changed (`"order":0.0`), oracle state still deep-equal | same |
| Rust-created assignment + page + jpeg Blob | listed, opens, image loads (image/jpeg), `.bak` created, key order identical to JS | same |
| Rust edit of one page (+ meta.updated, .bak) | oracle shows edit, other pages deep-equal | same |
| localStorage settings round trip | byte-identical; Rust-set `theme` honoured; unknown key `rustWrote` survives an oracle re-save | settings.json overrides localStorage, see below |
| Legacy `ocr_sessions_v2` | 2 sessions byte-identical round trip | same |
| Legacy `ocr_images/images` | read (string data URLs) | same |

Untouched documents: parsed-JSON deep equality held for every document in every successful variant
(`parsedJsonDeepEqualFailures: []`). Non-JSON entries (images, originals, exports) are never rewritten and
their sha256 matched (`nonJsonChanged: []`).

## Mismatch classes (JS `JSON.stringify` vs serde_json)

Identical (verified, no action): 2-space pretty layout including empty `[]` / `{}`; compact layout;
`": "` / `","` separators; raw UTF-8 output (no `\u` escaping of non-ASCII, U+2028/2029, U+007F, bidi, ZWJ,
non-characters); `\b \f \n \r \t \" \\` short escapes; other controls as lowercase `\u00xx`; `/` unescaped;
`\u` astral pairs (JS emits raw char, so does serde); `-0` (JS writes `0`, never seen from JS);
`0.30000000000000004`, `5e-324`, 1.7976931348623157e308, 2^53, 1e21 (`1e+21` both), 1e22, 1e16,
1e15, 1e-7, 1.23e-5.

| # | Class | Effect | Fix |
| --- | --- | --- | --- |
| M1 | serde_json default float parsing is not correctly rounded: `0.09642857142857143` became `...145`, `0.38571428571428573` became `0.3857142857142858`, `0.020535714285714286` became `0.02053571428571429` | **silent data change**, 5 of 12 docs; oracle state not deep-equal | enable `float_roundtrip` (or `arbitrary_precision`) |
| M2 | Number notation thresholds: JS prints fixed for `[1e-6, 1e-5)` (`0.000001`) and integers `1e17..1e21` (`300000000000000000000`); ryu prints `1e-6`, `3e+20` | byte-only, parses to the same number | `arbitrary_precision` keeps original text (0 diffs); otherwise accept |
| M3 | Lone surrogate escapes (`"\ud800"`, which JSON.stringify emits) are rejected: `unexpected end of hex escape` / `lone leading surrogate` | **page unreadable** (and would be dropped by a naive loader) | `lenient_in`/`lenient_out` in lib.rs (sentinel U+10FFFE + 4 hex, lossless; round trip byte-identical) |
| M4 | Key order: typed struct emits declaration order, file order is JS insertion order. Seen in real data: `corrections` objects are `{applied, word, term, conf}` (judge does `Object.assign({applied:false}, c)`), old-schema pages, hand-added keys | byte-only; deep-equal still true; noisy diffs / git churn | keep documents as `Value` (preserve_order) and use typed views for fields you touch, or declare fields in file order and keep `flatten extra` |
| M5 | `f64` for numeric fields prints integers as `0.0` / `1.0` (`"order":0.0`, `"schemaVersion":1.0`, box `w:1`) | every doc changes; JS reads it fine but file churn | use `serde_json::Number` (or `Value`), never `f64`/`u32` for JS-written numbers (JS could write 1.5 into an int field and fail the parse) |
| M6 | `null` vs absent: `Option<T>` maps `student: null` / `stats: null` to None and drops the key (or emits `null` for missing keys) | semantic: JS code distinguishes `p.student` null and missing in places | `Option<Option<T>>` with a `double_option` deserializer, `skip_serializing_if = "Option::is_none"` on everything optional (done in lib.rs) |
| M7 | Hand-edited / foreign files: duplicate keys (typed: error, Value: last wins like JS), nesting > 128 (serde_json recursion limit; JS none), BOM (both fail), trailing newline dropped, `1.0`/`1E2` literals (JS normalises on its next save, arb preserves) | rare | handle errors by leaving the document untouched and surfacing it, never overwrite |
| M8 | Integer-like object keys: JS re-orders them first on every stringify; serde (preserve_order) keeps insertion order. User-named topics in settings could hit this | cosmetic | ignore |
| M9 | Without `preserve_order` serde_json sorts keys alphabetically (documented BTreeMap behaviour, not exercised here) | every doc reordered | the feature is mandatory |
| M10 | `fs.read` semantics: JS decodes Blob/ArrayBuffer with `TextDecoder` (BOM stripped, invalid UTF-8 replaced); Rust must do the same (`stored_text`) | edge | done |

Conventions to keep (all verified from the oracle's files): `project.json`, `project.json.bak` and
`settings.json` are pretty (2 spaces, no trailing newline); `pages/<id>.json`, `localStorage` blobs are
compact; `.bak` is written (only if the old copy still parses) *before* every write of a `project.json`
except at creation; settings.json strips `apiKey`, `profiles`, `storage`, `noticeSeen`; keys of `ocr_settings_v1` are
DEFAULTS order then extras; images are `Blob` typed `image/jpeg` under `images/<id>.jpg`.

## Rules for the Rust data layer

1. serde_json features: `preserve_order` + (`arbitrary_precision` preferred, else `float_roundtrip`).
   `arbitrary_precision` worked with `flatten` and `Number` fields in 1.0.154 (the historically broken
   combination is typed `u64`/`f64` + flatten on older versions, retest on upgrade). Numbers computed by
   Rust still print Rust-style; JS reads them fine.
2. Document model: `Value` (or `Map<String, Value>`) as the source of truth; typed accessors for the fields
   the UI edits. If typed structs are used: `Number` for numbers, double-option for nullables,
   `skip_serializing_if` on all optionals, `#[serde(flatten)] extra: Map` last, no `deny_unknown_fields`.
3. Lone surrogates: pre/post process as in `lenient_in`/`lenient_out`, or leave the document untouched.
4. Open `ocr_fs` with version **1** exactly. Any higher version triggers an upgrade and the JS app's
   `indexedDB.open('ocr_fs', 1)` then fails with VersionError. If the DB does not exist create both stores
   (`kv`, `files`) like the JS `onupgradeneeded`. `open_db` here aborts the upgrade instead of creating.
5. Value type preservation: write back strings as strings, Blobs as Blobs; JS reads all of string / Blob /
   ArrayBuffer / Uint8Array so mixed is tolerated, but keep the type.
6. One IDB transaction per awaited op (do not hold a transaction across `Blob.text()`), writes resolved on
   transaction `complete`, not request `success`.
7. API key lives only in localStorage; never copy it into settings.json. In folder mode settings.json is
   applied over localStorage at startup (observed: setting `theme` only in localStorage was ignored until
   `settings.json` was changed too).
8. No cross-tab lock exists in the JS app; two tabs (JS + Rust) can last-write-wins. Consider Web Locks
   if both apps are ever open at once.
9. Rust must tolerate absent optional fields (normalize() in JS fills defaults only in memory) and the
   `status: "transcribing"` crash-recovery rule (JS resets it to `pending` on open).

## Folder mode (FileSystemDirectoryHandle)

Headless pickers cannot be driven, so the oracle was run with `window.showDirectoryPicker` replaced by
`navigator.storage.getDirectory()` (an OPFS `FileSystemDirectoryHandle`). That makes the *real* JS code path
run: `fs.chooseFolder()` stores the handle with `kvSet('root', h)` (structured clone into IDB `kv`),
`fs.init()` after reload reads it back, `queryPermission` and `folderBackend`. Findings:

- A handle stored by JS reads back from Rust via a plain IDB `get('root')`: structured clone yields a live
  object whose constructor is `FileSystemDirectoryHandle`; with
  `#[wasm_bindgen(js_name = FileSystemDirectoryHandle)] type DirHandle;` `dyn_into::<DirHandle>()` succeeds
  (`is_dir_handle: true`). No serialization step is involved; the handle is not JSON.
- `queryPermission({mode:'readwrite'})` works with no user gesture and returned `granted` (OPFS always
  grants). Rust listed the directory (`values()` async iterator via `Reflect`), read, wrote with
  `getFileHandle({create:true})` + `createWritable()` + `close()`, created directories, and the oracle then
  opened the Rust-written data (same checks as IDB mode, all green).
- The FSA types were handwritten externs (top of lib.rs: 3 types, 9 methods) rather than web-sys (which
  may gate them behind `--cfg=web_sys_unstable_apis`; not checked). They were enough and are small.
- NOT verified (cannot be automated): a real on-disk handle from the picker. Expected from the platform
  and from `fs.init()`'s own logic: the handle structured-clones identically; `queryPermission` returns
  `granted` only while the permission persists (same session, or Chrome's persistent "allow on every
  visit"), otherwise `prompt`; reads/writes then throw NotAllowedError. `requestPermission` and
  `showDirectoryPicker` need transient user activation, so the Rust app must call them synchronously from
  a click handler (before the first `await`), exactly as `fs.reconnect()` does. The same-origin Rust app
  sees the same persisted-permission state as the JS app (permission is per origin + handle).

## Reproduce

```
cd spikes/s3-storage-compat
npm install            # playwright 1.60 only
bash build.sh          # needs wasm-bindgen-cli 0.2.129 on PATH (already installed)
node run.mjs           # ~4 min; writes out/results.json
```

Files: `rust/` crate, `harness.html` (loads the 3 wasm builds), `serve.mjs`, `run.mjs`, `out/`.
Nothing outside this folder was changed and nothing was committed.
