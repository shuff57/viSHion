/* node run.js -> byte-identity check of js/render.js (JS) vs Rust pipeline + markdown-it/KaTeX via wasm-bindgen,
   plus glue-cost numbers. Writes inline-cases.json (renderInline inputs/outputs) for the pulldown-cmark test. */
const fs = require('fs'), path = require('path'), zlib = require('zlib');
const REPO = path.resolve(__dirname, '..', '..');
const markdownit = require('./vendor/markdown-it-14.1.0/dist/markdown-it.min.js');
const katex = require('./vendor/katex-0.16.9/dist/katex.min.js');
const { corpus, synthetic } = require('./corpus');

/* --- JS path: the real js/render.js, loaded unmodified --- */
const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const inlineCases = [];
const win = {
  OCR: { render: {}, image: {}, esc: (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ESC[c]) },
  markdownit: (o) => { const m = markdownit(o), orig = m.renderInline.bind(m); m.renderInline = (s, e) => { const h = orig(s, e); inlineCases.push({ t: s, html: h }); return h; }; return m; },
  katex,
};
new Function('window', fs.readFileSync(path.join(REPO, 'js', 'render.js'), 'utf8'))(win);
const R = win.OCR.render;

/* --- Rust path --- */
globalThis.markdownit = markdownit; globalThis.katex = katex;
const rs = require('./pkg-node/vishion_s5.js');
const rust = new rs.Renderer();

let total = 0, units = 0, bad = 0, unitMismatch = 0;
const mism = [];
const check = (name, text) => {
  total++;
  const ju = R.units(text), ru = rust.units(text);
  if (JSON.stringify(ju) !== JSON.stringify(ru)) { unitMismatch++; mism.push({ name, kind: 'units', js: ju, rust: ru }); return; }
  for (const u of ju) {
    units++;
    let jh, rh;
    try { jh = R.unit(u); } catch (e) { jh = 'THROW:' + e.message; }
    try { rh = rust.unit(u); } catch (e) { rh = 'THROW:' + (e && e.message); }
    if (jh !== rh) { bad++; mism.push({ name, kind: 'html', unit: u, js: jh, rust: rh }); }
  }
};
corpus.forEach((c) => check(c.name, c.text));
console.log(`corpus: ${corpus.length} transcripts (${synthetic.length} synthetic), ${units} units`);
console.log(`unit-split mismatches: ${unitMismatch}; html byte mismatches: ${bad}`);
mism.slice(0, 8).forEach((m) => console.log(JSON.stringify(m).slice(0, 600)));

/* katex missing => code.math-fb fallback, same in both */
{
  const saveW = win.katex, saveG = globalThis.katex;
  win.katex = undefined; delete globalThis.katex;
  const u = 'a $x<y$ and $$z$$';
  console.log('no-katex fallback identical:', R.unit(u) === rust.unit(u), JSON.stringify(rust.unit(u)));
  win.katex = saveW; globalThis.katex = saveG;
}

/* --- hostile boundary inputs (documented divergences, not asserted) --- */
const hostile = { 'lone high surrogate': 'a\ud83d b $x$', 'lone low surrogate': '\ude00 $x$', 'NUL': 'a\u0000b', 'valid astral': '😀 $\\text{😀}$' };
for (const [k, v] of Object.entries(hostile)) {
  const j = R.unit(v), r = rust.unit(v);
  console.log(`hostile ${k}: ${j === r ? 'same' : 'DIFF js=' + JSON.stringify(j).slice(0, 60) + ' rust=' + JSON.stringify(r).slice(0, 60)}`);
}

/* --- glue cost --- */
const texts = corpus.map((c) => c.text), N = 40;
const time = (f) => { const t = process.hrtime.bigint(); f(); return Number(process.hrtime.bigint() - t) / 1e6; };
const jsAll = () => { for (const t of texts) for (const u of R.units(t)) R.unit(u); };
const rsAll = () => { for (const t of texts) rust.render_all(t); };
jsAll(); rsAll();
let a = [], b = [];
for (let i = 0; i < 5; i++) { a.push(time(() => { for (let k = 0; k < N; k++) jsAll(); })); b.push(time(() => { for (let k = 0; k < N; k++) rsAll(); })); }
const med = (x) => x.slice().sort((p, q) => p - q)[Math.floor(x.length / 2)];
const nUnits = units * N;
console.log(`full pipeline, ${nUnits} units: JS ${med(a).toFixed(1)} ms (${(med(a) * 1000 / nUnits).toFixed(1)} us/unit), Rust ${med(b).toFixed(1)} ms (${(med(b) * 1000 / nUnits).toFixed(1)} us/unit)`);
const md = markdownit({ html: false, linkify: false, breaks: true });
const short = 'Margin: check the value x two *em* ~~s~~ [illegible]', M = 20000;
const t1 = time(() => { for (let i = 0; i < M; i++) md.renderInline(short); });
const t2 = time(() => { for (let i = 0; i < M; i++) rust.bare_inline(short); });
console.log(`bare renderInline x${M}: JS direct ${(t1 * 1000 / M).toFixed(2)} us, via Rust extern ${(t2 * 1000 / M).toFixed(2)} us, boundary overhead ${((t2 - t1) * 1000 / M).toFixed(2)} us/call`);
const K = 3000, tex = 'x^2 + \\frac{a}{b}';
const o = { displayMode: false, throwOnError: false };
const t3 = time(() => { for (let i = 0; i < K; i++) katex.renderToString(tex, o); });
const t4 = time(() => { for (let i = 0; i < K; i++) rust.bare_katex(tex); });
console.log(`bare katex x${K}: JS direct ${(t3 * 1000 / K).toFixed(1)} us, via Rust ${(t4 * 1000 / K).toFixed(1)} us, overhead ${((t4 - t3) * 1000 / K).toFixed(2)} us/call`);

const gz = (f) => zlib.gzipSync(fs.readFileSync(f)).length;
for (const d of ['pkg-web']) {
  const wasm = path.join(__dirname, d, 'vishion_s5_bg.wasm'), js = path.join(__dirname, d, 'vishion_s5.js');
  if (fs.existsSync(wasm)) console.log(`${d}: wasm ${fs.statSync(wasm).size} B (gz ${gz(wasm)}), glue js ${fs.statSync(js).size} B (gz ${gz(js)})`);
}
const uniq = [...new Map(inlineCases.filter((c) => c.t.isWellFormed()).map((c) => [c.t, c])).values()];
fs.writeFileSync(path.join(__dirname, 'inline-cases.json'), JSON.stringify(uniq));
console.log('distinct renderInline inputs recorded:', uniq.length);
process.exit(bad || unitMismatch ? 1 : 0);
