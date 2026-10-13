//! Page pictures for the review screen's tools: prepare a picked file (js/image.js `prepare`: longest side at
//! most 1600 px, JPEG quality 0.9) and turn a stored page a quarter turn. Pure Rust (`image` crate), the same
//! on native and wasm, so there is no platform canvas involved. `deskew` is js/deskew.js: the angle whose
//! row projection of ink is sharpest, then a rotation about the centre filled with the corner colour.
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::DynamicImage;

pub const MAX_SIDE: u32 = 1600;
pub const QUALITY: u8 = 90;
/// js/image.js `MAX_BYTES`.
pub const MAX_BYTES: usize = 40 * 1024 * 1024;

fn encode(im: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = im.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY).encode_image(&rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Decode a picked file and re-encode it as the JPEG the assignment stores.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("The file is larger than 40 MB.".into());
    }
    let im = image::load_from_memory(bytes).map_err(|_| "That is not a picture this app can read.".to_string())?;
    let im = if im.width().max(im.height()) > MAX_SIDE { im.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle) } else { im };
    encode(&im)
}

/// A quarter turn clockwise (js `OCR.image.rotate(blob, 90)`).
pub fn rotate90(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let im = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    encode(&im.rotate90())
}

/// js/deskew.js: only tilts between -10 and 10 degrees are looked for, and anything under 0.4 is left alone.
const RANGE: f64 = 10.0;
const MIN: f64 = 0.4;
const SAMPLE_W: u32 = 600;

/// Ink points (relative to the picture centre) of a copy of the page at most 600 wide: pixels well below the
/// mean around them, in short runs (a table or page edge makes one long straight run and is skipped).
struct Ink {
    xs: Vec<f32>,
    ys: Vec<f32>,
    h: usize,
}

fn sample(im: &DynamicImage) -> Ink {
    let w = SAMPLE_W.min(im.width()).max(1);
    let h = ((im.height() as f64 * w as f64 / im.width().max(1) as f64).round() as u32).max(1);
    let small = im.resize_exact(w, h, FilterType::Triangle).to_rgba8();
    let (w, h) = (w as usize, h as usize);
    let lum: Vec<f32> = small
        .pixels()
        .map(|p| {
            // composited over white, like the canvas the JS draws on
            let a = p[3] as f32 / 255.0;
            let c = |v: u8| v as f32 * a + 255.0 * (1.0 - a);
            ((c(p[0]) as u32 * 299 + c(p[1]) as u32 * 587 + c(p[2]) as u32 * 114) / 1000) as f32
        })
        .collect();
    let s = w + 1;
    let mut sum = vec![0f64; s * (h + 1)];
    for y in 0..h {
        let mut run = 0f64;
        for x in 0..w {
            run += lum[y * w + x] as f64;
            sum[(y + 1) * s + x + 1] = sum[y * s + x + 1] + run;
        }
    }
    let r = 6usize.max((w as f64 * 0.06).round() as usize);
    let mean = |x: usize, y: usize| -> f64 {
        let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        (sum[y1 * s + x1] - sum[y0 * s + x1] - sum[y1 * s + x0] + sum[y0 * s + x0]) / (((x1 - x0) * (y1 - y0)) as f64)
    };
    let mx = (w as f64 * 0.04).round() as usize;
    let my = (h as f64 * 0.04).round() as usize;
    let long_run = (w as f64 * 0.3).round() as usize;
    let ink = |x: usize, y: usize| (lum[y * w + x] as f64) < mean(x, y) * 0.8;
    let (mut xs, mut ys) = (vec![], vec![]);
    for y in my..h.saturating_sub(my) {
        let mut x = mx;
        while x < w.saturating_sub(mx) {
            if !ink(x, y) {
                x += 1;
                continue;
            }
            let mut e = x;
            while e < w - mx && ink(e, y) {
                e += 1;
            }
            if e - x < long_run {
                for k in x..e {
                    xs.push(k as f32 - w as f32 / 2.0);
                    ys.push(y as f32 - h as f32 / 2.0);
                }
            }
            x = e;
        }
    }
    Ink { xs, ys, h }
}

fn score(p: &Ink, deg: f64, step: usize) -> f64 {
    let r = deg * std::f64::consts::PI / 180.0;
    let (sin, cos) = (r.sin(), r.cos());
    let mut bins = vec![0f32; p.h * 2 + 4];
    let off = p.h as f64 + 2.0;
    let mut i = 0;
    while i < p.xs.len() {
        let y = (-(p.xs[i] as f64) * sin + p.ys[i] as f64 * cos).round() + off;
        if y >= 0.0 && (y as usize) < bins.len() {
            bins[y as usize] += 1.0;
        }
        i += step;
    }
    bins.iter().map(|b| (*b as f64) * (*b as f64)).sum()
}

/// `I.skewAngle`: the clockwise tilt of the text in degrees (0 when unsure).
pub fn skew_angle(im: &DynamicImage) -> f64 {
    let p = sample(im);
    if p.xs.len() < 400 {
        return 0.0;
    }
    let step = 1usize.max(p.xs.len() / 20000);
    let (mut best, mut top) = (0.0f64, -1.0f64);
    let flat = score(&p, 0.0, step);
    let mut a = -RANGE;
    while a <= RANGE {
        let sc = score(&p, a, step);
        if sc > top {
            top = sc;
            best = a;
        }
        a += 0.5;
    }
    let mut a = best - 0.5;
    let hi = best + 0.5;
    while a <= hi {
        let sc = score(&p, a, step);
        if sc > top {
            top = sc;
            best = a;
        }
        a += 0.1;
    }
    if best.abs() < MIN || top < flat * 1.03 {
        return 0.0;
    }
    (best * 10.0).round() / 10.0
}

/// Average of the four corners of a 16 x 16 copy, so exposed corners blend in instead of drawing a white edge.
fn edge_color(im: &DynamicImage) -> [f32; 3] {
    let t = im.resize_exact(16, 16, FilterType::Triangle).to_rgb8();
    let mut c = [0f32; 3];
    for (x, y) in [(0, 0), (15, 0), (0, 15), (15, 15)] {
        let p = t.get_pixel(x, y);
        for k in 0..3 {
            c[k] += p[k] as f32 / 4.0;
        }
    }
    c
}

/// Turn the picture `angle` degrees counter-clockwise (canvas `rotate(-angle)`) about its centre, bilinear,
/// with the corner colour behind it. The size stays.
fn rotate_about_centre(im: &DynamicImage, angle: f64) -> DynamicImage {
    let src = im.to_rgb8();
    let (w, h) = (src.width() as i64, src.height() as i64);
    let fill = edge_color(im);
    let a = angle * std::f64::consts::PI / 180.0;
    let (sin, cos) = (a.sin(), a.cos());
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let mut out = image::RgbImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let sx = dx * cos - dy * sin + cx - 0.5;
            let sy = dx * sin + dy * cos + cy - 0.5;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = ((sx - x0) as f32, (sy - y0) as f32);
            let at = |xi: i64, yi: i64| -> [f32; 3] {
                if xi < 0 || yi < 0 || xi >= w || yi >= h {
                    fill
                } else {
                    let p = src.get_pixel(xi as u32, yi as u32);
                    [p[0] as f32, p[1] as f32, p[2] as f32]
                }
            };
            let (xi, yi) = (x0 as i64, y0 as i64);
            let (a00, a10, a01, a11) = (at(xi, yi), at(xi + 1, yi), at(xi, yi + 1), at(xi + 1, yi + 1));
            let mut px = [0u8; 3];
            for k in 0..3 {
                let top = a00[k] * (1.0 - fx) + a10[k] * fx;
                let bot = a01[k] * (1.0 - fx) + a11[k] * fx;
                px[k] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
            }
            out.put_pixel(x as u32, y as u32, image::Rgb(px));
        }
    }
    DynamicImage::ImageRgb8(out)
}

/// `I.deskew`: the straightened JPEG and the angle that was removed, or `None` and 0 when the page is level.
pub fn deskew(bytes: &[u8]) -> Result<(Option<Vec<u8>>, f64), String> {
    let im = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let angle = skew_angle(&im);
    if angle == 0.0 {
        return Ok((None, 0.0));
    }
    Ok((Some(encode(&rotate_about_centre(&im, angle))?), angle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let im = image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30]));
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(im).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size(b: &[u8]) -> (u32, u32) {
        let i = image::load_from_memory(b).unwrap();
        (i.width(), i.height())
    }

    #[test]
    fn prepare_shrinks_to_1600_and_stores_a_jpeg() {
        let out = prepare(&png(3200, 1200)).unwrap();
        assert_eq!(&out[..2], &[0xFF, 0xD8], "JPEG magic");
        assert_eq!(size(&out), (1600, 600));
        assert_eq!(size(&prepare(&png(300, 400)).unwrap()), (300, 400), "small pictures are not enlarged");
    }

    #[test]
    fn rotate_swaps_the_sides() {
        let j = prepare(&png(300, 200)).unwrap();
        assert_eq!(size(&rotate90(&j).unwrap()), (200, 300));
    }

    #[test]
    fn junk_is_refused_with_a_sentence() {
        assert!(prepare(b"not a picture").unwrap_err().contains("picture"));
    }

    /// Dark dashes along lines tilted by `deg` (clockwise, y down): a page of handwriting.
    fn tilted_page(deg: f64) -> Vec<u8> {
        let (w, h) = (900u32, 1160u32);
        let mut im = image::RgbImage::from_pixel(w, h, image::Rgb([250, 250, 246]));
        let t = deg.to_radians().tan();
        for line in 0..14 {
            let y0 = 160.0 + line as f64 * 62.0;
            let mut x = 100.0f64;
            while x < 780.0 {
                for dx in 0..14 {
                    let xx = x + dx as f64;
                    let y = y0 + (xx - 450.0) * t;
                    for dy in 0..3 {
                        let (px, py) = (xx as i64, (y as i64) + dy);
                        if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                            im.put_pixel(px as u32, py as u32, image::Rgb([30, 40, 130]));
                        }
                    }
                }
                x += 22.0;
            }
        }
        encode(&DynamicImage::ImageRgb8(im)).unwrap()
    }

    #[test]
    fn deskew_finds_the_tilt_and_levels_the_page() {
        let (fixed, angle) = deskew(&tilted_page(4.0)).unwrap();
        assert!((angle - 4.0).abs() <= 0.6, "angle {angle}");
        let fixed = fixed.expect("a straightened picture");
        // the straightened page has (almost) no tilt left, and the size is unchanged
        let again = image::load_from_memory(&fixed).unwrap();
        assert_eq!((again.width(), again.height()), (900, 1160));
        assert!(skew_angle(&again).abs() < 0.9, "{}", skew_angle(&again));
        // the other way round
        let (_, back) = deskew(&tilted_page(-3.0)).unwrap();
        assert!((back + 3.0).abs() <= 0.6, "angle {back}");
    }

    #[test]
    fn a_level_or_blank_page_is_left_alone() {
        assert_eq!(deskew(&tilted_page(0.0)).unwrap().1, 0.0);
        let blank = encode(&DynamicImage::ImageRgb8(image::RgbImage::from_pixel(300, 400, image::Rgb([240, 240, 240])))).unwrap();
        assert!(deskew(&blank).unwrap().0.is_none());
    }
}
