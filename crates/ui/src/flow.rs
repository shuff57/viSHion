//! Inline text flow: places a row of atoms (words, flagged-word chips, math) into wrapped lines, the way a
//! browser lays out `.tx-text` (js/render.js output under css/components.css): one fixed line pitch, break
//! opportunities after spaces, nothing broken inside a chip. Pure geometry, so it is unit-tested natively.
//! egui's own wrapping cannot do this because it gives every widget its own row height.

/// One thing to place. `w`/`h` are its size, `trail` the width of whitespace after it that does not have to
/// fit on the line, `brk` whether a line may break after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub w: f32,
    pub h: f32,
    pub trail: f32,
    pub brk: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub x: f32,
    /// Top of the line box this item sits in.
    pub line_top: f32,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub top: f32,
    pub h: f32,
    /// Horizontal extent of the items on the line (for the reading underline).
    pub x0: f32,
    pub x1: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub placed: Vec<Placed>,
    pub lines: Vec<Line>,
    pub height: f32,
}

/// Greedy line breaking. Every line is `pitch` tall unless an item is taller (display-size math), so a
/// paragraph of plain text has the same rhythm whether or not it holds chips.
pub fn layout(items: &[Item], width: f32, pitch: f32) -> Flow {
    let mut placed: Vec<Placed> = Vec::with_capacity(items.len());
    let mut lines: Vec<Line> = vec![];
    let mut x = 0.0f32;
    let mut top = 0.0f32;
    let mut line = 0usize;
    let mut line_h = pitch;
    let mut x_first = 0.0f32;
    let mut x_last_end = 0.0f32;
    let mut i = 0;
    let mut first_on_line = true;
    while i < items.len() {
        // a segment runs to the next break opportunity
        let mut j = i;
        let mut seg_w = 0.0;
        let mut seg_h: f32 = 0.0;
        loop {
            let it = &items[j];
            seg_w += it.w + it.trail;
            seg_h = seg_h.max(it.h);
            if it.brk || j + 1 == items.len() {
                break;
            }
            j += 1;
        }
        let tail = items[j].trail;
        let fit = seg_w - tail;
        if !first_on_line && x + fit > width + 0.01 {
            // close the line
            lines.push(Line { top, h: line_h, x0: x_first, x1: x_last_end });
            top += line_h;
            line += 1;
            line_h = pitch;
            x = 0.0;
            first_on_line = true;
        }
        for k in i..=j {
            let it = &items[k];
            if first_on_line {
                x_first = x;
                first_on_line = false;
            }
            placed.push(Placed { x, line_top: top, line });
            line_h = line_h.max(it.h);
            x += it.w;
            x_last_end = x;
            x += it.trail;
        }
        // items of a segment may be taller than the pitch: the line grew after earlier ones were placed
        i = j + 1;
        let _ = seg_h;
    }
    if !placed.is_empty() {
        lines.push(Line { top, h: line_h, x0: x_first, x1: x_last_end });
    }
    // a line that grew taller shifts the lines below it; recompute tops from the final heights
    let mut y = 0.0;
    for (n, l) in lines.iter_mut().enumerate() {
        l.top = y;
        y += l.h;
        for p in placed.iter_mut().filter(|p| p.line == n) {
            p.line_top = l.top;
        }
    }
    Flow { placed, lines, height: y.max(if items.is_empty() { pitch } else { 0.0 }) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: f32) -> Item {
        Item { w, h: 28.0, trail: 6.0, brk: true }
    }

    #[test]
    fn wraps_at_the_width_and_keeps_one_pitch() {
        let items: Vec<Item> = (0..6).map(|_| word(100.0)).collect();
        let f = layout(&items, 330.0, 37.4);
        // 100 + 6 + 100 + 6 + 100 = 312 fits; the fourth word starts line 2
        assert_eq!(f.lines.len(), 2);
        assert_eq!(f.placed[3].line, 1);
        assert!((f.lines[1].top - 37.4).abs() < 0.01);
        assert!((f.height - 74.8).abs() < 0.01);
    }

    #[test]
    fn trailing_space_does_not_force_a_wrap() {
        // exactly 106 + 100 = 206 of text, then a trailing space: fits in 206
        let f = layout(&[word(100.0), word(100.0)], 206.0, 37.4);
        assert_eq!(f.lines.len(), 1);
    }

    #[test]
    fn a_chip_is_never_split_and_has_no_extra_gap() {
        let chip = Item { w: 80.0, h: 30.0, trail: 0.0, brk: false };
        let comma = Item { w: 6.0, h: 28.0, trail: 6.0, brk: true };
        // chip + comma form one segment: they stay together
        let f = layout(&[word(100.0), chip, comma], 190.0, 37.4);
        assert_eq!(f.placed[1].line, 1);
        assert_eq!(f.placed[2].line, 1);
        assert!((f.placed[2].x - 80.0).abs() < 0.01, "no gap after the chip");
    }

    #[test]
    fn a_tall_item_grows_only_its_own_line() {
        let big = Item { w: 40.0, h: 70.0, trail: 0.0, brk: true };
        let f = layout(&[word(150.0), big, word(150.0), word(150.0)], 200.0, 37.4);
        assert!(f.lines.iter().any(|l| (l.h - 70.0).abs() < 0.01));
        assert!(f.lines.iter().filter(|l| (l.h - 37.4).abs() < 0.01).count() >= 1);
        assert!((f.height - f.lines.iter().map(|l| l.h).sum::<f32>()).abs() < 0.01);
    }

    #[test]
    fn an_overlong_item_is_placed_not_dropped() {
        let f = layout(&[word(500.0)], 100.0, 37.4);
        assert_eq!(f.placed.len(), 1);
        assert_eq!(f.lines.len(), 1);
    }
}
