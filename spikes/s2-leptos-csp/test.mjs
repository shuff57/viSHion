// Drives both apps (Leptos/WASM and the current JS app) via `wrangler dev` (local only) and measures.
import { chromium } from 'playwright';
import { spawn, execSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const N = +(process.env.N || 10);
const servers = [];

function serve(config, port) {
  const p = spawn('wrangler', ['dev', '--config', config, '--port', String(port), '--ip', '127.0.0.1', '--local'],
    { cwd: here, shell: true, env: { ...process.env, WRANGLER_SEND_METRICS: 'false', CI: '1' } });
  let log = '';
  p.stdout.on('data', d => log += d);
  p.stderr.on('data', d => log += d);
  servers.push(p);
  return new Promise((res, rej) => {
    const t0 = Date.now();
    const iv = setInterval(async () => {
      try {
        const r = await fetch(`http://127.0.0.1:${port}/`);
        if (r.ok) { clearInterval(iv); res(); }
      } catch {}
      if (Date.now() - t0 > 90000) { clearInterval(iv); rej(new Error('wrangler dev timeout\n' + log)); }
    }, 500);
  });
}
function killAll() {
  for (const p of servers) {
    try { execSync(`taskkill /PID ${p.pid} /T /F`, { stdio: 'ignore' }); } catch {}
  }
}

const pwRoot = path.join(os.homedir(), 'AppData/Local/ms-playwright');
const chromiumDir = fs.readdirSync(pwRoot).filter(d => /^chromium-\d+$/.test(d)).sort().pop();
const exe = fs.readdirSync(path.join(pwRoot, chromiumDir)).map(d => path.join(pwRoot, chromiumDir, d, 'chrome.exe')).find(fs.existsSync);

const median = a => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
const readyScript = (cond) => `
  window.__ready = null; window.__violations = [];
  document.addEventListener('securitypolicyviolation', e => window.__violations.push(e.violatedDirective + ' ' + e.blockedURI));
  const chk = () => { if (window.__ready === null && (${cond})) window.__ready = performance.now(); };
  new MutationObserver(chk).observe(document, { childList: true, subtree: true });
  document.addEventListener('DOMContentLoaded', chk);
`;
const apps = {
  leptos: { port: 8791, ready: `document.querySelector('#inc')` },
  legacy: { port: 8792, ready: `document.querySelector('#topbar') && document.querySelector('#topbar').childElementCount > 0` },
};

async function loadOnce(ctx, url, app) {
  const page = await ctx.newPage();
  const con = [];
  page.on('console', m => con.push(`${m.type()}: ${m.text()}`));
  page.on('pageerror', e => con.push('pageerror: ' + e.message));
  await page.addInitScript(readyScript(apps[app].ready));
  await page.goto(url, { waitUntil: 'load' });
  await page.waitForFunction('window.__ready !== null', null, { timeout: 15000 });
  const m = await page.evaluate(() => {
    const nav = performance.getEntriesByType('navigation')[0];
    const res = performance.getEntriesByType('resource');
    return {
      ready: window.__ready, load: nav.loadEventEnd, dcl: nav.domContentLoadedEventEnd,
      mounted: performance.getEntriesByName('app-mounted')[0]?.startTime ?? null,
      transfer: nav.transferSize + res.reduce((a, r) => a + r.transferSize, 0),
      requests: res.length + 1, violations: window.__violations,
    };
  });
  return { page, con, m };
}

async function measure(browser, app, n) {
  const url = `http://127.0.0.1:${apps[app].port}/`;
  const cold = [], warm = [], coldT = [], warmT = [], coldLoad = [], warmLoad = [];
  let viol = [];
  for (let i = 0; i < n; i++) {                 // cold: brand-new context = empty cache
    const ctx = await browser.newContext();
    const r = await loadOnce(ctx, url, app);
    cold.push(r.m.ready); coldT.push(r.m.transfer); coldLoad.push(r.m.load); viol = viol.concat(r.m.violations);
    await ctx.close();
  }
  const ctx = await browser.newContext();       // warm: same context, cache primed
  await (await loadOnce(ctx, url, app)).page.close();
  for (let i = 0; i < n; i++) {
    const r = await loadOnce(ctx, url, app);
    warm.push(r.m.ready); warmT.push(r.m.transfer); warmLoad.push(r.m.load);
    await r.page.close();
  }
  await ctx.close();
  const f = a => a.map(x => +x.toFixed(0));
  return {
    app, n, cold_ready_ms: median(cold), warm_ready_ms: median(warm),
    cold_load_ms: median(coldLoad), warm_load_ms: median(warmLoad),
    cold_all: f(cold), warm_all: f(warm),
    cold_transfer_bytes: median(coldT), warm_transfer_bytes: median(warmT), cspViolations: viol,
  };
}

function pick(r) {
  if (!r) return null;
  const h = r.headers;
  return { status: r.status, 'content-type': h['content-type'], 'cache-control': h['cache-control'], 'content-encoding': h['content-encoding'] };
}

const out = {};
try {
  await serve('wrangler.jsonc', apps.leptos.port);
  await serve('wrangler.legacy.jsonc', apps.legacy.port);
  const browser = await chromium.launch({ executablePath: exe });

  // ---- functional + CSP + headers (Leptos)
  const ctx = await browser.newContext();
  const respHeaders = {};
  const base = `http://127.0.0.1:${apps.leptos.port}`;
  const page = await ctx.newPage();
  page.on('response', r => { respHeaders[new URL(r.url()).pathname] = { status: r.status(), headers: r.headers() }; });
  const con = [];
  page.on('console', m => con.push(`${m.type()}: ${m.text()}`));
  page.on('pageerror', e => con.push('pageerror: ' + e.message));
  await page.addInitScript(readyScript(apps.leptos.ready));
  await page.addInitScript(() => localStorage.setItem('ocr_settings_v1', '{"theme":"dark"}'));
  await page.goto(base + '/', { waitUntil: 'networkidle' });
  await page.waitForSelector('#inc');
  for (let i = 0; i < 3; i++) await page.click('#inc');
  const count = await page.textContent('#count');
  const stored = await page.textContent('#stored');
  const fontOk = await page.evaluate(async () => {
    await document.fonts.ready;
    return [...document.fonts].some(f => f.family.includes('Caveat') && f.status === 'loaded');
  });
  const violations = await page.evaluate(() => window.__violations);
  const keys = Object.keys(respHeaders);
  const wasmPath = keys.find(p => p.endsWith('.wasm'));
  const jsPath = keys.find(p => p.endsWith('.js'));
  const fontPath = keys.find(p => p.endsWith('.woff2'));
  out.functional = {
    count, stored, fontOk, violations,
    cspHeader: respHeaders['/']?.headers['content-security-policy'],
    cspConsoleErrors: con.filter(c => /content security policy|refused|violat/i.test(c)),
    allConsole: con,
    wasm: { path: wasmPath, ...pick(respHeaders[wasmPath]) },
    js: { path: jsPath, ...pick(respHeaders[jsPath]) },
    font: { path: fontPath, ...pick(respHeaders[fontPath]) },
    index: pick(respHeaders['/']),
    tokensCss: pick(respHeaders['/css/tokens.css']),
  };
  await ctx.close();


  // ---- negative controls: prove the CSP is really enforced (header rewritten in flight)
  out.negative = {};
  const variants = {
    no_wasm_unsafe_eval: c => c.replace(" 'wasm-unsafe-eval'", ''),
    no_inline_hash: c => c.replace(/ 'sha256-(?!1E5ezg)[^']+'/, ''),
    strict_style_no_unsafe_inline: c => c.replace(" 'unsafe-inline'", ''),
  };
  for (const [name, fn] of Object.entries(variants)) {
    const c2 = await browser.newContext();
    const p2 = await c2.newPage();
    const msgs = [];
    p2.on('console', m => msgs.push(m.type() + ': ' + m.text()));
    p2.on('pageerror', e => msgs.push('pageerror: ' + e.message));
    await p2.route(base + '/', async route => {
      const r = await route.fetch();
      const h = { ...r.headers() };
      h['content-security-policy'] = fn(h['content-security-policy']);
      await route.fulfill({ response: r, headers: h });
    });
    await p2.addInitScript(readyScript(apps.leptos.ready));
    await p2.goto(base + '/', { waitUntil: 'load' });
    await p2.waitForTimeout(1500);
    out.negative[name] = {
      mounted: await p2.evaluate(() => !!document.querySelector('#inc')),
      h1Font: await p2.evaluate(() => { const h = document.querySelector('h1'); return h ? getComputedStyle(h).fontFamily : null; }),
      violations: await p2.evaluate(() => window.__violations),
      console: msgs.filter(m => !/integrity|404/.test(m)).slice(0, 3),
    };
    await c2.close();
  }

  // ---- sizes
  const dist = path.join(here, 'dist');
  const wasm = fs.readFileSync(path.join(dist, path.basename(wasmPath)));
  const js = fs.readFileSync(path.join(dist, path.basename(jsPath)));
  const sz = b => ({
    raw: b.length, gzip: zlib.gzipSync(b, { level: 9 }).length,
    brotli: zlib.brotliCompressSync(b, { params: { [zlib.constants.BROTLI_PARAM_QUALITY]: 11 } }).length,
  });
  out.sizes = { wasm: sz(wasm), js_glue: sz(js) };

  // ---- timing
  out.timing = [await measure(browser, 'leptos', N), await measure(browser, 'legacy', N)];
  await browser.close();
} catch (e) { out.error = String(e.stack || e); }
finally { killAll(); }
fs.writeFileSync(path.join(here, 'results.json'), JSON.stringify(out, null, 2));
console.log(JSON.stringify(out, null, 2));
