// CSP check: load the built app from wrangler dev in Chrome and require zero CSP violations, a mounted
// canvas, and the DOM control twin. Also a negative control: without 'wasm-unsafe-eval' it must NOT mount.
// Usage: node crates/app/web/check.mjs   (run build.mjs first; CHROME env overrides the browser path)
import { chromium } from 'playwright-core';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { serve } from './serve.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };

// waitForFunction evals a string/function in-page, which the CSP (rightly) forbids; poll with evaluate instead.
async function settled(p) {
  for (let i = 0; i < 300; i++) {
    if (await p.evaluate(() => window.__ready === true || !!window.__error)) return;
    await new Promise(r => setTimeout(r, 100));
  }
  throw new Error('app neither mounted nor failed in 30s');
}

const { port, stop } = await serve(+(process.env.PORT || 8788));
const base = `http://127.0.0.1:${port}`;
let browser;
try {
  browser = await chromium.launch({
    executablePath: exe,
    args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'],
  });
  const track = `
    window.__violations = [];
    document.addEventListener('securitypolicyviolation', e => window.__violations.push(e.violatedDirective + ' ' + e.blockedURI));`;

  // ---- positive: real headers, untouched
  const ctx1 = await browser.newContext({ viewport: { width: 1280, height: 800 }, permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await ctx1.newPage();
  const logs = [];
  page.on('console', m => { if (['error', 'warning'].includes(m.type())) logs.push(`${m.type()}: ${m.text()}`); });
  page.on('pageerror', e => logs.push('pageerror: ' + e.message));
  await page.addInitScript(track);
  const resp = await page.goto(base + '/?demo=1&theme=light', { waitUntil: 'load' });
  const csp = resp.headers()['content-security-policy'] || '';
  await settled(page);
  const state = await page.evaluate(() => ({ ready: window.__ready === true, error: window.__error || null, violations: window.__violations }));
  const html = await (await fetch(base + '/')).text();
  const inline = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)].filter(m => !/\bsrc\s*=/.test(m[1]));
  const hash = "'sha256-" + createHash('sha256').update(inline[0][2], 'utf8').digest('base64') + "'";
  expect(csp.includes("'wasm-unsafe-eval'"), "CSP header has 'wasm-unsafe-eval'");
  expect(csp.includes(hash), 'CSP header has the hash of the inline bootstrap script');
  expect(!/script-src[^;]*'unsafe-eval'/.test(csp.replace("'wasm-unsafe-eval'", '')), "CSP has no plain 'unsafe-eval'");
  expect(state.ready, 'app mounted (start() resolved)' + (state.error ? ' error=' + state.error : ''));
  expect(state.violations.length === 0, 'zero CSP violations' + (state.violations.length ? ': ' + state.violations.join('; ') : ''));
  expect(!logs.some(l => /content security policy|refused to/i.test(l)), 'no CSP messages in console');
  expect(await page.evaluate(() => { const c = document.getElementById('c'); return c && c.width > 0 && c.height > 0; }), 'canvas has a drawing surface');
  // egui repaints on demand; nudge the pointer so the first frame and the control sync run in headless
  const pump = async (n = 6) => { for (let i = 0; i < n; i++) { await page.mouse.move(700 + i, 500 + i); await page.waitForTimeout(80); } };
  const twinIds = () => page.evaluate(() => [...document.querySelectorAll('#controls button, #controls input')].map(e => e.dataset.id));
  await pump();
  await page.waitForSelector('#controls [data-id="done"]', { timeout: 20000 });
  expect(true, 'DOM control twin present for the review screen: #controls [data-id=done]');
  const tray = await page.evaluate(() => ({
    h1: document.querySelector('#controls h1')?.textContent,
    status: document.querySelector('#controls [role=status][data-id=student-status]')?.textContent,
    words: [...document.querySelectorAll('#controls p button[data-id^="word-"]')].map(b => b.getAttribute('aria-label') || b.textContent),
    math: [...document.querySelectorAll('#controls p [role=math]')].map(m => m.textContent),
  }));
  expect(tray.h1 === 'Maya R.', 'twin exposes the student as the h1: ' + tray.h1);
  expect(/to check$/.test(tray.status || ''), 'twin exposes a live status: ' + tray.status);
  expect(tray.words.length >= 3, 'flagged words are inline buttons inside the transcript paragraphs: ' + JSON.stringify(tray.words));
  expect(tray.math.length >= 2, 'math is exposed as role=math with its TeX: ' + JSON.stringify(tray.math));
  // what a screen reader is given: Chrome's own accessibility tree for the twin
  const snap = await page.locator('#controls').ariaSnapshot();
  expect(/heading "Maya R\." \[level=1\]/.test(snap) && /status/.test(snap) && /button "Spelling to check: confidance\. Show options\."/.test(snap)
    && /button "Side by side" \[pressed\]/.test(snap), 'accessibility tree: h1, live status, named word buttons and pressed toggles');
  const nameOf = (id) => page.evaluate((i) => {
    const e = [...document.querySelectorAll('#controls [data-id]')].find(x => x.dataset.id === i);
    return e ? (e.getAttribute('aria-label') || e.textContent) : null;
  }, id);
  const pressed = (id) => page.evaluate((i) => {
    const e = [...document.querySelectorAll('#controls [data-id]')].find(x => x.dataset.id === i);
    return e ? e.getAttribute('aria-pressed') : null;
  }, id);
  const statusText = () => page.evaluate(() => document.querySelector('#controls [data-id=student-status]')?.textContent);
  // ---- real keyboard flow (no programmatic focus): Tab order, no canvas trap, names, focus ring, activation
  const active = () => page.evaluate(() => {
    const a = document.activeElement;
    return a ? { tag: a.tagName, id: a.id, dataId: a.getAttribute('data-id') } : null;
  });
  // 1. Accessibility surface: nothing focusable may be unnamed or hidden-but-tabbable.
  const surface = await page.evaluate(() => {
    const out = { tabbable: [], hiddenFocusable: [] };
    for (const el of document.querySelectorAll('canvas, input, button, a[href], select, textarea, [tabindex]')) {
      const hidden = el.closest('[aria-hidden="true"]') !== null;
      const name = (el.getAttribute('aria-label') || el.textContent || '').trim();
      const tabbable = el.tabIndex >= 0 && !el.disabled;
      if (tabbable) out.tabbable.push({ tag: el.tagName, id: el.id, dataId: el.getAttribute('data-id'), name, hidden });
      if (tabbable && hidden) out.hiddenFocusable.push(el.tagName + '#' + el.id);
    }
    const c = document.getElementById('c');
    out.canvas = { tabindex: c.getAttribute('tabindex'), ariaHidden: c.getAttribute('aria-hidden') };
    const agent = document.querySelector('body > input[type=text]');
    out.agent = agent && { tabindex: agent.getAttribute('tabindex'), label: agent.getAttribute('aria-label'), ariaHidden: agent.getAttribute('aria-hidden') };
    return out;
  });
  expect(surface.canvas.tabindex === '-1' && surface.canvas.ariaHidden === 'true', 'canvas is tabindex=-1 and aria-hidden (not in Tab order, not exposed)');
  expect(surface.hiddenFocusable.length === 0, 'no tabbable element is aria-hidden' + (surface.hiddenFocusable.length ? ': ' + surface.hiddenFocusable : ''));
  expect(surface.tabbable.length > 0 && surface.tabbable.every(t => t.name), 'every tabbable element has an accessible name (' + surface.tabbable.length + ' controls)');
  expect(!!surface.agent && surface.agent.tabindex === '-1' && !!surface.agent.label, 'eframe text agent is named and out of the Tab order: ' + JSON.stringify(surface.agent));

  // pixel probe for the on-canvas focus ring: same pointer position every time so hover cannot differ
  const shot = async () => { await page.mouse.move(640, 740); await page.waitForTimeout(500); return (await page.screenshot()).toString('base64'); };
  await page.mouse.click(640, 4); // click on the transcript pane: canvas gets pointer focus
  expect((await active()).id === 'c', 'click focuses the canvas (pointer path still works)');
  const idle = await shot();

  // 2. Tab from the canvas must leave it and land on the first control (the old trap)
  const firstControl = await page.evaluate(() => document.querySelector('#controls button:not([disabled]), #controls input').dataset.id);
  await page.keyboard.press('Tab');
  const afterTab = await active();
  expect(afterTab && afterTab.dataId === firstControl, `Tab from canvas lands on the first twin control ${firstControl}, got ` + JSON.stringify(afterTab));
  await pump(); await page.waitForTimeout(300);
  const ringOn = await shot();
  expect(ringOn !== idle, 'on-canvas focus ring is drawn when the twin has focus');
  // 3. Tab again goes on past the controls (no trap, nothing unnamed in between)
  const seen = [];
  for (let i = 0; i < 4; i++) { await page.keyboard.press('Tab'); seen.push(await active()); }
  expect(!seen.some(s => s && s.id === 'c') && !seen.some(s => s && s.tag === 'INPUT'),
    'repeated Tab never lands on the canvas or the text agent: ' + JSON.stringify(seen.map(s => s && s.dataId)));
  // 4. Shift+Tab from the canvas must also leave it
  await page.mouse.click(640, 4);
  expect((await active()).id === 'c', 'canvas focused again by click');
  await page.keyboard.press('Shift+Tab');
  const afterShift = await active();
  expect(!(afterShift && afterShift.id === 'c'), 'Shift+Tab leaves the canvas (no keyboard trap), now on ' + JSON.stringify(afterShift && afterShift.dataId));
  // 5. Focus ring clears on blur (click away from the control)
  await page.mouse.click(640, 4);
  await page.keyboard.press('Tab');
  expect((await active()).dataId === firstControl, 'twin focused again via Tab');
  await pump(); await page.waitForTimeout(300);
  const ringOn2 = await shot();
  await page.mouse.click(640, 4);
  const ringOff = await shot();
  expect(ringOn2 !== ringOff && ringOff === idle, 'focus ring clears on blur (frame after blur equals the idle frame)');

  // 6. Tab (real key presses only) until a given control has focus
  const tabTo = async (id, limit = 80) => {
    await page.mouse.click(640, 4);
    for (let i = 0; i < limit; i++) {
      await page.keyboard.press('Tab');
      if ((await active()).dataId === id) return true;
    }
    return false;
  };
  const waitFor = async (fn, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await fn()) return true; await pump(2); } return false; };

  // layout toggle: Enter activates, then Space
  expect(await tabTo('layout-text'), 'Tab reaches the "Text only" control');
  await page.keyboard.press('Enter');
  expect(await waitFor(async () => (await pressed('layout-text')) === 'true'), 'Enter on the focused twin reaches egui (Text only becomes pressed)');
  expect((await active()).dataId === 'layout-text', 'twin keeps keyboard focus after its state changed (no DOM re-order blur)');
  expect(!(await twinIds()).includes('boxes'), 'text-only layout drops the page pane controls');
  await page.keyboard.press('Shift+Tab');
  expect((await active()).dataId === 'layout-split', 'Shift+Tab moves back to "Side by side"');
  await page.keyboard.press('Space');
  expect(await waitFor(async () => (await pressed('layout-split')) === 'true'), 'Space on the focused twin reaches egui (Side by side pressed again)');

  // review flow by keyboard: open a word, accept the suggestion, the counter drops
  const before = await statusText();
  const wordId = await page.evaluate(() => [...document.querySelectorAll('#controls button[data-id^="word-"]')].map(b => b.dataset.id).find(i => /-c:0$/.test(i)));
  expect(!!wordId, 'a suggested word control exists: ' + wordId);
  expect(await tabTo(wordId), 'Tab reaches the flagged word inline in the transcript');
  await page.keyboard.press('Enter');
  expect(await waitFor(async () => (await nameOf('alt-suggested')) !== null), 'Enter on a word opens its choices in the tray');
  expect(await tabTo('alt-suggested'), 'Tab reaches the suggestion choice');
  await page.keyboard.press('Enter');
  expect(await waitFor(async () => (await statusText()) !== before), `accepting the suggestion changes the status ("${before}" -> "${await statusText()}")`);

  // typed fix by keyboard: the twin textbox feeds egui, Enter presses "Use this"
  const ill = await page.evaluate(() => [...document.querySelectorAll('#controls button[data-id^="word-"]')].map(b => b.dataset.id).find(i => /-i:0$/.test(i)));
  expect(await tabTo(ill), 'Tab reaches the unreadable word');
  await page.keyboard.press('Enter');
  expect(await waitFor(async () => (await twinIds()).includes('typed-word')), 'the unreadable word offers a text box');
  const beforeTyped = await statusText();
  expect(await tabTo('typed-word'), 'Tab reaches the typed-fix text box');
  await page.keyboard.type('hours', { delay: 30 });
  await page.keyboard.press('Enter');
  expect(await waitFor(async () => (await statusText()) !== beforeTyped), `typing and Enter resolves the word ("${beforeTyped}" -> "${await statusText()}")`);

  // clipboard: Copy text writes the student's transcript through navigator.clipboard
  expect(await tabTo('copy'), 'Tab reaches Copy text');
  await page.keyboard.press('Enter');
  let clip = '';
  expect(await waitFor(async () => { clip = await page.evaluate(() => navigator.clipboard.readText().catch(() => '')); return clip.includes('Maya R.'); }),
    'Copy text puts the student\'s transcript on the clipboard: ' + JSON.stringify(clip.slice(0, 60)));
  expect(clip.includes('confidence') && clip.includes('hours'), 'the copied text carries the accepted suggestion and the typed fix');
  expect((await page.evaluate(() => window.__violations)).length === 0, 'still zero violations after interaction');
  await page.screenshot({ path: path.join(here, '../dist/check.png') });

  // ---- real storage: an assignment in IndexedDB `ocr_fs` (as the legacy app writes it) opens, edits and saves
  {
    const ctx3 = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const p3 = await ctx3.newPage();
    await p3.addInitScript(track);
    await p3.goto(base + '/?theme=light', { waitUntil: 'load' });
    await settled(p3);
    const empty = async () => { for (let i = 0; i < 6; i++) { await p3.mouse.move(600 + i, 300 + i); await p3.waitForTimeout(80); } };
    await empty();
    await p3.waitForSelector('#controls [data-id="empty-title"]', { timeout: 20000 });
    expect((await p3.textContent('#controls [data-id="empty-title"]')) === 'No assignments yet', 'an empty IndexedDB shows the empty state, not a crash');
    // the same files the JS app would have written: project.json and pages/<id>.json as strings, the image as a Blob
    await p3.evaluate(async () => {
      const db = await new Promise((res, rej) => {
        const r = indexedDB.open('ocr_fs', 1);
        r.onupgradeneeded = () => { r.result.createObjectStore('kv'); r.result.createObjectStore('files'); };
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
      });
      const c = document.createElement('canvas');
      c.width = 600; c.height = 800;
      const g = c.getContext('2d');
      g.fillStyle = '#fcfcf8'; g.fillRect(0, 0, 600, 800);
      g.fillStyle = '#26358f';
      for (let i = 0; i < 6; i++) g.fillRect(60, 120 + i * 60, 400, 6);
      const jpg = await new Promise(r => c.toBlob(r, 'image/jpeg', 0.8));
      const page = { id: 'p1', name: 'p1.jpg', order: 0, status: 'done', raw: 'The confidance interval is wide.', corrected: 'The confidance interval is wide.',
        corrections: [{ applied: false, word: 'confidance', term: 'confidence', conf: 0.9 }], boxes: [], timings: { ms: 1000, tokens: 10 },
        error: '', rotation: 0, student: { name: 'Ana L.' }, reviewed: false, resolved: {}, edits: [], guesses: {} };
      const meta = { schemaVersion: 1, name: 'Unit 5', topic: 'General', prompt: '', order: ['p1'], created: 1, updated: 1, ui: {}, stats: { pages: 1, done: 1 } };
      await new Promise((res, rej) => {
        const t = db.transaction('files', 'readwrite');
        const s = t.objectStore('files');
        s.put(JSON.stringify(meta, null, 2), 'unit-5/project.json');
        s.put(JSON.stringify(page), 'unit-5/pages/p1.json');
        s.put(jpg, 'unit-5/images/p1.jpg');
        t.oncomplete = res; t.onerror = () => rej(t.error);
      });
      db.close();
    });
    await p3.reload({ waitUntil: 'load' });
    await settled(p3);
    await empty();
    await p3.waitForSelector('#controls [data-id="student-name"]', { timeout: 20000 });
    expect((await p3.textContent('#controls [data-id="student-name"]')) === 'Ana L.', 'the assignment stored in IndexedDB opens through the project layer');
    expect((await p3.textContent('#controls [data-id="save-state"]')).includes('Saved in this browser'), 'save indicator names the browser store');
    const wid = await p3.evaluate(() => [...document.querySelectorAll('#controls button[data-id^="word-"]')].map(b => b.dataset.id)[0]);
    await p3.evaluate((id) => document.querySelector(`#controls [data-id="${id}"]`).click(), wid);
    await empty();
    await p3.waitForSelector('#controls [data-id="alt-suggested"]', { timeout: 10000 });
    await p3.evaluate(() => document.querySelector('#controls [data-id="alt-suggested"]').click());
    const readPage = () => p3.evaluate(async () => {
      const db = await new Promise((res, rej) => { const r = indexedDB.open('ocr_fs', 1); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
      const v = await new Promise((res, rej) => { const q = db.transaction('files').objectStore('files').get('unit-5/pages/p1.json'); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
      db.close();
      return v;
    });
    let saved = '';
    for (let i = 0; i < 60 && !saved.includes('"c:0":"suggested"'); i++) { await empty(); saved = await readPage(); }
    expect(saved.includes('"c:0":"suggested"') && saved.includes('"corrected":"The confidence interval is wide."'),
      'accepting a suggestion is written back to IndexedDB as the same page document: ' + saved.slice(0, 160));
    const reopened = JSON.parse(saved);
    expect(reopened.reviewed === false && reopened.id === 'p1' && reopened.guesses && Array.isArray(reopened.edits), 'unknown and untouched fields survive the round trip');
    expect((await p3.evaluate(() => window.__violations)).length === 0, 'zero CSP violations with real storage');
    await ctx3.close();
  }

  // ---- negative control: the policy is really enforced
  const ctx2 = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const p2 = await ctx2.newPage();
  await p2.addInitScript(track);
  await p2.route(u => u.pathname === '/', async route => {
    const r = await route.fetch();
    const h = { ...r.headers() };
    h['content-security-policy'] = h['content-security-policy'].replace(" 'wasm-unsafe-eval'", '');
    await route.fulfill({ response: r, headers: h });
  });
  await p2.goto(base + '/?demo=1', { waitUntil: 'load' });
  await settled(p2);
  const neg = await p2.evaluate(() => ({ ready: window.__ready === true, error: window.__error || null }));
  expect(!neg.ready && !!neg.error, "negative control: without 'wasm-unsafe-eval' the app fails to mount");
} catch (e) {
  failures.push(String(e.stack || e));
  console.error(e);
} finally {
  if (browser) await browser.close().catch(() => {});
  stop();
}
console.log(failures.length ? `\nCSP check FAILED (${failures.length})` : '\nCSP check passed');
process.exit(failures.length ? 1 : 0);
