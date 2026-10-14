// Blind-critic checks on the DOM transcript (round 3): Windows contrast themes (forced colors) and a finger drag.
// FAILING BY DESIGN until the gaps are closed.
// Usage: node crates/app/web/transcript-dom-critic-3.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8795));
let browser;

try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });

  // 1. forced colors (Windows contrast themes) repaint `color: transparent` text in the system colour: the TeX
  //    source laid over each formula for find and selection must stay unpainted (needs forced-color-adjust: none)
  {
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, bypassCSP: true, forcedColors: 'active' });
    const page = await ctx.newPage();
    await page.goto(`http://127.0.0.1:${port}/?demo=1&theme=light&tray=0`, { waitUntil: 'load' });
    for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
    await page.waitForSelector('#controls p.tx .tx-hit', { timeout: 20000 });
    for (let i = 0; i < 8; i++) { await page.mouse.move(1250 + (i % 3), 770 + i); await page.waitForTimeout(70); }
    await page.addStyleTag({ content: '#controls p.tx math, #controls p.tx .katex { visibility: hidden !important; }' });
    for (let i = 0; i < 6; i++) { await page.mouse.move(1250 + (i % 3), 770 + i); await page.waitForTimeout(70); }
    const box = await page.evaluate(() => { const r = document.querySelector('#controls p.tx .tx-hit').getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; });
    const png = await page.screenshot({ clip: box });
    // a painted glyph makes the strip uneven; an unpainted one is the flat page colour
    const probe = await page.evaluate(async (b64) => {
      const img = new Image(); img.src = 'data:image/png;base64,' + b64; await img.decode();
      const c = document.createElement('canvas'); c.width = img.width; c.height = img.height;
      const g = c.getContext('2d'); g.drawImage(img, 0, 0);
      const d = g.getImageData(0, 0, c.width, c.height).data; const first = [d[0], d[1], d[2]];
      let diff = 0; for (let i = 0; i < d.length; i += 4) if (Math.abs(d[i] - first[0]) + Math.abs(d[i + 1] - first[1]) + Math.abs(d[i + 2] - first[2]) > 60) diff++;
      return diff;
    }, png.toString('base64'));
    expect(probe === 0, `forced colors: the TeX source over a formula is not painted (${probe} painted pixels)`);
    await ctx.close();
  }

  // 2. a finger drag over the transcript text scrolls the page, as it does over the rest of it (no mouse: egui has
  //    no pointer position for the wheel events the drag is turned into)
  {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 700 }, hasTouch: true, isMobile: true, bypassCSP: true });
    const page = await ctx.newPage();
    await page.goto(`http://127.0.0.1:${port}/?demo=1&theme=light&tray=0`, { waitUntil: 'load' });
    for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
    await page.waitForSelector('#controls p.tx', { timeout: 20000 });
    await page.waitForTimeout(1500);
    const cdp = await ctx.newCDPSession(page);
    const swipe = async (x, y0, y1) => {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y: y0 }] });
      for (let i = 1; i <= 10; i++) { await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y: y0 + (y1 - y0) * i / 10 }] }); await page.waitForTimeout(16); }
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await page.waitForTimeout(600);
    };
    const top = () => page.evaluate(() => document.querySelector('#controls p.tx').getBoundingClientRect().top);
    await swipe(200, 600, 300); await swipe(200, 600, 300);   // bring the text into view (over the scan: works)
    const t0 = await top();
    const over = await page.evaluate(() => !!document.elementFromPoint(200, 420)?.closest('p.tx'));
    await swipe(200, 500, 350);
    const t1 = await top();
    expect(over && t0 < 400 && t1 < t0 - 60, `a finger drag over the text scrolls it: ${Math.round(t0)} -> ${Math.round(t1)} (text under the finger: ${over})`);
    await ctx.close();
  }
} finally {
  await browser?.close();
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall passed');
process.exit(failures.length ? 1 : 0);
