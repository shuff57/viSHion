/* Straightens a tilted page photo so text rows run horizontally, which is what the line-box
   scan (image.lineBands) assumes. Finds the angle whose row projection of ink is sharpest.
   Rotation only: it does not fix perspective (a page photographed from a steep angle). */
(function () {
  'use strict';
  const OCR = window.OCR, I = OCR.image;
  const RANGE = 10, MIN = 0.4, W = 600;

  /* Average colour of the four corners, so exposed corners blend in instead of drawing a white edge. */
  async function edgeColor(bmp) {
    const c = document.createElement('canvas');
    c.width = c.height = 16;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(bmp, 0, 0, 16, 16);
    const d = g.getImageData(0, 0, 16, 16).data, at = [0, 15, 15 * 16, 15 * 16 + 15];
    const t = [0, 0, 0];
    at.forEach((i) => { t[0] += d[i * 4]; t[1] += d[i * 4 + 1]; t[2] += d[i * 4 + 2]; });
    return 'rgb(' + t.map((v) => Math.round(v / at.length)).join(',') + ')';
  }

  async function sample(blob) {
    const bmp = await createImageBitmap(blob);
    const w = Math.min(W, bmp.width), h = Math.max(1, Math.round((bmp.height * w) / bmp.width));
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.fillStyle = '#fff'; g.fillRect(0, 0, w, h);
    g.drawImage(bmp, 0, 0, w, h);
    const px = g.getImageData(0, 0, w, h).data, lum = new Uint8Array(w * h);
    for (let i = 0; i < w * h; i++) lum[i] = (px[i * 4] * 299 + px[i * 4 + 1] * 587 + px[i * 4 + 2] * 114) / 1000;
    /* Ink is a pixel well below the average around it, so a dark table or a shadow is not mistaken for writing.
       The outer 4% is skipped: that is where the table and page edges are. */
    const S = w + 1, sum = new Float64Array(S * (h + 1)), R = Math.max(6, Math.round(w * 0.06));
    for (let y = 0; y < h; y++) { let run = 0; for (let x = 0; x < w; x++) { run += lum[y * w + x]; sum[(y + 1) * S + x + 1] = sum[y * S + x + 1] + run; } }
    const mean = (x, y) => {
      const x0 = Math.max(0, x - R), x1 = Math.min(w, x + R + 1), y0 = Math.max(0, y - R), y1 = Math.min(h, y + R + 1);
      return (sum[y1 * S + x1] - sum[y0 * S + x1] - sum[y1 * S + x0] + sum[y0 * S + x0]) / ((x1 - x0) * (y1 - y0));
    };
    const mx = Math.round(w * 0.04), my = Math.round(h * 0.04), xs = [], ys = [], longRun = Math.round(w * 0.3);
    for (let y = my; y < h - my; y++) {
      let x = mx;
      while (x < w - mx) {
        if (!(lum[y * w + x] < mean(x, y) * 0.8)) { x++; continue; }
        let e = x; /* a run of ink; writing has short ones, a table or page edge has a long straight one */
        while (e < w - mx && lum[y * w + e] < mean(e, y) * 0.8) e++;
        if (e - x < longRun) for (let k = x; k < e; k++) { xs.push(k - w / 2); ys.push(y - h / 2); }
        x = e;
      }
    }
    if (bmp.close) bmp.close();
    return { xs, ys, h, w, size: bmp.width };
  }

  const score = (p, deg, step) => {
    const r = (deg * Math.PI) / 180, sin = Math.sin(r), cos = Math.cos(r), bins = new Float32Array(p.h * 2 + 4), off = p.h + 2;
    for (let i = 0; i < p.xs.length; i += step) { const y = Math.round(-p.xs[i] * sin + p.ys[i] * cos) + off; if (y >= 0 && y < bins.length) bins[y]++; }
    let t = 0;
    for (let i = 0; i < bins.length; i++) t += bins[i] * bins[i];
    return t;
  };

  /* Returns the clockwise tilt of the text in degrees (0 when unsure). */
  I.skewAngle = async (blob) => {
    const p = await sample(blob);
    if (p.xs.length < 400) return 0;
    const step = Math.max(1, Math.floor(p.xs.length / 20000));
    let best = 0, top = -1, flat = score(p, 0, step);
    for (let a = -RANGE; a <= RANGE; a += 0.5) { const s = score(p, a, step); if (s > top) { top = s; best = a; } }
    for (let a = best - 0.5; a <= best + 0.5; a += 0.1) { const s = score(p, a, step); if (s > top) { top = s; best = a; } }
    if (Math.abs(best) < MIN || top < flat * 1.03) return 0; /* not clearly better than as-is */
    return Math.round(best * 10) / 10;
  };

  /* Returns { blob, angle }. blob is the input when no correction was needed. */
  I.deskew = async (blob) => {
    let angle = 0;
    try { angle = await I.skewAngle(blob); } catch (e) { angle = 0; }
    if (!angle) return { blob, angle: 0 };
    const bmp = await createImageBitmap(blob);
    const c = document.createElement('canvas');
    c.width = bmp.width; c.height = bmp.height;
    const g = c.getContext('2d');
    g.fillStyle = await edgeColor(bmp); g.fillRect(0, 0, c.width, c.height);
    g.translate(c.width / 2, c.height / 2);
    g.rotate((-angle * Math.PI) / 180);
    g.drawImage(bmp, -bmp.width / 2, -bmp.height / 2);
    if (bmp.close) bmp.close();
    const out = await new Promise((r) => c.toBlob(r, 'image/jpeg', 0.9));
    c.width = c.height = 0;
    return { blob: out, angle };
  };
})();
