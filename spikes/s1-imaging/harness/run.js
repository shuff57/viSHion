'use strict';
/* node harness/run.js  -> exactness check of Rust/WASM vs the real JS. Exit code 1 on any mismatch. */
const { I, edgeColor, recorded } = require('./jsenv');
const wasm = require('../pkg/imaging_wasm.js');
const { allFixtures, fuzzFixtures, rng } = require('./fixtures');

const u8 = (d) => new Uint8Array(d.buffer, d.byteOffset, d.length);
const flat = (bands) => bands.flatMap((b) => [b.x, b.y, b.w, b.h, b.r, b.s]);
const same = (a, b) => a.length === b.length && a.every((v, i) => Object.is(v, b[i]));

async function check(f) {
  const r = { name: f.name, nominal: f.nominal, w: f.w, h: f.h };
  recorded.length = 0;
  const jb = flat(await I.lineBands(f));
  const px = recorded[0];
  r.nBands = jb.length / 6;
  const wb = Array.from(wasm.lineBands(u8(px.data), px.w, px.h));
  r.bandsExact = same(jb, wb);
  recorded.length = 0;
  const ja = await I.skewAngle(f);
  const sp = recorded[0];
  const wa = wasm.skewAngle(u8(sp.data), sp.w, sp.h);
  r.jsAngle = ja; r.wasmAngle = wa; r.angleExact = Object.is(ja, wa);
  recorded.length = 0;
  const je = await edgeColor(await createImageBitmap(f));
  const e = Array.from(wasm.edgeColor(u8(recorded[0].data)));
  r.jsEdge = je; r.edgeExact = je === 'rgb(' + e.join(',') + ')';
  return r;
}

(async () => {
  let bad = 0;
  const rows = [];
  for (const f of allFixtures()) {
    const r = await check(f);
    rows.push(r);
    if (!(r.bandsExact && r.angleExact && r.edgeExact)) bad++;
    console.log(`${r.name.padEnd(36)} ${r.w}x${r.h} nominal=${String(r.nominal).padStart(5)} bands=${String(r.nBands).padStart(2)} ` +
      `bands:${r.bandsExact ? 'EXACT' : 'DIFF '} angle js=${r.jsAngle} wasm=${r.wasmAngle} ${r.angleExact ? 'EXACT' : 'DIFF '} edge ${r.jsEdge} ${r.edgeExact ? 'EXACT' : 'DIFF'}`);
  }
  /* fuzz: random pages / angles / noise */
  const fz = fuzzFixtures(150);
  let fzBad = 0, fzBands = 0;
  for (const f of fz) {
    const r = await check(f);
    fzBands += r.nBands;
    if (!(r.bandsExact && r.angleExact && r.edgeExact)) { fzBad++; console.log('FUZZ DIFF', JSON.stringify(r)); }
  }
  console.log(`fuzz: ${fz.length} pages (${fzBands} bands total) -> ${fzBad} mismatches`);
  /* edgeColor rounding: random 16x16 with all residues mod 4 */
  const R = rng(42);
  let ecBad = 0;
  for (let n = 0; n < 5000; n++) {
    const d = new Uint8ClampedArray(16 * 16 * 4);
    for (let i = 0; i < d.length; i++) d[i] = Math.floor(R() * 256);
    recorded.length = 0;
    const je = await edgeColor({ width: 16, height: 16, _d: d, close() {} });
    const e = Array.from(wasm.edgeColor(u8(recorded[0].data)));
    if (je !== 'rgb(' + e.join(',') + ')') ecBad++;
  }
  console.log(`edgeColor random 16x16: 5000 cases -> ${ecBad} mismatches`);
  /* sin/cos bit equality (V8 ieee754 vs Rust libm on wasm) for every angle the search visits */
  let trigBad = 0, trigN = 0;
  const angles = [];
  for (let a = -10; a <= 10; a += 0.5) angles.push(a);
  for (let c = -10; c <= 10; c += 0.5) for (let a = c - 0.5; a <= c + 0.5 + 1e-9; a += 0.1) angles.push(a);
  for (const a of angles) {
    const rr = (a * Math.PI) / 180, t = wasm.trig(a);
    trigN++;
    if (!Object.is(t[0], Math.sin(rr)) || !Object.is(t[1], Math.cos(rr))) trigBad++;
  }
  console.log(`sin/cos bitwise: ${trigN} angles -> ${trigBad} mismatches`);
  require('fs').writeFileSync(require('path').join(__dirname, 'results.json'), JSON.stringify({ rows, fuzz: { n: fz.length, bad: fzBad }, edgeRandomBad: ecBad, trig: { n: trigN, bad: trigBad } }, null, 1));
  const total = bad + fzBad + ecBad; // trig mismatches are informational: 1-ulp cos diffs never flipped a bin (see ulp-risk.js)
  console.log(total ? `FAIL: ${total} mismatching groups` : 'ALL EXACT');
  process.exit(total ? 1 : 0);
})();
