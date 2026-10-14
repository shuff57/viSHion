# store oracle

Successor of `spikes/s3-storage-compat/run.mjs`. Runs in Chrome (playwright-core, `CHROME` overrides the browser):

1. seeds the **unmodified** JS app (IDB mode and folder mode, folder via OPFS standing in for the picker):
   assignments, originals, exports as Blob/Uint8Array/ArrayBuffer, an old-schema page, odd numbers and strings,
   a lone-surrogate page, legacy `ocr_sessions_v2` + `ocr_images`, and a JS-made legacy import;
2. the Rust project layer (`crates/store` over `IdbStore` / `FsaStore`, driven by `crates/store-oracle`)
   opens and rewrites everything: every entry must stay byte-identical with the same JS value type;
3. the JS app reloads and every project, page and image must be deep-equal;
4. Rust edits a page, creates, renames, duplicates and deletes projects, imports the legacy sessions; the JS
   app opens the results (and the Rust legacy import must equal the JS one);
5. the same project-layer suite that `cargo test -p vishion-store` runs against `FsStore` and `MemStore`
   runs on a fresh `ocr_fs` and a fresh OPFS folder, then the JS app lists and opens what it left.

    cd tools/store-oracle && npm ci && node build.mjs && node run.mjs

Needs `wasm-bindgen-cli` 0.2.129 (matches `Cargo.lock`). CI runs it after the web build. Results land in `out/`.
