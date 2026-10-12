'use strict';
/* How often would a 1-ulp cos/sin difference flip a Math.round bin? Synthetic points shaped like the sample. */
const f = new Float64Array(1), u = new BigInt64Array(f.buffer);
const bump = (v, d) => { f[0] = v; u[0] += BigInt(d); return f[0]; };
let flips = 0, n = 0;
for (let ai = -200; ai <= 200; ai++) { if (ai === 0) continue; // cos(0)=1 and sin(0)=0 are exact on both sides
  const r = (ai * 0.05 * Math.PI) / 180, s = Math.sin(r), c = Math.cos(r);
  const s2 = bump(s, 1), c2 = bump(c, 1);
  for (let x = -300; x <= 300; x += 0.5) for (let y = -400; y <= 400; y += 0.5) {
    n++;
    if (Math.round(-x * s + y * c) !== Math.round(-x * s2 + y * c2)) flips++;
  }
}
console.log(`points*angles=${n} flips=${flips} rate=${(flips / n).toExponential(2)}`);
