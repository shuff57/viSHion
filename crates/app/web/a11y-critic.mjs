// Blind-critic checks the phase-4 twin does not meet yet (each FAIL is a gap against the JS app).
// Usage: node crates/app/web/a11y-critic.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8790));
let browser;

async function open(viewport, opts = {}) {
  const ctx = await browser.newContext({ viewport, bypassCSP: true, ...opts });
  const page = await ctx.newPage();
  await page.goto(`http://127.0.0.1:${port}/?demo=1&theme=light`, { waitUntil: 'load' });
  for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
  const pump = async (n = 5) => { for (let i = 0; i < n; i++) { await page.mouse.move(viewport.width - 30 + (i % 3), viewport.height - 30 + i); await page.waitForTimeout(70); } };
  await pump(8);
  await page.waitForSelector('#controls [data-id]', { timeout: 20000 });
  return { ctx, page, pump };
}
const shot = async (page) => 'data:image/png;base64,' + (await page.screenshot()).toString('base64');
const changed = (page, a, b) => page.evaluate(async ([a, b]) => {
  const ld = async (u) => { const i = new Image(); i.src = u; await i.decode(); const c = document.createElement('canvas'); c.width = i.width; c.height = i.height; const x = c.getContext('2d'); x.drawImage(i, 0, 0); return x.getImageData(0, 0, i.width, i.height).data; };
  const A = await ld(a), B = await ld(b); let n = 0;
  for (let k = 0; k < A.length; k += 4) if (Math.abs(A[k] - B[k]) + Math.abs(A[k + 1] - B[k + 1]) + Math.abs(A[k + 2] - B[k + 2]) > 30) n++;
  return n;
}, [a, b]);

try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });

  // 1. Every Tab stop shows where focus is (WCAG 2.4.7) and is on screen (2.4.11): a keyboard user at 400% zoom or on a phone
  for (const vp of [{ width: 1280, height: 800 }, { width: 390, height: 844 }, { width: 320, height: 256 }]) {
    const { ctx, page, pump } = await open(vp, vp.width === 320 ? { deviceScaleFactor: 4 } : {});
    await page.mouse.click(vp.width / 2, 2);
    const off = [];
    for (let i = 0; i < 45; i++) {
      await page.keyboard.press('Tab'); await pump(3);
      const r = await page.evaluate(() => { const e = document.activeElement; if (!e?.dataset?.id) return null; const b = e.getBoundingClientRect(); return { id: e.dataset.id, top: b.top, bottom: b.bottom, h: innerHeight }; });
      if (r && (r.bottom > r.h + 1 || r.top < -1) && !off.includes(r.id)) off.push(r.id);
    }
    expect(off.length === 0, `${vp.width}x${vp.height}: focus never lands off screen (Tab does not scroll it into view)` + (off.length ? ': ' + off.slice(0, 8) : ''));
    await ctx.close();
  }

  // 2. Every focusable twin element has a visible focus indicator
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    await page.evaluate(() => document.activeElement.blur()); await pump(4);
    const base = await shot(page);
    const none = [];
    for (const id of ['transcript', 'stage', 'text-view', 'transcribe-again']) {
      await page.evaluate(() => document.activeElement.blur());
      await page.evaluate((i) => document.querySelector(`#controls [data-id="${i}"]`).focus(), id); await pump(4);
      const n = await changed(page, base, await shot(page));
      if (n < 400) none.push(`${id} (${n}px changed)`);
    }
    expect(none.length === 0, 'every Tab stop draws a focus ring of a visible size' + (none.length ? ': ' + none : ''));
    await ctx.close();
  }

  // 3. What the JS app says that the twin does not
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    const snap = await page.locator('body').ariaSnapshot();
    expect(/button "Fit the page to the pane"[^\n]*100%/.test(snap) || /zoom[^\n]*100%/i.test(snap), 'the zoom level is exposed (JS: button "Fit the page to the pane": 100%)');
    expect(/\bimg\b/.test(snap), 'the page image has a text alternative (JS: img "The scanned handwritten page")');
    expect(/2 pages, \d+ words/.test(snap), 'the page and word count are exposed (JS: "2 pages, 96 words")');
    await ctx.close();
  }

  // 4. A modal dialog leaves nothing of the screen behind it in the tree
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    await page.evaluate(() => document.querySelector('#controls [data-id="settings"]').focus());
    await page.keyboard.press('Enter'); await pump(6);
    const snap = await page.locator('body').ariaSnapshot();
    expect(!/heading "Maya R\."/.test(snap) && !/- main:/.test(snap), 'behind a dialog: no main landmark or h1 left in the tree');
    await ctx.close();
  }

  // 5. Blind critic round 3 (each FAIL is a gap)
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const val = () => page.evaluate(() => document.querySelector('#controls [data-id="typed-word"]')?.value ?? null);
    const focus = async (id) => { await page.evaluate((i) => document.querySelector(`#controls [data-id="${i}"]`)?.focus(), id); await pump(3); };
    // the text box is the next word's box after a typed fix, and a screen reader reads its value: it must be empty
    await focus('word-p-maya-1-i:0'); await page.keyboard.press('Enter'); await pump(4);
    await focus('typed-word'); await page.keyboard.type('mean'); await page.keyboard.press('Enter'); await pump(6);
    const v = await val();
    expect(v === '', 'after a typed fix the next word\'s text box is empty (JS re-renders the tray), not stale: ' + JSON.stringify(v));
    // a button that disables itself by its own action hands focus on (A11Y.md says so)
    await page.mouse.click(640, 3);
    for (let i = 0; i < 40; i++) { await page.keyboard.press('Tab'); if (await page.evaluate(() => document.activeElement?.dataset?.id) === 'zoom-in') break; }
    for (let i = 0; i < 5; i++) { await page.keyboard.press('Enter'); await pump(4); }
    const act = await page.evaluate(() => document.activeElement?.dataset?.id || document.activeElement?.tagName);
    expect(act !== 'BODY' && act !== 'HTML', 'zooming in to the maximum keeps focus on a control (Zoom in turns disabled under it): ' + act);
    // js/main.js sets the tab title to "<assignment> | viSHion" outside the home screen
    const title = await page.title();
    expect(/\| viSHion$/.test(title), 'the document title names the assignment (JS: "Unit 5 | viSHion"): ' + JSON.stringify(title));
    await ctx.close();
  }
  // 6. Blind critic round 4 (each FAIL is a gap)
  {
    // the same action twice says the same words: a live region whose text did not change is not re-read (JS appends a new toast node each time)
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    await page.evaluate(() => { window.__m = 0; new MutationObserver(() => { window.__m++; }).observe(document.getElementById('announce'), { childList: true, characterData: true, subtree: true }); });
    const press = async (id) => { await page.evaluate((i) => document.querySelector('#controls [data-id="' + i + '"]')?.focus(), id); await page.keyboard.press('Enter'); await pump(6); await page.waitForTimeout(700); await pump(2); };
    await press('copy'); const a = await page.evaluate(() => window.__m);
    await press('copy'); const c = await page.evaluate(() => window.__m);
    expect(c > a, 'pressing Copy text twice announces it twice (live region written again): writes ' + a + ' then ' + c);
    await ctx.close();
  }
  {
    // 400% zoom on a phone: Settings must be where it is drawn, not under the page pane
    const { ctx, page } = await open({ width: 320, height: 568 }, { deviceScaleFactor: 2 });
    const r = await page.evaluate(() => { const b = document.querySelector('#controls [data-id="settings"]').getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height }; });
    const url = 'data:image/png;base64,' + (await page.screenshot()).toString('base64');
    const bad = await page.evaluate(async ([u, r]) => { const i = new Image(); i.src = u; await i.decode(); const c = document.createElement('canvas'); c.width = i.width; c.height = i.height; const x = c.getContext('2d'); x.drawImage(i, 0, 0);
      const d = x.getImageData(0, 0, i.width, i.height).data; let n = 0, t = 0;
      for (let yy = Math.round(r.y * 2) + 2; yy < (r.y + r.h) * 2 - 2; yy += 4) for (let xx = Math.round(r.x * 2) + 2; xx < (r.x + r.w) * 2 - 2; xx += 4) { const k = (yy * i.width + xx) * 4; t++; if (d[k] < 235 && d[k + 2] > d[k] + 8 && d[k + 1] > 190) n++; }
      return t ? n / t : 1; }, [url, r]);
    expect(bad < 0.1, '320 css px wide: the Settings button is visible, not covered by the page pane (' + Math.round(bad * 100) + '% of its rectangle is pane colour)');
    await ctx.close();
  }
  // 7. Blind critic round 5 (each FAIL is a gap)
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const focus = async (id) => { await page.evaluate((i) => document.querySelector('#controls [data-id="' + i + '"]')?.focus(), id); await pump(3); };
    const press = async (id) => { await focus(id); await page.keyboard.press('Enter'); await pump(6); await page.waitForTimeout(500); await pump(2); };
    const said = () => page.evaluate(() => document.getElementById('announce').textContent + ' ' + document.getElementById('announce-alert').textContent);
    const stat = () => page.evaluate(() => document.querySelector('#controls [data-id="save-state"]')?.textContent ?? '');

    // a. one decision is one utterance: the announcement, the count of words and the panel's position are three live regions
    await page.evaluate(() => {
      window.__regions = new Set();
      new MutationObserver((ms) => { for (const m of ms) { const el = m.target.nodeType === 1 ? m.target : m.target.parentElement; const r = el && el.closest('[role=status],[role=alert],[aria-live]'); if (r && r.textContent.trim()) window.__regions.add(r.id || r.dataset.id); } }).observe(document.body, { childList: true, characterData: true, subtree: true });
    });
    await press('alt-suggested');
    const regions = await page.evaluate(() => [...window.__regions]);
    expect(regions.length <= 1, 'one decision writes one live region (A11Y.md: one utterance per action), not ' + regions.length + ': ' + regions);

    // c. Rotate: the header must not say "Saving…" for the rest of the session (RotateImage never answers Event::Saved)
    await press('rotate');
    let s = '';
    for (let i = 0; i < 12; i++) { await pump(2); await page.waitForTimeout(250); s = await stat(); if (!/^Saving/.test(s)) break; }
    expect(!/^Saving/.test(s), 'after Rotate the save indicator settles (it says ' + JSON.stringify(s) + ' for good)');
    await ctx.close();
  }
  {
    // b. Next word says which word it landed on, and the page when it crosses one (a decision does both)
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const press = async (id) => { await page.evaluate((i) => document.querySelector('#controls [data-id="' + i + '"]')?.focus(), id); await pump(3); await page.keyboard.press('Enter'); await pump(6); await page.waitForTimeout(500); await pump(2); };
    const said = () => page.evaluate(() => document.getElementById('announce').textContent.trim());
    await press('next-word');
    expect(/populaton|illegible|could not be read|spelling/i.test(await said()), 'Next word to check names the word it moved to, not only "Word 2 of 4": ' + JSON.stringify(await said()));
    await press('next-word'); await press('next-word');
    expect(/page 2/i.test(await said()), 'Next word to check across a page boundary says the page changed: ' + JSON.stringify(await said()));
    await ctx.close();
  }
  {
    // d. a zoomed page can be panned from the keyboard (JS: .scan-stage is overflow:auto with tabindex=0, arrows scroll it)
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    await page.evaluate(() => document.querySelector('#controls [data-id="stage"]').focus());
    for (let i = 0; i < 4; i++) { await page.keyboard.press('+'); await pump(3); }
    await pump(4);
    const base = await shot(page);
    for (const k of ['ArrowDown', 'ArrowDown', 'ArrowRight', 'PageDown']) { await page.keyboard.press(k); await pump(3); }
    await pump(4);
    const n = await changed(page, base, await shot(page));
    expect(n > 5000, 'arrow keys and PageDown pan the zoomed page image (' + n + ' px changed)');
    await ctx.close();
  }
  {
    // e. Change student: a refused Save is not silent, and "A new student…" leads to its text box (JS focuses it)
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const focus = async (id) => { await page.evaluate((i) => document.querySelector('#controls [data-id="' + i + '"]')?.focus(), id); await pump(3); };
    const press = async (id) => { await focus(id); await page.keyboard.press('Enter'); await pump(6); await page.waitForTimeout(500); await pump(2); };
    await press('change-student');
    await press('dialog-ok');
    const r = await page.evaluate(() => ({ id: document.activeElement?.dataset?.id, said: document.getElementById('announce').textContent + document.getElementById('announce-alert').textContent }));
    expect(r.id !== 'dialog-ok' || r.said.trim() !== '', 'Save with no student chosen says why or moves focus to the field (JS focuses the select): focus ' + r.id + ', said ' + JSON.stringify(r.said));
    await press('cs-new');
    const f = await page.evaluate(() => document.activeElement?.dataset?.id);
    expect(f === 'cs-name', '"A new student…" moves focus to its text box (JS: input.focus()): focus is on ' + f);
    await ctx.close();
  }
  {
    // f. forced colours: the canvas follows the user's system colours (JS is DOM, so the browser does it)
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, forcedColors: 'active', colorScheme: 'dark', bypassCSP: true });
    const page = await ctx.newPage();
    await page.goto(`http://127.0.0.1:${port}/?demo=1`, { waitUntil: 'load' });
    for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
    for (let i = 0; i < 8; i++) { await page.mouse.move(1200 + i, 700 + i); await page.waitForTimeout(70); }
    const canvas = await page.evaluate(() => { const d = document.createElement('div'); d.style.color = 'Canvas'; document.body.append(d); const c = getComputedStyle(d).color; d.remove(); return c; });
    const url = 'data:image/png;base64,' + (await page.screenshot()).toString('base64');
    const px = await page.evaluate(async (u) => { const i = new Image(); i.src = u; await i.decode(); const c = document.createElement('canvas'); c.width = i.width; c.height = i.height; const x = c.getContext('2d'); x.drawImage(i, 0, 0); const d = x.getImageData(900, 200, 1, 1).data; return `rgb(${d[0]}, ${d[1]}, ${d[2]})`; }, url);
    expect(px === canvas, 'forced-colors: the text pane is drawn in the system Canvas colour ' + canvas + ', not ' + px);
    await ctx.close();
  }
} catch (e) {
  console.error(e);
  failures.push('exception: ' + (e && e.message));
} finally {
  if (browser) await browser.close();
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall critic checks passed');
process.exit(failures.length ? 1 : 0);
