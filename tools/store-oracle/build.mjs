// Builds the wasm harness: cargo (wasm32, release) -> wasm-bindgen --target web -> tools/store-oracle/pkg.
// Needs wasm-bindgen-cli matching Cargo.lock (0.2.129). Usage: node tools/store-oracle/build.mjs
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');
const run = (cmd, args) => execFileSync(cmd, args, { cwd: repo, stdio: 'inherit' });
run('cargo', ['build', '-p', 'vishion-store-oracle', '--lib', '--target', 'wasm32-unknown-unknown', '--release']);
const pkg = path.join(here, 'pkg');
fs.rmSync(pkg, { recursive: true, force: true });
run('wasm-bindgen', ['--target', 'web', '--no-typescript', '--out-dir', pkg,
  path.join(repo, 'target/wasm32-unknown-unknown/release/vishion_store_oracle.wasm')]);
console.log('built', pkg);
