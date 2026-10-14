// Generates blind2.json: JS-written documents (JSON.stringify) and what the JS app would write after a
// load/save cycle. Seeded differently from the builder's fixtures. Usage: node gen_blind2.mjs
import fs from 'node:fs';
let s = 0x5eed1234 >>> 0;
const rnd = () => { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; };
const ri = (n) => Math.floor(rnd() * n);
const pick = (a) => a[ri(a.length)];
const ATOMS = ['a', 'Z', ' ', '\n', '\t', '"', '\\', '/', '\u0000', '\u0001', '\u001f', '\u007f', ' ', ' ', '﻿', '�',
  'é', '日本', '😀', '\ud800', '\udfff', '\udc00', '\ud83d', '\ude00', '<', '>', '&', "'", '\u{10FFFE}', '\u{10FFFF}', '\u{FFFE}', 'u', 'd800', '\\u', '\\ud800', '\u{1d7ce}', '̀'];
const str = () => { let o = ''; const n = ri(12); for (let i = 0; i < n; i++) o += pick(ATOMS); return o; };
const NUMS = [0, 1, -1, 0.5, 1e21, 1e-7, 123456789.123456789, 5e-324, 1.7976931348623157e308, 2 ** 53, 2 ** 53 + 2, 0.1 + 0.2, 1e16, 1e15, 4.5e15, 1e300, 1.5e-5, 0.000001];
const num = () => (rnd() < 0.5 ? pick(NUMS) : (rnd() - 0.5) * 10 ** (ri(40) - 20));
const KEYS = ['id', 'raw', 'x', '__proto__', 'constructor', 'toString', '10', '2', '01', '-1', '4294967295', '4294967294', '', ' ', 'é', '\ud800', '\u{10FFFE}', 'a.b', 'a/b', 'corrected'];
function val(d) {
  const r = rnd();
  if (d > 4 || r < 0.3) return str();
  if (r < 0.5) return num();
  if (r < 0.58) return null;
  if (r < 0.64) return rnd() < 0.5;
  if (r < 0.8) { const a = []; for (let i = ri(4); i > 0; i--) a.push(val(d + 1)); return a; }
  const o = {}; for (let i = ri(5); i > 0; i--) o[pick(KEYS)] = val(d + 1); return o;
}
const cases = [];
for (let i = 0; i < 400; i++) {
  const page = { id: 'p' + i, name: str(), order: i, status: pick(['done', 'pending', 'error']), raw: str(), corrected: str() };
  for (let j = ri(6); j > 0; j--) page[pick(KEYS)] = val(0);
  const text = JSON.stringify(page);
  cases.push({ id: 'p' + i, text, resave: JSON.stringify(JSON.parse(text)) });
}
const metas = [];
for (let i = 0; i < 150; i++) {
  const m = {};
  for (let j = ri(8); j > 0; j--) m[pick(['name', 'order', 'schemaVersion', 'topic', 'prompt', 'ui', 'stats', 'created', 'updated', 'futureField', ...KEYS])] = pick([val(0), str(), num(), null, [], {}]);
  if (rnd() < 0.5) m.order = [];
  if (rnd() < 0.5) m.name = str();
  const text = rnd() < 0.5 ? JSON.stringify(m, null, 2) : JSON.stringify(m);
  const norm = Object.assign({ schemaVersion: 1, name: 'Untitled', topic: 'General', prompt: '', order: [], created: 7, updated: 7, ui: {}, stats: null }, JSON.parse(text));
  metas.push({ text, normPretty: JSON.stringify(norm, null, 2) });
}
fs.writeFileSync(new URL('./blind2.json', import.meta.url), JSON.stringify({ pages: cases, metas }));
console.log(cases.length, metas.length);
