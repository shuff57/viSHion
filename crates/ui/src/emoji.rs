//! Colour emoji. egui draws glyphs from outlines in one colour, so emoji would come out as monochrome
//! shapes; the browser the JS app runs in draws them in colour. This module finds the emoji clusters in a
//! text (`segments`, pure) and rasterises them from a COLR v0 face (Twemoji Mozilla: layers of outlines
//! with palette colours) into small RGBA bitmaps that the transcript draws as images between the words.
//! The same code runs on native and wasm; the face is linked in natively and fetched on demand on the web.
use std::collections::HashMap;
use ttf_parser::{colr, gsub::SubstitutionSubtable, GlyphId, RgbaColor};

/// Width of a drawn emoji in ems (the glyph advance is one em; browsers draw colour emoji a little larger).
pub const WIDTH_EM: f32 = 1.12;
/// The face's vertical extent in font units above and below the baseline, and its em.
const ASCENT: f32 = 477.0;
const DESCENT: f32 = 91.0;
const UPEM: f32 = 512.0;
/// Height of a drawn emoji in ems.
pub const HEIGHT_EM: f32 = WIDTH_EM * (ASCENT + DESCENT) / UPEM;
/// Where the baseline sits in the bitmap, as a fraction of its height from the top.
pub const BASELINE: f32 = ASCENT / (ASCENT + DESCENT);

/// `Emoji_Presentation=Yes` (emoji-data.txt): shown as emoji without a variation selector.
const PRESENTATION: &[(u32, u32)] = &[
    (0x231A, 0x231B), (0x23E9, 0x23EC), (0x23F0, 0x23F0), (0x23F3, 0x23F3), (0x25FD, 0x25FE), (0x2614, 0x2615),
    (0x2648, 0x2653), (0x267F, 0x267F), (0x2693, 0x2693), (0x26A1, 0x26A1), (0x26AA, 0x26AB), (0x26BD, 0x26BE),
    (0x26C4, 0x26C5), (0x26CE, 0x26CE), (0x26D4, 0x26D4), (0x26EA, 0x26EA), (0x26F2, 0x26F3), (0x26F5, 0x26F5),
    (0x26FA, 0x26FA), (0x26FD, 0x26FD), (0x2705, 0x2705), (0x270A, 0x270B), (0x2728, 0x2728), (0x274C, 0x274C),
    (0x274E, 0x274E), (0x2753, 0x2755), (0x2757, 0x2757), (0x2795, 0x2797), (0x27B0, 0x27B0), (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C), (0x2B50, 0x2B50), (0x2B55, 0x2B55), (0x1F004, 0x1F004), (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E), (0x1F191, 0x1F19A), (0x1F1E6, 0x1F1FF), (0x1F201, 0x1F201), (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F), (0x1F232, 0x1F236), (0x1F238, 0x1F23A), (0x1F250, 0x1F251), (0x1F300, 0x1F320),
    (0x1F32D, 0x1F335), (0x1F337, 0x1F37C), (0x1F37E, 0x1F393), (0x1F3A0, 0x1F3CA), (0x1F3CF, 0x1F3D3),
    (0x1F3E0, 0x1F3F0), (0x1F3F4, 0x1F3F4), (0x1F3F8, 0x1F43E), (0x1F440, 0x1F440), (0x1F442, 0x1F4FC),
    (0x1F4FF, 0x1F53D), (0x1F54B, 0x1F54E), (0x1F550, 0x1F567), (0x1F57A, 0x1F57A), (0x1F595, 0x1F596),
    (0x1F5A4, 0x1F5A4), (0x1F5FB, 0x1F64F), (0x1F680, 0x1F6C5), (0x1F6CC, 0x1F6CC), (0x1F6D0, 0x1F6D2),
    (0x1F6D5, 0x1F6D7), (0x1F6DC, 0x1F6DF), (0x1F6EB, 0x1F6EC), (0x1F6F4, 0x1F6FC), (0x1F7E0, 0x1F7EB),
    (0x1F7F0, 0x1F7F0), (0x1F90C, 0x1F93A), (0x1F93C, 0x1F945), (0x1F947, 0x1F9FF), (0x1FA70, 0x1FA7C),
    (0x1FA80, 0x1FA88), (0x1FA90, 0x1FABD), (0x1FABF, 0x1FAC5), (0x1FACE, 0x1FADB), (0x1FAE0, 0x1FAE8),
    (0x1FAF0, 0x1FAF8),
];

const ZWJ: char = '\u{200D}';
const VS16: char = '\u{FE0F}';
const KEYCAP: char = '\u{20E3}';

fn presentation(c: char) -> bool {
    let u = c as u32;
    PRESENTATION.binary_search_by(|&(a, b)| if u < a { std::cmp::Ordering::Greater } else if u > b { std::cmp::Ordering::Less } else { std::cmp::Ordering::Equal }).is_ok()
}

fn is_modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

fn is_tag(c: char) -> bool {
    ('\u{E0020}'..='\u{E007F}').contains(&c)
}

fn is_regional(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// A piece of text: ordinary characters, or one emoji cluster (a sequence drawn as one picture).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Seg<'a> {
    Text(&'a str),
    Emoji(&'a str),
}

/// Does an emoji cluster start at `i` of `ch`? Returns where it ends.
fn cluster_end(ch: &[(usize, char)], i: usize) -> Option<usize> {
    let c = ch[i].1;
    let next = ch.get(i + 1).map(|x| x.1);
    let mut j = i + 1;
    if is_regional(c) {
        if next.is_some_and(is_regional) {
            return Some(i + 2);
        }
        return Some(i + 1);
    }
    let keycap = (c.is_ascii_digit() || c == '#' || c == '*') && next == Some(VS16) && ch.get(i + 2).map(|x| x.1) == Some(KEYCAP);
    if keycap {
        return Some(i + 3);
    }
    let base = presentation(c) || (next == Some(VS16) && (c as u32) >= 0xA9 && !c.is_ascii());
    if !base {
        return None;
    }
    // modifiers, selectors, tags, and ZWJ joins: ZWJ takes the next symbol along
    loop {
        match ch.get(j).map(|x| x.1) {
            Some(m) if m == VS16 || m == KEYCAP || is_modifier(m) || is_tag(m) => j += 1,
            Some(ZWJ) => match ch.get(j + 1).map(|x| x.1) {
                Some(n) if presentation(n) || (n as u32) >= 0x2190 && !n.is_ascii() => j += 2,
                _ => break,
            },
            _ => break,
        }
    }
    Some(j)
}

/// Split `s` into runs of text and emoji clusters. Concatenating the pieces gives `s` back.
pub fn segments(s: &str) -> Vec<Seg<'_>> {
    let ch: Vec<(usize, char)> = s.char_indices().collect();
    let mut out = vec![];
    let (mut i, mut text_from) = (0usize, 0usize);
    while i < ch.len() {
        // cheap exit: ASCII and Latin text never starts an emoji
        if (ch[i].1 as u32) < 0x231A && !(ch[i].1.is_ascii_digit() || ch[i].1 == '#' || ch[i].1 == '*') {
            i += 1;
            continue;
        }
        match cluster_end(&ch, i) {
            Some(j) => {
                let from = ch[i].0;
                if from > text_from {
                    out.push(Seg::Text(&s[text_from..from]));
                }
                let to = ch.get(j).map_or(s.len(), |x| x.0);
                out.push(Seg::Emoji(&s[from..to]));
                text_from = to;
                i = j;
            }
            None => i += 1,
        }
    }
    if text_from < s.len() {
        out.push(Seg::Text(&s[text_from..]));
    }
    out
}

/// True when `s` has at least one emoji cluster.
pub fn has_emoji(s: &str) -> bool {
    segments(s).iter().any(|g| matches!(g, Seg::Emoji(_)))
}

// ------------------------------------------------------------------ the face

/// A COLR v0 face. The bytes live for the whole run (linked in, or leaked once when fetched).
pub struct EmojiFont {
    face: ttf_parser::Face<'static>,
}

struct LayerPainter<'a> {
    face: &'a ttf_parser::Face<'a>,
    scale: f32,
    /// Offset of the glyph origin in the bitmap, in pixels.
    ox: f32,
    oy: f32,
    w: usize,
    h: usize,
    glyph: Option<GlyphId>,
    /// Premultiplied RGBA, row by row.
    px: &'a mut Vec<f32>,
}

struct Outline {
    r: ab_glyph_rasterizer::Rasterizer,
    scale: f32,
    ox: f32,
    oy: f32,
    last: ab_glyph_rasterizer::Point,
    start: ab_glyph_rasterizer::Point,
}

impl Outline {
    fn pt(&self, x: f32, y: f32) -> ab_glyph_rasterizer::Point {
        ab_glyph_rasterizer::point(self.ox + x * self.scale, self.oy - y * self.scale)
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.last = self.pt(x, y);
        self.start = self.last;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.pt(x, y);
        self.r.draw_line(self.last, p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.pt(x1, y1), self.pt(x, y));
        self.r.draw_quad(self.last, c, p);
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.pt(x1, y1), self.pt(x2, y2), self.pt(x, y));
        self.r.draw_cubic(self.last, c1, c2, p);
        self.last = p;
    }
    fn close(&mut self) {
        if self.last != self.start {
            self.r.draw_line(self.last, self.start);
            self.last = self.start;
        }
    }
}

impl colr::Painter<'_> for LayerPainter<'_> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.glyph = Some(glyph_id);
    }

    fn paint(&mut self, paint: colr::Paint<'_>) {
        let (Some(g), colr::Paint::Solid(RgbaColor { red, green, blue, alpha })) = (self.glyph, paint) else { return };
        let zero = ab_glyph_rasterizer::point(0.0, 0.0);
        let mut o = Outline { r: ab_glyph_rasterizer::Rasterizer::new(self.w, self.h), scale: self.scale, ox: self.ox, oy: self.oy, last: zero, start: zero };
        if self.face.outline_glyph(g, &mut o).is_none() {
            return;
        }
        let (cr, cg, cb, ca) = (red as f32 / 255.0, green as f32 / 255.0, blue as f32 / 255.0, alpha as f32 / 255.0);
        let w = self.w;
        let px = &mut *self.px;
        o.r.for_each_pixel_2d(|x, y, cov| {
            let a = (cov * ca).clamp(0.0, 1.0);
            if a <= 0.0 {
                return;
            }
            let i = (y as usize * w + x as usize) * 4;
            let keep = 1.0 - a;
            px[i] = cr * a + px[i] * keep;
            px[i + 1] = cg * a + px[i + 1] * keep;
            px[i + 2] = cb * a + px[i + 2] * keep;
            px[i + 3] = a + px[i + 3] * keep;
        });
    }

    fn push_clip(&mut self) {}
    fn push_clip_box(&mut self, _: colr::ClipBox) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _: colr::CompositeMode) {}
    fn pop_layer(&mut self) {}
    fn push_transform(&mut self, _: ttf_parser::Transform) {}
    fn pop_transform(&mut self) {}
}

/// A drawn emoji: premultiplied RGBA, `w * h` pixels.
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

impl EmojiFont {
    pub fn new(data: &'static [u8]) -> Option<EmojiFont> {
        let face = ttf_parser::Face::parse(data, 0).ok()?;
        face.tables().colr.as_ref()?;
        Some(EmojiFont { face })
    }

    /// `GSUB` ligatures (the face joins ZWJ sequences, keycaps and tones this way).
    fn shape(&self, cluster: &str) -> Vec<GlyphId> {
        let gids: Vec<GlyphId> = cluster.chars().filter_map(|c| self.face.glyph_index(c)).collect();
        let Some(gsub) = self.face.tables().gsub else { return gids };
        let mut out = Vec::with_capacity(gids.len());
        let mut i = 0;
        'next: while i < gids.len() {
            for li in 0..gsub.lookups.len() {
                let Some(lookup) = gsub.lookups.get(li) else { continue };
                for sub in lookup.subtables.into_iter::<SubstitutionSubtable>() {
                    let SubstitutionSubtable::Ligature(lig) = sub else { continue };
                    let Some(ci) = lig.coverage.get(gids[i]) else { continue };
                    let Some(set) = lig.ligature_sets.get(ci) else { continue };
                    for l in set {
                        let n = l.components.len() as usize;
                        if i + 1 + n <= gids.len() && l.components.into_iter().enumerate().all(|(k, g)| gids[i + 1 + k] == g) {
                            out.push(l.glyph);
                            i += 1 + n;
                            continue 'next;
                        }
                    }
                }
            }
            out.push(gids[i]);
            i += 1;
        }
        out
    }

    /// Can this cluster be drawn from the face? (Every base character has a coloured glyph.)
    pub fn can_draw(&self, cluster: &str) -> bool {
        let g = self.shape(cluster);
        let colr = self.face.tables().colr.as_ref();
        !g.is_empty() && g.iter().any(|id| colr.is_some_and(|c| c.contains(*id)))
    }

    /// Draw a cluster at `em_px` pixels per em (the bitmap is `WIDTH_EM x HEIGHT_EM` ems per glyph).
    pub fn render(&self, cluster: &str, em_px: f32) -> Option<Bitmap> {
        let gids = self.shape(cluster);
        let colr = self.face.tables().colr.as_ref()?;
        let drawn: Vec<GlyphId> = gids.into_iter().filter(|g| colr.contains(*g)).collect();
        if drawn.is_empty() {
            return None;
        }
        let scale = em_px / UPEM;
        let cell_w = (WIDTH_EM * em_px).ceil().max(1.0);
        let h = (HEIGHT_EM * em_px).ceil().max(1.0) as usize;
        let w = (cell_w as usize) * drawn.len();
        let mut px = vec![0f32; w * h * 4];
        // the glyph advance is one em; the cell is a little wider, so centre the glyph in it
        let pad = (cell_w - em_px) / 2.0;
        for (k, g) in drawn.iter().enumerate() {
            let mut p = LayerPainter {
                face: &self.face,
                scale,
                ox: k as f32 * cell_w + pad,
                oy: BASELINE * h as f32,
                w,
                h,
                glyph: None,
                px: &mut px,
            };
            colr.paint(*g, 0, &mut p, &[], RgbaColor::new(0, 0, 0, 255));
        }
        let rgba = px.chunks_exact(4).flat_map(|c| [(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8, (c[3] * 255.0).round() as u8]).collect();
        Some(Bitmap { w, h, rgba })
    }
}

// ------------------------------------------------------------------ textures

/// The face, plus the textures of clusters already drawn (per size).
#[derive(Default)]
pub struct EmojiCache {
    font: Option<EmojiFont>,
    tex: HashMap<(String, u32), Option<egui::TextureHandle>>,
}

impl EmojiCache {
    pub fn set_font(&mut self, data: &'static [u8]) {
        self.font = EmojiFont::new(data);
        self.tex.clear();
    }

    pub fn has_font(&self) -> bool {
        self.font.is_some()
    }

    /// How many pictures have been drawn so far (tests).
    pub fn drawn(&self) -> usize {
        self.tex.values().filter(|t| t.is_some()).count()
    }

    /// The picture of `cluster` for text of `px` points, and its size in points; `None` when the face cannot
    /// draw it (the caller falls back to ordinary text).
    pub fn get(&mut self, ctx: &egui::Context, cluster: &str, px: f32) -> Option<(egui::TextureId, egui::Vec2)> {
        let font = self.font.as_ref()?;
        let ppp = ctx.pixels_per_point().max(1.0);
        // draw at least twice the logical size so scaling to the screen stays sharp
        let scale = ppp.max(2.0);
        let key = (cluster.to_string(), (px * scale).round() as u32);
        if !self.tex.contains_key(&key) {
            let tex = font.render(cluster, px * scale).map(|b| {
                let img = egui::ColorImage::from_rgba_premultiplied([b.w, b.h], &b.rgba);
                ctx.load_texture(format!("emoji-{cluster}"), img, egui::TextureOptions::LINEAR)
            });
            self.tex.insert(key.clone(), tex);
        }
        let t = self.tex.get(&key)?.as_ref()?;
        let n = cluster_glyphs(font, cluster).max(1) as f32;
        Some((t.id(), egui::vec2(WIDTH_EM * px * n, HEIGHT_EM * px)))
    }
}

fn cluster_glyphs(font: &EmojiFont, cluster: &str) -> usize {
    let g = font.shape(cluster);
    let colr = font.face.tables().colr.as_ref();
    g.iter().filter(|id| colr.is_some_and(|c| c.contains(**id))).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<(bool, &str)> {
        segments(s).into_iter().map(|g| match g { Seg::Text(t) => (false, t), Seg::Emoji(e) => (true, e) }).collect()
    }

    #[test]
    fn plain_text_has_no_emoji() {
        assert_eq!(kinds("Last line with 95% and #7 and 1. ok"), vec![(false, "Last line with 95% and #7 and 1. ok")]);
        assert!(!has_emoji("check \u{2713} and \u{2192} arrows \u{00A9}"), "text-style symbols stay text");
    }

    #[test]
    fn a_smile_is_one_cluster_between_text() {
        assert_eq!(kinds("so \u{1F600} ok"), vec![(false, "so "), (true, "\u{1F600}"), (false, " ok")]);
        assert_eq!(kinds("\u{65E5}\u{672C}\u{8A9E} \u{1F600}"), vec![(false, "\u{65E5}\u{672C}\u{8A9E} "), (true, "\u{1F600}")]);
    }

    #[test]
    fn sequences_stay_together() {
        // family: man ZWJ woman ZWJ girl
        let fam = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(kinds(fam), vec![(true, fam)]);
        // thumbs up with a skin tone
        assert_eq!(kinds("\u{1F44D}\u{1F3FD}!"), vec![(true, "\u{1F44D}\u{1F3FD}"), (false, "!")]);
        // a flag is two regional indicators
        assert_eq!(kinds("\u{1F1EF}\u{1F1F5}"), vec![(true, "\u{1F1EF}\u{1F1F5}")]);
        // a text symbol asked to be emoji with VS16, and a keycap
        assert_eq!(kinds("\u{2764}\u{FE0F}"), vec![(true, "\u{2764}\u{FE0F}")]);
        assert_eq!(kinds("1\u{FE0F}\u{20E3}"), vec![(true, "1\u{FE0F}\u{20E3}")]);
    }

    #[test]
    fn the_bundled_face_draws_colour() {
        let font = EmojiFont::new(include_bytes!("../assets/fonts/twemoji-colr.ttf")).expect("a COLR face");
        assert!(font.can_draw("\u{1F600}"));
        let b = font.render("\u{1F600}", 48.0).expect("a bitmap");
        assert!(b.w >= 48 && b.h >= 48);
        // yellow face: some pixels are opaque and more red than blue
        let mut yellow = 0;
        for p in b.rgba.chunks_exact(4) {
            if p[3] > 200 && p[0] > 200 && p[1] > 150 && p[2] < 120 {
                yellow += 1;
            }
        }
        assert!(yellow > 300, "{yellow} yellow pixels");
        // a family is one ligature glyph, one cell wide
        let fam = font.render("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", 48.0).expect("family");
        let one = font.render("\u{1F468}", 48.0).unwrap();
        assert_eq!(fam.w, one.w, "the ZWJ sequence is joined into one picture");
        // a letter is not an emoji
        assert!(!font.can_draw("a"));
    }
}
