// Blind-critic checks on the DOM transcript (round 1): what a person who selects, copies, zooms to 400% or pinch
// zooms over the text gets. Passing since the DOM transcript got copy, line breaks, TeX for formulas, scrolling formulas and ctrl+wheel (A11Y.md).
// Usage: node crates/app/web/transcript-dom-critic.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8793));
let browser;

async function open(viewport, query = 'demo=1&theme=light&tray=0') {
  const ctx = await browser.newContext({ viewport, bypassCSP: true, permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await ctx.newPage();
  await page.goto(`http://127.0.0.1:${port}/?${query}`, { waitUntil: 'load' });
  for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
  const pump = async (n = 5) => { for (let i = 0; i < n; i++) { await page.mouse.move(viewport.width - 30 + (i % 3), viewport.height - 30 + i); await page.waitForTimeout(70); } };
  await pump(8);
  await page.waitForSelector('#controls p.tx', { timeout: 20000 });
  await pump(6);
  return { ctx, page, pump };
}

try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });

  // 1. Selected transcript text can be copied (eframe's document "copy" listener must not swallow it)
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    await page.bringToFront();
    await page.evaluate(() => navigator.clipboard.writeText('SEED'));
    const at = await page.evaluate(() => { const r = document.querySelectorAll('#controls p.tx')[3].getBoundingClientRect(); return [r.left + 50, r.top + 12]; });
    await page.mouse.click(at[0], at[1], { clickCount: 3 });
    await pump(3);
    const sel = await page.evaluate(() => getSelection().toString());
    expect(/^It does not mean/.test(sel), 'a triple click selects the line: ' + JSON.stringify(sel.slice(0, 30)));
    await page.keyboard.press('Control+c');
    await page.waitForTimeout(300);
    const clip = await page.evaluate(() => navigator.clipboard.readText());
    expect(clip.startsWith('It does not mean'), 'Ctrl+C puts the selected transcript text on the clipboard: ' + JSON.stringify(clip.slice(0, 30)));
    await ctx.close();
  }

  // 2. Selecting across lines keeps the lines apart, and a formula is not one symbol per line
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    const sel = await page.evaluate(() => { const c = document.querySelector('#controls .tx-clip:has(p.tx)'); getSelection().selectAllChildren(c); return getSelection().toString(); });
    expect(!/samples,about|mean\.probably|wide\.Margin/.test(sel), 'the end of a line and the start of the next are not glued: ' + JSON.stringify(sel.slice(-160)));
    const sym = sel.split('\n').filter((l) => l.length <= 1).length;
    expect(sym < 4, 'a formula is not copied one symbol per line (' + sym + ' one-character lines)');
    await ctx.close();
  }

  // 3. 400% zoom (320 CSS px): a formula wider than the column is not cut off and cannot be reached
  {
    const { ctx, page } = await open({ width: 320, height: 1100 });
    const cut = await page.evaluate(() => [...document.querySelectorAll('#controls p.tx')]
      .filter((p) => p.querySelector('.tx-math'))
      .map((p) => ({ id: p.dataset.id, need: p.scrollWidth, have: p.clientWidth }))
      .filter((m) => m.need > m.have + 1));
    expect(cut.length === 0, 'no formula is wider than its column at 320px (clipped by .tx-clip): ' + JSON.stringify(cut));
    await ctx.close();
  }

  // 4. Ctrl+wheel / trackpad pinch over the text is the browser's zoom (WCAG 1.4.4), as in the JS app
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    await page.evaluate(() => { window.__wp = []; document.documentElement.addEventListener('wheel', (e) => { setTimeout(() => window.__wp.push(e.defaultPrevented), 0); }, { capture: true, passive: true }); });
    const at = await page.evaluate(() => { const r = document.querySelectorAll('#controls p.tx')[3].getBoundingClientRect(); return [r.left + 50, r.top + 12]; });
    await page.keyboard.down('Control');
    await page.mouse.move(at[0], at[1]);
    await page.mouse.wheel(0, -100);
    await page.keyboard.up('Control');
    await page.waitForTimeout(300);
    const wp = await page.evaluate(() => window.__wp);
    expect(wp.length > 0 && wp.every((p) => !p), 'ctrl+wheel over the transcript is left to the browser to zoom: defaultPrevented=' + JSON.stringify(wp));
    await ctx.close();
  }
} catch (e) {
  failures.push(String(e.stack || e));
  console.error(e);
} finally {
  if (browser) await browser.close().catch(() => {});
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall DOM transcript critic checks passed');
process.exit(failures.length ? 1 : 0);
