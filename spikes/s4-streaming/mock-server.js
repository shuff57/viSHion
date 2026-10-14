/* Mock OpenAI-style streaming server for S4. Variant is the first path segment:
     POST /v/<variant>/v1/chat/completions      GET /v/<variant>/slots (always 404, counted)
     GET /_log  -> per-request log (did the client hang up early?)   POST /_reset
   Run standalone: node mock-server.js [port] */
const http = require('http');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chunk = (content, extra) => 'data: ' + JSON.stringify(Object.assign({ id: 'c1', object: 'chat.completion.chunk', model: 'm',
  choices: [{ index: 0, delta: content === undefined ? {} : { content }, finish_reason: null }] }, extra || {})) + '\n\n';
const WORDS = ['Hello', ' wor', 'ld $x^2$', '\n', 'Margin: caf\u00e9 \ud83d\ude00 done'];
const FINISH = 'data: ' + JSON.stringify({ id: 'c1', choices: [{ index: 0, delta: {}, finish_reason: 'stop' }],
  timings: { predicted_n: 11, predicted_per_second: 42.5, prompt_n: 300 } }) + '\n\n';
const USAGE = 'data: ' + JSON.stringify({ id: 'c1', choices: [], usage: { prompt_tokens: 300, completion_tokens: 5, total_tokens: 305 } }) + '\n\n';
const ROLE = 'data: ' + JSON.stringify({ id: 'c1', choices: [{ index: 0, delta: { role: 'assistant', content: '' }, finish_reason: null }] }) + '\n\n';
const DONE = 'data: [DONE]\n\n';
const crlf = (s) => s.replace(/\n/g, '\r\n');
const bodyWords = () => WORDS.map((w) => chunk(w));

/* each variant: async (w) => ..., where w(strOrBuf, gapMs) writes, w.destroy(), w.end(), w.req */
const V = {
  normal: async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, USAGE, DONE]) await w(f, 3); },
  'one-packet': async (w) => { await w([ROLE, ...bodyWords(), FINISH, USAGE, DONE].join('')); },
  crlf: async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, USAGE, DONE]) await w(crlf(f), 3); },
  'cr-only': async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, USAGE, DONE]) await w(f.replace(/\n/g, '\r'), 3); },
  'drop-mid': async (w) => { for (const f of [ROLE, ...bodyWords().slice(0, 3)]) await w(f, 5); await sleep(20); w.destroy(); },
  'done-no-newline': async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, USAGE]) await w(f, 3); await w('data: [DONE]', 3); },
  'done-one-newline': async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, USAGE]) await w(f, 3); await w('data: [DONE]\n', 3); },
  'no-done': async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, USAGE]) await w(f, 3); },
  malformed: async (w) => { const b = bodyWords(); for (const f of [ROLE, b[0], b[1], 'data: {"choices":[{"delta":{"content":"BAD"\n\n', b[2], b[3], b[4], FINISH, USAGE, DONE]) await w(f, 3); },
  'empty-data': async (w) => { for (const f of [ROLE, ...bodyWords(), 'data:\n\n', FINISH, USAGE, DONE]) await w(f, 3); },
  'null-payload': async (w) => { for (const f of [ROLE, ...bodyWords(), 'data: null\n\n', FINISH, USAGE, DONE]) await w(f, 3); },
  'scalar-payloads': async (w) => { for (const f of [ROLE, ...bodyWords(), 'data: 123\n\ndata: "x"\n\ndata: []\n\ndata: true\n\n', FINISH, USAGE, DONE]) await w(f, 3); },
  comments: async (w) => { for (const f of [': keep-alive\n\n', 'event: ping\n\n', ROLE, ...bodyWords(), 'id: 7\nretry: 100\n\n', FINISH, USAGE, DONE]) await w(f, 3); },
  'split-bytes': async (w) => {
    const all = Buffer.from([ROLE, ...bodyWords(), FINISH, USAGE, DONE].join(''));
    for (let i = 0; i < all.length; i += 7) await w(all.subarray(i, i + 7), 1); /* 7-byte writes split emoji and lines */
  },
  bom: async (w) => { await w(Buffer.from([0xEF, 0xBB, 0xBF])); for (const f of [ROLE, ...bodyWords(), FINISH, USAGE, DONE]) await w(f, 3); },
  'bad-utf8': async (w) => {
    await w(ROLE, 3); await w(Buffer.concat([Buffer.from('data: {"choices":[{"delta":{"content":"a'), Buffer.from([0xFF, 0xE2, 0x82]), Buffer.from('"}}]}\n\n')]), 3);
    for (const f of [FINISH, USAGE, DONE]) await w(f, 3);
  },
  'lone-surrogate': async (w) => {
    for (const f of [ROLE, 'data: {"choices":[{"delta":{"content":"a\\ud83d"}}]}\n\n', 'data: {"choices":[{"delta":{"content":"\\ude00b"}}]}\n\n', FINISH, USAGE, DONE]) await w(f, 3);
  },
  'data-after-done': async (w) => { for (const f of [ROLE, ...bodyWords(), DONE, chunk('LATE'), USAGE]) await w(f, 3); },
  'timings-only': async (w) => { for (const f of [ROLE, ...bodyWords(), FINISH, DONE]) await w(f, 3); },
  'no-stats': async (w) => { for (const f of [ROLE, ...bodyWords(), chunk(undefined), DONE]) await w(f, 3); },
  'nbsp-nel': async (w) => { /* U+0085 and U+FEFF around data: lines (JS trim vs Rust trim) */
    for (const f of [ROLE, '\u0085' + bodyWords()[0], '\ufeff' + bodyWords()[1], '\u00a0' + bodyWords()[2], FINISH, USAGE, DONE]) await w(f, 3);
  },
  slow: async (w) => { await w(ROLE); for (let i = 0; i < 20; i++) { await w(chunk('w' + i + ' '), 60); } for (const f of [FINISH, USAGE, DONE]) await w(f, 3); },
  gated: async (w) => { /* two chunks, then hold the stream open until the client goes away (or 8 s) */
    await w(ROLE); await w(chunk('one ')); await w(chunk('two '));
    for (let i = 0; i < 160 && !w.gone(); i++) await sleep(50);
    if (!w.gone()) { for (const f of [FINISH, USAGE, DONE]) await w(f); }
  },
  'slow-headers': async (w) => { await sleep(600); for (const f of [ROLE, ...bodyWords(), FINISH, USAGE, DONE]) await w(f, 3); },
  big: async (w) => { await w(ROLE); let s = ''; for (let i = 0; i < 4000; i++) { s += chunk('tok' + i + ' '); if (i % 50 === 49) { await w(s, 0); s = ''; } } for (const f of [FINISH, USAGE, DONE]) await w(f); },
};
const FAIL = {
  '503': [503, '{"error":{"message":"Loading model","type":"unavailable"}}'],
  '503-emoji': [503, 'a'.repeat(159) + '\ud83d\ude00' + ' tail beyond limit'],
  '503-empty': [503, ''],
  '500': [500, 'Internal Server Error'],
  '401': [401, '{"error":"bad key"}'],
};

const log = [];
function start(port) {
  const server = http.createServer(async (req, res) => {
    res.setHeader('Access-Control-Allow-Origin', '*');
    res.setHeader('Access-Control-Allow-Headers', '*');
    if (req.method === 'OPTIONS') { res.writeHead(204); return res.end(); }
    if (req.url === '/_log') { res.writeHead(200, { 'Content-Type': 'application/json' }); return res.end(JSON.stringify(log)); }
    if (req.url === '/_reset') { log.length = 0; res.writeHead(200); return res.end('ok'); }
    const m = req.url.match(/^\/v\/([^/]+)(\/.*)$/);
    if (!m) { res.writeHead(404); return res.end('nope'); }
    const [, variant, path] = m;
    const entry = { variant, path, method: req.method, gone: false, ended: false, t0: Date.now() };
    log.push(entry);
    res.on('close', () => { entry.gone = !res.writableFinished; entry.closedAfter = Date.now() - entry.t0; });
    if (path === '/slots') { res.writeHead(404, { 'Content-Type': 'application/json' }); return res.end('{"error":"no slots"}'); }
    req.resume();
    await new Promise((r) => req.on('end', r));
    if (FAIL[variant]) { res.writeHead(FAIL[variant][0], { 'Content-Type': 'text/plain; charset=utf-8' }); return res.end(FAIL[variant][1]); }
    const fn = V[variant];
    if (!fn) { res.writeHead(404); return res.end('unknown variant'); }
    if (variant !== 'slow-headers') res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
    else res.writeHead(200, { 'Content-Type': 'text/event-stream' });
    const w = async (data, gap) => { if (res.destroyed) return; res.write(data); if (gap) await sleep(gap); else await new Promise((r) => setImmediate(r)); };
    w.destroy = () => req.socket.destroy();
    w.gone = () => res.destroyed || res.writableEnded || req.socket.destroyed;
    try { await fn(w); } catch (e) { /* client went away */ }
    if (!res.destroyed && !res.writableEnded) { entry.ended = true; res.end(); }
  });
  return new Promise((resolve) => server.listen(port || 0, '127.0.0.1', () => resolve({ server, port: server.address().port, log })));
}
module.exports = { start, variants: Object.keys(V), failures: Object.keys(FAIL) };
if (require.main === module) start(+process.argv[2] || 8190).then((s) => console.log('mock on', s.port));
