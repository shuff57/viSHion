// Deploy-leak check: serve the repo root the way `wrangler deploy` would publish it (root wrangler.jsonc, whose
// asset directory is crates/app/dist, with dist/.assetsignore) and require that the Rust app is served while the
// old JS app, the Rust sources and the check screenshots are NOT. Run build.mjs first.
// Usage: node crates/app/web/check-assetsignore.mjs
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const wranglerJs = path.join(here, 'node_modules/wrangler/bin/wrangler.js');
const port = +(process.env.PORT || 8790);

const dist = path.join(repo, 'crates/app/dist');
const mustServe = ['index.html', 'favicon.svg', 'vishion_app.js', 'vishion_app_bg.wasm', 'katex.min.js'];
// the site is only what the build wrote: not the old JS app, not the sources, not screenshots from check runs
const mustHide = [
  'js/main.js', 'css/base.css', 'tests/words.test.js', 'Cargo.toml', 'crates/README.md', 'crates/core/src/lib.rs',
  'docs/RUST-REWRITE-PLAN.md', '.github/workflows/rust.yml', 'wrangler.jsonc', 'check.png',
];
// a stray screenshot in dist must stay out (check.mjs and parity.mjs write them there)
fs.writeFileSync(path.join(dist, 'leak-probe.png'), 'x');
mustHide.push('leak-probe.png');

const p = spawn(process.execPath, [wranglerJs, 'dev', '--config', path.join(repo, 'wrangler.jsonc'),
  '--port', String(port), '--ip', '127.0.0.1', '--local'], {
  cwd: repo, detached: process.platform !== 'win32',
  env: { ...process.env, WRANGLER_SEND_METRICS: 'false', CI: '1' },
});
let log = '';
p.stdout.on('data', d => (log += d));
p.stderr.on('data', d => (log += d));
const stop = () => {
  try {
    if (process.platform === 'win32') spawn('taskkill', ['/PID', String(p.pid), '/T', '/F'], { stdio: 'ignore' });
    else process.kill(-p.pid, 'SIGKILL');
  } catch {}
};
const failures = [];
try {
  const t0 = Date.now();
  for (;;) {
    try { if ((await fetch(`http://127.0.0.1:${port}/index.html`)).ok) break; } catch {}
    if (Date.now() - t0 > 120000) throw new Error('wrangler dev (root) did not start\n' + log);
    await new Promise(r => setTimeout(r, 500));
  }
  for (const f of mustServe) {
    const s = (await fetch(`http://127.0.0.1:${port}/${f}`)).status;
    console.log((s === 200 ? 'ok   ' : 'FAIL ') + `served ${f} -> ${s}`);
    if (s !== 200) failures.push(f);
  }
  for (const f of mustHide) {
    const r = await fetch(`http://127.0.0.1:${port}/${f}`);
    const ct = r.headers.get('content-type') || '';
    // a 200 text/html for a non-html path would be an SPA fallback, also a leak of nothing; only real hits fail
    const leaked = r.status === 200 && !(ct.includes('text/html') && !f.endsWith('.html'));
    console.log((leaked ? 'FAIL ' : 'ok   ') + `hidden ${f} -> ${r.status}`);
    if (leaked) failures.push(f);
  }
} catch (e) {
  failures.push(String(e.stack || e));
  console.error(e);
} finally {
  stop();
  fs.rmSync(path.join(dist, 'leak-probe.png'), { force: true });
}
console.log(failures.length ? `\n.assetsignore (deploy leak) check FAILED (${failures.length})` : '\n.assetsignore (deploy leak) check passed');
process.exit(failures.length ? 1 : 0);
