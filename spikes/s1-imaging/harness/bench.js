'use strict';
/* node harness/bench.js  (fresh process: reports cold first call and median of 20 warm runs) */
const { I, recorded } = require('./jsenv');
const wasm = require('../pkg/imaging_wasm.js');
const { makePage } = require('./fixtures');
const u8 = (d) => new Uint8Array(d.buffer, d.byteOffset, d.length);
const now = () => Number(process.hrtime.bigint()) / 1e6;
const med = (a) => { const s = [...a].sort((x, y) => x - y); return (s[9] + s[10]) / 2; };

const specs = [
  ['plain page, 22 lines', { angle: 3, noise: 8 }],
  ['ruled page + noise', { angle: -4, ruled: 1, noise: 14 }],
];
async function main() {
  const out = [];
  for (const [label, o] of specs) {
    for (const W of [420, 600]) {
      const H = Math.round((W * 4) / 3);
      const f = makePage({ W, H, seed: 9, ...o });
      for (const fn of ['lineBands', 'skewAngle']) {
        // pixels exactly as the JS function receives them
        recorded.length = 0;
        const jsCall = () => I[fn](f);
        // cold JS (first call, fresh process for the first spec only is meaningful)
        let t = now(); await jsCall(); const jsCold = now() - t;
        const px = recorded[0];
        const d = u8(px.data);
        const wCall = fn === 'lineBands' ? () => wasm.lineBands(d, px.w, px.h) : () => wasm.skewAngle(d, px.w, px.h);
        t = now(); wCall(); const wCold = now() - t;
        for (let i = 0; i < 5; i++) { await jsCall(); wCall(); }
        const js = [], ws = [];
        for (let i = 0; i < 20; i++) { t = now(); await jsCall(); js.push(now() - t); }
        for (let i = 0; i < 20; i++) { t = now(); wCall(); ws.push(now() - t); }
        out.push({ label, fn, srcW: W, work: px.w + 'x' + px.h, jsCold, wCold, jsMed: med(js), wMed: med(ws) });
      }
    }
  }
  console.log('| page | fn | source W | work size | JS cold ms | WASM cold ms | JS median ms | WASM median ms | speedup (warm) |');
  console.log('|---|---|---|---|---|---|---|---|---|');
  for (const r of out) console.log(`| ${r.label} | ${r.fn} | ${r.srcW} | ${r.work} | ${r.jsCold.toFixed(2)} | ${r.wCold.toFixed(2)} | ${r.jsMed.toFixed(2)} | ${r.wMed.toFixed(2)} | ${(r.jsMed / r.wMed).toFixed(1)}x |`);
}
main();
