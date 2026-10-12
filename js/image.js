/* Image preprocessing: EXIF-correct, downscale, re-encode, thumbnails, line detection. */
(function () {
  'use strict';
  const OCR = window.OCR, I = OCR.image;
  const MAX = 1600, QUALITY = 0.9, MAX_BYTES = 40 * 1024 * 1024;

  /* Returns an error message for files we cannot take, or null when the file is fine. */
  I.check = (file) => {
    const n = (file.name || '').toLowerCase();
    if (/\.(heic|heif)$/.test(n) || /hei[cf]/.test(file.type)) return 'HEIC images are not supported here. Convert the file to JPG or PNG first.';
    if (/\.pdf$/.test(n) || file.type === 'application/pdf') return 'PDFs are not supported here. Convert each page to JPG or PNG first.';
    if (!/^image\/(png|jpe?g|webp)$/i.test(file.type) && !/\.(png|jpe?g|webp)$/.test(n)) return 'Unsupported file type. Use PNG, JPG or WebP.';
    if (file.size > MAX_BYTES) return 'The file is larger than 40 MB.';
    return null;
  };

  async function draw(bmp, deg) {
    const s = Math.min(1, MAX / Math.max(bmp.width, bmp.height));
    const w = Math.round(bmp.width * s), h = Math.round(bmp.height * s);
    const swap = deg % 180 !== 0;
    const c = document.createElement('canvas');
    c.width = swap ? h : w; c.height = swap ? w : h;
    const g = c.getContext('2d');
    g.fillStyle = '#fff'; g.fillRect(0, 0, c.width, c.height);
    g.translate(c.width / 2, c.height / 2);
    g.rotate((deg * Math.PI) / 180);
    g.drawImage(bmp, -w / 2, -h / 2, w, h);
    const blob = await new Promise((r) => c.toBlob(r, 'image/jpeg', QUALITY));
    const out = { blob, width: c.width, height: c.height };
    c.width = c.height = 0; /* release the canvas memory right away */
    return out;
  }

  /* One full-size decoded image at a time: callers await this in sequence. */
  I.prepare = async (file) => {
    let bmp;
    try { bmp = await createImageBitmap(file, { imageOrientation: 'from-image' }); }
    catch (e) { bmp = await createImageBitmap(file); }
    try { return await draw(bmp, 0); } finally { if (bmp.close) bmp.close(); }
  };

  I.rotate = async (blob, deg) => {
    const bmp = await createImageBitmap(blob);
    try { return await draw(bmp, deg); } finally { if (bmp.close) bmp.close(); }
  };

  I.thumb = async (blob, h) => {
    h = h || 160;
    const bmp = await createImageBitmap(blob);
    const s = h / bmp.height;
    const c = document.createElement('canvas');
    c.width = Math.max(1, Math.round(bmp.width * s)); c.height = h;
    c.getContext('2d').drawImage(bmp, 0, 0, c.width, c.height);
    if (bmp.close) bmp.close();
    return new Promise((r) => c.toBlob((b) => r(URL.createObjectURL(b)), 'image/jpeg', 0.7));
  };

  I.toDataURL = (blob) => new Promise((res, rej) => {
    const r = new FileReader();
    r.onload = () => res(r.result);
    r.onerror = () => rej(r.error);
    r.readAsDataURL(blob);
  });

  /* Finds bands of ink by row. Returns normalized boxes [{x,y,w,h,r}].
     This is a heuristic stand-in: it works for plain paper, not ruled or heavily shadowed pages. */
  I.lineBands = async (blob) => {
    const bmp = await createImageBitmap(blob);
    const W = 420, H = Math.max(40, Math.round((bmp.height * W) / bmp.width));
    const c = document.createElement('canvas');
    c.width = W; c.height = H;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(bmp, 0, 0, W, H);
    if (bmp.close) bmp.close();
    const px = g.getImageData(0, 0, W, H).data, lum = new Uint8Array(W * H);
    for (let i = 0; i < W * H; i++) {
      lum[i] = (px[i * 4] * 299 + px[i * 4 + 1] * 587 + px[i * 4 + 2] * 114) / 1000;
    }
    /* Local background by box mean (integral image). A pixel is ink when it is well below the mean around it. */
    const S = W + 1, sum = new Float64Array(S * (H + 1)), R = Math.max(8, Math.round(W * 0.06));
    for (let y = 0; y < H; y++) {
      let run = 0;
      for (let x = 0; x < W; x++) { run += lum[y * W + x]; sum[(y + 1) * S + x + 1] = sum[y * S + x + 1] + run; }
    }
    const mean = (x, y) => {
      const x0 = Math.max(0, x - R), x1 = Math.min(W, x + R + 1), y0 = Math.max(0, y - R), y1 = Math.min(H, y + R + 1);
      return (sum[y1 * S + x1] - sum[y0 * S + x1] - sum[y1 * S + x0] + sum[y0 * S + x0]) / ((x1 - x0) * (y1 - y0));
    };
    const row = new Float32Array(H), x0 = new Int16Array(H).fill(W), x1 = new Int16Array(H).fill(-1);
    const mx = Math.round(W * 0.04), my = Math.round(H * 0.04);
    /* Ink runs along a row. Writing makes short runs; a ruled line, a page edge or the edge of the table makes
       one long straight run, which would otherwise turn into a band of its own or glue two lines together. */
    const longRun = Math.round(W * 0.25);
    for (let y = my; y < H - my; y++) {
      let x = mx;
      while (x < W - mx) {
        if (!(lum[y * W + x] < mean(x, y) * 0.88)) { x++; continue; }
        let e = x + 1;
        while (e < W - mx && lum[y * W + e] < mean(e, y) * 0.88) e++;
        if (e - x < longRun) { row[y] += e - x; if (x < x0[y]) x0[y] = x; if (e - 1 > x1[y]) x1[y] = e - 1; }
        x = e;
      }
    }
    const sm = Array.from(row, (v, y) => (v + (row[y - 1] || 0) + (row[y + 1] || 0)) / 3);
    const live = sm.filter((v) => v > 0).sort((a, b) => a - b);
    /* Rows from a page edge or paper grain sit at a low, steady level; writing rises well above it. Take the
       threshold from the gap between that noise and the strong rows, so a page with few lines still works. */
    const noise = live.length ? live[Math.floor(live.length * 0.4)] : 0;
    const strong = live.length ? live[Math.min(live.length - 1, Math.floor(live.length * 0.92))] : 0;
    const typical = Math.max(noise, strong * 0.7);
    const p75 = live.length ? live[Math.floor(live.length * 0.75)] : 0;
    const thr = Math.max(2, p75 * 0.3, noise + (strong - noise) * 0.25); /* the stricter of the two readings */
    /* The brightness of the page itself, to tell a band of writing from the dark table or a shadow. */
    const sorted = Array.from(lum).sort((a, b) => a - b), paper = sorted[Math.floor(sorted.length * 0.75)] || 255;
    let bands = [];
    let s = -1, gap = 0;
    for (let y = 0; y <= H; y++) {
      const on = y < H && sm[y] >= thr;
      if (on) { if (s < 0) s = y; gap = 0; }
      else if (s >= 0) {
        gap++;
        if (gap > 2 || y === H) { const e = y - gap; if (e - s >= 3) bands.push([s, e]); s = -1; gap = 0; }
      }
    }
    bands = bands.filter(([a, b]) => {
      let lo = W, hi = -1, peak = 0, dark = 0, n = 0, deep = 0;
      for (let y = a; y <= b; y++) {
        if (x0[y] < lo) lo = x0[y];
        if (x1[y] > hi) hi = x1[y];
        if (sm[y] > peak) peak = sm[y];
      }
      for (let y = a; y <= b; y += 2) for (let x = Math.max(0, lo); x <= Math.min(W - 1, hi); x += 3) { dark += lum[y * W + x]; n++; if (lum[y * W + x] < paper * 0.55) deep++; }
      const wide = false;
      const faint = peak < typical * 0.45;                                                           /* a slanted edge, not writing */
      const onTable = n && (dark / n < paper * 0.5 || deep / n > 0.4);                                                    /* far darker than the page */
      return !wide && !faint && !onTable;
    });
    /* Grow each band to take in ascenders and descenders, without running into its neighbour. */
    bands = bands.map(([a, b], k) => {
      let peak = 0;
      for (let y = a; y <= b; y++) if (sm[y] > peak) peak = sm[y];
      /* Stray rows (a page edge, a ruled line far off) must not pull the band along: the floor sits above the
         background level of the page, and a band grows by at most half its own height each way. */
      const base = live.length ? live[Math.floor(live.length * 0.25)] : 0;
      const floor = Math.max(1, peak * 0.25, base * 2), room = Math.ceil((b - a + 1) * 0.5);
      const above = Math.max(k ? bands[k - 1][1] + 1 : 0, a - room), below = Math.min(k < bands.length - 1 ? bands[k + 1][0] - 1 : H - 1, b + room);
      while (a - 1 > above && sm[a - 1] >= floor) a--;
      while (b + 1 < below && sm[b + 1] >= floor) b++;
      return [a, b, peak];
    });
    return bands.map(([a, b, pk]) => {
      let lo = W, hi = -1;
      for (let y = a; y <= b; y++) { if (x0[y] < lo) lo = x0[y]; if (x1[y] > hi) hi = x1[y]; }
      const pad = 3, xs = Math.max(0, lo - pad), xe = Math.min(W, hi + pad + 1), ys = Math.max(0, a - pad), ye = Math.min(H, b + pad + 1);
      return { x: xs / W, y: ys / H, w: (xe - xs) / W, h: (ye - ys) / H, r: 0, s: pk };
    });
  };
})();
