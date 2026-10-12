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
  const page = await (await browser.newContext()).newPage();
  const logs = [];
  page.on('console', m => { if (['error', 'warning'].includes(m.type())) logs.push(`${m.type()}: ${m.text()}`); });
  page.on('pageerror', e => logs.push('pageerror: ' + e.message));
  await page.addInitScript(track);
  const resp = await page.goto(base + '/', { waitUntil: 'load' });
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
  for (let i = 0; i < 5; i++) { await page.mouse.move(300 + i * 3, 200 + i); await page.waitForTimeout(80); }
  await page.waitForSelector('#controls [data-id="count"]', { timeout: 10000 });
  expect(true, 'DOM control twin present: #controls [data-id=count]');
  const twin = page.locator('#controls [data-id="count"]');
  expect((await twin.textContent()) === 'Count: 0', 'twin label starts at "Count: 0"');
  // ---- real keyboard flow (no programmatic focus): Tab order, no canvas trap, names, focus ring, activation
  const active = () => page.evaluate(() => {
    const a = document.activeElement;
    return a ? { tag: a.tagName, id: a.id, dataId: a.getAttribute('data-id') } : null;
  });
  const pump = async (n = 6) => { for (let i = 0; i < n; i++) { await page.mouse.move(700 + i, 500 + i); await page.waitForTimeout(80); } };
  // 1. Accessibility surface: nothing focusable may be unnamed or hidden-but-tabbable.
  const surface = await page.evaluate(() => {
    const out = { tabbable: [], hiddenFocusable: [] };
    for (const el of document.querySelectorAll('canvas, input, button, a[href], select, textarea, [tabindex]')) {
      const hidden = el.closest('[aria-hidden="true"]') !== null;
      const name = (el.getAttribute('aria-label') || el.textContent || '').trim();
      const tabbable = el.tabIndex >= 0;
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
  expect(surface.tabbable.length > 0 && surface.tabbable.every(t => t.name), 'every tabbable element has an accessible name: ' + JSON.stringify(surface.tabbable.map(t => t.name)));
  expect(!!surface.agent && surface.agent.tabindex === '-1' && !!surface.agent.label, 'eframe text agent is named and out of the Tab order: ' + JSON.stringify(surface.agent));

  // pixel probe for the on-canvas focus ring: same pointer position every time so hover cannot differ
  const shot = async () => { await page.mouse.move(900, 600); await page.waitForTimeout(500); return (await page.screenshot()).toString('base64'); };
  await page.mouse.click(900, 600); // click far from the button: canvas gets pointer focus, no ring
  expect((await active()).id === 'c', 'click focuses the canvas (pointer path still works)');
  const idle = await shot();

  // 2. Tab from the canvas must leave it and land on the first control (the old trap)
  await page.keyboard.press('Tab');
  const afterTab = await active();
  expect(afterTab && afterTab.dataId === 'count', 'Tab from canvas lands on twin [data-id=count], got ' + JSON.stringify(afterTab));
  await pump(); await page.waitForTimeout(300);
  const ringOn = await shot();
  expect(ringOn !== idle, 'on-canvas focus ring is drawn when the twin has focus');
  // 3. Tab again goes on past the controls (no trap, nothing unnamed in between)
  const seen = [];
  for (let i = 0; i < 4; i++) { await page.keyboard.press('Tab'); seen.push(await active()); }
  expect(!seen.some(s => s && s.id === 'c') && !seen.some(s => s && s.tag === 'INPUT'),
    'repeated Tab never lands on the canvas or the text agent: ' + JSON.stringify(seen));
  // 4. Shift+Tab from the canvas must also leave it
  await page.mouse.click(900, 600);
  expect((await active()).id === 'c', 'canvas focused again by click');
  await page.keyboard.press('Shift+Tab');
  const afterShift = await active();
  expect(!(afterShift && afterShift.id === 'c'), 'Shift+Tab leaves the canvas (no keyboard trap), now on ' + JSON.stringify(afterShift));
  // 5. Focus ring clears on blur (click away from the control)
  // bring focus to the twin by Tab from the canvas, then blur by clicking the canvas
  await page.mouse.click(900, 600);
  await page.keyboard.press('Tab');
  expect((await active()).dataId === 'count', 'twin focused again via Tab');
  await pump(); await page.waitForTimeout(300);
  const ringOn2 = await shot();
  await page.mouse.click(900, 600);
  const ringOff = await shot();
  expect(ringOn2 !== ringOff && ringOff === idle, 'focus ring clears on blur (frame after blur equals the idle frame)');
  // 6. Activation with real keys: Tab to the button, then Enter, then Space
  await page.mouse.click(900, 600);
  await page.keyboard.press('Tab');
  expect((await active()).dataId === 'count', 'twin focused via Tab before activation');
  const waitCount = async (n) => { for (let i = 0; i < 40 && (await twin.textContent()) !== 'Count: ' + n; i++) { await pump(2); } return (await twin.textContent()) === 'Count: ' + n; };
  await page.keyboard.press('Enter');
  expect(await waitCount(1), 'Enter on the focused twin reaches egui (count 0 -> 1)');
  expect((await active()).dataId === 'count', 'twin keeps keyboard focus after its label changed (no DOM re-order blur)');
  await page.keyboard.press('Space');
  expect(await waitCount(2), 'Space on the focused twin reaches egui (count 1 -> 2)');
  expect((await page.evaluate(() => window.__violations)).length === 0, 'still zero violations after interaction');
  await page.screenshot({ path: path.join(here, '../dist/check.png') });

  // ---- negative control: the policy is really enforced
  const ctx2 = await browser.newContext();
  const p2 = await ctx2.newPage();
  await p2.addInitScript(track);
  await p2.route(base + '/', async route => {
    const r = await route.fetch();
    const h = { ...r.headers() };
    h['content-security-policy'] = h['content-security-policy'].replace(" 'wasm-unsafe-eval'", '');
    await route.fulfill({ response: r, headers: h });
  });
  await p2.goto(base + '/', { waitUntil: 'load' });
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
