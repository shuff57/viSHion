'use strict';
/* Deterministic synthetic page photos. Every fixture is an opaque RGBA buffer {w,h,data}. */

function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/* spec: {W,H,angle,ruled,noise,border:'none'|'white'|'color',shadow,lines,pitch,seed,blank} */
function makePage(spec) {
  const { W, H } = spec, R = rng(spec.seed || 1), s = W / 600;
  const paper = [236, 234, 228];
  const page = new Uint8Array(W * H * 3);
  for (let i = 0; i < W * H; i++) { page[i * 3] = paper[0]; page[i * 3 + 1] = paper[1]; page[i * 3 + 2] = paper[2]; }
  const put = (x, y, c) => { if (x >= 0 && y >= 0 && x < W && y < H) { const i = (y * W + x) * 3; page[i] = c[0]; page[i + 1] = c[1]; page[i + 2] = c[2]; } };
  const pitch = Math.round((spec.pitch || 30) * s);
  const top = Math.round(0.1 * H), nLines = spec.blank ? 0 : (spec.lines || Math.floor((H * 0.8) / pitch));
  if (spec.ruled) {
    for (let l = 0; l < Math.floor((H * 0.85) / pitch); l++) {
      const y = top + l * pitch + Math.round(4 * s);
      for (let x = 0; x < W; x++) put(x, y, [165, 185, 228]);
    }
    for (let y = 0; y < H; y++) { put(Math.round(0.07 * W), y, [225, 140, 140]); }
  }
  for (let l = 0; l < nLines; l++) {
    const base = top + l * pitch;
    let x = Math.round(W * (0.09 + R() * 0.02));
    const end = Math.round(W * (0.55 + R() * 0.38));
    while (x < end) {
      const nl = 2 + Math.floor(R() * 6); // letters per word
      for (let k = 0; k < nl && x < end; k++) {
        const lw = Math.max(2, Math.round((2 + R() * 3) * s));
        let gh = Math.round((7 + R() * 3) * s);
        if (R() < 0.25) gh = Math.round(gh * 1.7);
        const desc = R() < 0.15 ? Math.round(3 * s) : 0;
        const c = 25 + Math.floor(R() * 30);
        for (let yy = base - gh; yy < base + desc; yy++) for (let xx = x; xx < x + lw; xx++) put(xx, yy, [c, c, c + 8]);
        if (R() < 0.5) for (let xx = x; xx < x + lw + 2; xx++) put(xx, base - Math.round(4 * s), [c, c, c + 8]);
        x += lw + Math.max(1, Math.round(R() * 2 * s));
      }
      x += Math.round((5 + R() * 4) * s);
    }
  }
  /* rotate (clockwise on screen) about the centre; bilinear; outside the page uses the border colour */
  const th = (spec.angle * Math.PI) / 180, cs = Math.cos(th), sn = Math.sin(th);
  const bcol = spec.border === 'color' ? [118, 82, 56] : spec.border === 'white' ? [255, 255, 255] : paper;
  const m = spec.border && spec.border !== 'none' ? Math.round(0.05 * W) : 0; // exposed margin
  const data = new Uint8ClampedArray(W * H * 4);
  const cx = W / 2, cy = H / 2;
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const dx = x + 0.5 - cx, dy = y + 0.5 - cy;
      const sx = cs * dx + sn * dy + cx - 0.5, sy = -sn * dx + cs * dy + cy - 0.5;
      let c;
      if (sx < m || sy < m || sx > W - 1 - m || sy > H - 1 - m) c = bcol;
      else {
        const x0 = Math.floor(sx), y0 = Math.floor(sy), fx = sx - x0, fy = sy - y0;
        const x1 = Math.min(W - 1, x0 + 1), y1 = Math.min(H - 1, y0 + 1);
        c = [0, 0, 0].map((_, k) => {
          const a = page[(y0 * W + x0) * 3 + k], b = page[(y0 * W + x1) * 3 + k], cc = page[(y1 * W + x0) * 3 + k], d = page[(y1 * W + x1) * 3 + k];
          return Math.round(a * (1 - fx) * (1 - fy) + b * fx * (1 - fy) + cc * (1 - fx) * fy + d * fx * fy);
        });
      }
      const i = (y * W + x) * 4;
      const g = spec.shadow ? 1 - 0.28 * (x / W) - 0.1 * (y / H) : 1;
      for (let k = 0; k < 3; k++) {
        let v = c[k] * g;
        if (spec.noise) v += (R() * 2 - 1) * spec.noise;
        data[i + k] = Math.max(0, Math.min(255, Math.round(v)));
      }
      data[i + 3] = 255;
    }
  }
  return { w: W, h: H, data };
}

const BASE = [
  ['plain_0', { angle: 0 }],
  ['plain_-8', { angle: -8 }],
  ['plain_+8', { angle: 8 }],
  ['plain_-3', { angle: -3 }],
  ['plain_+1.5', { angle: 1.5 }],
  ['plain_+0.3', { angle: 0.3 }],
  ['ruled_0', { angle: 0, ruled: 1 }],
  ['ruled_-5', { angle: -5, ruled: 1 }],
  ['ruled_+4', { angle: 4, ruled: 1 }],
  ['ruled_+7.5', { angle: 7.5, ruled: 1 }],
  ['plain_+2_noise', { angle: 2, noise: 14 }],
  ['ruled_-2_noise', { angle: -2, ruled: 1, noise: 14 }],
  ['plain_-6_noise_shadow', { angle: -6, noise: 10, shadow: 1 }],
  ['plain_+3_whiteborder', { angle: 3, border: 'white' }],
  ['plain_-4_colorborder', { angle: -4, border: 'color' }],
  ['ruled_+5_colorborder_noise', { angle: 5, ruled: 1, border: 'color', noise: 12 }],
  ['blank', { angle: 0, blank: 1 }],
  ['blank_noise', { angle: 0, blank: 1, noise: 20 }],
  ['sparse_3lines', { angle: -1, lines: 3, pitch: 60 }],
  ['dense_-1', { angle: -1, pitch: 20 }],
];

function allFixtures() {
  const out = [];
  BASE.forEach(([name, o], i) => {
    for (const W of [420, 600]) {
      const H = Math.round((W * 4) / 3);
      out.push({ name: name + '_w' + W, nominal: o.angle, ...makePage({ W, H, seed: 100 + i, ...o }) });
    }
  });
  return out;
}

function fuzzFixtures(n) {
  const R = rng(777), out = [];
  for (let i = 0; i < n; i++) {
    const W = R() < 0.5 ? 420 : 600, H = 300 + Math.floor(R() * 500);
    const o = { angle: Math.round((R() * 24 - 12) * 100) / 100, ruled: R() < 0.4, noise: R() < 0.5 ? 6 + R() * 20 : 0,
      border: ['none', 'white', 'color'][Math.floor(R() * 3)], shadow: R() < 0.2, pitch: 18 + Math.floor(R() * 40) };
    out.push({ name: 'fuzz' + i, nominal: o.angle, ...makePage({ W, H, seed: 5000 + i, ...o }) });
  }
  return out;
}

module.exports = { allFixtures, fuzzFixtures, makePage, rng };
