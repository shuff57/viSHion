/* node fuzz.js [N]  -> differential fuzz of the real js/ocr.js loop vs the Rust client, no network.
   globalThis.fetch is stubbed to return a Response whose body is a random byte stream split at random
   offsets (mid-codepoint, mid-line, mid-CRLF). Compares full event sequence + outcome. */
const { jsClient, rustClient } = require('./clients');
const N = +process.argv[2] || 3000;
let seed = 12345;
const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
const pick = (a) => a[Math.floor(rnd() * a.length)];
const enc = new TextEncoder();
const frag = [
  () => 'data: ' + JSON.stringify({ choices: [{ delta: { content: pick(['a', ' b', 'é', '😀', '\n', '$x$', '\\', '"', '日本', ' ', '\u0085']) } }] }),
  () => 'data: ' + JSON.stringify({ choices: [{ delta: { content: '' } }] }),
  () => 'data: ' + JSON.stringify({ choices: [] , usage: { completion_tokens: pick([0, 3, 7, '9']) } }),
  () => 'data: ' + JSON.stringify({ choices: [{ delta: {} }], timings: { predicted_n: pick([0, 4]), predicted_per_second: pick([0, 1.5, 33.333333333333336, 1e-7, 123456789012345680000]) } }),
  () => 'data: [DONE]', () => 'data:[DONE]', () => 'data: [DONE] ', () => 'data: [done]',
  () => 'data: {"choices":[{"delta":{"content":"cut', () => 'data:', () => 'data: null', () => 'data: 5', () => 'data: "s"', () => 'data: []', () => 'data: {}',
  () => 'data: {"choices":{"0":{"delta":{"content":"objkey"}}}}', () => 'data: {"choices":"abc"}', () => 'data: {"choices":[null]}',
  () => 'data: {"choices":[{"delta":{"content":12}}]}', () => 'data: {"choices":[{"delta":{"content":true}}]}',
  () => 'data: {"choices":[{"delta":{"content":"dup"}}],"choices":[{"delta":{"content":"last"}}]}',
  () => ': comment', () => 'event: x', () => '', () => '  data: {"choices":[{"delta":{"content":"ws"}}]}', () => 'DATA: {}', () => 'data:{"choices":[{"delta":{"content":"nospace"}}]}',
  () => '﻿data: {"choices":[{"delta":{"content":"bomline"}}]}', () => ' data: {"choices":[{"delta":{"content":"nbsp"}}]}', () => '\u0085data: {}',
];
function gen() {
  const eol = pick(['\n', '\n', '\r\n', '\n\n', '\r', '\r\n\r\n']);
  const n = 1 + Math.floor(rnd() * 12);
  let s = '';
  for (let i = 0; i < n; i++) s += (rnd() < 0.6 ? pick(frag.slice(0, 5)) : pick(frag))() + eol;
  if (rnd() < 0.5) s += 'data: [DONE]' + eol;
  if (rnd() < 0.3) s += pick(frag)(); /* unterminated tail */
  let b = Buffer.from(s, 'utf8');
  if (rnd() < 0.15) b = Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), b]);
  if (rnd() < 0.15) { const at = Math.floor(rnd() * b.length); b = Buffer.concat([b.subarray(0, at), Buffer.from([pick([0xff, 0xc0, 0xe2, 0xf0])]), b.subarray(at)]); }
  const parts = []; let i = 0;
  while (i < b.length) { const k = 1 + Math.floor(rnd() * pick([1, 3, 9, 40])); parts.push(b.subarray(i, i + k)); i += k; }
  return parts;
}
let parts = null;
globalThis.fetch = async (input) => {
  const url = typeof input === 'string' ? input : input.url;
  if (url.endsWith('/slots')) return new Response('{}', { status: 404 });
  const p = parts;
  return new Response(new ReadableStream({ start(c) { for (const x of p) c.enqueue(new Uint8Array(x)); c.close(); } }), { status: 200 });
};
(async () => {
  let diff = 0; const kinds = {};
  for (let i = 0; i < N; i++) {
    parts = gen();
    const o = { origin: 'http://x', signal: undefined };
    const j = await jsClient(o), r = await rustClient(o), jf = await (async () => { parts = parts; return null; })();
    const a = JSON.stringify(j), b = JSON.stringify(r);
    const k = j.outcome.ok ? 'ok' : j.outcome.kind;
    kinds[k] = (kinds[k] || 0) + 1; kinds["_chunks>0"] = (kinds["_chunks>0"] || 0) + (j.events.length > 1 ? 1 : 0);
    if (a !== b) {
      diff++;
      if (diff <= 5) { console.log('DIFF #' + i, '\n  bytes:', JSON.stringify(Buffer.concat(parts).toString('latin1')).slice(0, 300), '\n  js  :', a.slice(0, 400), '\n  rust:', b.slice(0, 400)); }
    }
  }
  console.log(`fuzz: ${N} streams, ${diff} differing; JS outcome mix`, kinds);
})();
