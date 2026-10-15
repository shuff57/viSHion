//! Capture on phone (`design/Phone.dc.html`): the pure parts. A camera frame comes in as RGBA; `detect_page`
//! finds the sheet of paper in it (a large, bright, fairly even region against a darker table), and `Steady`
//! says when the sheet has held still long enough for the auto-capture toggle to take the photo.

/// A found page as fractions of the frame: left, top, right, bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Found {
    pub fn area(&self) -> f32 {
        (self.x1 - self.x0) * (self.y1 - self.y0)
    }
}

/// The least share of the frame a page must cover to count as found.
pub const MIN_AREA: f32 = 0.25;

fn luma(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// The bounding box of the bright region of an RGBA frame `w` x `h`, or `None` when the page does not
/// cover [`MIN_AREA`] of it. Works on a coarse grid (about 64 cells across) so it is cheap per frame.
pub fn detect_page(rgba: &[u8], w: usize, h: usize) -> Option<Found> {
    if w < 8 || h < 8 || rgba.len() < w * h * 4 {
        return None;
    }
    let step = (w / 64).max(1);
    let (cols, rows) = (w / step, h / step);
    let mut grid = vec![0f32; cols * rows];
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for r in 0..rows {
        for c in 0..cols {
            let i = ((r * step) * w + c * step) * 4;
            let l = luma(&rgba[i..i + 3]);
            grid[r * cols + c] = l;
            lo = lo.min(l);
            hi = hi.max(l);
        }
    }
    // a frame with no contrast is a wall or a lens cap, not a page
    if hi - lo < 40.0 {
        return None;
    }
    let cut = lo + (hi - lo) * 0.55;
    let (mut x0, mut y0, mut x1, mut y1) = (cols, rows, 0usize, 0usize);
    let mut n = 0usize;
    for r in 0..rows {
        // a row or column counts when most of it is bright: single bright pixels do not stretch the box
        let bright = (0..cols).filter(|&c| grid[r * cols + c] >= cut).count();
        if bright * 4 >= cols {
            y0 = y0.min(r);
            y1 = y1.max(r + 1);
        }
    }
    for c in 0..cols {
        let bright = (0..rows).filter(|&r| grid[r * cols + c] >= cut).count();
        if bright * 4 >= rows {
            x0 = x0.min(c);
            x1 = x1.max(c + 1);
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    for r in y0..y1 {
        for c in x0..x1 {
            if grid[r * cols + c] >= cut {
                n += 1;
            }
        }
    }
    // the box must be mostly page, or it is two bright things far apart
    if n * 100 < (x1 - x0) * (y1 - y0) * 60 {
        return None;
    }
    let f = Found { x0: x0 as f32 / cols as f32, y0: y0 as f32 / rows as f32, x1: x1 as f32 / cols as f32, y1: y1 as f32 / rows as f32 };
    (f.area() >= MIN_AREA).then_some(f)
}

/// Counts consecutive frames in which the page sat in about the same place.
#[derive(Debug, Default, Clone)]
pub struct Steady {
    last: Option<Found>,
    run: u32,
}

impl Steady {
    /// Frames the page must hold still before auto-capture fires.
    pub const NEED: u32 = 12;
    const TOL: f32 = 0.04;

    /// Feed one frame's detection. Returns true on the frame the page has been steady for [`Self::NEED`]
    /// frames; it re-arms only after the page leaves or moves.
    pub fn feed(&mut self, now: Option<Found>) -> bool {
        let same = match (self.last, now) {
            (Some(a), Some(b)) => (a.x0 - b.x0).abs() < Self::TOL && (a.y0 - b.y0).abs() < Self::TOL && (a.x1 - b.x1).abs() < Self::TOL && (a.y1 - b.y1).abs() < Self::TOL,
            _ => false,
        };
        self.last = now;
        if now.is_none() || !same {
            self.run = if now.is_some() { 1 } else { 0 };
            return false;
        }
        self.run += 1;
        self.run == Self::NEED
    }

    pub fn reset(&mut self) {
        *self = Steady::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dark table with a bright page at the given fractions.
    fn frame(w: usize, h: usize, page: Option<(f32, f32, f32, f32)>) -> Vec<u8> {
        let mut v = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let on = page.is_some_and(|(a, b, c, d)| fx >= a && fx < c && fy >= b && fy < d);
                let l = if on { 235 } else { 40 };
                let i = (y * w + x) * 4;
                v[i..i + 4].copy_from_slice(&[l, l, l, 255]);
            }
        }
        v
    }

    #[test]
    fn a_bright_sheet_is_found_where_it_is() {
        let f = detect_page(&frame(320, 240, Some((0.2, 0.1, 0.8, 0.9))), 320, 240).unwrap();
        assert!((f.x0 - 0.2).abs() < 0.05 && (f.y0 - 0.1).abs() < 0.05, "{f:?}");
        assert!((f.x1 - 0.8).abs() < 0.05 && (f.y1 - 0.9).abs() < 0.05, "{f:?}");
    }

    #[test]
    fn nothing_or_a_speck_is_not_a_page() {
        assert!(detect_page(&frame(320, 240, None), 320, 240).is_none());
        assert!(detect_page(&frame(320, 240, Some((0.4, 0.4, 0.5, 0.5))), 320, 240).is_none());
        assert!(detect_page(&[0u8; 10], 320, 240).is_none());
    }

    #[test]
    fn auto_capture_fires_once_after_the_page_holds_still() {
        let a = Found { x0: 0.2, y0: 0.1, x1: 0.8, y1: 0.9 };
        let mut s = Steady::default();
        let fired: Vec<bool> = (0..Steady::NEED + 5).map(|_| s.feed(Some(a))).collect();
        assert_eq!(fired.iter().filter(|f| **f).count(), 1);
        assert!(fired[Steady::NEED as usize - 1]);
        // moving restarts the count, and losing the page too
        let b = Found { x0: 0.4, ..a };
        assert!(!s.feed(Some(b)));
        assert!(!s.feed(None));
        let again: Vec<bool> = (0..Steady::NEED).map(|_| s.feed(Some(a))).collect();
        assert!(again[Steady::NEED as usize - 1]);
    }
}
