// Serve crates/app/dist with `wrangler dev` (local mode) so the response headers are the real _headers CSP.
// Usage: node crates/app/web/serve.mjs [port]   (default 8788; run build.mjs first)
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const wranglerJs = path.join(here, 'node_modules/wrangler/bin/wrangler.js');

/** Start wrangler dev; resolves { port, stop } once / answers 200. */
export async function serve(port = 8788) {
  const p = spawn(process.execPath, [wranglerJs, 'dev', '--config', path.join(here, 'wrangler.jsonc'),
    '--port', String(port), '--ip', '127.0.0.1', '--local'], {
    cwd: here, detached: process.platform !== 'win32',
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
  const t0 = Date.now();
  for (;;) {
    try {
      if ((await fetch(`http://127.0.0.1:${port}/`)).ok) return { port, stop };
    } catch {}
    if (Date.now() - t0 > 90000) { stop(); throw new Error('wrangler dev did not start\n' + log); }
    await new Promise(r => setTimeout(r, 500));
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { port } = await serve(+(process.argv[2] || 8788));
  console.log(`serving crates/app/dist at http://127.0.0.1:${port}/ (ctrl+c to stop)`);
}
