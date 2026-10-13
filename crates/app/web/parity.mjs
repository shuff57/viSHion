// Native vs web parity: both builds open the same demo assignment and must expose the same control list
// (ids, order, accessible names, pressed state) and the same screen, in light and dark.
// The native binary dumps its controls (`--dump-controls`) and a screenshot; the web build is driven in Chrome.
// Usage: node crates/app/web/parity.mjs   (run build.mjs and `cargo build -p vishion-app` first)
//        VISHION_BIN=<path> overrides the native binary, CHROME=<path> the browser.
// Screenshots land in crates/app/dist/parity-{native,web}-{light,dark}.png for a side-by-side look.
import { chromium } from 'playwright-core';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { serve } from './serve.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const dist = path.resolve(here, '../dist');
const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const ext = process.platform === 'win32' ? '.exe' : '';
const bin = process.env.VISHION_BIN
  || ['release', 'debug'].map(p => path.join(repo, 'target', p, 'vishion' + ext)).filter(p => fs.existsSync(p))
    .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0]; // the most recent build
if (!bin) throw new Error('no native binary: run `cargo build -p vishion-app` first');

const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };

function native(theme) {
  const png = path.join(dist, `parity-native-${theme}.png`);
  const out = execFileSync(bin, ['--demo', '--theme', theme, '--screenshot', png, '--dump-controls'], { encoding: 'utf8', timeout: 120000 });
  return out.split('\n').filter(l => l.startsWith('{')).map(l => JSON.parse(l));
}

const { port, stop } = await serve(+(process.env.PORT || 8789));
let browser;
try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
  for (const theme of ['light', 'dark']) {
    const nat = native(theme);
    const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
    await page.goto(`http://127.0.0.1:${port}/?demo=1&theme=${theme}`);
    for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
    for (let i = 0; i < 8; i++) { await page.mouse.move(700 + i, 500 + i); await page.waitForTimeout(80); }
    await page.waitForSelector('#controls [data-id="done"]', { timeout: 20000 });
    const web = await page.evaluate(() => [...document.querySelectorAll('#controls button, #controls input')].map(e => ({
      id: e.dataset.id,
      label: e.tagName === 'INPUT' ? e.getAttribute('aria-label') : (e.getAttribute('aria-label') || e.textContent),
      pressed: e.hasAttribute('aria-pressed') ? e.getAttribute('aria-pressed') === 'true' : null,
    })));
    await page.screenshot({ path: path.join(dist, `parity-web-${theme}.png`) });
    const key = c => `${c.id}|${c.label}|${c.pressed}`;
    expect(nat.length > 15, `[${theme}] native reports ${nat.length} controls`);
    expect(JSON.stringify(nat.map(key)) === JSON.stringify(web.map(key)),
      `[${theme}] native and web expose the same ${web.length} controls in the same order`
      + (JSON.stringify(nat.map(key)) === JSON.stringify(web.map(key)) ? '' : '\n  native: ' + nat.map(c => c.id).join(' ') + '\n  web:    ' + web.map(c => c.id).join(' ')));
    const small = nat.filter(c => c.size[0] < 44 || c.size[1] < 44).map(c => `${c.id} ${c.size}`);
    expect(small.length === 0, `[${theme}] every target is at least 44 x 44` + (small.length ? ': ' + small.join(', ') : ''));
    await page.context().close();
  }
} catch (e) {
  failures.push(String(e.stack || e));
  console.error(e);
} finally {
  if (browser) await browser.close().catch(() => {});
  stop();
}
console.log(failures.length ? `\nparity check FAILED (${failures.length})` : '\nparity check passed');
process.exit(failures.length ? 1 : 0);
