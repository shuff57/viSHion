// Phase 2: CSP-free window.fetch comparison, native dialogs (driven by SendKeys), persisted scope, restart persistence,
// File System Access API in WebView2, anchor download.
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { launch, sleep, save, here, OUT, psAsync, ps } from './lib.mjs';

const R = {};
const mock = spawn('node', [path.join(here, 'mock', 'server.mjs')], { stdio: ['ignore', fs.openSync(path.join(OUT, 'mock2.log'), 'w'), 'inherit'] });
await sleep(800);
const mk = n => fs.mkdtempSync(path.join(os.tmpdir(), n));
const dir1 = mk('s6-jsdlg-'), dir2 = mk('s6-cmddlg-'), dir3 = mk('s6-fsa-');
fs.writeFileSync(path.join(dir3, 'hello.txt'), 'x');
const withTimeout = (p, ms, label) => Promise.race([p, sleep(ms).then(() => ({ TIMEOUT: label }))]);
const typeInto = async (page, evalFn, text, label) => {
  const d = psAsync('dialog.ps1', text, 3000);
  const r = await withTimeout(evalFn(), 25000, label).catch(e => ({ ERR: String(e) }));
  R['dialogDriver_' + label] = await withTimeout(d, 10000, 'drv');
  return r;
};

let app = await launch();
let { page } = app;
await page.evaluate(() => s6.seedStorage());

// --- 1. window.fetch with CSP bypassed, to see pure CORS / network behaviour
const cdp = await page.context().newCDPSession(page);
await cdp.send('Page.setBypassCSP', { enabled: true });
await page.reload(); await page.waitForFunction(() => window.s6);
const B = 'http://127.0.0.1:8799';
const run = (url, impl, opts) => page.evaluate(([u, i, o]) => s6.sse(u, i, o), [url, impl, opts]);
const summ = r => ({ ...r, chunks: r.chunks.map(c => c.at + 'ms/' + c.n + 'B') });
R.windowFetch = {
  noCors: summ(await run(`${B}/v1/chat/completions`, 'window')),
  cors: summ(await run(`${B}/v1/chat/completions?cors=1`, 'window')),
  corsAbort: summ(await run(`${B}/slow?cors=1`, 'window', { abortAfterMs: 1000 })),
};
await sleep(800);
await cdp.send('Page.setBypassCSP', { enabled: false });
await page.reload(); await page.waitForFunction(() => window.s6);

// --- 2. JS dialog plugin: open({directory:true}) -> auto scope grant
R.jsDialog = await typeInto(page, () => page.evaluate(() => s6.jsDialogPick()), dir1, 'jsdialog');
R.jsDialogPluginFs = await page.evaluate(d => s6.pluginFs(d.replace(/\\/g, '/')), dir1);
R.persistedScopeFile = fs.existsSync(path.join(process.env.APPDATA, 'app.vishion.s6', '.persisted-scope'));

// --- 3. Rust command pick_folder (dialog from Rust) + remember
R.cmdPick = await typeInto(page, () => page.evaluate(() => s6.inv('pick_folder')), dir2, 'cmdpick');
R.getFolder = await page.evaluate(() => s6.inv('get_folder'));

// --- 4. save_export (save dialog + write)
const savePath = path.join(OUT, 'exported.json');
fs.rmSync(savePath, { force: true });
R.saveExport = await typeInto(page, () => page.evaluate(() => {
  const b = new TextEncoder().encode(JSON.stringify({ exported: true, n: 1 }));
  return s6.inv('save_export', b, { headers: { 'x-name': 'vishion-export.json' } });
}), savePath, 'save');
R.saveExportContent = fs.existsSync(savePath) ? fs.readFileSync(savePath, 'utf8') : null;

// --- 5. showDirectoryPicker (File System Access API)
const fsaExpr = `(async()=>{const h=await window.showDirectoryPicker({mode:'readwrite'});const o={name:h.name};
  try{o.perm=await h.queryPermission({mode:'readwrite'});}catch(e){o.permErr=String(e)}
  try{const n=[];for await(const [k] of h.entries())n.push(k);o.entries=n;}catch(e){o.entriesErr=String(e)}
  try{const f=await h.getFileHandle('fsa-write.txt',{create:true});const w=await f.createWritable();await w.write('hi');await w.close();o.write='ok';}catch(e){o.writeErr=String(e)}
  return o;})()`;
R.fsaPicker = await typeInto(page, async () => {
  const r = await cdp.send('Runtime.evaluate', { expression: fsaExpr, awaitPromise: true, userGesture: true, returnByValue: true });
  return r.result.value ?? r.exceptionDetails?.exception?.description ?? r;
}, dir3, 'fsa');
R.fsaFileOnDisk = fs.existsSync(path.join(dir3, 'fsa-write.txt'));
// without user gesture:
R.fsaNoGesture = await page.evaluate(async () => { try { await window.showDirectoryPicker(); return 'opened'; } catch (e) { return e.name + ': ' + e.message; } });

// --- 6. anchor download
const dl = path.join(os.homedir(), 'Downloads');
const dlName = 's6-export-' + Date.now() + '.txt';
await page.evaluate(n => s6.anchorDownload(n), dlName);
await sleep(3000);
R.anchorDownload = {
  name: dlName, inDownloads: fs.existsSync(path.join(dl, dlName)),
  windowsAfter: (await psAsync('windows.ps1', 's6-desktop')).split('\n'),
  windowsWebview: (await psAsync('windows.ps1', 'msedgewebview2')).split('\n').filter(l => !/Chrome_|^$/.test(l)).slice(0, 8),
};
if (!R.anchorDownload.inDownloads) {
  R.anchorDownload.recentDownloads = fs.readdirSync(dl).filter(f => f.includes('s6-export'));
}
await page.keyboard.press('Escape');
save('phase2a.json', R);
await app.close();

// --- restart persistence; plugin-fs scope from persisted-scope only
app = await launch({ S6_NO_REALLOW: '1' });
page = app.page;
R.restart = {
  getFolder: await page.evaluate(() => s6.inv('get_folder')),
  env: await page.evaluate(() => s6.env()).then(e => ({ origin: e.origin, ls: e.localStorageKeys, idb: e.idbDatabases })),
  pluginFsDir1_persistedScopeOnly: await page.evaluate(d => s6.pluginFs(d.replace(/\\/g, '/')), dir1),
  pluginFsDir2_persistedScopeOnly: await page.evaluate(d => s6.pluginFs(d.replace(/\\/g, '/')), dir2),
  fsCmdListDir2: await page.evaluate(() => s6.inv('fs_list', {})),
};
await app.close();
mock.kill();
R.dirs = { dir1, dir2, dir3 };
save('phase2.json', R);
console.log(JSON.stringify(R, null, 1));
