// Shared helpers: launch the built exe with WebView2 remote debugging, attach via CDP.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

export const here = path.dirname(fileURLToPath(import.meta.url));
export const OUT = path.join(here, 'out');
fs.mkdirSync(OUT, { recursive: true });
export const EXE = process.env.S6_EXE || path.join(here, 'src-tauri', 'target', 'release', 's6-desktop.exe');
export const PORT = 9477;
export const sleep = ms => new Promise(r => setTimeout(r, ms));
export const IDENT = 'app.vishion.s6';
export const webviewDir = path.join(process.env.LOCALAPPDATA, IDENT);
export const cfgDir = path.join(process.env.APPDATA, IDENT);

export function wipe() {
  fs.rmSync(webviewDir, { recursive: true, force: true });
  fs.rmSync(cfgDir, { recursive: true, force: true });
}

export async function launch(extraEnv = {}) {
  const logFile = path.join(OUT, 's6.log');
  const env = {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
    S6_TEST: '1', S6_LOG: logFile, ...extraEnv,
  };
  const child = spawn(EXE, [], { env, stdio: 'ignore' });
  let browser, lastErr;
  for (let i = 0; i < 60; i++) {
    try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`); break; } catch (e) { lastErr = e; await sleep(500); }
  }
  if (!browser) throw new Error('could not attach: ' + lastErr);
  let page;
  for (let i = 0; i < 40 && !page; i++) {
    page = browser.contexts().flatMap(c => c.pages())[0];
    if (!page) await sleep(250);
  }
  await page.waitForFunction(() => window.s6 && window.__TAURI__ && document.readyState === 'complete');
  return {
    child, browser, page, logFile,
    async close() {
      try { await browser.close(); } catch {}
      try { execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); } catch {}
      await sleep(1500);
    },
  };
}

export function ps(script, ...args) {
  return execFileSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'tools', script), ...args.map(String)], { encoding: 'utf8' }).trim();
}
export function save(name, obj) { fs.writeFileSync(path.join(OUT, name), JSON.stringify(obj, null, 2)); }

export function psAsync(script, ...args) {
  return new Promise((res, rej) => {
    const c = spawn('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'tools', script), ...args.map(String)]);
    const to = setTimeout(() => c.kill(), 45000); let o = ''; c.stdout.on('data', d => (o += d)); c.stderr.on('data', d => (o += d));
    c.on('close', () => { clearTimeout(to); res(o.trim()); }); c.on('error', rej);
  });
}
