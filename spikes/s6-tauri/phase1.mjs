// Phase 1: origin/storage, wasm->tauri, HTTP (plugin vs window), native fs commands + plugin-fs scope.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { launch, wipe, sleep, save, here, OUT } from './lib.mjs';

const R = {};
wipe();
const mock = spawn('node', [path.join(here, 'mock', 'server.mjs')], { stdio: ['ignore', fs.openSync(path.join(OUT, 'mock.log'), 'w'), 'inherit'] });
await sleep(800);
const lan = Object.values(os.networkInterfaces()).flat().find(i => i.family === 'IPv4' && !i.internal)?.address;
R.lan = lan;

let app = await launch();
const { page } = app;
R.env0 = await page.evaluate(() => s6.env());
R.wasmStatus = await page.textContent('#wasm-status');
await page.evaluate(() => s6.seedStorage());
R.env1 = await page.evaluate(() => s6.env());

// ---- HTTP
const B = 'http://127.0.0.1:8799';
const run = (url, impl, opts) => page.evaluate(([u, i, o]) => s6.sse(u, i, o), [url, impl, opts]);
const summ = r => ({ ...r, chunks: r.chunks.map(c => c.at + 'ms/' + c.n + 'B'), chunkCount: r.chunks.length });
R.http = {};
R.http.pluginLocal = summ(await run(`${B}/v1/chat/completions`, 'plugin'));
R.http.windowNoCors = summ(await run(`${B}/v1/chat/completions`, 'window'));
R.http.windowCors = summ(await run(`${B}/v1/chat/completions?cors=1`, 'window'));
if (lan) {
  R.http.pluginLan = summ(await run(`http://${lan}:8799/v1/chat/completions`, 'plugin'));
  R.http.windowLanNoCors = summ(await run(`http://${lan}:8799/v1/chat/completions`, 'window'));
}
R.http.pluginAbort = summ(await run(`${B}/slow`, 'plugin', { abortAfterMs: 1000 }));
await sleep(1500);
R.http.windowAbortCors = summ(await run(`${B}/slow?cors=1`, 'window', { abortAfterMs: 1000 }));
await sleep(1000);
R.mockLog = fs.readFileSync(path.join(OUT, 'mock.log'), 'utf8');

// ---- native fs via custom commands
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 's6-data-'));
const other = fs.mkdtempSync(path.join(os.tmpdir(), 's6-other-'));
R.fs = { dir };
R.fs.noFolderBefore = await page.evaluate(() => s6.inv('get_folder'));
R.fs.listWithoutFolder = await page.evaluate(() => s6.inv('fs_list', {}).catch(e => 'ERR ' + e));
await page.evaluate(d => s6.inv('set_folder', { path: d }), dir);
R.fs.afterSet = await page.evaluate(() => s6.inv('get_folder'));
R.fs.ops = await page.evaluate(async () => {
  const o = {}, enc = s => new TextEncoder().encode(s);
  const w = (p, s) => s6.inv('fs_write', enc(s), { headers: { 'x-path': p } });
  await w('a.txt', 'v1'); await w('sub/b.json', '{"x":1}'); await w('a.txt', 'v2-overwrite');
  o.list = await s6.inv('fs_list', {});
  o.listSub = await s6.inv('fs_list', { rel: 'sub' });
  o.readA = new TextDecoder().decode(await s6.inv('fs_read', { rel: 'a.txt' }));
  const big = new Uint8Array(5 * 1024 * 1024).fill(7); const t0 = performance.now();
  await w('big.bin', ''); await s6.inv('fs_write', big, { headers: { 'x-path': 'big.bin' } });
  const back = await s6.inv('fs_read', { rel: 'big.bin' });
  o.big5MB = { ms: Math.round(performance.now() - t0), bytes: back.byteLength };
  for (const bad of ['../evil.txt', '..\\evil.txt', 'C:\\Windows\\x.txt', '/abs.txt', 'sub/../../e.txt'])
    o['reject ' + bad] = await w(bad, 'x').then(() => 'ALLOWED', e => 'denied: ' + e);
  await s6.inv('fs_remove', { rel: 'a.txt' });
  o.listAfterRemove = await s6.inv('fs_list', {});
  return o;
});
R.fs.onDisk = fs.readdirSync(dir, { recursive: true });
R.fs.noTempLeft = !R.fs.onDisk.some(n => n.includes('.tmp-'));
R.fs.evilNotCreated = !fs.existsSync(path.join(path.dirname(dir), 'evil.txt'));

// ---- plugin-fs scope: granted via runtime allow_directory (set_folder), denied elsewhere
R.pluginFsInScope = await page.evaluate(d => s6.pluginFs(d.replace(/\\/g, '/')), dir);
R.pluginFsOutOfScope = await page.evaluate(d => s6.pluginFs(d.replace(/\\/g, '/')), other);
save('phase1.json', R);
await app.close();
mock.kill();
fs.writeFileSync(path.join(OUT, 'dirs.json'), JSON.stringify({ dir, other }));
console.log(JSON.stringify(R, null, 1));
