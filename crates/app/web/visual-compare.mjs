// Visual comparison against the JS app (the oracle). The unmodified JS app (served from the repo root) seeds
// the sample assignment through its own API, then the same IndexedDB contents and the same localStorage
// settings are loaded into the Rust web build, and both are screenshotted at the same size and theme.
// Usage: node crates/app/web/visual-compare.mjs [outDir]   (run build.mjs first; Chrome needed)
//   env: SIZES=1280x800,390x844  THEMES=light,dark  STATE=initial|plain|original  CHROME=<path>
// Writes <outDir>/<size>-<theme>-<state>-{js,rs}.png and prints the boxes of the elements both sides have.
import { chromium } from 'playwright-core';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { serve } from './serve.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const out = path.resolve(process.argv[2] || path.join(repo, 'target/visual-compare'));
fs.mkdirSync(out, { recursive: true });
const exe = process.env.CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const sizes = (process.env.SIZES || '1280x800,390x844').split(',').map((s) => s.split('x').map(Number));
const themes = (process.env.THEMES || 'light,dark').split(',');
const states = (process.env.STATE || 'initial').split(',');

const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
function staticServer(root, port) {
  const s = http.createServer((q, r) => {
    let p = decodeURIComponent(q.url.split('?')[0]);
    if (p.endsWith('/')) p += 'index.html';
    const f = path.join(root, p);
    if (!f.startsWith(root) || !fs.existsSync(f) || fs.statSync(f).isDirectory()) { r.writeHead(404); r.end(); return; }
    r.writeHead(200, { 'content-type': MIME[path.extname(f)] || 'application/octet-stream' });
    fs.createReadStream(f).pipe(r);
  });
  return new Promise((res) => s.listen(port, '127.0.0.1', () => res(s)));
}

// the sample assignment of crates/app/src/demo.rs, written by the JS app itself
async function seed() {
  const OCR = window.OCR, S = OCR.state;
  const W = 900, H = 1160, TOP = 150, PITCH = 62;
  const jpeg = (lines) => new Promise((res) => {
    const c = document.createElement('canvas'); c.width = W; c.height = H;
    const g = c.getContext('2d');
    g.fillStyle = '#fcfcf8'; g.fillRect(0, 0, W, H);
    g.fillStyle = '#c3d4ea'; for (let y = 40; y < H; y += 31) g.fillRect(0, y, W, 1);
    g.fillStyle = '#e7a3a3'; g.fillRect(86, 0, 2, H);
    g.fillStyle = '#26358f';
    lines.forEach((l, i) => {
      let x = 110;
      for (const w of l.split(/\s+/).filter(Boolean)) {
        const ww = Math.min(200, Math.max(10, w.length * 9));
        for (let dx = 0; dx < ww; dx++) g.fillRect(x + dx, TOP + i * PITCH + 14 + Math.sin(dx * 0.55) * 7, 2, 3);
        x += ww + 14; if (x > 700) break;
      }
    });
    c.toBlob(res, 'image/jpeg', 0.82);
  });
  const seeds = [
    ['p-maya-1', 'maya-1.jpg', 'Maya R.', 'done', 'The 95% confidance interval for the average commute time is (18.4, 24.1) minutes.\n$\\bar{x} \\pm 1.96 \\cdot \\frac{s}{\\sqrt{n}} = 21.25 \\pm 1.96 \\cdot \\frac{9.2}{\\sqrt{40}}$\n$$\\bar{x} \\pm 1.96\\cdot\\frac{s}{\\sqrt{n}} = (18.4,\\ 24.1)$$\nIt does not mean that 95% of students commute in that range. If we took many samples,\nabout 95% of those [illegible] would capture the true populaton mean.\n~~probably~~ so the interval is likely wide.\nMargin: check my units!', ''],
    ['p-maya-2', 'maya-2.jpg', 'Maya R.', 'done', 'A wider interval means we know the mean less precisely.\nA larger sample gives a narrower interval, because the standerd error gets smaller as n goes up.', ''],
    ['p-ana-1', 'ana-1.jpg', 'Ana L.', 'done', 'The sample mean is 21.25 minutes.\nThere is nothing wrong with this page.', ''],
    ['p-ben-1', 'ben-1.jpg', 'Ben T.', 'pending', '', ''],
    ['p-omar-1', 'omar-1.jpg', 'Omar K.', 'error', '', 'The transcription server did not answer.'],
  ];
  await OCR.fs.useBrowserStorage();
  const slug = await OCR.fs.createProject('Unit 5 written response');
  await OCR.fs.openProject(slug);
  const list = [];
  for (const [id, name, who, status, raw, error] of seeds) {
    const p = S.newPage(name); p.id = id; p.status = status; p.raw = raw; p.error = error;
    p.student = { name: who, source: 'page', confirmed: true };
    const lines = raw.split('\n').filter(Boolean);
    p.boxes = lines.map((_, i) => ({ x: 100 / W, y: (TOP + i * PITCH) / H, w: 660 / W, h: 44 / H, r: 0 }));
    if (status === 'done') p.timings = { ms: 14200, tokens: 212 };
    if (id === 'p-maya-1') p.guesses = { 'i:0': [{ text: 'intervals', percent: 62 }, { text: 'intervening', percent: 21 }] };
    OCR.fs.saveImage(p.id, await jpeg(lines.length ? lines : ['...']), null);
    list.push(p);
  }
  S.addPages(list);
  S.updateMeta({ topic: 'Math/Education', ui: { selectedId: 'p-maya-1' } });
  S.saveSettings({ demo: true });
  for (const p of list) if (p.status === 'done') { S.updatePage(p.id, { corrections: [] }); await OCR.judge.run(p.id); }
  await OCR.fs.flushAll();
  return slug;
}

async function dumpIdb() {
  const db = await new Promise((res, rej) => { const r = indexedDB.open('ocr_fs', 1); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
  const get = (store, op, ...a) => new Promise((res) => { const r = db.transaction(store)[op === 'keys' ? 'objectStore' : 'objectStore'](store)[op === 'keys' ? 'getAllKeys' : 'get'](...a); r.onsuccess = () => res(r.result); });
  const out = [];
  for (const k of await get('files', 'keys')) {
    const v = await get('files', 'get', k);
    if (typeof v === 'string') out.push([k, 's', v]);
    else {
      const buf = new Uint8Array(v instanceof Blob ? await v.arrayBuffer() : v.buffer || v);
      let bin = ''; for (let i = 0; i < buf.length; i += 8192) bin += String.fromCharCode(...buf.subarray(i, i + 8192));
      out.push([k, v instanceof Blob ? 'b:' + v.type : 'a', btoa(bin)]);
    }
  }
  return out;
}

async function loadIdb(entries) {
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open('ocr_fs', 1);
    r.onupgradeneeded = () => { r.result.createObjectStore('kv'); r.result.createObjectStore('files'); };
    r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error);
  });
  await new Promise((res, rej) => {
    const t = db.transaction('files', 'readwrite'); const s = t.objectStore('files');
    for (const [k, kind, v] of entries) {
      if (kind === 's') s.put(v, k);
      else { const u = Uint8Array.from(atob(v), (c) => c.charCodeAt(0)); s.put(kind.startsWith('b:') ? new Blob([u], { type: kind.slice(2) }) : u.buffer, k); }
    }
    t.oncomplete = res; t.onerror = () => rej(t.error);
  });
  db.close();
}

const PJ = +(process.env.PORT_JS || 8791), PR = +(process.env.PORT_RS || 8792);
const js = await staticServer(repo, PJ);
const rs = await serve(PR);
const browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
try {
  // seed once, in a light-theme JS session
  const sctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await sctx.addInitScript(() => { try { if (!localStorage.getItem('ocr_settings_v1')) localStorage.setItem('ocr_settings_v1', JSON.stringify({ theme: 'light', storage: 'idb', noticeSeen: true })); } catch {} });
  const sp = await sctx.newPage();
  await sp.goto(`http://127.0.0.1:${PJ}/index.html`);
  await sp.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 30000 });
  const slug = await sp.evaluate(seed);
  const dump = await sp.evaluate(dumpIdb);
  await sctx.close();
  console.log(`seeded ${slug}: ${dump.length} files`);
  for (const [k, kind, v] of dump) if (kind !== 's' && /maya-1/.test(k)) fs.writeFileSync(path.join(out, 'page-' + path.basename(k)), Buffer.from(v, 'base64'));

  for (const [w, h] of sizes) for (const theme of themes) for (const state of states) {
    const tag = `${w}x${h}-${theme}-${state}`;
    const settings = JSON.stringify({ theme, storage: 'idb', noticeSeen: true, demo: true });
    const prep = async (base, isRs) => {
      const ctx = await browser.newContext({ viewport: { width: w, height: h } });
      const p = await ctx.newPage();
      await p.goto(`http://127.0.0.1:${base}/${isRs ? 'favicon.svg' : 'js/ns.js'}`);
      await p.evaluate(loadIdb, dump);
      await p.evaluate(([k, v]) => localStorage.setItem(k, v), ['ocr_settings_v1', settings]);
      return { ctx, p };
    };
    // JS
    {
      const { ctx, p } = await prep(PJ, false);
      await p.goto(`http://127.0.0.1:${PJ}/index.html`);
      await p.waitForFunction(() => document.documentElement.dataset.ready === '1', null, { timeout: 30000 });
      await p.evaluate(async (s) => { const OCR = window.OCR; await OCR.fs.openProject(s); OCR.ui.nav.review('p-maya-1'); }, slug);
      await p.waitForTimeout(1500);
      await page_state(p, state, false);
      await p.screenshot({ path: path.join(out, `${tag}-js.png`) });
      fs.writeFileSync(path.join(out, `${tag}-js.json`), JSON.stringify(await p.evaluate(boxes, 'js'), null, 1));
      await ctx.close();
    }
    // Rust
    {
      const { ctx, p } = await prep(PR, true);
      await p.goto(`http://127.0.0.1:${PR}/?project=${slug}${state === 'tray0' ? '&tray=0' : ''}`);
      for (let i = 0; i < 300 && !(await p.evaluate(() => window.__ready === true || !!window.__error)); i++) await p.waitForTimeout(100);
      p.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') console.log('console:', m.text().slice(0, 200)); });
      p.on('pageerror', (e) => console.log('pageerror:', String(e).slice(0, 300)));
      console.log('error?', await p.evaluate(() => window.__error));
      await p.waitForSelector('#controls [data-id="done"]', { timeout: 30000 }).catch(async (e) => { await p.screenshot({ path: path.join(out, 'fail.png') }); console.log(await p.evaluate(() => document.getElementById('controls').innerText.slice(0, 400))); throw e; });
      await p.waitForTimeout(2500);
      await page_state(p, state, true);
      await p.screenshot({ path: path.join(out, `${tag}-rs.png`) });
      fs.writeFileSync(path.join(out, `${tag}-rs.json`), JSON.stringify(await p.evaluate(boxes, 'rs'), null, 1));
      await ctx.close();
    }
    console.log('wrote', tag);
  }
} finally {
  await browser.close().catch(() => {});
  rs.stop();
  js.close();
}

// text boxes of the review screen on each side (the rendered text, not the invisible twin controls)
function boxes(side) {
  const sel = side === 'js'
    ? '.who-head h1, .who-head p, .tx-text .ln, .tx-foot, .legend, .tx-text .w, .tx-text .illegible'
    : '#controls .txc, #controls p.tx, #controls p.tx .w';
  return [...document.querySelectorAll(sel)].map((e) => {
    const r = e.getBoundingClientRect();
    return { t: (e.textContent || '').trim().slice(0, 28), x: +r.x.toFixed(1), y: +r.y.toFixed(1), w: +r.width.toFixed(1), h: +r.height.toFixed(1) };
  }).filter((b) => b.w > 0 && b.h > 0);
}

// Tab order: what holds focus after each of 20 Tab presses, by accessible name (STATE=tabs)
async function tabs(p) {
  const out = [];
  await p.mouse.click(2, 2).catch(() => {});
  for (let i = 0; i < 20; i++) {
    await p.keyboard.press('Tab');
    await p.waitForTimeout(120);
    out.push(await p.evaluate(() => {
      const e = document.activeElement;
      if (!e || e === document.body) return '(body)';
      const lab = e.getAttribute('aria-label') || e.getAttribute('title') || (e.textContent || '').trim().slice(0, 30) || e.tagName;
      return e.tagName.toLowerCase() + ':' + lab.replace(/\s+/g, ' ').slice(0, 40) + (e.closest('.tx-text,.tx-clip,#transcript') ? ' [transcript]' : '');
    }));
  }
  return out;
}

async function page_state(p, state, isRs) {
  if (state === 'initial') return;
  if (state === 'tabs') { console.log((isRs ? 'RS' : 'JS') + ' tab order:\n  ' + (await tabs(p)).map((x, i) => (i + 1) + '. ' + x).join('\n  ')); return; }
  if (state === 'tray0') { if (!isRs) await p.getByRole('button', { name: 'Close the word panel' }).click(); await p.waitForTimeout(500); return; }
  const label = state === 'plain' ? /Show plain text/ : /^Original$/;
  const loc = isRs ? p.locator('#controls button').filter({ hasText: label }).first() : p.getByRole('button', { name: label }).first();
  await loc.click({ force: true }).catch(() => {});
  await p.waitForTimeout(800);
}
