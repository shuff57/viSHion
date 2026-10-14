//! Bit-exact port of the pure analysis loops in js/image.js (lineBands),
//! js/deskew.js (skewAngle's sample/score, edgeColor).
//!
//! Every function starts from RGBA pixels that the *canvas* already resampled
//! (drawImage + getImageData); the resampling itself is not part of this crate.

/// JS Math.round: halves round toward +infinity (not away from zero).
#[inline]
pub fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `lum[i] = (r*299 + g*587 + b*114) / 1000` stored in a Uint8Array (truncation).
fn luminance(rgba: &[u8], n: usize) -> Vec<u8> {
    (0..n)
        .map(|i| {
            let v = rgba[i * 4] as u32 * 299 + rgba[i * 4 + 1] as u32 * 587 + rgba[i * 4 + 2] as u32 * 114;
            (v / 1000) as u8
        })
        .collect()
}

/// Integral image in f64 (Float64Array), stride w+1.
fn integral(lum: &[u8], w: usize, h: usize) -> Vec<f64> {
    let s = w + 1;
    let mut sum = vec![0f64; s * (h + 1)];
    for y in 0..h {
        let mut run = 0f64;
        for x in 0..w {
            run += lum[y * w + x] as f64;
            sum[(y + 1) * s + x + 1] = sum[y * s + x + 1] + run;
        }
    }
    sum
}

struct Boxmean<'a> {
    sum: &'a [f64],
    s: i64,
    w: i64,
    h: i64,
    r: i64,
}
impl<'a> Boxmean<'a> {
    #[inline]
    fn at(&self, x: i64, y: i64) -> f64 {
        let x0 = (x - self.r).max(0);
        let x1 = (x + self.r + 1).min(self.w);
        let y0 = (y - self.r).max(0);
        let y1 = (y + self.r + 1).min(self.h);
        let g = |yy: i64, xx: i64| self.sum[(yy * self.s + xx) as usize];
        (g(y1, x1) - g(y0, x1) - g(y1, x0) + g(y0, x0)) / (((x1 - x0) * (y1 - y0)) as f64)
    }
}

/// Port of I.lineBands after the canvas step. `rgba` is w*h*4 (w=420 in the app).
/// Returns flat [x,y,w,h,r,s] per band.
pub fn line_bands(rgba: &[u8], w: usize, h: usize) -> Vec<f64> {
    let lum = luminance(rgba, w * h);
    let sum = integral(&lum, w, h);
    let r = js_round(w as f64 * 0.06).max(8.0) as i64;
    let bm = Boxmean { sum: &sum, s: (w + 1) as i64, w: w as i64, h: h as i64, r };
    let (wi, hi_) = (w as i64, h as i64);

    let mut row = vec![0f32; h]; // Float32Array
    let mut x0 = vec![w as i16; h]; // Int16Array.fill(W)
    let mut x1 = vec![-1i16; h];
    let mx = js_round(w as f64 * 0.04) as i64;
    let my = js_round(h as f64 * 0.04) as i64;
    let long_run = js_round(w as f64 * 0.25) as i64;
    let mut y = my;
    while y < hi_ - my {
        let mut x = mx;
        while x < wi - mx {
            let px = lum[(y * wi + x) as usize] as f64;
            if !(px < bm.at(x, y) * 0.88) {
                x += 1;
                continue;
            }
            let mut e = x + 1;
            while e < wi - mx && (lum[(y * wi + e) as usize] as f64) < bm.at(e, y) * 0.88 {
                e += 1;
            }
            if e - x < long_run {
                let yy = y as usize;
                row[yy] += (e - x) as f32;
                if x < x0[yy] as i64 { x0[yy] = x as i16; }
                if e - 1 > x1[yy] as i64 { x1[yy] = (e - 1) as i16; }
            }
            x = e;
        }
        y += 1;
    }

    // sm is a plain JS Array of doubles; row[-1] / row[H] are undefined -> 0.
    let sm: Vec<f64> = (0..h)
        .map(|y| {
            let v = row[y] as f64;
            let p = if y > 0 { row[y - 1] as f64 } else { 0.0 };
            let n = if y + 1 < h { row[y + 1] as f64 } else { 0.0 };
            (v + p + n) / 3.0
        })
        .collect();
    let mut live: Vec<f64> = sm.iter().copied().filter(|&v| v > 0.0).collect();
    live.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ln = live.len();
    let at = |f: f64| -> f64 {
        if ln == 0 { 0.0 } else { live[((ln as f64 * f).floor() as usize).min(ln - 1)] }
    };
    // note: the JS indexes live[Math.floor(len*0.4)] unclamped; 0.4/0.75/0.25 never reach len.
    let noise = if ln > 0 { live[(ln as f64 * 0.4).floor() as usize] } else { 0.0 };
    let strong = if ln > 0 { live[(ln - 1).min((ln as f64 * 0.92).floor() as usize)] } else { 0.0 };
    let typical = noise.max(strong * 0.7);
    let p75 = at(0.75);
    let thr = 2f64.max(p75 * 0.3).max(noise + (strong - noise) * 0.25);

    // paper = sorted[floor(n*0.75)] || 255  (counting sort; 0 is falsy -> 255)
    let mut hist = [0usize; 256];
    for &v in &lum { hist[v as usize] += 1; }
    let target = ((lum.len() as f64) * 0.75).floor() as usize;
    let mut acc = 0usize;
    let mut paper_v = 0u8;
    let mut found = false;
    for (v, &c) in hist.iter().enumerate() {
        acc += c;
        if acc > target { paper_v = v as u8; found = true; break; }
    }
    let paper = if !found || paper_v == 0 { 255.0 } else { paper_v as f64 };

    let mut bands: Vec<(i64, i64)> = Vec::new();
    let mut s: i64 = -1;
    let mut gap: i64 = 0;
    for y in 0..=h as i64 {
        let on = y < hi_ && sm[y as usize] >= thr;
        if on {
            if s < 0 { s = y; }
            gap = 0;
        } else if s >= 0 {
            gap += 1;
            if gap > 2 || y == hi_ {
                let e = y - gap;
                if e - s >= 3 { bands.push((s, e)); }
                s = -1;
                gap = 0;
            }
        }
    }
    bands.retain(|&(a, b)| {
        let (mut lo, mut hi, mut peak) = (wi, -1i64, 0f64);
        for y in a..=b {
            let yy = y as usize;
            if (x0[yy] as i64) < lo { lo = x0[yy] as i64; }
            if (x1[yy] as i64) > hi { hi = x1[yy] as i64; }
            if sm[yy] > peak { peak = sm[yy]; }
        }
        let (mut dark, mut n, mut deep) = (0f64, 0f64, 0f64);
        let xa = lo.max(0);
        let xb = hi.min(wi - 1);
        let mut y = a;
        while y <= b {
            let mut x = xa;
            while x <= xb {
                let v = lum[(y * wi + x) as usize] as f64;
                dark += v;
                n += 1.0;
                if v < paper * 0.55 { deep += 1.0; }
                x += 3;
            }
            y += 2;
        }
        let faint = peak < typical * 0.45;
        let on_table = n > 0.0 && (dark / n < paper * 0.5 || deep / n > 0.4);
        !faint && !on_table
    });

    // grow: JS reads neighbours from the *old* array inside map()
    let old = bands.clone();
    let base = if ln > 0 { live[(ln as f64 * 0.25).floor() as usize] } else { 0.0 };
    let grown: Vec<(i64, i64, f64)> = old
        .iter()
        .enumerate()
        .map(|(k, &(mut a, mut b))| {
            let mut peak = 0f64;
            for y in a..=b { if sm[y as usize] > peak { peak = sm[y as usize]; } }
            let floor = 1f64.max(peak * 0.25).max(base * 2.0);
            let room = ((b - a + 1) as f64 * 0.5).ceil() as i64;
            let above = (if k > 0 { old[k - 1].1 + 1 } else { 0 }).max(a - room);
            let below = (if k < old.len() - 1 { old[k + 1].0 - 1 } else { hi_ - 1 }).min(b + room);
            while a - 1 > above && sm[(a - 1) as usize] >= floor { a -= 1; }
            while b + 1 < below && sm[(b + 1) as usize] >= floor { b += 1; }
            (a, b, peak)
        })
        .collect();

    let mut out = Vec::with_capacity(grown.len() * 6);
    for (a, b, pk) in grown {
        let (mut lo, mut hi) = (wi, -1i64);
        for y in a..=b {
            let yy = y as usize;
            if (x0[yy] as i64) < lo { lo = x0[yy] as i64; }
            if (x1[yy] as i64) > hi { hi = x1[yy] as i64; }
        }
        let pad = 3;
        let xs = (lo - pad).max(0);
        let xe = wi.min(hi + pad + 1);
        let ys = (a - pad).max(0);
        let ye = hi_.min(b + pad + 1);
        out.extend_from_slice(&[
            xs as f64 / w as f64,
            ys as f64 / h as f64,
            (xe - xs) as f64 / w as f64,
            (ye - ys) as f64 / h as f64,
            0.0,
            pk,
        ]);
    }
    out
}

/// `score(p, deg, step)` from deskew.js
fn score(xs: &[f64], ys: &[f64], h: usize, deg: f64, step: usize) -> f64 {
    let r = (deg * std::f64::consts::PI) / 180.0;
    let (sin, cos) = (r.sin(), r.cos());
    let mut bins = vec![0f32; h * 2 + 4]; // Float32Array
    let off = (h + 2) as f64;
    let len = bins.len() as f64;
    let mut i = 0;
    while i < xs.len() {
        let y = js_round((-xs[i]) * sin + ys[i] * cos) + off;
        if y >= 0.0 && y < len { bins[y as usize] += 1.0; }
        i += step;
    }
    let mut t = 0f64;
    for &b in &bins { t += (b as f64) * (b as f64); }
    t
}

/// Port of I.skewAngle after the canvas step. `rgba` is w*h*4 (w = min(600, src width)),
/// assumed opaque (the JS composites onto white first).
pub fn skew_angle(rgba: &[u8], w: usize, h: usize) -> f64 {
    let lum = luminance(rgba, w * h);
    let sum = integral(&lum, w, h);
    let r = js_round(w as f64 * 0.06).max(6.0) as i64;
    let bm = Boxmean { sum: &sum, s: (w + 1) as i64, w: w as i64, h: h as i64, r };
    let (wi, hi_) = (w as i64, h as i64);
    let mx = js_round(w as f64 * 0.04) as i64;
    let my = js_round(h as f64 * 0.04) as i64;
    let long_run = js_round(w as f64 * 0.3) as i64;
    let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    let mut y = my;
    while y < hi_ - my {
        let mut x = mx;
        while x < wi - mx {
            if !((lum[(y * wi + x) as usize] as f64) < bm.at(x, y) * 0.8) { x += 1; continue; }
            let mut e = x;
            while e < wi - mx && (lum[(y * wi + e) as usize] as f64) < bm.at(e, y) * 0.8 { e += 1; }
            if e - x < long_run {
                for k in x..e {
                    xs.push(k as f64 - w as f64 / 2.0);
                    ys.push(y as f64 - h as f64 / 2.0);
                }
            }
            x = e;
        }
        y += 1;
    }
    const RANGE: f64 = 10.0;
    const MIN: f64 = 0.4;
    if xs.len() < 400 { return 0.0; }
    let step = ((xs.len() / 20000).max(1)) as usize;
    let (mut best, mut top) = (0f64, -1f64);
    let flat = score(&xs, &ys, h, 0.0, step);
    let mut a = -RANGE;
    while a <= RANGE {
        let s = score(&xs, &ys, h, a, step);
        if s > top { top = s; best = a; }
        a += 0.5;
    }
    a = best - 0.5;
    while a <= best + 0.5 { // JS re-reads `best` in the condition
        let s = score(&xs, &ys, h, a, step);
        if s > top { top = s; best = a; }
        a += 0.1;
    }
    if best.abs() < MIN || top < flat * 1.03 { return 0.0; }
    js_round(best * 10.0) / 10.0
}

/// edgeColor on the 16x16 canvas data: mean of the four corner pixels, Math.round per channel.
pub fn edge_color(rgba16: &[u8]) -> [u8; 3] {
    let at = [0usize, 15, 15 * 16, 15 * 16 + 15];
    let mut t = [0u32; 3];
    for &i in &at {
        for c in 0..3 { t[c] += rgba16[i * 4 + c] as u32; }
    }
    [0, 1, 2].map(|c| js_round(t[c] as f64 / 4.0) as u8)
}
