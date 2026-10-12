// Spike S3 driver: seeds the REAL JS app (oracle), lets Rust/WASM read/rewrite/create data at the same
// origin, then reloads the oracle and asserts nothing was lost.  Usage: node run.mjs
import { chromium } from 'playwright';
import { serve } from './serve.mjs';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PORT = 8791, BASE = `http://127.0.0.1:${PORT}`;
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
  localStorage.setItem('ocr_sessions_v2', JSON.stringify([
    { id: 's1', name: 'Legacy one', created: 1700000000000, lastRaw: 'legacy transcript → one', preRaw: 'legacy raw', hasImage: true, topic: 'General', prompt: 'p', appliedDiff: [{ orig: 'teh', term: 'the' }] },
    { id: 's2', name: 'Legacy é', created: 1700000001000, lastRaw: 'two', hasImage: false, extraLegacy: { n: 1.5 } }]));
  await new Promise((res, rej) => {
    const r = indexedDB.open('ocr_images', 1);
    r.onupgradeneeded = () => r.result.createObjectStore('images');
    r.onsuccess = () => { const tx = r.result.transaction('images', 'readwrite'); tx.objectStore('images').put('data:image/png;base64,iVBORw0KGgo=', 's1'); tx.oncomplete = () => { r.result.close(); res(); }; };
    r.onerror = () => rej(r.error);
  });
  return { A, B };
}

/* ------------------------------------------------------------------ format battery */
function jsCases() {
  const o = (v) => v;
  const nums = { '1e-7': 1e-7, '1e21': 1e21, '0.1+0.2': 0.1 + 0.2, 'minus-zero': -0, '1e16': 1e16, '1.2345678901234568e20': 123456789012345680000, '0.000001': 0.000001, '5e-324': 5e-324, 'max': 1.7976931348623157e308, '100': 100, '1.5': 1.5, '2**53': 2 ** 53, '2**53+2': 2 ** 53 + 2, '1e300': 1e300, '1.23e-5': 1.23e-5, '0.00001': 0.00001, '1e15': 1e15, '4.5e15': 4.5e15, '1e-6': 1e-6, 'u64max+': 18446744073709551616, '1.0 as int': 1, '-1': -1, '-1.5e-10': -1.5e-10, 'dateNow': 1760000000000, '3e20': 3e20, '1e22': 1e22, '9.5e-7': 9.5e-7 };
  const cases = [];
  for (const [k, v] of Object.entries(nums)) cases.push({ name: 'number ' + k, value: { x: v }, kind: 'value' });
  for (const [k, v] of Object.entries(nums)) cases.push({ name: 'page.boxes.w number ' + k, value: { id: 'p', boxes: [{ x: 0, y: 0, w: v, h: 0.1, r: 0 }] }, kind: 'page', pretty: false });
  const strs = {
    'escapes': 'tab\tnl\ncr\rbs\bff\f quote" back\\ slash/',
    'controls': '\u0000\u0001\u001f\u007f\u0080\u009f',
    'ls ps': 'a b c',
    'bidi/zw': '‮​﻿',
    'astral': '😀 𝓐',
    'nfc nfd': 'café café',
    'html': '</script><!-- -->&amp;',
    'lone high': 'x\ud800y',
    'lone low': 'x\udc00y',
    'lone pair reversed': '\udc00\ud800',
    'non-chars': '￾￿',
    'empty': ''
  };
  for (const [k, v] of Object.entries(strs)) cases.push({ name: 'string ' + k, value: { raw: v }, kind: 'value' });
  cases.push({ name: 'int-like key order', value: { b: 1, 10: 2, 2: 3, a: 4, 1: 5 }, kind: 'value', note: 'JS orders integer keys first on stringify' });
  cases.push({ name: 'empties pretty', value: { a: [], b: {}, c: [[], {}], d: [{}], e: { f: [] } }, kind: 'meta', pretty: true });
  cases.push({ name: 'null vs missing stats/student', value: { name: 'n', stats: null }, kind: 'meta', pretty: true });
  cases.push({ name: 'page student null', value: { id: 'x', student: null, reviewed: false }, kind: 'page', pretty: false });
  cases.push({ name: 'page key order differs from struct order', value: { name: 'n', id: 'x', raw: 'r', status: 'done', order: 0 }, kind: 'page', pretty: false });
  cases.push({ name: 'meta key order differs', value: { updated: 2, name: 'n', schemaVersion: 1, created: 1 }, kind: 'meta', pretty: true });
  cases.push({ name: 'page unknown field kept', value: { id: 'x', futureThing: { a: [1, 2, 3], b: null } }, kind: 'page', pretty: false });
  cases.push({ name: 'meta pretty nested', value: { schemaVersion: 1, name: 'n', order: ['a', 'b'], ui: { selectedId: 'a', layout: 'split' }, stats: { pages: 2, done: 1 } }, kind: 'meta', pretty: true });
  cases.push({ name: 'settings compact', value: { theme: 'auto', topics: { General: { description: 'd', terms: [] } }, reference: [], model: '' }, kind: 'settings', pretty: false });
  cases.push({ name: 'deep nesting 150', raw: '['.repeat(150) + ']'.repeat(150), kind: 'value', note: 'serde_json recursion limit 128' });
  cases.push({ name: 'duplicate keys', raw: '{"id":"a","id":"b"}', kind: 'page', pretty: false, note: 'JSON.parse last wins; typed struct errors' });
  cases.push({ name: 'duplicate keys value', raw: '{"id":"a","id":"b"}', kind: 'value', pretty: false });
  cases.push({ name: 'BOM prefix', raw: '﻿{"a":1}', kind: 'value', note: 'JSON.parse also fails on BOM' });
  cases.push({ name: 'trailing newline', raw: '{"a":1}\n', kind: 'value', note: 'both accept; reserialise drops the newline' });
  cases.push({ name: 'whitespace-heavy input', raw: '{ "a" : [ 1 , 2 ] }', kind: 'value', note: 'reformat expected' });
  cases.push({ name: 'literal 1.0 / 1E2 / -0 in input', raw: '{"a":1.0,"b":1E2,"c":-0,"d":1e+2,"e":0.10}', kind: 'value', note: 'only hand-edited files; JS never emits these' });
  cases.push({ name: 'key __proto__', raw: '{"__proto__":{"a":1},"b":2}', kind: 'value' });
  return cases.map((c) => ({ ...c, pretty: c.pretty == null ? false : c.pretty }));
}

function battery(pageRs, label) {
  return pageRs; // placeholder, real runner below
}

async function runBattery(page, cases, which) {
  return page.evaluate(async ({ cases, which }) => {
    const rs = { std: window.rs, arb: window.rsArb, frt: window.rsFrt }[which];
    const out = [];
    for (const c of cases) {
      const text = c.raw != null ? c.raw : JSON.stringify(c.value, null, c.pretty ? 2 : undefined);
      let jsParse = null, jsOut = null;
      try { jsParse = JSON.parse(text); jsOut = JSON.stringify(jsParse, null, c.pretty ? 2 : undefined); } catch (e) { jsParse = 'ERR ' + e.message; }
      const row = { name: c.name, kind: c.kind, note: c.note || null, input: text.length > 120 ? text.slice(0, 117) + '...' : text, jsOut: jsOut && jsOut.length > 160 ? jsOut.slice(0, 157) + '...' : jsOut, jsParseErr: typeof jsParse === 'string' ? jsParse : null, variants: {} };
      for (const v of (c.kind === 'value' ? ['value', 'value+sur'] : ['value', 'typed', 'naive', 'value+sur', 'typed+sur'])) {
        try {
          const r = rs.reserialize(text, c.kind, v, c.pretty);
          row.variants[v] = { ok: true, same_as_input: r === text, same_as_js: jsOut != null ? r === jsOut : null, out: r.length > 160 ? r.slice(0, 157) + '...' : r };
        } catch (e) { row.variants[v] = { ok: false, error: String(e).slice(0, 140) }; }
      }
      out.push(row);
    }
    return out;
  }, { cases, which });
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

/* ------------------------------------------------------------------ one backend scenario */
async function scenario(browser, mode) {
  const R = { mode };
  const ctx = await browser.newContext();
  if (mode === 'folder') await ctx.addInitScript(() => { window.showDirectoryPicker = async () => navigator.storage.getDirectory(); });
  const page = await ctx.newPage();
  const logs = [];
  page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') logs.push(m.type() + ': ' + m.text().slice(0, 200)); });
  page.on('pageerror', (e) => logs.push('pageerror: ' + String(e).slice(0, 200)));
  await page.route(/cdn\.jsdelivr\.net/, (r) => r.abort());
  const snap = mode === 'idb' ? snapIdb : snapOpfs;
  const open = async (url) => { await page.goto(BASE + url); await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 30000 }); };

  /* 1. seed the oracle */
  await open('/index.html');
  const seeded = await page.evaluate(seedOracle, { mode });
  R.seeded = seeded;
  await page.waitForTimeout(800);
  await page.evaluate(() => OCR.fs.flushAll());
  const S0 = await page.evaluate(snap);
  const D0 = await page.evaluate(dumpOracle);
  R.files = Object.keys(S0.files).length;
  R.fileTypes = Object.fromEntries(Object.entries(S0.files).map(([k, v]) => [k.replace(/[0-9a-z]{13,}/g, '<id>'), v.type]).filter((e, i, a) => a.findIndex((x) => x[0] === e[0]) === i));
  R.kv = S0.kv;
  R.projectsInOracle = D0.projects.map((p) => `${p.slug} (${p.pages} pages, ${p.done} done)`);
  fs.writeFileSync(path.join(OUT, `snapshot-${mode}.json`), JSON.stringify(S0, null, 1));
  await page.screenshot({ path: path.join(OUT, `oracle-${mode}-home.png`) });

  /* 2. Rust reads everything */
  await open('/spikes/s3-storage-compat/harness.html');
  R.features = await page.evaluate(() => ({ std: window.rs.serde_json_features(), arb: window.rsArb.serde_json_features(), frt: window.rsFrt.serde_json_features() }));
  R.inspect = JSON.parse(await page.evaluate((m) => window.rs.inspect(m), mode));
  R.inspectArb = JSON.parse(await page.evaluate((m) => window.rsArb.inspect(m), mode));
  R.inspectFrt = JSON.parse(await page.evaluate((m) => window.rsFrt.inspect(m), mode));
  R.ls = JSON.parse(await page.evaluate(() => window.rs.ls_report()));
  R.lsFrt = JSON.parse(await page.evaluate(() => window.rsFrt.ls_report()));
  R.lsArb = JSON.parse(await page.evaluate(() => window.rsArb.ls_report()));
  R.legacy = JSON.parse(await page.evaluate(() => window.rs.legacy_report()));
  if (mode === 'folder') R.probeRoot = JSON.parse(await page.evaluate(() => window.rs.probe_root()));
  /* Rust listing must equal the JS listing */
  R.listing = { js: Object.keys(S0.files).sort().length, rust: R.inspect.count, same: deepEq(Object.keys(S0.files).sort(), R.inspect.files.map((f) => f.path).sort()) };
  const summarize = (insp) => {
    const VS = ['value', 'typed', 'naive', 'value+sur', 'typed+sur'];
    const t = { docs: 0, parseFail: [] };
    VS.forEach((v) => { t[v] = { same: 0, diff: [], err: [] }; });
    for (const f of insp.files) {
      if (!f.doc) continue;
      t.docs++;
      if (!f.doc.parse_ok) { t.parseFail.push({ path: f.path, error: f.doc.error }); for (const v of ['value+sur', 'typed+sur']) { const d = f.doc[v]; if (d) { if (d.identical) t[v].same++; else if (d.error) t[v].err.push({ path: f.path, error: d.error }); else t[v].diff.push({ path: f.path, diff: d.diff }); } } continue; }
      for (const v of VS) {
        const d = f.doc[v];
        if (d.identical) t[v].same++;
        else if (d.error) t[v].err.push({ path: f.path, error: d.error });
        else t[v].diff.push({ path: f.path, diff: d.diff });
      }
    }
    return t;
  };
  R.inspectSummary = summarize(R.inspect);
  R.inspectArbSummary = summarize(R.inspectArb);
  R.inspectFrtSummary = summarize(R.inspectFrt);

  /* 3. write-back variants, then reload the oracle and compare */
  R.writeback = {};
  for (const [build, variant] of [['frt', 'value'], ['frt', 'typed'], ['arb', 'value'], ['frt', 'naive'], ['frt', 'value+sur'], ['frt', 'typed+sur'], ['std', 'value']]) {
    const tag = build + '/' + variant;
    await open('/spikes/s3-storage-compat/harness.html');
    await page.evaluate(restore, { S0, mode });
    const rep = JSON.parse(await page.evaluate(([m, v, b]) => ({ std: window.rs, arb: window.rsArb, frt: window.rsFrt })[b].roundtrip_write(m, v), [mode, variant, build]));
    const S1 = await page.evaluate(snap);
    const c = cmpSnap(S0, S1);
    const jsonKeys = Object.keys(S0.files).filter(isJsonKey).filter((k) => k in S1.files);
    const badDeep = jsonDocsEqual(S0, S1, jsonKeys);
    const nonJsonChanged = c.changed.filter((k) => !isJsonKey(k));
    await open('/index.html');
    const D1 = await page.evaluate(dumpOracle);
    R.writeback[tag] = {
      rust: { written: rep.written, bytesChanged: rep.changed.length, errors: rep.errors },
      changedKeys: c.changed.map((k) => k.replace(/[0-9a-z]{13,}/g, '<id>')),
      byteChangedCount: c.changed.length, added: c.added, removed: c.removed, lsChanged: c.lsChanged, nonJsonChanged,
      parsedJsonDeepEqualFailures: badDeep,
      oracleStateDeepEqual: deepEq(D0.detail, D1.detail) && deepEq(D0.projects.map((p) => ({ ...p, updated: 0 })), D1.projects.map((p) => ({ ...p, updated: 0 }))),
      oracleStateDiffs: deepEq(D0.detail, D1.detail) ? [] : Object.keys(D0.detail).filter((k) => !deepEq(D0.detail[k], D1.detail[k]))
    };
    if (variant !== 'naive') { /* keep S0 as the reference for each variant: restore bytes for next run */
      /* values written back are byte-identical for value/typed unless listed in changedKeys */
    }
    await page.screenshot({ path: path.join(OUT, `oracle-${mode}-after-${build}-${variant}.png`) });
  }

  /* 4. Rust creates a project and edits a page; oracle must open both */
  await open('/spikes/s3-storage-compat/harness.html');
  await page.evaluate(restore, { S0, mode });
  const jpegBytes = Array.from(await page.evaluate(async () => {
    const c = document.createElement('canvas'); c.width = 64; c.height = 80;
    const g = c.getContext('2d'); g.fillStyle = '#fff'; g.fillRect(0, 0, 64, 80); g.fillStyle = '#000'; g.fillRect(8, 20, 40, 4);
    const b = await new Promise((r) => c.toBlob(r, 'image/jpeg', 0.9));
    return Array.from(new Uint8Array(await b.arrayBuffer()));
  }));
  const made = JSON.parse(await page.evaluate(([m, bytes]) => window.rs.make_project(m, 'Made by Rust', new Uint8Array(bytes), 'Rust transcript line 1\nline 2 → arrow'), [mode, jpegBytes]));
  const target = seeded.A;
  await page.evaluate(([m, slug, id]) => window.rs.edit_page(m, slug, id, '\n[edited by rust]'), [mode, target.slug, target.ids[3]]);
  const S2 = await page.evaluate(snap);
  await open('/index.html');
  const D2 = await page.evaluate(dumpOracle);
  const newSlug = made.slug, newDetail = D2.detail[newSlug];
  R.rustCreate = {
    slug: newSlug,
    oracleListsIt: D2.projects.some((p) => p.slug === newSlug),
    openOk: newDetail && newDetail.ok,
    pageRaw: newDetail && newDetail.pages[made.id] && newDetail.pages[made.id].raw,
    imageLoads: newDetail && newDetail.imgs[made.id] && newDetail.imgs[made.id].size > 0,
    imageMime: newDetail && newDetail.imgs[made.id] && newDetail.imgs[made.id].type,
    metaKeys: newDetail && Object.keys(newDetail.meta),
    pageKeys: newDetail && Object.keys(newDetail.pages[made.id]),
    projectJson: S2.files[newSlug + '/project.json'].text,
    pageJson: S2.files[newSlug + '/pages/' + made.id + '.json'].text,
    bakCreated: (newSlug + '/project.json.bak') in S2.files,
    editedPageCorrected: D2.detail[target.slug].pages[target.ids[3]].corrected.slice(-30),
    editedReviewed: D2.detail[target.slug].pages[target.ids[3]].reviewed,
    otherPagesUnchanged: target.ids.slice(0, 3).every((id) => deepEq(D0.detail[target.slug].pages[id], D2.detail[target.slug].pages[id])),
    appleProjectsUntouched: deepEq(D0.detail[seeded.B.slug], D2.detail[seeded.B.slug])
  };
  await page.screenshot({ path: path.join(OUT, `oracle-${mode}-after-rust-create.png`) });
  /* UI-level check: open Rust-made project through the real UI */
  const homeText = await page.locator('#app').innerText();
  R.rustCreate.homeShowsIt = homeText.includes('Made by Rust');
  R.rustCreate.homeShowsOthers = homeText.includes('Algebra') && homeText.includes('Clinic');

  /* 5. settings round trip through localStorage (idb run only: same origin storage anyway) */
  await open('/spikes/s3-storage-compat/harness.html');
  await page.evaluate(() => window.rs.ls_set_theme('dark'));
  await open('/index.html');
  if (mode === 'folder') { await open('/spikes/s3-storage-compat/harness.html'); await page.evaluate(() => window.rs.settings_json_set_theme('folder', 'dark')); await open('/index.html'); R.settingsNote = 'settings.json also set from Rust'; }
  R.settingsFromRust = await page.evaluate(() => ({ theme: OCR.state.d.settings.theme, rustWrote: OCR.state.d.settings.rustWrote, dataTheme: document.documentElement.dataset.theme, demo: OCR.state.d.settings.demo }));
  /* oracle re-saves settings: does it keep the unknown key? */
  await page.evaluate(() => OCR.state.saveSettings({ keepOriginals: false }));
  R.settingsAfterOracleSave = await page.evaluate(() => JSON.parse(localStorage.getItem('ocr_settings_v1')).rustWrote);

  /* 6. folder mode only: can Rust use the handle after a full reload, with no gesture? */
  if (mode === 'folder') {
    await open('/index.html');
    R.folderAfterReload = await page.evaluate(() => ({ folder: OCR.state.d.folder, projects: OCR.state.d.projects.length, kind: OCR.fs.kind() }));
    await open('/spikes/s3-storage-compat/harness.html');
    R.probeRootAfter = JSON.parse(await page.evaluate(() => window.rs.probe_root()));
  }

  R.consoleLogs = logs.slice(0, 20);
  await ctx.close();
  return R;
}

/* ------------------------------------------------------------------ main */
const srv = await serve(PORT);
const browser = await chromium.launch();
try {
  /* format battery needs only the harness page */
  const bctx = await browser.newContext();
  const bp = await bctx.newPage();
  await bp.goto(BASE + '/spikes/s3-storage-compat/harness.html');
  await bp.waitForFunction(() => document.documentElement.dataset.ready === '1');
  const cases = jsCases();
  results.batteryStd = await runBattery(bp, cases, 'std');
  results.batteryArb = await runBattery(bp, cases, 'arb');
  results.batteryFrt = await runBattery(bp, cases, 'frt');
  await bctx.close();

  results.idb = await scenario(browser, 'idb');
  results.folder = await scenario(browser, 'folder');
} finally {
  await browser.close();
  srv.close();
}
results.finished = new Date().toISOString();
fs.writeFileSync(path.join(OUT, 'results.json'), JSON.stringify(results, null, 1));
console.log('wrote', path.join(OUT, 'results.json'));
