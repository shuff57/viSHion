// Trunk -> OS tmp dir (Windows "Access is denied" gotcha, see s2 README), then copy to ./dist
import { execFileSync } from 'node:child_process';
import fs from 'node:fs'; import os from 'node:os'; import path from 'node:path';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const tmp = path.join(os.tmpdir(), 's6-trunk-dist');
fs.rmSync(tmp, { recursive: true, force: true });
fs.rmSync(path.join(here, 'dist'), { recursive: true, force: true });
const args = ['build', ...(process.env.S6_DEBUG ? [] : ['--release']), '--dist', tmp];
execFileSync('trunk', args, { cwd: here, stdio: 'inherit', shell: true });
fs.cpSync(tmp, path.join(here, 'dist'), { recursive: true });
