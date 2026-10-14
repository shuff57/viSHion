//! `image.js lineBands`: finds bands of ink by row on a page picture, as fractions of the image. Pure
//! arithmetic over a luminance buffer (420 pixels wide, like the JS canvas), so it is unit-tested natively
//! and behaves the same on the web. Used when a page has no stored line boxes (after a rotate, or for a
//! page the model gave none for), so the marks on the page land on ink.

/// Width the page is scanned at (`lineBands` uses a 420 pixel canvas).
pub const SCAN_W: u32 = 420;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Strength (the peak ink of the band): the weakest is the stray one when there are too many.
    pub s: f32,
}

fn round(v: f64) -> i64 {
    (v + 0.5).floor() as i64
}

/// `lum` is `w * h` luminance values (`(r*299 + g*587 + b*114) / 1000`).
pub fn line_bands(lum: &[u8], w: usize, h: usize) -> Vec<Band> {
    if w < 8 || h < 8 || lum.len() < w * h {
        return vec![];
    }
    let (wi, hi) = (w as i64, h as i64);
    // local background by box mean (integral image)
    let sw = w + 1;
    let mut sum = vec![0f64; sw * (h + 1)];
    for y in 0..h {
        let mut run = 0f64;
        for x in 0..w {
            run += lum[y * w + x] as f64;
            sum[(y + 1) * sw + x + 1] = sum[y * sw + x + 1] + run;
        }
    }
    let r = 8.max(round(w as f64 * 0.06)) as i64;
    let mean = |x: i64, y: i64| -> f64 {
        let x0 = (x - r).max(0) as usize;
        let x1 = (x + r + 1).min(wi) as usize;
        let y0 = (y - r).max(0) as usize;
        let y1 = (y + r + 1).min(hi) as usize;
        (sum[y1 * sw + x1] - sum[y0 * sw + x1] - sum[y1 * sw + x0] + sum[y0 * sw + x0]) / (((x1 - x0) * (y1 - y0)) as f64)
    };
    let mut row = vec![0f32; h];
    let mut x0 = vec![wi as i32; h];
    let mut x1 = vec![-1i32; h];
    let mx = round(w as f64 * 0.04);
    let my = round(h as f64 * 0.04);
    // writing makes short runs; a ruled line, a page edge or the table edge makes one long run: skip those
    let long_run = round(w as f64 * 0.25);
    let ink = |x: i64, y: i64| (lum[y as usize * w + x as usize] as f64) < mean(x, y) * 0.88;
    for y in my..(hi - my) {
        let mut x = mx;
        while x < wi - mx {
            if !ink(x, y) {
                x += 1;
                continue;
            }
            let mut e = x + 1;
            while e < wi - mx && ink(e, y) {
                e += 1;
            }
            if e - x < long_run {
                row[y as usize] += (e - x) as f32;
                if (x as i32) < x0[y as usize] {
                    x0[y as usize] = x as i32;
                }
                if ((e - 1) as i32) > x1[y as usize] {
                    x1[y as usize] = (e - 1) as i32;
                }
            }
            x = e;
        }
    }
    let at = |i: i64| if i < 0 || i >= hi { 0.0 } else { row[i as usize] };
    let sm: Vec<f32> = (0..hi).map(|y| (row[y as usize] + at(y - 1) + at(y + 1)) / 3.0).collect();
    let mut live: Vec<f32> = sm.iter().copied().filter(|v| *v > 0.0).collect();
    live.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ln = live.len();
    let noise = if ln > 0 { live[(ln as f64 * 0.4).floor() as usize] } else { 0.0 };
    let strong = if ln > 0 { live[(ln - 1).min((ln as f64 * 0.92).floor() as usize)] } else { 0.0 };
    let typical = noise.max(strong * 0.7);
    let p75 = if ln > 0 { live[(ln as f64 * 0.75).floor() as usize] } else { 0.0 };
    let thr = 2f32.max(p75 * 0.3).max(noise + (strong - noise) * 0.25);
    // the brightness of the page itself, to tell a band of writing from the dark table or a shadow
    let mut sorted: Vec<u8> = lum[..w * h].to_vec();
    sorted.sort_unstable();
    let paper = match sorted[(sorted.len() as f64 * 0.75).floor() as usize] {
        0 => 255.0,
        v => v as f64,
    };

    let mut bands: Vec<(i64, i64)> = vec![];
    let (mut s, mut gap) = (-1i64, 0i64);
    for y in 0..=hi {
        let on = y < hi && sm[y as usize] >= thr;
        if on {
            if s < 0 {
                s = y;
            }
            gap = 0;
        } else if s >= 0 {
            gap += 1;
            if gap > 2 || y == hi {
                let e = y - gap;
                if e - s >= 3 {
                    bands.push((s, e));
                }
                s = -1;
                gap = 0;
            }
        }
    }
    bands.retain(|&(a, b)| {
        let (mut lo, mut hi_x, mut peak) = (wi as i32, -1i32, 0f32);
        for y in a..=b {
            lo = lo.min(x0[y as usize]);
            hi_x = hi_x.max(x1[y as usize]);
            peak = peak.max(sm[y as usize]);
        }
        let (mut dark, mut n, mut deep) = (0f64, 0u32, 0u32);
        let mut y = a;
        while y <= b {
            let mut x = (lo.max(0)) as i64;
            while x <= ((wi - 1).min(hi_x as i64)) {
                let v = lum[y as usize * w + x as usize] as f64;
                dark += v;
                n += 1;
                if v < paper * 0.55 {
                    deep += 1;
                }
                x += 3;
            }
            y += 2;
        }
        let faint = peak < typical * 0.45;
        let on_table = n > 0 && (dark / (n as f64) < paper * 0.5 || (deep as f64) / (n as f64) > 0.4);
        !faint && !on_table
    });
    // grow each band to take in ascenders and descenders, without running into its neighbour
    let base = if ln > 0 { live[(ln as f64 * 0.25).floor() as usize] } else { 0.0 };
    let grown: Vec<(i64, i64, f32)> = bands
        .iter()
        .enumerate()
        .map(|(k, &(mut a, mut b))| {
            let mut peak = 0f32;
            for y in a..=b {
                peak = peak.max(sm[y as usize]);
            }
            let floor = 1f32.max(peak * 0.25).max(base * 2.0);
            let room = (((b - a + 1) as f64) * 0.5).ceil() as i64;
            let above = (if k > 0 { bands[k - 1].1 + 1 } else { 0 }).max(a - room);
            let below = (if k + 1 < bands.len() { bands[k + 1].0 - 1 } else { hi - 1 }).min(b + room);
            while a - 1 > above && sm[(a - 1) as usize] >= floor {
                a -= 1;
            }
            while b + 1 < below && sm[(b + 1) as usize] >= floor {
                b += 1;
            }
            (a, b, peak)
        })
        .collect();
    grown
        .into_iter()
        .map(|(a, b, pk)| {
            let (mut lo, mut hi_x) = (wi as i32, -1i32);
            for y in a..=b {
                lo = lo.min(x0[y as usize]);
                hi_x = hi_x.max(x1[y as usize]);
            }
            let pad = 3i64;
            let xs = (lo as i64 - pad).max(0);
            let xe = (hi_x as i64 + pad + 1).min(wi);
            let ys = (a - pad).max(0);
            let ye = (b + pad + 1).min(hi);
            Band {
                x: xs as f32 / w as f32,
                y: ys as f32 / h as f32,
                w: (xe - xs) as f32 / w as f32,
                h: (ye - ys) as f32 / h as f32,
                s: pk,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paper with a little grain and dark writing (short dashes) on the given row ranges.
    fn page(w: usize, h: usize, rows: &[(usize, usize)]) -> Vec<u8> {
        let mut v = vec![235u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if (x * 7 + y * 13) % 29 == 0 {
                    v[y * w + x] = 120;
                }
            }
        }
        for &(a, b) in rows {
            for y in a..b {
                for x in 40..(w - 60) {
                    if (x / 2) % 3 == 0 {
                        v[y * w + x] = 40;
                    }
                }
            }
        }
        v
    }

    #[test]
    fn finds_one_band_per_line_of_ink() {
        let (w, h) = (420, 560);
        let lines = [(80, 96), (140, 156), (200, 216), (260, 276), (330, 346), (400, 416)];
        let b = line_bands(&page(w, h, &lines), w, h);
        assert_eq!(b.len(), 6, "{b:?}");
        for (band, (a, z)) in b.iter().zip(lines) {
            let mid = (band.y + band.h / 2.0) * h as f32;
            assert!((mid - (a + z) as f32 / 2.0).abs() < 6.0, "band at {mid}, ink at {a}..{z}");
            assert!(band.x < 0.15 && band.x + band.w > 0.8);
        }
    }

    #[test]
    fn a_blank_page_has_no_bands() {
        assert!(line_bands(&vec![240u8; 420 * 560], 420, 560).is_empty());
    }
}
