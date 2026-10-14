// Blind critic phase 2 round 7 fixture. Different seed/atoms from gen_blind2.mjs. Usage: node gen_blind6.mjs
import fs from 'node:fs';
let s = 0xC0FFEE11 >>> 0;
const rnd = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
const ri = (n) => Math.floor(rnd() * n);
const pick = (a) => a[ri(a.length)];
const BAD = ['\ud800', '\udbff', '\udc00', '\udfff', '\ud83d', '\ude00'];
const ATOMS = ['q', 'W', ' ', '\n', '\r\n', '\t', '"', '\\', '/', '\u0000', '\u0008', '\u000c', '\u001e', '\u007f', '\u0080', ' ', ' ',
  '﻿', '�', 'ñ', 'ß', 'Ω', '汉字', '👩‍👩‍👧', '🇯🇵', '\u{10FFFF}', '\u{10FFFE}', '\u{10FFFD}', '\u{F0000}', '\u{E000}', '\u{FFFF}',
  'd800', 'D800', 'dfff', '\\ud800', '\\\\ud800', '$&', '${x}', '</script>', '%2e%2e', '..', 'NUL', '\u{10FFFE}d800', '\u{10FFFE}dc00', '\u{10FFFE}\u{10FFFE}', ...BAD, ...BAD];
const str = (n = 14) => { let o = ''; const k = ri(n); for (let i = 0; i < k; i++) o += pick(ATOMS); return o; };
const noBad = () => { let o = ''; const k = ri(10); const A = ATOMS.filter((a) => !/[\ud800-\udfff]/.test(a)); for (let i = 0; i < k; i++) o += pick(A); return o; };
const NUMS = [0, 1, 1.5, 100, 1e21, 1e-7, 123456789012, 2 ** 31, 2 ** 53 - 1, 0.1 + 0.2, 1 / 3, 1e300, -1e-300, 4.9e-324, 12345.6789e5];
const KEYS = ['id', 'raw', 'x', 'constructor', 'toString', 'valueOf', '0', '1', '9', '123', '4294967294', '4294967295', '4294967296', '00', '-0', '1.5', '1e3', '', ' ', 'é', '\ud800', '\u{10FFFE}', '\u{10FFFE}d800', 'a.b', 'edits', 'boxes'];
function val(d) {
  const r = rnd();
  if (d > 3 || r < 0.3) return str();
  if (r < 0.5) return pick(NUMS);
  if (r < 0.56) return null;
  if (r < 0.62) return rnd() < 0.5;
  if (r < 0.8) { const a = []; for (let i = ri(5); i > 0; i--) a.push(val(d + 1)); return a; }
  const o = {}; for (let i = ri(5); i > 0; i--) o[pick(KEYS)] = val(d + 1); return o;
}
const pages = [];
for (let i = 0; i < 500; i++) {
  const p = { id: 'g' + i, name: str(), order: i, status: pick(['done', 'pending', 'transcribing', 'error']), raw: str(), corrected: str(), corrections: val(1), edits: [{ text: str(), ts: pick(NUMS) }], boxes: [], rotation: pick([0, 90, 180, 270]) };
  for (let j = ri(5); j > 0; j--) p[pick(KEYS)] = val(0);
  const text = JSON.stringify(p);
  const edit = noBad();
  const q0 = JSON.parse(text); if (q0.status === 'transcribing') q0.status = 'pending'; const afterOpen = JSON.stringify(q0); const q = q0; q.corrected = edit;
  pages.push({ id: 'g' + i, text, afterOpen, edit, afterEdit: JSON.stringify(q) });
}
const metas = [];
for (let i = 0; i < 300; i++) {
  const m = {};
  for (let j = ri(9); j > 0; j--) m[pick(['name', 'order', 'schemaVersion', 'topic', 'prompt', 'ui', 'stats', 'created', 'updated', 'future', 'extra', ...KEYS])] = pick([val(0), str(), pick(NUMS), null, [], {}, ['a', 'b'], { pages: pick(NUMS), done: pick(NUMS) }]);
  if (rnd() < 0.4) m.order = [];
  const text = rnd() < 0.5 ? JSON.stringify(m, null, 2) : JSON.stringify(m);
  const parsed = JSON.parse(text);
  const norm = Object.assign({ schemaVersion: 1, name: 'Untitled', topic: 'General', prompt: '', order: [], created: 7, updated: 7, ui: {}, stats: null }, parsed);
  const renamed = Object.assign({}, norm, { name: 'rené' + i, updated: 99 });
  metas.push({ text, normPretty: JSON.stringify(norm, null, 2), renamedPretty: JSON.stringify(renamed, null, 2) });
}
fs.writeFileSync(new URL('./blind6.json', import.meta.url), JSON.stringify({ pages, metas }));
console.log(pages.length, metas.length);
