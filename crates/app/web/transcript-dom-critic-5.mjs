// Blind-critic checks on the DOM transcript (round 5): Select-all + copy yields the page's text, not the loader.
// Usage: node crates/app/web/transcript-dom-critic-5.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8797));
let browser;
try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, bypassCSP: true, permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await ctx.newPage();
  await page.goto(`http://127.0.0.1:${port}/?demo=1&theme=light&tray=0`, { waitUntil: 'load' });
  for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
  await page.waitForSelector('#controls p.tx', { timeout: 20000 });
  for (let i = 0; i < 8; i++) { await page.mouse.move(1250 + (i % 3), 770 + i); await page.waitForTimeout(70); }
  await page.bringToFront();
  await page.evaluate(() => navigator.clipboard.writeText('SEED'));
  await page.mouse.click(1000, 600);
  await page.keyboard.press('Control+a');
  await page.keyboard.press('Control+c');
  await page.waitForTimeout(300);
  const clip = await page.evaluate(() => navigator.clipboard.readText());
  expect(/confidance interval/.test(clip), 'Ctrl+A, Ctrl+C copies the transcript: ' + JSON.stringify(clip.slice(0, 30)));
  expect(!/import init|await start\(|needs JavaScript/.test(clip), 'and nothing of the loader script or its noscript text: ' + JSON.stringify((clip.match(/viSHion needs[\s\S]{0,60}/) || [''])[0]));
  await ctx.close();
} finally {
  if (browser) await browser.close();
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall passed');
process.exit(failures.length ? 1 : 0);
