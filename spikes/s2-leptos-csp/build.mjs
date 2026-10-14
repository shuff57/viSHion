// Build the Leptos app with Trunk, then write dist/_headers with the repo CSP adapted for WASM.
// Also stage ./dist-legacy (current JS app + its unmodified _headers) for comparison.
//
// Trunk builds into an OS temp dir first: on this Windows box Trunk's final
// "rename .stage/css -> dist/css" fails with "Access is denied" when dist is inside the repo.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');
const dist = path.join(here, 'dist');
const tmp = path.join(os.tmpdir(), 's2-trunk-dist');

fs.rmSync(tmp, { recursive: true, force: true });
fs.rmSync(dist, { recursive: true, force: true });
execFileSync('trunk', ['build', '--release', '--dist', tmp], { cwd: here, stdio: 'inherit', shell: true });
fs.cpSync(tmp, dist, { recursive: true });
fs.rmSync(path.join(dist, 'fonts', '_workspace'), { recursive: true, force: true });

// --- CSP: repo header + 'wasm-unsafe-eval' + hash of every inline script in dist/index.html
const html = fs.readFileSync(path.join(dist, 'index.html'), 'utf8');
const hashes = [];
for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
  if (/\bsrc\s*=/.test(m[1])) continue;               // external, covered by 'self'
  hashes.push("'sha256-" + createHash('sha256').update(m[2], 'utf8').digest('base64') + "'");
}
const repoHeaders = fs.readFileSync(path.join(repo, '_headers'), 'utf8');
let csp = repoHeaders.match(/Content-Security-Policy:\s*(.+)/)[1].trim();
csp = csp.replace(/script-src ([^;]*)/, (_, v) => `script-src ${v} 'wasm-unsafe-eval' ${hashes.join(' ')}`);
if (process.env.S2_STRICT_STYLE) csp = csp.replace(" 'unsafe-inline'", '');   // experiment only
const headers = `/*
  X-Content-Type-Options: nosniff
  Referrer-Policy: no-referrer
  Permissions-Policy: camera=(), microphone=(), geolocation=()
  Content-Security-Policy: ${csp}

/fonts/*
  Cache-Control: public, max-age=31536000, immutable

# Trunk emits content-hashed filenames, so JS/WASM can be immutable. HTML must revalidate.
/*.wasm
  Cache-Control: public, max-age=31536000, immutable
/*.js
  Cache-Control: public, max-age=31536000, immutable
/index.html
  Cache-Control: public, max-age=0, must-revalidate
`;
fs.writeFileSync(path.join(dist, '_headers'), headers);
console.log('inline script hashes:', hashes.join(' '));

// --- legacy app copy for comparison (same _headers as production)
const leg = path.join(here, 'dist-legacy');
fs.rmSync(leg, { recursive: true, force: true });
fs.mkdirSync(leg);
for (const f of ['index.html', '_headers', 'favicon.svg', 'logo.svg']) fs.copyFileSync(path.join(repo, f), path.join(leg, f));
for (const d of ['css', 'js']) fs.cpSync(path.join(repo, d), path.join(leg, d), { recursive: true });
fs.cpSync(path.join(repo, 'fonts'), path.join(leg, 'fonts'), { recursive: true });
fs.rmSync(path.join(leg, 'fonts', '_workspace'), { recursive: true, force: true });
