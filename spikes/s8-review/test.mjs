import { createRequire } from 'module';
import http from 'http'; import fs from 'fs'; import path from 'path';
const require = createRequire(import.meta.url);
const { chromium } = require('../s2-leptos-csp/node_modules/playwright');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };
const srv = http.createServer((q, r) => {
  const f = path.join('dist', q.url === '/' ? 'index.html' : q.url.split('?')[0]);
  fs.readFile(f, (e, d) => { if (e) { r.writeHead(404); r.end(); } else { r.writeHead(200, { 'content-type': types[path.extname(f)] || 'application/octet-stream' }); r.end(d); } });
}).listen(8799);
const b = await chromium.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', args: ['--use-angle=swiftshader','--enable-unsafe-swiftshader','--ignore-gpu-blocklist'] });
const p = await b.newPage({ viewport: { width: 1280, height: 800 } });
const errs = []; p.on('pageerror', e => errs.push(e.message)); p.on('console', m => { if (m.type() === 'error' || m.type() === 'warning') errs.push(m.text().slice(0, 160)); });
await p.goto('http://127.0.0.1:8799/'); await p.waitForFunction('window.__ready === true');
const wiggle = async () => { for (let i = 0; i < 5; i++) { await p.mouse.move(700 + i * 3, 500 + i); await p.waitForTimeout(80); } };
const ids = async () => p.evaluate(() => [...document.querySelectorAll('#controls [data-id]')].map(e => e.dataset.id + (e.hasAttribute('aria-pressed') ? '(' + e.getAttribute('aria-pressed') + ')' : '')));
const out = {};
await wiggle(); await p.waitForTimeout(300);
out.initialControls = await ids();
await p.screenshot({ path: 'shot-0.png' });

// 1. DOM focus then Enter on "Use confidence"
await p.locator('[data-id="use-suggested"]').focus(); await wiggle();
await p.screenshot({ path: 'shot-1-focus.png' });
await p.keyboard.press('Enter'); await wiggle(); await p.waitForTimeout(200);
out.afterUse = await ids();

// 2. DOM: go to illegible word, type through canvas text box
await p.locator('[data-id="word-1"]').evaluate(e => e.click()); await wiggle();
out.atIllegible = await ids();
await p.screenshot({ path: 'shot-2-illegible.png' });
// click the canvas text box (bottom tray, left area) and type
await p.mouse.click(190, 735); await p.waitForTimeout(200);
await p.keyboard.type('hours', { delay: 40 }); await wiggle();
await p.screenshot({ path: 'shot-3-typed.png' });
await p.keyboard.press('Enter'); await wiggle(); await p.waitForTimeout(200);
out.afterType = await ids();

// 3. real pointer click on a canvas word (flagged word in transcript)
await p.mouse.click(300, 260); await wiggle();
await p.screenshot({ path: 'shot-4-final.png' });
out.errs = errs.slice(0, 5);
console.log(JSON.stringify(out, null, 1));
await b.close(); srv.close();
