/* node browser.js -> same comparison inside real Chromium (real window, real fetch/streams, CORS preflight).
   Playwright is taken from the global @playwright/cli install. */
const http = require('http'), fs = require('fs'), path = require('path');
const { start } = require('./mock-server');
const PW = process.env.PW_CORE || 'C:/Users/shuff/AppData/Roaming/npm/node_modules/@playwright/cli/node_modules/playwright-core';
const { chromium } = require(PW);
const REPO = path.resolve(__dirname, '..', '..');

const PAGE = `<!doctype html><meta charset=utf-8><title>s4</title><script>
window.OCR = { ocr: {}, state: { d: { settings: { ocrUrl: '', model: 'ocr', apiKey: 'k', defaultPrompt: 'T', topics: {} }, project: { meta: {} } } }, image: { toDataURL: async () => 'data:,x' } };
</script><script src="/ocr.js"></script><script type="module">
import init, { transcribe } from '/pkg-web/vishion_s4.js';
await init();
const norm = (e) => ({ ok: false, name: e && e.name, kind: e && e.kind, message: e && e.message, partial: e && e.partial });
window.run = async (which, origin, abortAfterTexts, abortAfterMs) => {
  const ctl = new AbortController(), events = []; let n = 0;
  if (abortAfterMs) setTimeout(() => ctl.abort(), abortAfterMs);
  const hook = () => { if (abortAfterTexts && ++n === abortAfterTexts) ctl.abort(); };
  try {
    if (which === 'js') {
      OCR.state.d.settings.ocrUrl = origin;
      const r = await OCR.ocr.transcribe(new Blob(['x']), { signal: ctl.signal,
        onStatus: (m) => events.push({ t: 'status', m }), onText: (c, f) => { events.push({ t: 'text', chunk: String(c), full: f }); hook(); } });
      return { events, outcome: { ok: true, text: r.text, tokens: r.tokens, tps: r.tps ?? null } };
    }
    const r = await transcribe(origin, '{}', 'k', ctl.signal, (ev) => {
      if (ev.type === 'status') events.push({ t: 'status', m: ev.message }); else { events.push({ t: 'text', chunk: ev.chunk, full: ev.full }); hook(); } }, false);
    return { events, outcome: { ok: true, text: OCR.ocr.clean(r.text), tokens: r.tokens, tps: r.tps ?? null } };
  } catch (e) { return { events, outcome: norm(e) }; }
};
window.ready = true;
</script>`;

(async () => {
  const mock = await start(0);
  const web = http.createServer((req, res) => {
    const u = req.url.split('?')[0];
    const map = { '/': [PAGE, 'text/html'], '/ocr.js': [fs.readFileSync(path.join(REPO, 'js', 'ocr.js')), 'text/javascript'] };
    if (map[u]) { res.writeHead(200, { 'Content-Type': map[u][1] }); return res.end(map[u][0]); }
    const f = path.join(__dirname, u);
    if (u.startsWith('/pkg-web/') && fs.existsSync(f)) { res.writeHead(200, { 'Content-Type': f.endsWith('.wasm') ? 'application/wasm' : 'text/javascript' }); return res.end(fs.readFileSync(f)); }
    res.writeHead(404); res.end();
  });
  await new Promise((r) => web.listen(0, '127.0.0.1', r));
  const browser = await chromium.launch({ executablePath: process.env.CHROME || 'C:/Users/shuff/AppData/Local/ms-playwright/chromium-1234/chrome-win64/chrome.exe' });
  const page = await browser.newPage();
  page.on('pageerror', (e) => console.log('pageerror', e.message));
  await page.goto('http://127.0.0.1:' + web.address().port + '/');
  await page.waitForFunction('window.ready === true', null, { timeout: 15000 });
  const O = (v) => `http://127.0.0.1:${mock.port}/v/${v}`;
  const cases = [
    ['normal', O('normal')], ['crlf', O('crlf')], ['split-bytes', O('split-bytes')], ['bom', O('bom')], ['bad-utf8', O('bad-utf8')],
    ['drop-mid', O('drop-mid')], ['done-no-newline', O('done-no-newline')], ['malformed', O('malformed')], ['empty-data', O('empty-data')],
    ['lone-surrogate', O('lone-surrogate')], ['http-503', O('503')], ['http-503-emoji', O('503-emoji')], ['http-500', O('500')],
    ['unreachable', 'http://127.0.0.1:1'], ['abort after 2nd chunk', O('gated'), 2], ['abort timer 400ms', O('slow'), 0, 400], ['big', O('big')],
  ];
  for (const [name, origin, aN, aMs] of cases) {
    const j = await page.evaluate(([o, n, ms]) => window.run('js', o, n, ms), [origin, aN || 0, aMs || 0]);
    const r = await page.evaluate(([o, n, ms]) => window.run('rust', o, n, ms), [origin, aN || 0, aMs || 0]);
    const same = JSON.stringify(j) === JSON.stringify(r);
    const sum = (x) => (x.outcome.ok ? 'ok tokens=' + x.outcome.tokens : x.outcome.name + '/' + (x.outcome.kind || '-') + (x.outcome.message ? ' "' + x.outcome.message.slice(0, 40) + '"' : ''));
    console.log((same ? 'SAME ' : 'DIFF ') + name.padEnd(24) + ' js: ' + sum(j).padEnd(60) + ' rust: ' + sum(r) + `  events ${j.events.length}/${r.events.length}`);
  }
  await browser.close(); web.close(); mock.server.close();
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });
