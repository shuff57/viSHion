// Blind-critic round 13: failing checks for gaps found by a keyboard-only walk (not part of `npm run a11y`).
//   node crates/app/web/a11y-critic-r13.mjs      (exit 1 while the gaps are open)
import { chromium } from 'playwright-core';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import { serve } from './serve.mjs';
const require = createRequire(import.meta.url);
const axeSource = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const { port, stop } = await serve(8793);
const browser = await chromium.launch({ executablePath: process.env.CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe',
  args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 }, bypassCSP: true })).newPage();
await page.goto(`http://127.0.0.1:${port}/?demo=1&theme=light`);
for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true)); i++) await page.waitForTimeout(100);
const pump = async () => { for (let i = 0; i < 5; i++) { await page.mouse.move(1250 + (i % 3), 770 + i); await page.waitForTimeout(70); } };
await pump();
const fails = [];
const expect = (ok, m) => { console.log((ok ? 'ok   ' : 'FAIL ') + m); if (!ok) fails.push(m); };
const say = () => page.evaluate(() => document.getElementById('announce').textContent + '|' + document.getElementById('announce-alert').textContent);
const tabTo = async (id) => { await page.mouse.click(640, 3); for (let i = 0; i < 90; i++) { await page.keyboard.press('Tab'); if (await page.evaluate(() => document.activeElement?.dataset?.id) === id) return true; } return false; };

// 1. plain-text view: axe serious scrollable-region-focusable
await tabTo('text-view'); await page.keyboard.press('Enter'); await pump(); await pump();
await page.evaluate(axeSource);
const v = await page.evaluate(async () => (await window.axe.run(document, {})).violations.filter(x => x.impact === 'serious' || x.impact === 'critical').map(x => x.id));
expect(v.length === 0, 'axe: no serious/critical in the plain-text view: ' + v);
await tabTo('text-view'); await page.keyboard.press('Enter'); await pump();

// 2. removing a page is announced
await tabTo('page-p-maya-2'); await page.keyboard.press('Enter'); await pump();
const before = await say();
await tabTo('remove'); await page.keyboard.press('Enter'); await pump();
await tabTo('dialog-ok'); await page.keyboard.press('Enter'); await pump(); await pump();
const after = await say();
expect(after !== before && /remov/i.test(after), 'removing a page says so: ' + JSON.stringify(after));

// 3. "Use all suggestions" can be undone with Ctrl+Z
await tabTo('word-p-maya-1-c:0'); await page.keyboard.press('Enter'); await pump();
await tabTo('use-all'); await page.keyboard.press('Enter'); await pump();
await page.keyboard.press('Control+z'); await pump();
expect(!/Nothing to undo/.test(await say()), 'Ctrl+Z after "Use all" undoes it: ' + JSON.stringify(await say()));
await stop(); await browser.close();
process.exit(fails.length ? 1 : 0);
