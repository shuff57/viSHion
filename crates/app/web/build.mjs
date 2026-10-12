// Build the web bundle into crates/app/dist:
//   cargo build (wasm32, release) -> wasm-bindgen --target web -> index.html + _headers (CSP with hash).
// Usage: node crates/app/web/build.mjs [--debug]
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const dist = path.resolve(here, '../dist');
const debug = process.argv.includes('--debug');
const profile = debug ? 'debug' : 'release';
const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { cwd: repo, stdio: 'inherit', ...opts });

run('cargo', ['build', '-p', 'vishion-app', '--lib', '--target', 'wasm32-unknown-unknown', ...(debug ? [] : ['--release'])]);

fs.rmSync(dist, { recursive: true, force: true });
fs.mkdirSync(dist, { recursive: true });
run('wasm-bindgen', [
  '--target', 'web', '--no-typescript', '--out-dir', dist,
  path.join(repo, 'target/wasm32-unknown-unknown', profile, 'vishion_app.wasm'),
]);
fs.copyFileSync(path.join(here, 'index.html'), path.join(dist, 'index.html'));
fs.copyFileSync(path.join(repo, 'favicon.svg'), path.join(dist, 'favicon.svg'));

// CSP = the repo's production policy + 'wasm-unsafe-eval' + sha256 of every inline script in index.html.
const html = fs.readFileSync(path.join(dist, 'index.html'), 'utf8');
const hashes = [];
for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
  if (/\bsrc\s*=/.test(m[1])) continue; // external, covered by 'self'
  hashes.push("'sha256-" + createHash('sha256').update(m[2], 'utf8').digest('base64') + "'");
}
const repoHeaders = fs.readFileSync(path.join(repo, '_headers'), 'utf8');
let csp = repoHeaders.match(/Content-Security-Policy:\s*(.+)/)[1].trim();
csp = csp.replace(/script-src ([^;]*)/, (_, v) => `script-src ${v} 'wasm-unsafe-eval' ${hashes.join(' ')}`);
const headers = `/*
  X-Content-Type-Options: nosniff
  Referrer-Policy: no-referrer
  Permissions-Policy: camera=(), microphone=(), geolocation=()
  Content-Security-Policy: ${csp}

/index.html
  Cache-Control: public, max-age=0, must-revalidate
`;
fs.writeFileSync(path.join(dist, '_headers'), headers);
const wasm = fs.statSync(path.join(dist, 'vishion_app_bg.wasm')).size;
console.log(`dist: ${dist}\nwasm: ${wasm} bytes\ninline script hashes: ${hashes.join(' ')}`);
