#!/usr/bin/env node
/* Parity oracle for crates/core.
 *
 *   node tools/parity/run.js            regenerate tools/parity/fixture.json from the UNMODIFIED js/*.js, then
 *                                       run `cargo test -p vishion-core` (the Rust side reads the fixture)
 *   node tools/parity/run.js --no-cargo regenerate the fixture only
 *
 * The JS files are loaded exactly as tests/*.test.js and spikes/s5-markdown-katex/run.js load them
 * (new Function('window', source)(stubWindow)); nothing in js/ is edited. Each fixture case holds the
 * input, the JS result, and for stateful operations the op list. crates/core/tests/parity.rs replays
 * every case through the Rust crate and compares the serialised results byte for byte.
 */
'use strict';
const fs = require('fs'), path = require('path'), cp = require('child_process');
const REPO = path.resolve(__dirname, '..', '..');
const src = (f) => fs.readFileSync(path.join(REPO, f), 'utf8');
const FIXTURE = path.join(__dirname, 'fixture.json');

/* ---------- deterministic clock and PRNG ---------- */
const RealDate = Date;
let clock = 1760000000000;
class FakeDate extends RealDate {
  constructor(...a) { if (a.length) super(...a); else super(clock); }
  static now() { return clock; }
}
globalThis.Date = FakeDate;
function rng(seed) { let a = seed >>> 0; return () => { a = (a + 0x6d2b79f5) >>> 0; let t = a; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
const R = rng(20251012);
const digest = (buf) => ({ len: buf.length, sha256: require('crypto').createHash('sha256').update(buf).digest('hex') });
const pick = (a) => a[Math.floor(R() * a.length)];
const chance = (p) => R() < p;

/* ---------- global stubs the JS files touch at call time ---------- */
const store = {};
globalThis.document = { documentElement: { dataset: {} } };
globalThis.localStorage = { getItem: (k) => (k in store ? store[k] : null), setItem: (k, v) => { store[k] = String(v); }, removeItem: (k) => { delete store[k]; } };

/* A fresh window with the real ns.js/state.js/ocr.js/words.js/judge.js/guess.js/groups.js/student.js/export.js/render.js. */
function loadJs(opts) {
  opts = opts || {};
  const win = {};
  if (opts.markdownit) win.markdownit = opts.markdownit;
  if (opts.katex) win.katex = opts.katex;
  const load = (f) => new Function('window', src(f))(win);
  load('js/ns.js');
  const OCR = win.OCR;
  OCR.uid = (() => { let n = 0; return () => 'id' + (++n); })(); /* deterministic */
  OCR.fs = { savePage() {}, saveMeta() {}, deletePage() {}, saveSettings() {}, loadImage: async () => 'BLOB', writeExport: async () => false };
  OCR.image = { toDataURL: async () => 'data:image/jpeg;base64,AAAA' };
  OCR.toast = () => {};
  const queued = new Set();
  OCR.queue = { isQueued: (id) => queued.has(id), isRunning: () => false };
  ['js/state.js', 'js/ocr.js', 'js/judge.js', 'js/words.js', 'js/guess.js', 'js/groups.js', 'js/student.js', 'js/export.js', 'js/render.js'].forEach(load);
  return { win, OCR, S: OCR.state, queued };
}
const clone = (v) => JSON.parse(JSON.stringify(v));
const J = (v) => JSON.stringify(v === undefined ? null : v);

/* ---------- generators ---------- */
const WORDS = ['the', 'confidance', 'interval', 'mean', 'standrd', 'deviation', 'sample', 'x', 'is', 'a', 'quadratic', 'derivitive', 'cat', 'Cat', "don't", 'C++', 'a.b', '(x)', '-1', 'under_score', 'éclair', 'naïve', '5$', '100%', 'populaton', 'regresion', 'hypothesis', 'integral', 'İstanbul', '日本語', '😀', 'a$b', '$'];
const ILLS = ['[illegible]', '[Illegible]', '[ILLEGIBLE]', '[illegibl]', '[ iIlLeEgGiIbBlLeE ]'];
const MATHS = ['$x^2$', '$$y=1$$', '$a [illegible] b$', '$ pad $', '$$\nx\n$$', '$5 and $10', '$$ open', '$\\frac{1}{2}$ is $[illegible]$'];
const SEPS = [' ', '  ', '\n', '\r\n', '\t', '\n\n', ' ~~s~~ ', ', ', '. '];
function genRaw() {
  const n = 2 + Math.floor(R() * 13), parts = [];
  for (let i = 0; i < n; i++) {
    const r = R();
    parts.push(r < 0.45 ? pick(WORDS) : r < 0.65 ? pick(ILLS) : r < 0.8 ? pick(MATHS) : r < 0.85 ? 'c:0w' : pick(WORDS));
    parts.push(pick(SEPS));
  }
  return parts.join('');
}
function genPage(i) {
  const raw = genRaw();
  const toks = raw.match(/[A-Za-z][A-Za-z'.-]*|\[[^\]]*\]/g) || [];
  const corrections = [];
  const nc = Math.floor(R() * 5);
  for (let k = 0; k < nc; k++) {
    const w = chance(0.8) && toks.length ? pick(toks) : pick(WORDS);
    const c = { word: w, term: chance(0.7) ? w + pick(['s', 'e', '', 'ed']) : pick(WORDS), conf: pick([0.9, 0.55, 1, 0.3, 0.75]), applied: chance(0.35) };
    if (chance(0.1)) delete c.conf;
    if (chance(0.05)) c.word = '';
    corrections.push(c);
  }
  const p = { id: 'p' + i, name: pick(['quiz.png', 'Maya R.jpg', 'a.b.c', 'scan 02.PNG', '', 'Ünal p1.jpeg', 'x']), order: i, status: chance(0.88) ? 'done' : pick(['pending', 'transcribing', 'error']), raw, corrected: '', corrections, resolved: {}, edits: [], reviewed: chance(0.3), timings: { ms: pick([0, 250, 1234, 750, 12999, 5]), tokens: 3 } };
  if (chance(0.5)) p.corrected = chance(0.5) ? raw : raw.replace(/confidance/g, 'confidence');
  if (chance(0.35)) { const k = pick(['c:0', 'c:1', 'i:0', 'i:1', 'i:2']); p.edits.push({ key: k, text: pick(['the mean', 'x', '', 'a  b']) }); if (chance(0.5)) p.resolved[k] = pick(['typed', 'written', 'marked', 'suggested']); }
  if (chance(0.3)) p.student = { name: pick(['Maya R.', ' devon  t. ', '', '  ', 'Maya  r.']), source: chance(0.5) ? 'auto' : '' };
  else if (chance(0.15)) p.student = null;
  if (chance(0.05)) delete p.corrections;
  if (chance(0.05)) delete p.resolved;
  if (chance(0.05)) delete p.edits;
  return p;
}
const TYPED = ['', ' the   mean $', 'a\nb', '[illegible]', 'x[ILLEGIBLE]y', 'k', 'standard', '$$', '  '];
function genOps(p) {
  const ops = [], n = Math.floor(R() * 5);
  for (let k = 0; k < n; k++) {
    const r = R(), keys = ['c:0', 'c:1', 'c:2', 'i:0', 'i:1', 'i:2', 'i:9', 'x:1'];
    if (r < 0.45) ops.push({ op: 'choose', key: pick(keys), via: pick(['suggested', 'written', 'typed', 'typed', 'marked']), text: pick(TYPED) });
    else if (r < 0.6) ops.push({ op: 'reopen', key: pick(keys) });
    else if (r < 0.7) ops.push({ op: 'useAll' });
    else if (r < 0.85) ops.push({ op: 'toggle', idx: pick([0, 1, 2, 7, -1]), applied: chance(0.5) });
    else ops.push({ op: 'applyAll' });
  }
  return ops;
}

/* ---------- case builders ---------- */
const cases = [];
const add = (kind, input, expected, extra) => cases.push(Object.assign({ kind, input, expected }, extra || {}));

/* words/judge: a page, the pure reads, then an op sequence through the real choose/reopen/toggle. */
function pageCase(page, ops, settingsSaved) {
  const j = loadJs();
  const S = j.S, W = j.OCR.words, Jd = j.OCR.judge;
  const topic = 'Stats';
  const p = clone(page);
  S.d.settings = Object.assign({}, S.d.settings, { reference: [] });
  if (settingsSaved) Object.assign(S.d.settings, settingsSaved);
  S.d.project = { meta: { name: 'Q', topic, order: [p.id] }, pages: { [p.id]: p } };
  const itemKeys = (pg) => { const k = W.items(pg).map((i) => i.key); return k.concat(['i:7', 'zz']); };
  const snap = (pg) => ({
    effective: W.effective(pg), marked: W.marked(pg, 'corrected'), original: W.marked(pg, 'original'),
    items: W.items(pg).map((i) => Object.assign({}, i)), count: W.count(pg),
    ctx: itemKeys(pg).map((k) => [k, W.contextFor(pg, k)]),
    page: clone(pg), reference: clone(S.d.settings.reference || [])
  });
  const out = { initial: snap(p), steps: [] };
  const recorded = [];
  ops.forEach((o, k) => {
    clock = 1760000000000 + (k + 1) * 1000;
    const op = Object.assign({}, o, { now: clock });
    recorded.push(op);
    const pg = S.page(p.id);
    let ret = null, threw = false;
    try {
    if (o.op === 'choose') ret = W.choose(p.id, o.key, o.via, o.text);
    else if (o.op === 'reopen') W.reopen(p.id, o.key);
    else if (o.op === 'useAll') W.useAll([p.id]);
    else if (o.op === 'toggle') Jd.toggle(p.id, o.idx, o.applied);
    else if (o.op === 'applyAll') Jd.applyAll(p.id);
    } catch (e) { threw = true; }
    const after = S.page(p.id);
    const s = snap(after);
    s.ret = ret; s.threw = threw;
    out.steps.push(s);
  });
  add('page', { page, topic, ops: recorded, settings: settingsSaved || {} }, out);
}

/* ---- words.test.js scenarios, literally ---- */
{
  const mk = (o) => Object.assign({ id: 'p1', status: 'done', raw: '', corrected: '', corrections: [], resolved: {}, edits: [] }, o);
  pageCase(mk({ raw: 'a plain line', corrected: 'a plain line' }), []);
  const t2 = 'the confidance interval is [illegible] here';
  pageCase(mk({ raw: t2, corrected: t2, corrections: [{ word: 'confidance', term: 'confidence', conf: 0.9, applied: false }] }),
    [{ op: 'choose', key: 'c:0', via: 'suggested' }, { op: 'choose', key: 'i:0', via: 'typed', text: ' the   mean $' }, { op: 'reopen', key: 'i:0' }]);
  pageCase(mk({ raw: '$x = [illegible]$ and [illegible]', corrected: '$x = [illegible]$ and [illegible]' }), []);
  pageCase(mk({ status: 'transcribing', raw: '[illegible]' }), []);
  const t9 = 'we use $x$ and the [illegible] deviation of the sample is small';
  pageCase(mk({ raw: t9, corrected: t9 }), [{ op: 'choose', key: 'i:0', via: 'typed', text: 'standard' }, { op: 'choose', key: 'i:0', via: 'typed', text: 'standard' }, { op: 'choose', key: 'i:0', via: 'typed', text: 'other' }]);
  pageCase(mk({ raw: t9, corrected: t9 }), [{ op: 'choose', key: 'i:0', via: 'typed', text: 'standard' }], { rememberFixes: false });
  /* replacement-pattern quirks in judge.rebuild: `$&`, `$$`, `$1` in a term */
  pageCase(mk({ raw: 'cost of populaton and populaton', corrected: 'x', corrections: [{ word: 'populaton', term: "a$&b$$c$1$'d", conf: 1, applied: false }] }), [{ op: 'toggle', idx: 0, applied: true }, { op: 'applyAll' }]);
  pageCase(mk({ raw: 'ab cd', corrected: 'ab cd', corrections: [{ word: '', term: 'X', conf: 1, applied: false }] }), [{ op: 'applyAll' }]);
}
/* ---- fuzz pages ---- */
for (let i = 0; i < 260; i++) { const p = genPage(i); pageCase(p, genOps(p)); }
/* ---- S5 corpus transcripts as pages (raw, with and without corrections) ---- */
const s5 = require(path.join(REPO, 'spikes', 's5-markdown-katex', 'corpus.js'));
s5.corpus.forEach((c, i) => {
  const toks = c.text.match(/[A-Za-z]{4,}/g) || [];
  const corr = toks.slice(0, 2).map((w) => ({ word: w, term: w + 'x', conf: 0.8, applied: false }));
  pageCase({ id: 'c' + i, name: c.name, order: i, status: 'done', raw: c.text, corrected: c.text, corrections: corr, resolved: {}, edits: [] },
    corr.length ? [{ op: 'applyAll' }, { op: 'choose', key: 'i:0', via: 'typed', text: 'fix' }] : [{ op: 'choose', key: 'i:0', via: 'typed', text: 'fix' }]);
});

/* ---- judge heuristic / accept / remote ---- */
{
  const lists = [['confidence interval', 'standard deviation', 'population', 'sample'], ['derivative', 'integral', 'quadratic', 'polynomial', 'coefficient', 'theorem'], ['Hypertension', 'edema', 'prn', 'ibuprofen', 'metformin', 'x'], []];
  const texts = ['the populaton mean and a standrd deviation, so the sample is good, we are sure', 'a derivitive and the integrel of a quadratc polynomal with coeficient', 'Hypertension and hypertensoin edemma ibuprofin metforman', "don't deriv-ative integral's samples sampel", 'populaton Populaton POPULATON populaton', ''];
  for (const list of lists) for (const text of texts) {
    const j = loadJs();
    add('heuristic', { text, list }, j.OCR.judge.heuristic(text, list));
  }
  /* texts built from glossary terms with one or two character edits, so the heuristic has hits */
  const mutate = (w) => { let t = w; for (let k = 0; k < 1 + Math.floor(R() * 2); k++) { const i = Math.floor(R() * t.length), r = R(); t = r < 0.34 ? t.slice(0, i) + t.slice(i + 1) : r < 0.67 ? t.slice(0, i) + 'q' + t.slice(i) : t.slice(0, i) + 'z' + t.slice(i + 1); } return t; };
  for (const list of lists.filter((l) => l.length)) for (let k = 0; k < 6; k++) {
    const ws = []; for (let m = 0; m < 3 + Math.floor(R() * 4); m++) { const t = pick(list).split(' ')[0]; ws.push(chance(0.6) ? mutate(t) : pick(['the', 'and', 'is', 'Sample', t])); }
    const text = ws.join(pick([' ', ', ', '. ']));
    add('heuristic', { text, list }, loadJs().OCR.judge.heuristic(text, list));
  }
  const xs = [{ word: 'populaton', term: 'population', conf: 0.9 }, { word: 'sure', term: 'confident', conf: 0.9 }, { word: 'good', term: 'population', conf: 0.9 }, { word: 'sample', term: 'population', conf: 0.9 },
    { word: 'missing', term: 'sample', conf: 0.9 }, { word: 'standrd', term: 'standard deviation', conf: 1 }, { word: '', term: 'x', conf: 1 }, { word: 5, term: 'sample' }, null, {}, { word: 'sampel', term: 'SAMPLE' }, { word: 'Populaton', term: 'Population' }, { word: 'a', term: 'x' }];
  const j = loadJs();
  for (const list of lists) for (const x of xs) for (const text of texts.slice(0, 3)) add('accept', { x, list, text }, !!j.OCR.judge.accept(x, list, text));
}
async function remoteCases() {
  const replies = ['[{"word":"populaton","term":"population","conf":0.9}]', 'Sure!\n[{"word":"populaton","term":"population","conf":"0.4"},{"word":"sure","term":"x"}] done [1,2]', 'no array here', '{"a":1}', '[{"word":"populaton","term":"population","conf":0}]', '[{"word":"populaton","term":"population","conf":7},{"word":"sampel","term":"sample","conf":-3}]', '```json\n[{"word":"standrd","term":"standard deviation"}]\n```', '[]', '[1,"a",null]', 'junk [ not json ] more'];
  const list = ['confidence interval', 'standard deviation', 'population', 'sample'];
  const text = 'the populaton mean and a standrd deviation, so the sampel is good, we are sure';
  for (const strict of [false, true]) for (const reply of replies) {
    const j = loadJs();
    j.S.d.settings = { ocrUrl: 'http://x', model: 'm' };
    j.S.d.project = { meta: { topic: 'T' } };
    globalThis.fetch = async () => ({ ok: true, json: async () => ({ choices: [{ message: { content: reply } }] }) });
    let res;
    try { res = { ok: await j.OCR.judge.remote(text, list, strict) }; } catch (e) { res = { error: true }; }
    add('remote', { reply, list, text, strict }, res);
  }
}

/* ---- student ---- */
async function studentCases() {
  const vals = ['Maya R.', 'name: Maya', 'Student Name - Devon T', ' STUDENT   NAME : "Sofia V." ', 'Name:', 'name', 'null', 'None', 'N/A', 'unknown', 'Not Found', 'no name', 'a', 'x'.repeat(41), 'x'.repeat(40), '12345', 'José Ñandú', 'Maya R.;', 'Maya R. ,', "O'Brien", 'Name-Name', 'studentname: Al', '  ', '', '×÷', 'Ǆ', 'ɏ', 'ɐ', 'Maya R.', 5, null, undefined, { name: 'x' }, ['a'], true];
  globalThis.createImageBitmap = async () => ({ height: 100, width: 100, close() {} });
  globalThis.document = { documentElement: { dataset: {} }, createElement: () => ({ getContext: () => ({ drawImage() {} }), toBlob: (cb) => cb('BLOB'), set width(v) {}, set height(v) {} }) };
  const replies = [];
  vals.forEach((v) => { if (v !== undefined) replies.push(JSON.stringify({ name: v })); });
  replies.push('Here: {"name": "Maya R."} ok', 'nope', '{"name": null}', '[1]', 'null', '{"nam": "x"}', 'a {"name":"A"} b {"name":"B"}', '{"name":"unterminated');
  for (const reply of replies) {
    const j2 = loadJs();
    j2.S.d.settings = { ocrUrl: 'http://x', demo: false };
    globalThis.fetch = async () => ({ ok: true, json: async () => ({ choices: [{ message: { content: reply } }] }) });
    const r = await j2.OCR.student.read('BLOB', {});
    add('student_reply', { reply }, r.name);
  }
  const hints = ['', 'quiz.png', 'Maya R.jpg', 'scan 02.PNG', 'Ünal.png', '😀', 'a', 'page-14.jpeg', 'x'.repeat(50)];
  for (const hint of hints) {
    const j3 = loadJs();
    j3.S.d.settings = { demo: true };
    const r = await j3.OCR.student.read('BLOB', { hint });
    add('student_demo', { hint }, r.name);
  }
}

/* ---- guess: parse + numbered prompt ---- */
async function guessCases() {
  const replies = [
    'Sure!\n```json\n[{"n":1,"options":[{"text":"mean","percent":30},{"text":"median","percent":70},{"text":"Mean","percent":5},{"text":"[illegible]","percent":9},{"text":"mode","percent":400}]},{"n":9,"options":[{"text":"x","percent":50}]},{"n":2,"options":[]}]\n```',
    'no json here', '[{"n":"2","options":[{"text":"a","certainty":"40.5"},{"text":"b","percent":null,"certainty":10},{"text":"c","percent":"abc"}]}]',
    '[{"n":1,"options":[{"text":"a","percent":60},{"text":"b","percent":60},{"text":"c","percent":60},{"text":"d","percent":60}]}]',
    '[{"n":1.5,"options":[{"text":"a","percent":10}]},{"n":true,"options":[{"text":null,"percent":10},{"text":"  spaced   out  ","percent":5},0,false,"s",{"text":"x".repeat(41),"percent":9},{"text":"x".repeat(40),"percent":9}]}]',
    '[{"n":1,"options":[{"text":"a","percent":0.4},{"text":"b","percent":99.5},{"text":"c","percent":-5},{"text":5,"percent":3},{"text":"Illegible one","percent":4},{"text":"[illegible","percent":4}]}]',
    '[ [ {"n":1} ] ] [{"n":1,"options":[{"text":"z","percent":1}]}]', '[{"n":1,"options":[{"text":"a","percent":50}]},{"n":1,"options":[{"text":"b","percent":40}]}]',
    '[null,1,"x",{"n":[1],"options":[{"text":"q","percent":[5]}]}]', '[{"n":1,"options":[{"text":"a","percent":1e999}]}]'
  ];
  for (const count of [0, 1, 2, 3]) for (const reply of replies) {
    const j = loadJs();
    const W = j.OCR.words;
    add('guesses', { reply, count }, W.parseGuesses(reply, count));
  }
  /* the numbered transcript, read from the real request body */
  const raws = ['plain', 'a [illegible] b [Illegible]', '$[illegible]$ x [ILLEGIBLE] $$\n[illegible]\n$$ [illegible]', '[illegible][illegible]', 'no [illegibl] here', 'x $a [illegible]', '$ [illegible] $ [illegible]'];
  for (const raw of raws) {
    const j = loadJs();
    const S = j.S;
    S.d.settings = Object.assign({}, S.d.settings, { demo: false, ocrUrl: 'http://x', reference: [] });
    S.d.project = { meta: { topic: '', order: ['p'] }, pages: { p: { id: 'p', status: 'done', raw, corrected: raw } } };
    let body = null;
    globalThis.fetch = async (u, o) => { body = JSON.parse(o.body); return { ok: true, json: async () => ({ choices: [{ message: { content: '[]' } }] }) }; };
    const had = /\[illegible\]/i.test(raw);
    await j.OCR.words.guess('p');
    let numbered = null;
    if (body) { const t = body.messages[0].content[1].text; numbered = t.slice(t.indexOf('Transcript:\n') + 12); }
    add('numbered', { raw }, numbered === null ? null : numbered, { requested: had });
  }
}

/* ---- groups ---- */
function groupCases() {
  for (let round = 0; round < 40; round++) {
    const n = 1 + Math.floor(R() * 7), pages = [];
    for (let i = 0; i < n; i++) {
      const p = genPage(i);
      p.id = 'g' + i;
      if (chance(0.6)) p.student = { name: pick(['Maya R.', 'maya  r.', 'Devon T.', 'sofia v.', 'Zed', 'al 10', 'al 9', 'Al 2', 'Émile', 'emile', ' _x', '1', '10', '02', 'a-b', 'a b', 'ab']), source: pick(['auto', null, '', 'manual']) };
      pages.push(p);
    }
    const queuedIds = pages.filter(() => chance(0.4)).map((p) => p.id);
    const j = loadJs();
    j.S.d.project = { meta: { order: pages.map((p) => p.id) }, pages: Object.fromEntries(pages.map((p) => [p.id, clone(p)])) };
    queuedIds.forEach((id) => j.queued.add(id));
    const G = j.OCR.groups;
    const list = G.list();
    const out = {
      list: list.map((g) => ({ key: g.key, name: g.name, pages: g.pages.map((p) => p.id), open: g.open, words: g.words, failed: g.failed, status: g.status, source: g.source })),
      tally: G.tally(list), names: G.names(),
      matches: list.map((g) => ['all', 'processing', 'ready', 'check', 'failed', 'queued', 'waiting'].map((f) => G.matches(g, f))),
      of: pages.map((p) => { const g = G.of(p.id, list); return g ? g.key : null; }),
      nameOf: pages.map((p) => G.nameOf(clone(p))), keyOf: pages.map((p) => G.keyOf(clone(p)))
    };
    add('groups', { pages, queued: queuedIds }, out);
  }
}

/* ---- nat compare / slug ---- */
function natCases() {
  const j = loadJs();
  const ascii = []; for (let c = 32; c < 127; c++) ascii.push(String.fromCharCode(c));
  const names = ['a', 'A', 'á', 'b', 'Z', 'z', '10', '9', '2', '02', '002', 'a b', 'ab', 'a_b', 'a-b', 'a.b', 'Maya R.', 'maya r', 'Maya  R', 'Ünal', 'Unal', 'Zoe', 'émile', 'Emile', '_x', '1a', 'a1', 'a10', 'a2', 'a02', '', ' ', 'x 1 y', 'x 10 y', 'x 9 y', 'Al 2', 'Al 10', 'Devon T.', 'Devon T', 'Sofia V.', 'name (1)', 'name (2)', 'name1', 'Name 1', 'ñ', 'n', 'o', 'z1', 'z 1'].concat(ascii);
  const pairs = [];
  for (let i = 0; i < names.length; i++) for (let k = 0; k < names.length; k++) pairs.push([names[i], names[k]]);
  // Letters ICU gives their own weight or expands (Đ Ł Ø Æ Œ ð þ ı ß, fullwidth, ligatures, ...), words, and
  // every Latin-script / Nd / No code point in the BMP against a probe set.
  const words = ['Łukasz', 'Lukasz', 'Mały', 'Maly', 'Øystein', 'Oystein', 'Ozzy', 'Straße', 'Strasse', 'Æsir', 'Aesir', 'Đorđe', 'Djordje', 'Þór', 'Zed', 'ıstanbul', 'Istanbul', 'ｆｕｌｌ', 'full', 'ﬁle', 'file', 'ǅ', 'dz', 'Ħal', 'Hal', 'Œuvre', 'Oeuvre', 'ð', 'þ', 'ĸ', 'ŉ', 'ŀ', 'a²', 'a2', 'x①', 'x1', '１２', '12', '٣', '3', 'ａｂｃ', 'abc', 'ℓ', 'ǆ', 'ƒ', 'ƿ'];
  const wp = []; for (const a of words) for (const b of words.concat(['a', 'z', '9', '10', 'd', 'l', 'o', 's', 'ss', 'ae', 'oe', 'i', 'h', 'q', '_'])) wp.push([a, b]);
  const probes = ['a', 'd', 'h', 'i', 'l', 'o', 's', 'z', '1', '9', '10', 'ss', 'ae', '_'];
  const cps = [];
  for (let cp = 0xa0; cp <= 0xffff; cp++) {
    if (cp >= 0xd800 && cp < 0xe000) continue;
    const ch = String.fromCodePoint(cp);
    if (/\p{Script=Latin}|\p{Nd}|\p{No}/u.test(ch)) cps.push(ch);
  }
  for (const ch of cps) for (const pr of probes) wp.push([ch, pr]);
  // Seeded fuzz across scripts, punctuation, symbols, ignorables, kana, final forms, marks and digits.
  const pools = [
    'abcxyzABCXYZ', '0123456789', ' _-.,;:!?\'"()[]{}@*/\\&#%`^+<=>|~$',
    '¿¡«»·•—–…¶§®©°€£¥‘’“”',
    'áéñüßøæœđłðþıＡａ',
    'αβςσΩά', 'абдийґяЖ',
    'אךכםמת', 'اأبتضم٠٣',
    'ადსամ', 'あアいイかカ日本中文한글',
    '­​‍́̈ \t™★→∞×÷',
    '\u{1f600}\u{1f44d}\u{1d400}\u{20000}กขअक'];
  let seed = 424242; const rnd = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
  const fz = [];
  const pick = () => { const cs = Array.from(pools[Math.floor(rnd() * pools.length)]); return cs[Math.floor(rnd() * cs.length)]; };
  for (let i = 0; i < 4000; i++) { let t = ''; const n = 1 + Math.floor(rnd() * 5); for (let k = 0; k < n; k++) t += pick(); fz.push(t); }
  for (let i = 0; i + 1 < fz.length; i++) wp.push([fz[i], fz[i + 1]], [fz[i], fz[(i * 7 + 3) % fz.length]]);
  const blind = [['\u{10d3}', '\u{636}'], ['\u{10d0}', '\u{5ea}'], ['\u{561}', '\u{10d0}'], ['\u{3042}', '\u{30a2}'], ['\u{3c2}', '\u{3c3}'], ['\u{5da}', '\u{5db}'], ['\u{5dd}', '\u{5de}'], ['\u{439}', '\u{438}'], ['\u{491}', '\u{434}'], ['\u{623}', '\u{627}'], ['a\u{ad}', 'a'], ['a\u{200b}', 'a'], ['\u{bf}', '\u{2014}'], ['\u{bf}', '\u{2026}'], ['\u{ab}', '\u{b7}'], ['\u{ab}', '\u{bf}']];
  for (const p of blind) wp.push(p);
  // Student names as groups.list sorts them: curly quotes, em dashes, non-Latin scripts.
  const sn = ['Maya R.', 'Maya — R', 'O’Brien', "O'Brien", 'Αλέξης', 'Анна', 'דני', 'محمد', 'გიორგი', '王芳', 'あや', 'Zoë', 'Zoe', 'Åsa', 'Anaïs'];
  for (const a of sn) for (const b of sn) wp.push([a, b]);
  // Supplementary-plane Nd (all of them) with and without a digit-run prefix against mixed probes.
  const suppProbes = ['a', '!', '1', '2', '9', '11', '12', 'ᙲ', 'ሀ', '_', 'z'];
  for (let cp = 0x10000; cp < 0x110000; cp++) {
    const ch = String.fromCodePoint(cp);
    if (!/\p{Nd}/u.test(ch)) continue;
    for (const pre of ['', '1', '٢', '0']) for (const pr of suppProbes) wp.push([pre + ch, pre + pr], [pr + pre, ch + pre]);
  }
  // ICU's identical-prefix skip backs up over combining marks / contraction chars: sample the BMP and every lead block.
  for (let cp = 0x80; cp < 0x10000; cp += 5) { if (cp >= 0xd800 && cp < 0xe000) continue; const ch = String.fromCodePoint(cp); wp.push(['1' + ch, '1\u{1D7CF}'], ['٢\u{1FBF3}9', '٢' + ch + 'x']); }
  for (let b = 0; b < 1024; b++) { const ch = String.fromCodePoint(0x10000 + b * 1024 + 7); wp.push(['1' + ch, '1\u{1CCF5}'], ['1\u{1D7CF}9', '1' + ch + 'x']); }
  for (const m of ['̀', '́', '〪', '゙', '︠', '่', 'ְ', '່', '᪰']) wp.push(['1' + m, '1\u{1D7CF}'], ['1\u{1D7CF}9', '1' + m + 'x']);
  wp.push(['1\u{1D7CF}', '1\u{1D7D0}'], ['1\u{1D7CF}9', '1\u{1D7CF}10'], ['\u{1D7CF}1', '11'], ['2\u{1200}', '2\u{1D7CF}'], ['0\u{1CCF0}', '0!']);
  // Round-4 gaps: supplementary digits before/after/inside digit runs (ICU prefix skip over UTF-16 units).
  for (const [a, b] of [['100', '1\u{1D7CE}'], ['1\u{1D7CE}0', '10'], ['1\u{1D7CE}0', '1\u{1D7CE}'], ['100', '1\u{1CCF9}'], ['100', '1\u{1D7FF}'], ['100', '1\u{1FBF9}'],
    ['1\u{1D7CE}0', '17'], ['1\u{1D7CE}0', '1９'], ['1\u{1D7CE}0', '1\u{661}'], ['1\u{1D7CE}0', '1\u{967}'],
    ['\u{4E43}\u{967}\u{668}\u{2B9}', '\u{4E43}\u{967}\u{1CCF0}\u{1CCF6}\u{1D431}\u{16A62}'],
    ['\u{1CCF2}\u{1CCF7}\u{1CCF0}\u{11066}', '\u{1CCF2}\u{1CCF7}\u{1CCF0}\u{635}\u{648}\u{63C}'],
    ['\u{1CCF8}\u{1D7CE}\u{6F7}', '\u{1CCF8}\u{E58}']]) wp.push([a, b], [b, a]);
  {
    const dig = ['0', '1', '7', '9', '\u{661}', '\u{967}', '\u{6F7}', '\u{E58}', '９', '\u{1CCF0}', '\u{1CCF6}', '\u{1CCF9}', '\u{1D7CE}', '\u{1D7D3}', '\u{1D7FF}', '\u{1FBF0}', '\u{1FBF9}',
      '\u{11066}', '\u{16A62}', '\u{1E951}', '\u{104A1}', '\u{1D431}', 'a', 'x', '\u{4E43}', '\u{2B9}', '\u{635}', '\u{300}', '!', '\u{200B}'];
    let sd = 99991; const r2 = () => (sd = (sd * 1103515245 + 12345) % 2147483648) / 2147483648;
    const mk = () => { let t = ''; const n = 1 + Math.floor(r2() * 6); for (let k = 0; k < n; k++) t += dig[Math.floor(r2() * dig.length)]; return t; };
    for (let i = 0; i < 20000; i++) {
      const a = mk(); let b;
      const m = r2();
      if (m < 0.5) { b = Array.from(a).slice(0, Math.floor(r2() * a.length)).join('') + mk(); } else if (m < 0.75) { b = mk(); } else { const c = Array.from(a); c[Math.floor(r2() * c.length)] = dig[Math.floor(r2() * dig.length)]; b = c.join(''); }
      wp.push([a, b]);
    }
  }
  // Round-5 gaps: Han compatibility chars (Kangxi/CJK radicals, circled/parenthesized/telegraph ideographs) are
  // primary-equal to their NFKC ideograph and order with it; plus Latin letters NFD does not decompose.
  {
    const blocks = [[0x2e80, 0x33ff], [0xf900, 0xfaff], [0xfe30, 0xfe4f], [0xff00, 0xffef], [0x3400, 0x4dbf], [0x4e00, 0x9fff], [0x20000, 0x2a6df], [0x2f800, 0x2fa1f], [0x3200, 0x32ff], [0x1f200, 0x1f2ff]];
    const pool = []; for (const [lo, hi] of blocks) for (let cp = lo; cp <= hi; cp++) pool.push(cp);
    const S = (cp) => String.fromCodePoint(cp);
    const compat = pool.filter((cp) => S(cp).normalize('NFKC') !== S(cp));
    const han = [0x4e28, 0x4e2d, 0x5200, 0x65e0, 0x6c14, 0x6bcd, 0x9f8d, 0x9f9f, 0x4e00, 0x23394];
    for (const cp of compat) { const n = S(cp).normalize('NFKC'); wp.push([S(cp), n], [S(cp), n + 'a'], ['1' + S(cp), '1' + n]); for (const h of han) wp.push([S(cp), S(h)]); }
    for (const [a, b] of [[0x2f01, 0x4e28], [0x2f46, 0x5200], [0x2f46, 0x65e0], [0x2e9f, 0x6bcd], [0x2ef3, 0x9f9f], [0x32a5, 0x4e2d], [0x2f53, 0x9f8d], [0x2f53, 0x6c14], [0x3281, 0x4e2d], [0x328b, 0x23394], [0x33ff, 0x4e2d]]) wp.push([S(a), S(b)], [S(b), S(a)]);
    let s3 = 777; const r3 = () => (s3 = (s3 * 1103515245 + 12345) % 2147483648) / 2147483648;
    const pc = () => { const p = r3() < 0.5 ? compat : pool; return S(p[Math.floor(r3() * p.length)]); };
    for (let i = 0; i < 20000; i++) { let a = pc(), b = pc(); if (r3() < 0.3) { a += pc(); b += pc(); } wp.push([a, b]); }
    const lat = ['Đ', 'đ', 'Ł', 'ł', 'Ø', 'ø', 'Æ', 'æ', 'Œ', 'œ', 'ð', 'þ', 'ı', 'ß', 'Ħ', 'ħ', 'ŋ', 'ƀ', 'Ｄ', 'ｌ', 'ｏ', 'ｚ'];
    for (const a of lat) for (const b of 'abcdefghijklmnopqrstuvwxyz'.split('').concat(lat)) wp.push([a, b], [a + 'x', b + 'x']);
    for (const w of ['Đorđe', 'Łódź', 'Øre', 'Æble', 'Þór', 'Straße', 'Hærvejen', 'Ðaco']) for (const x of ['d', 'dz', 'e', 'l', 'm', 'o', 'p', 'ae', 'a', 'b', 's', 'ss', 't', 'th', 'z']) wp.push([w, x], [w, x + 'x']);
  }
  const allPairs = pairs.concat(wp);
  add('nat', { pairs: allPairs }, allPairs.map(([a, b]) => Math.sign(j.OCR.natCompare(a, b))));
  // Round-6: whole-list Array.sort (V8 TimSort) with natCompare as groups.list does; natCompare is not a total
  // order for BMP/supplementary digit mixes, so only a V8-identical sort reproduces the order. Sizes cross the
  // 64-element merge/gallop thresholds; lists mix presorted runs, reversed runs and digit-heavy cyclic names.
  {
    const dig = ['0', '1', '2', '7', '9', '\u{661}', '\u{967}', '\u{6F7}', '\u{E58}', '９', '\u{1CCF0}', '\u{1CCF6}', '\u{1D7CE}', '\u{1D7D3}', '\u{1D7E3}', '\u{1D7FF}', '\u{1FBF0}',
      '\u{11066}', '\u{16A62}', '\u{104A1}', 'a', 'e', 'x', 'Å', '\u{133}', '\u{1FB3}', '\u{4E43}', '\u{300}', ' ', '\u{1D400}', '\u{1D2E8}', 'j'];
    let sd = 31337; const r = () => (sd = (sd * 1103515245 + 12345) % 2147483648) / 2147483648;
    const mk = () => { let t = ''; const n = 1 + Math.floor(r() * 6); for (let k = 0; k < n; k++) t += dig[Math.floor(r() * dig.length)]; return t; };
    const nc = (a, b) => j.OCR.natCompare(a, b);
    const lists = [['\u{96f}\u{1d7d8}00\u{133}e', '\u{96f}\u{1d7e3}\u{1fb3}0\u{c5}\u{0}f', '0j\u{1d2e8}', '\u{1d7d3}\u{1d7d6}\u{1d7d1}\u{1d400}']];
    for (let i = 0; i < 400; i++) {
      const n = [3, 5, 12, 20, 33, 63, 64, 65, 100, 150, 300][Math.floor(r() * 11)];
      let l = []; for (let k = 0; k < n; k++) l.push(mk());
      const m = r();
      if (m < 0.3) { const h = Math.floor(n / 2); l = l.slice(0, h).sort(nc).concat(l.slice(h)); }
      else if (m < 0.5) { l = l.slice().sort(nc).reverse().concat(l.slice(0, 7)); }
      else if (m < 0.6) { l = l.concat(l.slice(0, Math.floor(n / 3))); }
      lists.push(l);
    }
    const words = ['Maya R.', 'maya r', 'Zoe', 'Zoë', 'Ünal', 'Unal', 'Åsa', 'Đorđe', 'Łukasz', 'Øystein', 'Straße', 'page2', 'page10', 'page1', 'Devon T.', '王芳', 'Анна', 'a', 'A', 'á'];
    for (let i = 0; i < 100; i++) { const n = 2 + Math.floor(r() * 150); const l = []; for (let k = 0; k < n; k++) l.push(words[Math.floor(r() * words.length)] + (r() < 0.5 ? '' : String(Math.floor(r() * 30)))); lists.push(l); }
    add('natsort', { lists }, lists.map((l) => l.slice().sort(nc)));
  }
  const slugs = ['Period 3 Quiz', '  --Hello, World!--  ', 'Café Ünal', 'ÀÉÎÕÜ', '', '!!!', 'a'.repeat(60), 'Ab-Cd_Ef', 'ﬁne ﬀ', '①②③', '日本語 test', 'x'.repeat(47) + ' y z', 'x'.repeat(47) + '--y', 'ǅ', 'ＡＢＣ'];
  add('slug', { slugs }, slugs.map((s) => j.OCR.slug(s)));
}

/* ---- number coercion + toFixed ---- */
function numCases() {
  const j = loadJs();
  const strs = ['', ' ', '1', ' 2 ', '0x10', '0X1f', '0b101', '0o17', '-0x10', '1e3', '1E-2', '.5', '5.', '.', '+1', '-1', 'Infinity', '-Infinity', 'infinity', 'NaN', 'abc', '1_000', '1,5', '١٢', '\n3\t', '1e', '--1', '0.1e1', '1e21', '1e-7'];
  add('number_str', { strs }, strs.map((s) => String(Number(s))));
  const fixed = [0, 0.04, 0.05, 0.25, 0.75, 1.25, 2.25, 0.35, 1.005, 10.95, 1234.5, 0.5, 12.999, 0.001, 5, 0.3, 0.7, 99.95, 1e-7, 0.0625, 0.125, 0.375, 1.5, 2.5];
  add('to_fixed', { xs: fixed }, fixed.map((x) => x.toFixed(1)));
  const secs = [0, 250, 1234, 750, 12999, 5, 125, 62.5, 1000, 4, 9999999];
  add('csv_secs', { ms: secs }, secs.map((ms) => ((ms / 1000) || 0).toFixed(1)));
}

/* ---- OCR.clean ---- */
function cleanCases() {
  const j = loadJs();
  const clean = j.OCR.ocr.clean;
  const base = ['Sylvia plath\n-1932 born', '```\nline one\nline two\n```', 'Here is the transcription:\n\nline one\nline two', "Sure! Here's the text:\nline one", 'line one\nline two\n\nNote: some words were hard to read.', 'line one\n\nLet me know if you need anything else!', 'Transcript of the meeting\nitem 1', 'a\n\n\n\nb', 'note to self: buy milk'];
  const frags = ['line one', 'line two', '```', '```json', '``` ', 'Here is the transcription:', "Here's the text", 'here are the notes', 'Sure, here is the transcript', 'OK. The transcription', 'Okay! the text:', 'the text of the poem', 'Transcript', 'transcription:', 'handwritten text', '', '', '  ', '\t', 'Note: some words were illegible', 'Notes: the image was unclear.', '(Note that the page is hard to read)', 'Let me know if you need more', 'I hope this helps', 'Please let me know', 'This is a transcription of the page', 'This was the transcription.', 'note to self', 'notes', 'The note', 'x  ', 'y\t', ' ', 'Transcript: more', '﻿BOM', 'Of course! Here are the words', 'certainly, here is: the text'];
  const inputs = base.slice();
  for (let i = 0; i < 500; i++) { const n = 1 + Math.floor(R() * 7), a = []; for (let k = 0; k < n; k++) a.push(pick(frags)); inputs.push(a.join(pick(['\n', '\n', '\n\n', '\r\n']))); }
  inputs.push(null, undefined, 5);
  add('clean', { inputs: inputs.map((s) => (s === undefined ? { undefined: true } : s)) }, inputs.map((s) => clean(s)));
}

/* ---- render: units, held math, html with and without libs ---- */
function renderCases(libs) {
  const texts = s5.corpus.map((c) => c.text);
  for (let i = 0; i < 60; i++) texts.push(genRaw());
  const all = { units: [], noLibs: [], libs: null };
  const jn = loadJs();
  const Rn = jn.OCR.render;
  const out = texts.map((t) => ({ text: t, units: Rn.units(t), html: Rn.units(t).map((u) => Rn.unit(u)) }));
  add('render', { texts }, out.map((o) => ({ units: o.units, html: o.html })));
  /* with markdown-it + KaTeX: record every inline / math call, replay by lookup in Rust */
  if (libs) {
    const inline = {}, maths = {};
    const md = (o) => { const m = libs.markdownit(o), orig = m.renderInline.bind(m); m.renderInline = (s, e) => { const h = orig(s, e); inline[s] = h; return h; }; return m; };
    const katex = { renderToString: (tex, o) => { const h = libs.katex.renderToString(tex, o); maths[JSON.stringify([tex, !!o.displayMode])] = h; return h; } };
    const jl = loadJs({ markdownit: md, katex });
    const Rl = jl.OCR.render;
    const res = texts.map((t) => Rl.units(t).map((u) => Rl.unit(u)));
    const inlineList = Object.entries(inline), mathList = Object.entries(maths);
    return { texts, inline: inlineList, maths: mathList, html: res };
  }
  return null;
}
function bandCases() {
  const j = loadJs();
  const Rn = j.OCR.render;
  for (let i = 0; i < 80; i++) {
    const nb = 1 + Math.floor(R() * 9), withS = chance(0.5), bands = [];
    let y = 0;
    for (let k = 0; k < nb; k++) { const h = Math.round((0.02 + R() * 0.1) * 1000) / 1000; y += Math.round(R() * 50) / 1000; const b = { x: Math.round(R() * 100) / 1000, y: Math.round(y * 1000) / 1000, w: Math.round((0.5 + R() * 0.4) * 1000) / 1000, h, r: chance(0.3) ? 1.5 : 0 }; if (withS) b.s = Math.round(R() * 100) / 100; bands.push(b); y += h; }
    const n = Math.floor(R() * 10);
    add('bands', { bands, n }, Rn.mapBands(clone(bands), n));
  }
}

/* ---- export ---- */
async function exportCases() {
  for (let round = 0; round < 40; round++) {
    const n = Math.floor(R() * 5), pages = [];
    for (let i = 0; i < n; i++) { const p = genPage(i); p.id = 'e' + i; if (chance(0.15)) delete p.order; if (chance(0.1)) p.order = String(i); pages.push(p); }
    pages.forEach((p, i) => { if (chance(0.15)) p.name = pages[0].name; if (chance(0.1)) delete p.name; });
    const metaName = pick(['Period 3 Quiz', 'Café Ünal', '', '  ', 'x'.repeat(60), 'a/b: c?']);
    const meta = { name: metaName, topic: pick(['Stats', '']), prompt: pick(['', 'Transcribe.\nCarefully']), order: pages.map((p) => p.id) };
    if (chance(0.1)) delete meta.prompt;
    const j = loadJs();
    const E = j.OCR.export;
    j.S.d.project = { meta: clone(meta), pages: Object.fromEntries(pages.map((p) => [p.id, clone(p)])) };
    const list = pages.map((p) => clone(p));
    clock = 1760000000000 + round * 86400000 + 12345;
    const res = { texts: [], labels: list.map((p) => E.label(p)), csv: E.csv(list), json: E.json(list), exported: new Date().toISOString(), perPage: E.perPage(list, 'corrected'), runs: [] };
    for (const mode of ['raw', 'corrected', 'both', 'x']) res.texts.push([mode, list.map((p) => E.pageText(p, mode))]);
    for (const mode of ['raw', 'corrected', 'both']) for (const sep of ['label', 'break', 'none']) for (const fmt of ['txt', 'md', 'docx']) {
      const o = { mode, sep, fmt };
      const c = E.combined(list, o);
      const d = digest(Buffer.from(await E.docx(list, o).arrayBuffer()));
      res.runs.push({ o, combined: c, docx: d });
    }
    /* E.run through a stubbed deliver (no document/download): name, mime, bytes */
    res.deliveries = [];
    for (const kind of ['combined', 'pages', 'json', 'csv', 'copy', 'all']) for (const o of [{ mode: 'corrected', sep: 'label', fmt: 'md' }, { mode: 'both', sep: 'break', fmt: 'docx' }, { mode: 'raw', sep: 'none', fmt: 'txt' }]) {
      let got = null;
      E.deliver = async (name, blob) => { got = { name, mime: blob.type, bytes: digest(Buffer.from(await blob.arrayBuffer())) }; };
      E.copyText = async (text) => { got = { copy: text }; return true; };
      await E.run(kind, list, o);
      res.deliveries.push({ kind, o, got });
    }
    add('export', { pages, meta, exported: res.exported }, res);
  }
}

/* ---- state.js: settings and assignment ops ---- */
function stateCases() {
  const saved = [{}, { theme: 'dark', ocrUrl: 'http://h:1', unknownKey: { z: 1 }, defaultPrompt: 'stale', exportFormat: 'docx' }, { topics: { Only: { description: 'd', terms: ['a'] } }, reference: [{ word: 'x' }] }];
  for (const s of saved) {
    store.ocr_settings_v1 = JSON.stringify(s);
    const j = loadJs();
    j.S.loadSettings();
    add('settings', { saved: s }, j.S.d.settings);
  }
  const j = loadJs();
  const np = (name) => j.S.newPage(name);
  add('new_page', { name: 'a.png', id: 'id1' }, (() => { j.OCR.uid = () => 'id1'; return np('a.png'); })());
  /* assignment ops */
  for (let round = 0; round < 25; round++) {
    const jj = loadJs();
    const S = jj.S;
    let k = 0;
    jj.OCR.uid = () => 'n' + (++k);
    const meta = { name: 'A', order: [], futureField: { z: 1 } };
    S.d.project = { meta, pages: {} };
    S.d.selectedId = 'sel'; /* selection is UI state; keep select() out of the comparison */
    const ops = [];
    const steps = 3 + Math.floor(R() * 6);
    for (let s = 0; s < steps; s++) {
      const r = R(), ids = meta.order;
      if (r < 0.4 || !ids.length) { const cnt = 1 + Math.floor(R() * 3); const names = []; for (let c = 0; c < cnt; c++) names.push(pick(['a.png', 'b.jpg', 'c', 'd.PNG'])); ops.push({ op: 'add', names }); const list = names.map((nm) => S.newPage(nm)); ops[ops.length - 1].ids = list.map((p) => p.id); S.addPages(list); }
      else if (r < 0.6) { const id = pick(ids); ops.push({ op: 'remove', id }); S.removePage(id); }
      else if (r < 0.8) { const a = Math.floor(R() * (ids.length + 1)) - (chance(0.1) ? 1 : 0), b = Math.floor(R() * ids.length); ops.push({ op: 'move', from: a, to: b }); S.movePage(a, b); }
      else { const id = pick(ids); const patch = pick([{ status: 'done' }, { status: 'error', error: 'boom' }, { raw: 'hi' }, { status: 'transcribing' }]); ops.push({ op: 'update', id, patch }); S.updatePage(id, clone(patch)); }
    }
    add('assignment', { ops }, { meta: clone(S.d.project.meta), pages: clone(S.d.project.pages), order: S.pages().map((p) => p.id), counts: S.counts() });
  }
}

/* ---------- main ---------- */
(async () => {
  let libs = null;
  const vend = path.join(REPO, 'spikes', 's5-markdown-katex', 'vendor');
  try {
    libs = { markdownit: require(path.join(vend, 'markdown-it-14.1.0', 'dist', 'markdown-it.min.js')), katex: require(path.join(vend, 'katex-0.16.9', 'dist', 'katex.min.js')) };
  } catch (e) { console.warn('markdown-it/katex vendor files not found; keeping the committed "render_libs" section: ' + e.message); }
  await remoteCases(); await studentCases(); await guessCases();
  groupCases(); natCases(); numCases(); cleanCases(); bandCases(); await exportCases(); stateCases();
  const rl = renderCases(libs);
  let renderLibs = rl;
  if (!renderLibs) { try { renderLibs = JSON.parse(fs.readFileSync(FIXTURE, 'utf8')).render_libs; } catch (e) { renderLibs = null; } }
  const fixture = { generated_by: 'tools/parity/run.js', note: 'Regenerate with: node tools/parity/run.js. Results come from the unmodified js/*.js.', cases, render_libs: renderLibs };
  fs.writeFileSync(FIXTURE, JSON.stringify(fixture));
  console.log('fixture: ' + cases.length + ' cases' + (renderLibs ? ', render_libs ' + renderLibs.texts.length + ' texts' : ', NO render_libs') + ' -> ' + path.relative(REPO, FIXTURE));
  if (process.argv.includes('--no-cargo')) return;
  const r = cp.spawnSync('cargo', ['test', '-p', 'vishion-core'], { cwd: REPO, stdio: 'inherit', shell: process.platform === 'win32' });
  process.exit(r.status === null ? 1 : r.status);
})().catch((e) => { console.error(e); process.exit(1); });
