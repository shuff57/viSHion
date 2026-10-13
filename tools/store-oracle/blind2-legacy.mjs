// Blind critic: legacy import must not create the `ocr_images` database when the old app never made one,
// must not touch an existing one, and Rust must open `ocr_fs` at exactly version 1 on a fresh origin.
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';
const PORT = +(process.env.PORT || 8794), BASE = `http://127.0.0.1:${PORT}`;
const srv = await serve(PORT);
const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined });
let failed = 0;
const ck = (n, ok, d) => { if (!ok) failed++; console.log((ok ? 'ok   ' : 'FAIL ') + n + (ok || d === undefined ? '' : '  -> ' + JSON.stringify(d).slice(0, 400))); };
try {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.route(/cdn\.jsdelivr\.net/, (r) => r.abort());
  await page.goto(BASE + '/tools/store-oracle/harness.html');
  await page.waitForFunction(() => document.documentElement.dataset.ready === '1');
  await page.evaluate(() => localStorage.setItem('ocr_sessions_v2', JSON.stringify([{ id: 's1', name: 'L', created: 1, lastRaw: 'text', hasImage: true }])));
  const r = JSON.parse(await page.evaluate(async () => window.rs.import_legacy('idb', '')));
  const dbs = await page.evaluate(async () => (await indexedDB.databases()).map((d) => d.name + '@' + d.version).sort());
  ck('import ok without ocr_images', r.pages === 1, r);
  ck('ocr_images database was not created', !dbs.some((d) => d.startsWith('ocr_images')), dbs);
  ck('ocr_fs created at version 1', dbs.includes('ocr_fs@1'), dbs);
  // a DB at version 2 must be refused, not upgraded/downgraded
  const ctx2 = await browser.newContext();
  const p2 = await ctx2.newPage();
  await p2.route(/cdn\.jsdelivr\.net/, (r) => r.abort());
  await p2.goto(BASE + '/tools/store-oracle/harness.html');
  await p2.waitForFunction(() => document.documentElement.dataset.ready === '1');
  await p2.evaluate(() => new Promise((res, rej) => { const q = indexedDB.open('ocr_fs', 2); q.onupgradeneeded = () => { q.result.createObjectStore('kv'); q.result.createObjectStore('files'); q.result.createObjectStore('extra'); }; q.onsuccess = () => { q.result.close(); res(); }; q.onerror = () => rej(q.error); }));
  const res2 = await p2.evaluate(async () => { try { await window.rs.suite_idb(); return 'opened'; } catch (e) { return 'err ' + String(e).slice(0, 120); } });
  const dbs2 = await p2.evaluate(async () => (await indexedDB.databases()).map((d) => d.name + '@' + d.version));
  ck('v2 database is refused, left at version 2', /^err/.test(res2) && dbs2.includes('ocr_fs@2'), { res2, dbs2 });
  // a v1 DB written by JS with extra store names / data must keep them
  await ctx2.close();
  await ctx.close();
} finally {
  await browser.close();
  srv.close();
}
console.log(failed ? `${failed} FAILED` : 'all ok');
process.exit(failed ? 1 : 0);
