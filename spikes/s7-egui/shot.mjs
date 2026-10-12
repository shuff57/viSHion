import { createRequire } from 'module';
import http from 'http'; import fs from 'fs'; import path from 'path';
const require = createRequire(import.meta.url);
const { chromium } = require('../s2-leptos-csp/node_modules/playwright');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };
const srv = http.createServer((q, r) => {
  const f = path.join('dist', q.url === '/' ? 'index.html' : q.url.split('?')[0]);
  fs.readFile(f, (e, d) => { if (e) { r.writeHead(404); r.end(); } else { r.writeHead(200, { 'content-type': types[path.extname(f)] || 'application/octet-stream' }); r.end(d); } });
}).listen(8797);
const b = await chromium.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', args: ['--use-angle=swiftshader','--enable-unsafe-swiftshader','--ignore-gpu-blocklist','--enable-webgl'] });
const p = await b.newPage({ viewport: { width: 1100, height: 760 } });
const logs = []; p.on('console', m => logs.push(m.type() + ': ' + m.text())); p.on('pageerror', e => logs.push('pageerror: ' + e.message));
const t0 = Date.now();
await p.goto('http://127.0.0.1:8797/');
await p.waitForFunction('window.__ready === true', null, { timeout: 60000 });
const loadMs = Date.now() - t0;
await p.waitForTimeout(4000); logs.push('webgl2=' + await p.evaluate(() => !!document.createElement('canvas').getContext('webgl2')));
// pick a math-heavy transcript: click list entries until one with math is shown; just shoot a few
await p.bringToFront(); for (let i = 0; i < 10; i++) { await p.mouse.move(300 + i * 5, 300); await p.waitForTimeout(100); }
await p.screenshot({ path: 'out-0.png' });
for (const [n, y] of [["demo0",598],["demo1",640],["syn01",724]]) { await p.mouse.click(60, y); for (let i = 0; i < 6; i++) { await p.mouse.move(600 + i * 7, 400); await p.waitForTimeout(120); } await p.screenshot({ path: "out-" + n + ".png" }); }
const dbg = await p.evaluate(() => { const c = document.getElementById('c'); return { w: c.width, h: c.height, cw: c.clientWidth, ch: c.clientHeight, nonzero: (() => { try { const t = document.createElement('canvas'); t.width = c.width; t.height = c.height; const x = t.getContext('2d'); x.drawImage(c, 0, 0); const d = x.getImageData(0, 0, t.width, t.height).data; let n = 0; for (let i = 0; i < d.length; i += 4) if (d[i] !== 255 || d[i+1] !== 255 || d[i+2] !== 255) n++; return n; } catch (e) { return String(e); } })() }; }); logs.push(JSON.stringify(dbg));
const a11y = await p.evaluate(() => ({ domNodes: document.querySelectorAll('*').length, ariaNodes: document.querySelectorAll('[role],[aria-label]').length, inputs: document.querySelectorAll('input,textarea,[contenteditable]').length }));
console.log(JSON.stringify({ loadMs, a11y, logs: logs.slice(0, 20) }));
await b.close(); srv.close();
