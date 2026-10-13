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

  // ---- review tools by real keyboard (fresh demo): strip, stage zoom, transcript underline, tray Escape, dialogs
  {
    const ctx4 = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const p4 = await ctx4.newPage();
    const reqs = [];
    p4.on('request', r => reqs.push(r.url()));
    await p4.addInitScript(track);
    await p4.goto(base + '/?demo=1&theme=light', { waitUntil: 'load' });
    await settled(p4);
    const pump4 = async (n = 4) => { for (let i = 0; i < n; i++) { await p4.mouse.move(700 + i, 500 + i); await p4.waitForTimeout(70); } };
    await pump4(8);
    await p4.waitForSelector('#controls [data-id="done"]', { timeout: 20000 });
    const active4 = () => p4.evaluate(() => { const a = document.activeElement; return a ? a.getAttribute('data-id') : null; });
    const attr4 = (id, name) => p4.evaluate(([i, n]) => {
      const e = [...document.querySelectorAll('#controls [data-id]')].find(x => x.dataset.id === i);
      return e ? e.getAttribute(n) : undefined;
    }, [id, name]);
    const has4 = async (id) => (await attr4(id, 'data-id')) !== undefined;
    const ids4 = () => p4.evaluate(() => [...document.querySelectorAll('#controls button, #controls input')].map(e => e.dataset.id));
    const wait4 = async (fn, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await fn()) return true; await pump4(2); } return false; };
    const tab4 = async (id, limit = 140) => {
      await p4.mouse.click(640, 4);
      for (let i = 0; i < limit; i++) { await p4.keyboard.press('Tab'); if ((await active4()) === id) return true; }
      return false;
    };
    expect(await wait4(async () => reqs.some(u => u.endsWith('/stix-two-math.otf'))), 'the math font is fetched on demand when a page has a formula (not linked into the wasm)');
    const wasmReq = reqs.find(u => u.endsWith('.wasm'));
    expect(!!wasmReq, 'the wasm itself is requested separately: ' + wasmReq);

    // strip: Arrow, Home, End, Alt+Arrow
    expect(await tab4('page-p-maya-1'), 'Tab reaches the first page thumbnail');
    await p4.keyboard.press('ArrowRight');
    expect(await wait4(async () => (await attr4('page-p-maya-2', 'aria-pressed')) === 'true'), 'ArrowRight on the strip selects the next page');
    expect((await active4()) === 'page-p-maya-2', 'focus follows the selection in the strip');
    await p4.keyboard.press('Home');
    expect(await wait4(async () => (await attr4('page-p-maya-1', 'aria-pressed')) === 'true'), 'Home selects the first page');
    await p4.keyboard.press('End');
    expect(await wait4(async () => (await attr4('page-p-maya-2', 'aria-pressed')) === 'true'), 'End selects the last page');
    await p4.keyboard.press('Alt+ArrowLeft');
    const order = async () => (await ids4()).filter(i => i && i.startsWith('page-'));
    expect(await wait4(async () => JSON.stringify(await order()) === JSON.stringify(['page-p-maya-2', 'page-p-maya-1'])),
      'Alt+ArrowLeft moves the page one place earlier: ' + JSON.stringify(await order()));
    await p4.keyboard.press('Alt+ArrowRight');
    expect(await wait4(async () => JSON.stringify(await order()) === JSON.stringify(['page-p-maya-1', 'page-p-maya-2'])), 'Alt+ArrowRight moves it back');

    // page stage: zoom keys only while it has focus
    expect(await has4('stage') && !(await has4('zoom-out')) === false, 'the page stage is a focusable group in the twin');
    expect((await attr4('zoom-out', 'disabled')) !== null, 'Zoom out starts disabled (fit)');
    expect(await tab4('stage'), 'Tab reaches the page stage');
    await p4.keyboard.press('+');
    expect(await wait4(async () => (await attr4('zoom-out', 'disabled')) === null), 'plus zooms in (Zoom out becomes available)');
    await p4.keyboard.press('0');
    expect(await wait4(async () => (await attr4('zoom-out', 'disabled')) !== null), 'zero fits the page again');

    // transcript: arrows move the underline only inside it; Escape clears it first, not the tray
    const reading = () => p4.evaluate(() => { const e = document.querySelector('#controls p[data-reading]'); return e ? e.dataset.id : null; });
    expect(await tab4('done'), 'Tab reaches Done');
    await p4.keyboard.press('ArrowDown');
    await pump4(3);
    expect((await reading()) === null, 'ArrowDown with focus in the header does not move the reading underline');
    expect(await tab4('transcript'), 'Tab reaches the transcript region');
    await p4.keyboard.press('ArrowDown');
    expect(await wait4(async () => /-0$/.test((await reading()) || '')), 'ArrowDown in the transcript underlines the first line: ' + await reading());
    await p4.keyboard.press('ArrowDown');
    expect(await wait4(async () => /-1$/.test((await reading()) || '')), 'ArrowDown again moves to the second line');
    await p4.keyboard.press('Escape');
    expect(await wait4(async () => (await reading()) === null), 'Escape clears the underline');
    expect(await has4('alt-suggested'), 'and leaves the tray open');
    // activating a flagged word underlines its line
    const word = (await ids4()).find(i => i && /^word-.*-c:0$/.test(i));
    expect(await tab4(word), 'Tab reaches a flagged word');
    await p4.keyboard.press('Enter');
    expect(await wait4(async () => (await reading()) !== null), 'activating a flagged word moves the underline to its line: ' + await reading());

    // tray: Escape inside it closes it and returns focus to a word
    expect(await tab4('alt-suggested'), 'Tab reaches a tray choice');
    await p4.keyboard.press('Escape');
    expect(await wait4(async () => !(await has4('alt-suggested'))), 'Escape inside the tray closes it');
    expect(await wait4(async () => ((await active4()) || '').startsWith('word-')), 'focus returns to a flagged word: ' + await active4());

    // dialogs: Settings (gear), Change student, Remove page; Escape closes; the screen is inert behind them
    expect(await tab4('settings'), 'Tab reaches the Settings gear');
    await p4.keyboard.press('Enter');
    expect(await wait4(async () => (await p4.textContent('#controls [data-id="dialog-title"]').catch(() => null)) === 'Settings'), 'Settings opens a dialog');
    const inDialog = await ids4();
    expect(!inDialog.includes('done') && inDialog.includes('theme-dark'), 'while a dialog is open only its controls are in the twin: ' + inDialog.join(' '));
    await p4.keyboard.press('Escape');
    expect(await wait4(async () => (await has4('done')) && !(await has4('dialog-title'))), 'Escape closes the dialog');
    expect(await tab4('change-student'), 'Tab reaches Change student');
    await p4.keyboard.press('Enter');
    expect(await wait4(async () => (await p4.textContent('#controls [data-id="dialog-title"]').catch(() => null)) === 'Change student'), 'Change student opens its dialog');
    expect((await ids4()).includes('cs-student-0') && (await ids4()).includes('cs-new'), 'it lists the other students and "A new student"');
    await p4.keyboard.press('Escape');
    expect(await wait4(async () => await has4('done')), 'Escape cancels it');
    expect(await tab4('remove'), 'Tab reaches Remove this page');
    await p4.keyboard.press('Enter');
    expect(await wait4(async () => (await p4.textContent('#controls [data-id="dialog-title"]').catch(() => null)) === 'Remove page'), 'Remove asks for confirmation');
    await p4.keyboard.press('Escape');
    expect(await wait4(async () => await has4('page-p-maya-1')), 'cancelling keeps the page');

    // back to the assignment and into a student again
    expect(await tab4('crumb'), 'Tab reaches the back-to-assignment crumb');
    await p4.keyboard.press('Enter');
    expect(await wait4(async () => (await p4.textContent('#controls [data-id="inbox-title"]').catch(() => null)) === 'Unit 5 written response'), 'the crumb shows the assignment overview');
    await p4.screenshot({ path: path.join(here, '../dist/check-inbox.png') });
    expect((await p4.evaluate(() => window.__violations)).length === 0, 'zero CSP violations through all of it');
    await ctx4.close();
  }

  // ---- phone width: boots under the same CSP and exposes the page pane and the tools
  {
    const ctx5 = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const p5 = await ctx5.newPage();
    await p5.addInitScript(track);
    await p5.goto(base + '/?demo=1&theme=dark', { waitUntil: 'load' });
    await settled(p5);
    for (let i = 0; i < 8; i++) { await p5.mouse.move(100 + i, 400 + i); await p5.waitForTimeout(70); }
    await p5.waitForSelector('#controls [data-id="done"]', { timeout: 20000 });
    const ids = await p5.evaluate(() => [...document.querySelectorAll('#controls button')].map(e => e.dataset.id));
    expect(['zoom-in', 'rotate', 'replace', 'remove', 'add-page', 'page-p-maya-1', 'settings', 'crumb'].every(i => ids.includes(i)),
      'phone width exposes the page pane tools and the header');
    expect((await p5.evaluate(() => window.__violations)).length === 0, 'zero CSP violations at phone width');
    await p5.screenshot({ path: path.join(here, '../dist/check-phone.png') });
    await ctx5.close();
  }

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
