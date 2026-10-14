import { createRequire } from 'module';
import http from 'http'; import fs from 'fs'; import path from 'path';
const require = createRequire(import.meta.url);
const { chromium } = require('../s2-leptos-csp/node_modules/playwright');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };
const srv = http.createServer((q, r) => {
  const f = path.join('dist', q.url === '/' ? 'index.html' : q.url.split('?')[0]);
  fs.readFile(f, (e, d) => { if (e) { r.writeHead(404); r.end(); } else { r.writeHead(200, { 'content-type': types[path.extname(f)] || 'application/octet-stream' }); r.end(d); } });
}).listen(8798);
const b = await chromium.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', args: ['--use-angle=swiftshader','--enable-unsafe-swiftshader','--ignore-gpu-blocklist'] });
const p = await b.newPage({ viewport: { width: 1100, height: 760 } });
await p.goto('http://127.0.0.1:8798/');
await p.waitForFunction('window.__ready === true');
for (let i = 0; i < 6; i++) { await p.mouse.move(600 + i * 7, 400); await p.waitForTimeout(150); }
console.log('--- initial'); console.log(await p.locator('body').ariaSnapshot());
await p.mouse.click(60, 598);
for (let i = 0; i < 6; i++) { await p.mouse.move(600 + i * 7, 400); await p.waitForTimeout(150); }
console.log('--- after selecting DEMO#0'); console.log(await p.locator('#a11y').ariaSnapshot());
console.log('--- keyboard: Tab focus target'); await p.keyboard.press('Tab'); console.log(await p.evaluate(() => document.activeElement && (document.activeElement.tagName + '#' + document.activeElement.id)));
await b.close(); srv.close();
