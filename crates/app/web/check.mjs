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
  await twin.focus();
  await page.keyboard.press('Enter');
  for (let i = 0; i < 5; i++) { await page.mouse.move(400 + i * 3, 260 + i); await page.waitForTimeout(80); }
  for (let i = 0; i < 40 && (await twin.textContent()) !== 'Count: 1'; i++) { await page.mouse.move(400 + (i % 7), 260 + (i % 5)); await page.waitForTimeout(100); }
  expect((await twin.textContent()) === 'Count: 1', 'keyboard activation of the twin reaches egui (count 0 -> 1)');
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
