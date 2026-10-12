/* Two clients with one signature, for the comparison harness (Node):
     run({ origin, signal, onTextHook }) -> { events, outcome }
   js   = the REAL js/ocr.js, loaded unmodified (same technique as tests/clean.test.js).
   rust = pkg-node build of src/web.rs; raw text is passed through the real O.clean for comparison. */
const fs = require('fs'), path = require('path');
const REPO = path.resolve(__dirname, '..', '..');

const settings = { ocrUrl: '', model: 'ocr', apiKey: 'sk-test', demo: false, defaultPrompt: 'Transcribe.', topics: {} };
/* window is passed as a parameter, not a global: a global `window` stub would hijack js_sys::global() */
const win = { OCR: { ocr: {}, state: { d: { settings, project: { meta: {} } } }, image: { toDataURL: async () => 'data:image/png;base64,AAAA' } } };
new Function('window', fs.readFileSync(path.join(REPO, 'js', 'ocr.js'), 'utf8'))(win);
const O = win.OCR.ocr;

const norm = (o) => JSON.parse(JSON.stringify(o, (k, v) => (v === undefined ? null : v)));
const BODY = JSON.stringify({ model: 'ocr', stream: true, messages: [] });

function errOutcome(e) {
  return { ok: false, name: e && e.name, kind: e && e.kind, message: e && e.message, partial: e && e.partial };
}

async function jsClient({ origin, signal, onTextHook }) {
  settings.ocrUrl = origin;
  const events = [];
  try {
    const r = await O.transcribe(new Blob(['x']), {
      signal, index: 0,
      onStatus: (m) => events.push({ t: 'status', m }),
      onText: (chunk, full) => { events.push({ t: 'text', chunk: String(chunk), full }); onTextHook /* chunk typeof differs for non-string content: known, see README */ && onTextHook(events.filter((e) => e.t === 'text').length); },
    });
    return { events, outcome: norm({ ok: true, text: r.text, tokens: r.tokens, tps: r.tps }) };
  } catch (e) { return { events, outcome: norm(errOutcome(e)) }; }
}

let rust = null;
function loadRust() { return rust || (rust = require('./pkg-node/vishion_s4.js')); }

async function rustClient({ origin, signal, onTextHook, flushTail }) {
  const R = loadRust(), events = [];
  const cb = (ev) => {
    if (ev.type === 'status') events.push({ t: 'status', m: ev.message });
    else { events.push({ t: 'text', chunk: ev.chunk, full: ev.full }); onTextHook && onTextHook(events.filter((e) => e.t === 'text').length); }
  };
  try {
    const r = await R.transcribe(origin, BODY, 'sk-test', signal, cb, !!flushTail);
    return { events, outcome: norm({ ok: true, text: O.clean(r.text), tokens: r.tokens, tps: r.tps }) };
  } catch (e) { return { events, outcome: norm(errOutcome(e)) }; }
}

module.exports = { jsClient, rustClient, O };
