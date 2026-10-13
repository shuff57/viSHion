// Blind critic (phase 2) end-to-end fuzz: the UNMODIFIED JS app writes randomly generated (own seed)
// projects, Rust opens + rewrites them through the project layer, the JS app reloads and must be
// deepStrictEqual; string documents must be byte-identical (only .bak may be added). IDB and folder mode.
// Usage: node build.mjs && node blind2.mjs   (CHROME=path overrides the browser)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';
import assert from 'node:assert';

const PORT = +(process.env.PORT || 8793), BASE = `http://127.0.0.1:${PORT}`;
const SEED = +(process.env.SEED || 424242);
const srv = await serve(PORT);
const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined });
let failed = 0;
const ck = (n, ok, d) => { if (!ok) failed++; console.log((ok ? 'ok   ' : 'FAIL ') + n + (ok || d === undefined ? '' : '  -> ' + JSON.stringify(d).slice(0, 600))); };

async function snap(mode) {
  const hash = async (buf) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', buf))].map((b) => b.toString(16).padStart(2, '0')).join('');
  const out = {};
  if (mode === 'idb') {
    const db = await new Promise((res, rej) => { const r = indexedDB.open('ocr_fs', 1); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
    const keys = await new Promise((res) => { const r = db.transaction('files').objectStore('files').getAllKeys(); r.onsuccess = () => res(r.result); });
    for (const k of keys) {
      const v = await new Promise((res) => { const r = db.transaction('files').objectStore('files').get(k); r.onsuccess = () => res(r.result); });
      if (typeof v === 'string') out[k] = { t: 'str', v };
      else if (v instanceof Blob) out[k] = { t: 'Blob:' + v.type, sha: await hash(await v.arrayBuffer()) };
      else out[k] = { t: Object.prototype.toString.call(v), sha: await hash(v instanceof ArrayBuffer ? v : v.buffer) };
    }
    const names = [...db.objectStoreNames].sort();
    out['__meta'] = { t: 'meta', v: JSON.stringify({ version: db.version, names }) };
    db.close();
  } else {
    const walk = async (d, pre) => {
      for await (const [name, h] of d.entries()) {
        if (h.kind === 'directory') await walk(h, pre + name + '/');
        else { const f = await h.getFile(); out[pre + name] = /\.(json|bak|damaged)$/.test(name) ? { t: 'str', v: await f.text() } : { t: 'File', sha: await hash(await f.arrayBuffer()) }; }
      }
    };
    await walk(await navigator.storage.getDirectory(), '');
  }
  return out;
}

async function seed({ mode, seedNo }) {
  let s = seedNo >>> 0;
  const rnd = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
  const ri = (n) => Math.floor(rnd() * n);
  const pick = (a) => a[ri(a.length)];
  const ATOMS = ['a', 'Bc', ' ', '\n', '\t', '"', '\\', '/', '\u0001', '\u007f', ' ', ' ', '﻿', 'é', '日本語', '😀', '\ud800', '\udfff', '\ud83d', '\ude00', '<', '&', '→', '̀', 'u', 'd800', '\u{10FFFE}', '\u{10FFFF}', '$x^2$', '~~s~~', '[illegible]'];
  const str = () => { let o = ''; for (let i = ri(10); i > 0; i--) o += pick(ATOMS); return o; };
  const num = () => pick([0, 1, 0.5, 1e21, 1e-7, 0.1 + 0.2, 5e-324, 2 ** 53, 1.7976931348623157e308, (rnd() - 0.5) * 10 ** (ri(30) - 15)]);
  const OCR = window.OCR, S = OCR.state;
  S.saveSettings({ demo: true });
  if (mode === 'idb') await OCR.fs.useBrowserStorage(); else await OCR.fs.chooseFolder();
  const jpeg = await new Promise((res) => { const c = document.createElement('canvas'); c.width = 20; c.height = 20; c.toBlob(res, 'image/jpeg'); });
  const slugs = [];
  for (let pi = 0; pi < 3; pi++) {
    const name = str() || 'proj';
    const slug = await OCR.fs.createProject(name + pi);
    await OCR.fs.openProject(slug);
    const list = [];
    for (let i = 0; i < 4 + ri(4); i++) {
      const p = S.newPage('f' + i + '.png');
      OCR.fs.saveImage(p.id, jpeg, rnd() < 0.3 ? { file: new File([jpeg], 'o.png', { type: 'image/png' }), ext: 'png' } : null);
      list.push(p);
    }
    S.addPages(list);
    for (const p of list) {
      const patch = { status: pick(['done', 'done', 'pending', 'error']), raw: str(), corrected: str(), error: str(), rotation: pick([0, 90, 180, 270]) };
      patch.boxes = Array.from({ length: ri(3) }, () => ({ x: num(), y: num(), w: num(), h: num(), r: 0 }));
      patch.corrections = Array.from({ length: ri(3) }, () => ({ word: str(), term: str(), conf: rnd(), applied: rnd() < 0.5 }));
      patch.resolved = Object.fromEntries(Array.from({ length: ri(3) }, (_, i) => [pick(['c:', 'i:', '7', '12']) + i, pick(['suggested', 'edited', 'kept'])]));
      patch.guesses = Object.fromEntries(Array.from({ length: ri(3) }, () => [str() || 'k', num()]));
      patch.student = rnd() < 0.5 ? { name: str(), source: 'chosen', confirmed: rnd() < 0.5 } : null;
      patch['futureField' + ri(3)] = { a: [num(), str(), null, { b: str() }] };
      S.updatePage(p.id, patch);
    }
    S.updateMeta({ topic: pick(['General', 'Medical', 'Math/Education']), prompt: str(), ui: { selectedId: list[ri(list.length)].id, layout: 'split' }, ['unknown' + pi]: { n: num(), s: str() } });
    await OCR.fs.flushAll();
    slugs.push(slug);
    await OCR.fs.closeProject();
  }
  await OCR.fs.flushAll();
  return slugs;
}

async function dump() {
  const OCR = window.OCR, S = OCR.state;
  const hash = async (buf) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', buf))].map((b) => b.toString(16).padStart(2, '0')).join('');
  await OCR.fs.refresh();
  const out = { projects: JSON.parse(JSON.stringify(S.d.projects)), detail: {} };
  for (const p of S.d.projects) {
    const ok = await OCR.fs.openProject(p.slug);
    const P = S.d.project, imgs = {};
    for (const id of P.meta.order) { const b = await OCR.fs.loadImage(id); imgs[id] = b ? { size: b.size, type: b.type, sha: await hash(await b.arrayBuffer()) } : null; }
    out.detail[p.slug] = { ok, meta: JSON.parse(JSON.stringify(P.meta)), pages: JSON.parse(JSON.stringify(P.pages)), imgs, sel: S.d.selectedId };
    await OCR.fs.closeProject();
  }
  return out;
}

try {
  for (const mode of ['idb', 'folder']) {
    for (const seedNo of [SEED, SEED + 1]) {
      const T = (s) => `[${mode}/${seedNo}] ${s}`;
      const ctx = await browser.newContext();
      if (mode === 'folder') await ctx.addInitScript(() => { window.showDirectoryPicker = async () => navigator.storage.getDirectory(); });
      const page = await ctx.newPage();
      const errs = [];
      page.on('pageerror', (e) => errs.push(String(e).slice(0, 200)));
      await page.route(/cdn\.jsdelivr\.net/, (r) => r.abort());
      const open = async (u) => { await page.goto(BASE + u); await page.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 30000 }); };
      await open('/index.html');
      const slugs = await page.evaluate(seed, { mode, seedNo });
      const S0 = await page.evaluate(snap, mode);
      const D0 = await page.evaluate(dump);
      ck(T('seeded ' + Object.keys(S0).length + ' entries, ' + slugs.length + ' projects'), slugs.length === 3 && Object.keys(S0).length > 20, slugs);
      await open('/tools/store-oracle/harness.html');
      const rep = JSON.parse(await page.evaluate(async (m) => window.rs.rewrite_all(m), mode));
      const S1 = await page.evaluate(snap, mode);
      const changed = Object.keys(S0).filter((k) => S1[k] === undefined || JSON.stringify(S0[k]) !== JSON.stringify(S1[k]));
      const added = Object.keys(S1).filter((k) => !(k in S0));
      if (process.env.DEBUG) for (const k of changed) if (!k.endsWith('.bak')) console.log(k, '\nBEFORE', JSON.stringify(S0[k]), '\nAFTER ', JSON.stringify(S1[k]));
      const unreadable = rep.unreadable;
      // an unrepresentable project.json (documented U+10FFFE limitation) is reported and left untouched, never restored from .bak
      ck(T('rewrite: unusable projects only for the documented U+10FFFE limitation, none from backup'), rep.unusable.every((u) => /unrepresentable/.test(u)) && rep.fromBackup.length === 0, rep);
      // lone surrogate + U+10FFFE followed by 'd800' text may legally be refused (unrepresentable)
      ck(T('rewrite: unreadable pages only for the documented U+10FFFE limitation'), unreadable.every((u) => /unrepresentable/.test(u)), unreadable);
      ck(T('rewrite: entries byte-identical, only .bak added/changed'), changed.every((k) => k.endsWith('.bak')) && added.every((k) => k.endsWith('.bak')), { changed: changed.slice(0, 5), added: added.filter((k) => !k.endsWith('.bak')).slice(0, 5) });
      await open('/index.html');
      const D1 = await page.evaluate(dump);
      let eq = true; try { assert.deepStrictEqual(D1.detail, D0.detail); } catch (e) { eq = false; }
      const diff = Object.keys(D0.detail).filter((k) => { try { assert.deepStrictEqual(D1.detail[k], D0.detail[k]); return false; } catch (e) { return true; } });
      ck(T('JS reload deep-equal'), eq || (unreadable.length && diff.length === 0), diff);
      ck(T('no JS page errors'), errs.length === 0, errs);
      if (mode === 'idb') {
        const db = JSON.parse(S1['__meta'].v);
        ck(T('ocr_fs still version 1 with stores kv, files'), db.version === 1 && db.names.join() === 'files,kv', db);
      }
      await ctx.close();
    }
  }
} finally {
  await browser.close();
  srv.close();
}
console.log(failed ? `${failed} FAILED` : 'all ok');
process.exit(failed ? 1 : 0);
