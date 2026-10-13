// Build the web bundle into crates/app/dist:
//   cargo build (wasm32, release) -> wasm-bindgen --target web -> index.html + _headers (CSP with hash).
// Usage: node crates/app/web/build.mjs [--debug]
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const dist = path.resolve(here, '../dist');
const debug = process.argv.includes('--debug');
const profile = debug ? 'debug' : 'release';
const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { cwd: repo, stdio: 'inherit', ...opts });

// Web clipboard (copy) needs `--cfg=web_sys_unstable_apis`; .cargo/config.toml sets it for wasm32, but an
// explicit RUSTFLAGS in the environment replaces that setting, so make sure it is present either way.
const CFG = '--cfg=web_sys_unstable_apis';
const rustflags = process.env.RUSTFLAGS && !process.env.RUSTFLAGS.includes(CFG) ? `${process.env.RUSTFLAGS} ${CFG}` : process.env.RUSTFLAGS;
run('cargo', ['build', '-p', 'vishion-app', '--lib', '--target', 'wasm32-unknown-unknown', ...(debug ? [] : ['--release'])],
  rustflags ? { env: { ...process.env, RUSTFLAGS: rustflags } } : {});

// clear the folder's contents, not the folder: a shell or editor with it as its working directory (EPERM on Windows)
fs.mkdirSync(dist, { recursive: true });
for (const f of fs.readdirSync(dist)) fs.rmSync(path.join(dist, f), { recursive: true, force: true });
run('wasm-bindgen', [
  '--target', 'web', '--no-typescript', '--out-dir', dist,
  path.join(repo, 'target/wasm32-unknown-unknown', profile, 'vishion_app.wasm'),
]);
// Size: wasm-opt -Oz (binaryen via the `wasm-opt` npm package) over wasm-bindgen's output. Skipped with
// --no-opt (a quick debug loop) or when the binary is not installed (`npm ci` in this folder).
const wasmFile = path.join(dist, 'vishion_app_bg.wasm');
const optBin = path.join(here, 'node_modules', 'wasm-opt', 'bin', process.platform === 'win32' ? 'wasm-opt.exe' : 'wasm-opt');
const before = fs.statSync(wasmFile).size;
if (!debug && !process.argv.includes('--no-opt') && fs.existsSync(optBin)) {
  const tmp = wasmFile + '.opt';
  run(optBin, ['-Oz', '--enable-bulk-memory', '--enable-sign-ext', '--enable-nontrapping-float-to-int', '--enable-mutable-globals',
    '--enable-multivalue', '--enable-reference-types', '--strip-debug', '--strip-producers', wasmFile, '-o', tmp]);
  fs.renameSync(tmp, wasmFile);
  console.log(`wasm-opt: ${before} -> ${fs.statSync(wasmFile).size} bytes`);
}
// The math font is not linked into the wasm (it would be 840 KB of the download): ui::math asks the host for
// it the first time a page has a formula, and the host fetches this file.
fs.copyFileSync(path.join(repo, 'crates/vendor/latex-rust/fonts/stix-two-math/STIXTwoMath-Regular.otf'), path.join(dist, 'stix-two-math.otf'));
// Same for the CJK / Hangul / emoji fallback face (4 MB): fetched only when a page has such text.
fs.copyFileSync(path.join(repo, 'crates/ui/assets/fonts/fallback-cjk-emoji.ttf'), path.join(dist, 'fallback-cjk-emoji.ttf'));
// And the colour emoji face (1.4 MB): fetched only when a page has an emoji.
fs.copyFileSync(path.join(repo, 'crates/ui/assets/fonts/twemoji-colr.ttf'), path.join(dist, 'twemoji-colr.ttf'));
// The transcript is DOM text (a user stylesheet and text spacing apply to it): the JS app's own faces, and KaTeX,
// which typesets formulas as MathML. Fetched when the page has a formula.
fs.mkdirSync(path.join(dist, 'fonts'), { recursive: true });
for (const f of fs.readdirSync(path.join(repo, 'fonts')).filter((n) => /^(instrument-sans|source-serif-4).*\.woff2$/.test(n))) {
  fs.copyFileSync(path.join(repo, 'fonts', f), path.join(dist, 'fonts', f));
}
fs.copyFileSync(path.join(repo, 'crates/ui/assets/fonts/hack-latin-400-normal.ttf'), path.join(dist, 'fonts/hack-latin-400-normal.ttf'));
fs.copyFileSync(path.join(repo, 'spikes/s5-markdown-katex/vendor/katex-0.16.9/dist/katex.min.js'), path.join(dist, 'katex.min.js'));
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

/stix-two-math.otf
  Cache-Control: public, max-age=31536000, immutable

/katex.min.js
  Cache-Control: public, max-age=31536000, immutable

/fonts/*
  Cache-Control: public, max-age=31536000, immutable

/fallback-cjk-emoji.ttf
  Cache-Control: public, max-age=31536000, immutable

/twemoji-colr.ttf
  Cache-Control: public, max-age=31536000, immutable
`;
fs.writeFileSync(path.join(dist, '_headers'), headers);
const wasm = fs.statSync(path.join(dist, 'vishion_app_bg.wasm')).size;
const gz = zlib.gzipSync(fs.readFileSync(path.join(dist, 'vishion_app_bg.wasm')), { level: 9 }).length;
const font = fs.statSync(path.join(dist, 'stix-two-math.otf')).size;
console.log(`dist: ${dist}\nwasm: ${wasm} bytes (${gz} gzip), math font ${font} bytes fetched on first formula\ninline script hashes: ${hashes.join(' ')}`);
// Plan risk "wasm size": about 4 MB and 2 MB gzipped. A release build over budget fails (not --debug / --no-opt).
const BUDGET = { raw: 4_500_000, gz: 2_000_000 };
if (!debug && !process.argv.includes('--no-opt') && (wasm > BUDGET.raw || gz > BUDGET.gz)) {
  console.error(`wasm over budget: ${wasm} bytes (max ${BUDGET.raw}), ${gz} gzip (max ${BUDGET.gz})`);
  process.exit(1);
}
