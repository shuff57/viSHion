// Store oracle (successor of spikes/s3-storage-compat/run.mjs). Seeds the REAL JS app, lets the Rust
// project layer (crates/store over IdbStore / FsaStore, driven by crates/store-oracle) read, rewrite and
// create data at the same origin, then reloads the unmodified JS app and asserts nothing was lost.
// Usage: node build.mjs && node run.mjs     (CHROME=path overrides the browser)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PORT = +(process.env.PORT || 8791), BASE = `http://127.0.0.1:${PORT}`;
const OUT = path.join(HERE, 'out');
fs.mkdirSync(OUT, { recursive: true });
const results = { started: new Date().toISOString() };

/* ------------------------------------------------------------------ in-page helpers (serialised) */
async function snapIdb() {
  const hash = async (buf) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', buf))].map((b) => b.toString(16).padStart(2, '0')).join('');
  const db = await new Promise((res, rej) => { const r = indexedDB.open('ocr_fs', 1); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
  const all = (store) => new Promise((res) => {
    const o = {}; const c = db.transaction(store).objectStore(store).openCursor();
    c.onsuccess = () => { const cur = c.result; if (cur) { o[cur.key] = cur.value; cur.continue(); } else res(o); };
  });
  const files = await all('files'), kv = await all('kv');
  const desc = async (v) => {
    if (typeof v === 'string') return { type: 'string', text: v };
    if (v instanceof Blob) return { type: 'Blob', mime: v.type, size: v.size, sha: await hash(await v.arrayBuffer()) };
    if (v instanceof ArrayBuffer) return { type: 'ArrayBuffer', size: v.byteLength, sha: await hash(v) };
    if (v instanceof Uint8Array) return { type: 'Uint8Array', size: v.length, sha: await hash(v) };
    return { type: Object.prototype.toString.call(v) };
  };
  const out = { files: {}, kv: {}, ls: {} };
  for (const k of Object.keys(files)) out.files[k] = await desc(files[k]);
  for (const k of Object.keys(kv)) out.kv[k] = { type: kv[k] && kv[k].constructor && kv[k].constructor.name };
  for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); out.ls[k] = localStorage.getItem(k); }
  db.close();
  return out;
}

async function snapOpfs() {
  const hash = async (buf) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', buf))].map((b) => b.toString(16).padStart(2, '0')).join('');
  const out = { files: {}, kv: {}, ls: {} };
  const walk = async (d, pre) => {
    for await (const [name, h] of d.entries()) {
      if (h.kind === 'directory') await walk(h, pre + name + '/');
      else {
        const f = await h.getFile();
        const isText = /\.(json|bak|md)$/.test(name);
        out.files[pre + name] = isText ? { type: 'string', text: await f.text() } : { type: 'File', mime: f.type, size: f.size, sha: await hash(await f.arrayBuffer()) };
      }
    }
  };
  await walk(await navigator.storage.getDirectory(), '');
  for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); out.ls[k] = localStorage.getItem(k); }
  return out;
}

async function restore({ S0, mode }) {
  /* put every JSON/string document of the baseline snapshot back (only strings are ever rewritten) */
  if (mode === 'idb') {
    const db = await new Promise((res, rej) => { const r = indexedDB.open('ocr_fs', 1); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
    for (const [k, v] of Object.entries(S0.files)) {
      if (v.type !== 'string') continue;
      await new Promise((res, rej) => { const t = db.transaction('files', 'readwrite'); t.objectStore('files').put(v.text, k); t.oncomplete = res; t.onerror = () => rej(t.error); });
    }
    /* remove anything added since */
    const keys = await new Promise((res) => { const r = db.transaction('files').objectStore('files').getAllKeys(); r.onsuccess = () => res(r.result); });
    for (const k of keys) if (!(k in S0.files)) await new Promise((res) => { const t = db.transaction('files', 'readwrite'); t.objectStore('files').delete(k); t.oncomplete = res; });
    db.close();
  } else {
    const root = await navigator.storage.getDirectory();
    for (const [k, v] of Object.entries(S0.files)) {
      if (v.type !== 'string') continue;
      const parts = k.split('/'); let d = root;
      for (const x of parts.slice(0, -1)) d = await d.getDirectoryHandle(x, { create: true });
      const w = await (await d.getFileHandle(parts[parts.length - 1], { create: true })).createWritable();
      await w.write(v.text); await w.close();
    }
  }
  /* localStorage back to baseline */
  for (const [k, v] of Object.entries(S0.ls)) localStorage.setItem(k, v);
}

async function dumpOracle() {
  const OCR = window.OCR, S = OCR.state;
  const hash = async (buf) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', buf))].map((b) => b.toString(16).padStart(2, '0')).join('');
  await OCR.fs.refresh();
  const out = { projects: JSON.parse(JSON.stringify(S.d.projects)), detail: {}, settings: JSON.parse(JSON.stringify(S.d.settings)), folder: S.d.folder };
  delete out.settings.apiKey;
  for (const p of S.d.projects) {
    const ok = await OCR.fs.openProject(p.slug);
    const P = S.d.project, imgs = {};
    for (const id of P.meta.order) {
      const b = await OCR.fs.loadImage(id);
      imgs[id] = b ? { size: b.size, type: b.type, sha: await hash(await b.arrayBuffer()) } : null;
    }
    out.detail[p.slug] = { ok, meta: JSON.parse(JSON.stringify(P.meta)), pages: JSON.parse(JSON.stringify(P.pages)), imgs, selected: S.d.selectedId };
    await OCR.fs.closeProject();
  }
  return out;
}

/* seeds the oracle through its own public APIs (Demo mode transcription, real image pipeline) */
async function seedOracle(cfg) {
  const OCR = window.OCR, S = OCR.state;
  const draw = (seed, lines) => new Promise((res) => {
    const c = document.createElement('canvas'); c.width = 900; c.height = 1200;
    const g = c.getContext('2d'); g.fillStyle = '#fff'; g.fillRect(0, 0, 900, 1200);
    g.strokeStyle = '#111'; g.lineWidth = 3;
    for (let l = 0; l < lines; l++) {
      const y = 140 + l * 110;
      g.beginPath(); g.moveTo(80, y);
      for (let x = 80; x < 80 + 500 + ((seed * 37 + l * 53) % 200); x += 14) g.lineTo(x, y + Math.sin((x + seed * 9 + l) / 9) * 22);
      g.stroke();
    }
    c.toBlob((b) => res(b), 'image/png');
  });
  S.saveSettings({ demo: true, guessIllegible: true });
  if (cfg.mode === 'idb') await OCR.fs.useBrowserStorage(); else await OCR.fs.chooseFolder();
  const made = {};
  async function mk(name, topic, prompt, n, keep, students) {
    const slug = await OCR.fs.createProject(name);
    await OCR.fs.openProject(slug);
    S.updateMeta({ topic, prompt });
    S.saveSettings({ keepOriginals: keep });
    const list = [];
    for (let i = 0; i < n; i++) {
      const raw = await draw(i + name.length, 9);
      const file = new File([raw], 'student' + (i + 1) + '.png', { type: 'image/png' });
      const prep = await OCR.image.prepare(file);
      const p = S.newPage(file.name);
      p.student = { name: students[i], source: 'chosen', confirmed: true };
      OCR.fs.saveImage(p.id, prep.blob, keep ? { file, ext: 'png' } : null);
      list.push(p);
    }
    S.addPages(list);
    await OCR.queue.start(list.map((p) => p.id));
    made[slug] = list.map((p) => p.id);
    return { slug, ids: list.map((p) => p.id) };
  }
  const A = await mk('Algebra: Quadratics', 'Math/Education', 'Transcribe carefully.', 4, false, ['Maya R.', 'Devon T.', 'Sofia V.', 'Aaliyah M.']);
  /* a corrected / reviewed page with suggestions, edits and resolved flags, written via the app's own updatePage */
  const a0 = S.page(A.ids[0]);
  S.updatePage(a0.id, {
    corrections: [{ word: 'confidance', term: 'confidence', conf: 0.9, applied: true }, { word: 'lisinopril', term: 'lisinopril', conf: 0.30000000000000004, applied: false }],
    corrected: a0.raw + '\nconfidence', reviewed: true, resolved: { 'c:0': 'suggested', 'i:0': 'edited' }, edits: [{ key: 'i:0', text: 'discriminant', at: Date.now() }]
  });
  S.updateMeta({ ui: { selectedId: A.ids[1], layout: 'text' }, futureMetaField: { a: [1, 2, { b: null }], note: 'unknown to the Rust model' } });
  await OCR.fs.flushAll();
  /* weird values on one page: every number / string class that JSON.stringify and serde_json may disagree on */
  const a1 = S.page(A.ids[1]);
  S.updatePage(a1.id, {
    raw: 'café café 😀   \u0001 \u001f "q" \\ </script> \u007f 日本語 ‮ /slash',
    boxes: [{ x: 0, y: 0.1, w: 1, h: 0.2, r: 0, s: 12.5 }, { x: 0.000001, y: 1e-7, w: 0.30000000000000004, h: 0.5, r: -0, s: 1e21 }],
    numbers: { a: 1e-7, b: 1e21, c: 0.1 + 0.2, d: -0, e: 1e16, f: 123456789012345680000, g: 0.000001, h: 5e-324, i: 1.7976931348623157e308, j: 100, k: 1.5, l: 2 ** 53, m: 2 ** 53 + 2, n: 1e300, o: 1.23e-5, p: 0.00001, q: 1e15, r: 4.5e+15 },
    intKeys: { 10: 'ten', 2: 'two', b: 'b', a: 'a', 1: 'one' },
    empties: { e: [], o: {}, ee: [[], {}], n: null, s: '' }
  });
  S.updatePage(A.ids[2], { raw: 'lone surrogate: \ud800 and trailing \udfff end', corrected: 'lone surrogate: \ud800 and trailing \udfff end' });
  const B = await mk('Clinic Notes — Écoles & 日本語 ✓', 'Medical', '', 3, true, ['Ana', null, 'Caleb P.']);
  S.updateMeta({ prompt: 'Spelling: use <b>"quotes"</b> & tabs\tand newline\nend' });
  await OCR.fs.flushAll();
  /* non-JSON siblings the folder layout contains */
  await OCR.fs.write(B.slug + '/exports/notes.md', new Blob(['# export\n']));
  await OCR.fs.write(B.slug + '/exports/bytes.bin', new Uint8Array([0, 1, 2, 250, 255]));
  await OCR.fs.write(B.slug + '/exports/ab.bin', new Uint8Array([9, 8, 7]).buffer);
  /* an OLD-schema page (missing optional fields, odd key order) and a hand-written project referencing it */
  const oldId = 'oldpage1';
  const oldPage = { name: 'old.jpg', id: oldId, status: 'done', raw: 'old text', order: 0, corrected: 'old text', boxes: [], corrections: [], timings: { ms: 10, tokens: 3 }, error: '', rotation: 0 };
  await OCR.fs.write(B.slug + '/pages/' + oldId + '.json', JSON.stringify(oldPage));
  await OCR.fs.write(B.slug + '/images/' + oldId + '.jpg', new Blob([new Uint8Array([0xff, 0xd8, 0xff, 0xd9])], { type: 'image/jpeg' }));
  const meta = JSON.parse(await OCR.fs.read(B.slug + '/project.json'));
  meta.order.push(oldId); meta.stats.pages += 1;
  await OCR.fs.write(B.slug + '/project.json', JSON.stringify(meta, null, 2));
  /* legacy single-file app data */
  const PNG1 = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
  localStorage.setItem('ocr_sessions_v2', JSON.stringify([
    { id: 's3', name: 'Legacy picture', created: 1700000002000, lastRaw: 'has a real picture', hasImage: true, appliedDiff: [] },
    { id: 's4', created: 1700000003000, hasImage: true },
    { id: 's1', name: 'Legacy one', created: 1700000000000, lastRaw: 'legacy transcript → one', preRaw: 'legacy raw', hasImage: true, topic: 'General', prompt: 'p', appliedDiff: [{ orig: 'teh', term: 'the' }] },
    { id: 's2', name: 'Legacy é', created: 1700000001000, lastRaw: 'two', hasImage: false, extraLegacy: { n: 1.5 } }]));
  await new Promise((res, rej) => {
    const r = indexedDB.open('ocr_images', 1);
    r.onupgradeneeded = () => r.result.createObjectStore('images');
    r.onsuccess = () => { const tx = r.result.transaction('images', 'readwrite'); tx.objectStore('images').put('data:image/png;base64,iVBORw0KGgo=', 's1'); tx.objectStore('images').put(PNG1, 's3'); tx.objectStore('images').put('data:image/png;base64,iVBORw0KGgo=', 's4'); tx.oncomplete = () => { r.result.close(); res(); }; };
    r.onerror = () => rej(r.error);
  });
  /* the oracle's own legacy import: Rust must produce the same project from the same data */
  const legacy = await OCR.fs.importLegacy({});
  await OCR.fs.flushAll();
  return { A, B, legacy };
}


/* ------------------------------------------------------------------ comparison helpers */
function cmpSnap(a, b) {
  const changed = [], added = [], removed = [];
  for (const k of Object.keys(a.files)) {
    if (!(k in b.files)) removed.push(k);
    else if (JSON.stringify(a.files[k]) !== JSON.stringify(b.files[k])) changed.push(k);
  }
  for (const k of Object.keys(b.files)) if (!(k in a.files)) added.push(k);
  const lsChanged = Object.keys({ ...a.ls, ...b.ls }).filter((k) => a.ls[k] !== b.ls[k]);
  return { changed, added, removed, lsChanged };
}
function deepEq(a, b) { try { assert.deepStrictEqual(a, b); return true; } catch (e) { return false; } }
function jsonDocsEqual(a, b, keys) {
  /* parsed-JSON deep equality for the given file keys */
  const bad = [];
  for (const k of keys) {
    try { assert.deepStrictEqual(JSON.parse(a.files[k].text), JSON.parse(b.files[k].text)); } catch (e) { bad.push(k); }
  }
  return bad;
}
const isJsonKey = (k) => /(^|\/)(project\.json(\.bak)?|settings\.json)$|\/pages\/[^/]+\.json$/.test(k);

/* ------------------------------------------------------------------ checks */
const checks = [];
function ck(name, ok, detail) {
  checks.push({ name, ok: !!ok, detail: ok ? undefined : detail });
  console.log((ok ? 'ok   ' : 'FAIL ') + name + (ok || detail === undefined ? '' : '  -> ' + JSON.stringify(detail).slice(0, 400)));
}
const omit = (o, ks) => Object.fromEntries(Object.entries(o).filter(([k]) => !ks.includes(k)));
const normLegacy = (d) => ({
  meta: omit(d.meta, ['created', 'updated', 'order']),
  pages: d.meta.order.map((id) => omit(d.pages[id], ['id', 'boxes'])),
  hasImg: d.meta.order.map((id) => !!d.imgs[id])
});
const META_KEYS = ['schemaVersion', 'name', 'topic', 'prompt', 'order', 'created', 'updated', 'ui', 'stats'];
const PAGE_KEYS = ['id', 'name', 'order', 'status', 'raw', 'corrected', 'corrections', 'boxes', 'timings', 'error', 'rotation', 'student', 'reviewed', 'resolved', 'edits', 'guesses'];
const exe = process.env.CHROME || undefined;

async function newBrowserPage(browser, mode) {
  const ctx = await browser.newContext();
  if (mode === 'folder' || mode === 'opfs') await ctx.addInitScript(() => { window.showDirectoryPicker = async () => navigator.storage.getDirectory(); });
  const page = await ctx.newPage();
  const logs = [];
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') logs.push(m.type() + ': ' + m.text().slice(0, 200)); });
  page.on('pageerror', (e) => logs.push('pageerror: ' + String(e).slice(0, 200)));
  await page.route(/cdn\.jsdelivr\.net/, (r) => r.abort());
  const open = async (url) => { await page.goto(BASE + url); await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 30000 }); };
  const H = '/tools/store-oracle/harness.html';
  const rs = async (fn, ...args) => page.evaluate(async ([fn, args]) => window.rs[fn](...args), [fn, args]);
  return { ctx, page, logs, open, rs, H };
}

/* ------------------------------------------------------------------ old data, both backends */
async function scenario(browser, mode) {
  const T = (s) => `[${mode}] ${s}`;
  const { ctx, page, logs, open, rs, H } = await newBrowserPage(browser, mode);
  const snap = mode === 'idb' ? snapIdb : snapOpfs;
  const R = { mode };

  /* seed the unmodified JS app */
  await open('/index.html');
  const seeded = await page.evaluate(seedOracle, { mode });
  await page.waitForTimeout(500);
  await page.evaluate(() => OCR.fs.flushAll());
  const S0 = await page.evaluate(snap);
  const D0 = await page.evaluate(dumpOracle);
  fs.writeFileSync(path.join(OUT, `snapshot-${mode}.json`), JSON.stringify(S0, null, 1));
  const keys0 = Object.keys(S0.files);
  const types0 = new Set(Object.values(S0.files).map((v) => v.type));
  R.files = keys0.length;
  R.types = [...types0];
  ck(T('seed has string and binary entries'), types0.has('string') && (types0.has('Blob') || types0.has('File')), [...types0]);
  ck(T('seed contains a lone-surrogate page'), Object.entries(S0.files).some(([k, v]) => v.type === 'string' && /\\ud800/.test(v.text || '')), null);
  ck(T('JS legacy import produced a project'), D0.detail[seeded.legacy.slug] && D0.detail[seeded.legacy.slug].ok, seeded.legacy);

  /* 1. Rust opens every project through the project layer and writes everything back untouched */
  await open(H);
  R.perm = mode === 'folder' ? await rs('folder_permission') : null;
  const rep = JSON.parse(await rs('rewrite_all', mode));
  R.rewrite = rep;
  ck(T('rewrite: every project opened, none unusable'), rep.projects === D0.projects.length && rep.unusable.length === 0, rep);
  ck(T('rewrite: no unreadable pages, none from backup'), rep.unreadable.length === 0 && rep.fromBackup.length === 0, rep);
  ck(T('rewrite: settings.json handled'), rep.settings === ('settings.json' in S0.files), { settings: rep.settings, has: 'settings.json' in S0.files });
  const S1 = await page.evaluate(snap);
  const c1 = cmpSnap(S0, S1);
  const notBak = c1.changed.filter((k) => !k.endsWith('.bak'));
  ck(T('rewrite: nothing removed, only .bak entries added'), c1.added.every((k) => k.endsWith('.bak')) && c1.removed.length === 0, c1);
  ck(T('rewrite: every entry byte-identical with the same JS value type (only .bak may change)'), notBak.length === 0, notBak);
  const badBak = c1.changed.concat(c1.added).filter((k) => k.endsWith('.bak')).filter((k) => S1.files[k].text !== S0.files[k.replace(/\.bak$/, '')].text);
  ck(T('rewrite: .bak holds the previous project.json'), badBak.length === 0, badBak);
  ck(T('rewrite: localStorage untouched'), c1.lsChanged.length === 0, c1.lsChanged);
  R.rewriteChangedBak = c1.changed.length;
  await open('/index.html');
  const D1 = await page.evaluate(dumpOracle);
  ck(T('rewrite: JS reload deep-equal (all projects, pages, images)'), deepEq(D0.detail, D1.detail), Object.keys(D0.detail).filter((k) => !deepEq(D0.detail[k], D1.detail[k])));

  /* 2. Rust edits, creates, duplicates, deletes, imports; JS reloads */
  await open(H);
  const target = seeded.A;
  await rs('edit_page', mode, target.slug, target.ids[3], '\n[edited by rust]');
  const jpegBytes = Array.from(await page.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 64; c.height = 80;
    const g = c.getContext('2d'); g.fillStyle = '#fff'; g.fillRect(0, 0, 64, 80); g.fillStyle = '#000'; g.fillRect(8, 20, 40, 4);
    const b = await new Promise((r) => c.toBlob(r, 'image/jpeg', 0.9));
    return Array.from(new Uint8Array(await b.arrayBuffer()));
  }));
  const made = JSON.parse(await rs('create_project', mode, 'Made by Rust: éè', new Uint8Array(jpegBytes), 'Rust transcript line 1\nline 2 → arrow'));
  const life = JSON.parse(await rs('lifecycle', mode, made.slug, 'Renamed by Rust'));
  const leg = JSON.parse(await rs('import_legacy', mode, ''));
  R.made = made; R.life = life; R.leg = leg;
  const S2 = await page.evaluate(snap);
  await open('/index.html');
  const D2 = await page.evaluate(dumpOracle);
  const dt = D2.detail[target.slug], dm = D2.detail[made.slug];
  const imgKey = made.slug + '/images/' + made.id + '.jpg';
  ck(T('edit: JS shows the Rust edit'), dt.pages[target.ids[3]].corrected.endsWith('[edited by rust]') && dt.pages[target.ids[3]].reviewed === true, dt.pages[target.ids[3]].corrected.slice(-30));
  ck(T('edit: other pages and projects deep-equal'), target.ids.slice(0, 3).every((id) => deepEq(D0.detail[target.slug].pages[id], dt.pages[id])) && deepEq(D0.detail[seeded.B.slug], D2.detail[seeded.B.slug]), null);
  ck(T('edit: untouched page JSON byte-identical'), target.ids.slice(0, 3).every((id) => { const k = target.slug + '/pages/' + id + '.json'; return S2.files[k].text === S0.files[k].text; }), null);
  ck(T('create: JS lists and opens the Rust project, page text and image'),
    dm && dm.ok && Object.values(dm.pages)[0].raw.startsWith('Rust transcript') && Object.values(dm.imgs)[0] && Object.values(dm.imgs)[0].size === jpegBytes.length, dm && { ok: dm.ok, imgs: dm.imgs });
  ck(T('create: image stored as a Blob typed image/jpeg (idb) / readable file (folder)'), mode === 'idb' ? S2.files[imgKey].type === 'Blob' && S2.files[imgKey].mime === 'image/jpeg' : S2.files[imgKey].size === jpegBytes.length, S2.files[imgKey]);
  ck(T('create: project.json key order and page key order match the JS app'), dm && deepEq(Object.keys(dm.meta).slice(0, 9), META_KEYS) && deepEq(Object.keys(Object.values(dm.pages)[0]), PAGE_KEYS), dm && [Object.keys(dm.meta), Object.keys(Object.values(dm.pages)[0])]);
  ck(T('create: project.json is pretty, page compact'), S2.files[made.slug + '/project.json'].text.startsWith('{\n  "schemaVersion": 1,\n') && !S2.files[made.slug + '/pages/' + made.id + '.json'].text.includes('\n'), null);
  const dd = D2.detail[life.dup];
  ck(T('lifecycle: duplicate opens in JS, renamed, deleted page gone'), dd && dd.ok && dd.meta.name === 'Renamed by Rust' && dd.meta.order.length === 0, dd && { name: dd.meta.name, order: dd.meta.order });
  ck(T('lifecycle: deleted copy is gone (no leftover keys or dirs)'), !D2.projects.some((p) => p.slug === life.deleted) && !Object.keys(S2.files).some((k) => k.startsWith(life.deleted + '/')), life);
  ck(T('lifecycle: export file written next to the project'), (made.slug + '/exports/rust-export.md') in S2.files && (life.dup + '/exports/rust-export.md') in S2.files, null);
  ck(T('lifecycle: page and image of the deleted page removed'), !Object.keys(S2.files).some((k) => k.startsWith(life.dup + '/images/') || k.startsWith(life.dup + '/pages/')), null);
  const jsLeg = D2.detail[seeded.legacy.slug], rsLeg = D2.detail[leg.slug];
  ck(T('legacy: Rust import equals the JS import (meta, pages, image presence)'), jsLeg && rsLeg && rsLeg.ok && deepEq(normLegacy(jsLeg), normLegacy(rsLeg)), rsLeg && { js: normLegacy(jsLeg), rs: normLegacy(rsLeg) });
  ck(T('legacy: counts equal and legacy storage untouched'), leg.pages === seeded.legacy.pages && leg.withImage === seeded.legacy.withImage && S2.ls.ocr_sessions_v2 === S0.ls.ocr_sessions_v2, { rust: leg, js: seeded.legacy });
  R.consoleLogs = logs.slice(0, 20);
  await ctx.close();
  return R;
}

/* ------------------------------------------------------------------ fresh store: the shared suite */
async function freshSuite(browser, mode) {
  const T = (s) => `[suite-${mode}] ${s}`;
  const { ctx, page, open, rs, H } = await newBrowserPage(browser, mode === 'opfs' ? 'opfs' : 'idb');
  await open(H);
  let res;
  try {
    res = mode === 'opfs' ? await page.evaluate(async () => window.rs.suite_handle(await navigator.storage.getDirectory())) : await rs('suite_idb');
  } catch (e) { res = 'ERR ' + String(e).slice(0, 400); }
  ck(T('project-layer suite passes (same checks as native FsStore)'), res === 'ok', res);
  await open('/index.html');
  const after = await page.evaluate(async (mode) => {
    const OCR = window.OCR;
    if (mode === 'opfs') await OCR.fs.chooseFolder(); else await OCR.fs.useBrowserStorage();
    await OCR.fs.refresh();
    const list = OCR.state.d.projects.map((p) => p.slug);
    const opened = {};
    for (const s of list) { opened[s] = await OCR.fs.openProject(s); await OCR.fs.closeProject(); }
    const dbs = indexedDB.databases ? await indexedDB.databases() : [];
    return { list, opened, ver: (dbs.find((d) => d.name === 'ocr_fs') || {}).version };
  }, mode);
  ck(T('JS app lists the suite projects'), after.list.length >= 5 && after.list.includes('algebra-quadratics'), after.list);
  ck(T('JS app opens every one of them'), Object.values(after.opened).every(Boolean), after.opened);
  if (mode === 'idb') ck(T('ocr_fs created by Rust at version 1'), after.ver === 1, after.ver);
  await ctx.close();
  return { mode, suite: res, after };
}

/* ------------------------------------------------------------------ main */
const srv = await serve(PORT);
const browser = await chromium.launch({ executablePath: exe });
let crashed = null;
try {
  results.suiteIdb = await freshSuite(browser, 'idb');
  results.suiteOpfs = await freshSuite(browser, 'opfs');
  results.idb = await scenario(browser, 'idb');
  results.folder = await scenario(browser, 'folder');
} catch (e) {
  crashed = e;
  console.error(e);
} finally {
  await browser.close();
  srv.close();
}
results.finished = new Date().toISOString();
results.checks = checks;
fs.writeFileSync(path.join(OUT, 'results.json'), JSON.stringify(results, null, 1));
const failed = checks.filter((c) => !c.ok);
console.log(`\n${checks.length - failed.length}/${checks.length} checks passed${crashed ? ' (run crashed)' : ''}`);
process.exit(failed.length || crashed ? 1 : 0);
