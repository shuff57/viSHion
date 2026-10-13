//! Math isolation (plan risks table, rule 4): the only module that knows `latex-rust`. A cache of shape
//! lists per (tex, display, size, colour). A layout error is not an error for the screen: the source is
//! shown in red monospace, exactly as `render.js` does with `<code class="math-fb">`.
use egui::{Color32, Pos2, Shape, Vec2};
use latex_rust::{Color, Dim, EguiOptions, MathFont};
use std::collections::HashMap;

pub struct Rendered {
    shapes: Vec<Shape>,
    pub size: Vec2,
    /// Distance from the top of `size` to the baseline, so pieces of one formula can share a baseline.
    pub ascent: f32,
}

impl Rendered {
    pub fn shapes(&self) -> impl Iterator<Item = Shape> + '_ {
        self.shapes.iter().cloned()
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct Key {
    tex: String,
    display: bool,
    px: u32,
    rgba: [u8; 4],
}

#[derive(Default)]
pub struct MathCache {
    font: Option<Option<MathFont>>,
    map: HashMap<Key, Option<Rendered>>,
    /// TeX sources that failed to lay out (shown as source).
    pub failed: Vec<String>,
    /// Math was asked for while the font had not arrived (web only: the font is fetched, not linked in).
    wanted: bool,
}

impl MathCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Web: the font arrived (fetched on first need, so the 840 KB OpenType file is not part of the wasm).
    /// Shapes that were laid out as "pending" are forgotten and laid out again.
    pub fn set_font(&mut self, bytes: Vec<u8>) {
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        self.font = Some(MathFont::from_bytes(leaked).ok());
        self.map.clear();
        self.failed.clear();
        self.wanted = false;
    }

    /// True while math is on screen and the font has not been supplied yet (web).
    pub fn wants_font(&self) -> bool {
        self.wanted && self.font.is_none()
    }

    /// The font is still on its way: the caller shows the TeX quietly, not as an error.
    pub fn pending(&self) -> bool {
        self.font.is_none() && cfg!(target_arch = "wasm32")
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn default_font() -> Option<MathFont> {
        MathFont::stix_two_math().ok()
    }

    /// Shapes for `tex` at `px` in `color`, `None` when it cannot be typeset. The STIX font is parsed
    /// on first use, so a screen without math never pays for it.
    pub fn get(&mut self, tex: &str, display: bool, px: f32, color: Color32) -> Option<&Rendered> {
        let key = Key { tex: tex.into(), display, px: px.round() as u32, rgba: color.to_array() };
        #[cfg(target_arch = "wasm32")]
        if self.font.is_none() {
            self.wanted = true;
            return None;
        }
        if !self.map.contains_key(&key) {
            #[cfg(not(target_arch = "wasm32"))]
            let font = self.font.get_or_insert_with(Self::default_font);
            #[cfg(target_arch = "wasm32")]
            let font = self.font.as_mut().expect("font present");
            let r = font.as_ref().and_then(|f| {
                let mut o = EguiOptions::new();
                o.font_size_pt = Dim::from_i64(key.px as i64);
                o.color = Color::rgb(color.r(), color.g(), color.b());
                o.display = display;
                let ast = latex_rust::parse(tex).ok()?;
                let style = if display { latex_rust::MathStyle::Display } else { latex_rust::MathStyle::Text };
                let tree = latex_rust::layout_with_em_size_pt(&ast, f, style, &o.font_size_pt).ok()?;
                let ascent = tree.height.to_dec_string().parse::<f32>().unwrap_or(0.0) * key.px as f32;
                latex_rust::shapes(&tree, f, &o, Pos2::ZERO, 1.0).ok().map(|(shapes, rect)| Rendered { shapes, size: rect.size(), ascent })
            });
            if r.is_none() && font.is_some() {
                self.failed.push(tex.into());
            }
            self.map.insert(key.clone(), r);
        }
        self.map[&key].as_ref()
    }

    /// Reserve room and paint. Returns `false` when the TeX could not be typeset (caller draws the source).
    pub fn paint(&mut self, ui: &mut egui::Ui, tex: &str, display: bool, px: f32, color: Color32) -> bool {
        match self.get(tex, display, px, color) {
            Some(r) => {
                let (rect, _) = ui.allocate_exact_size(r.size + Vec2::new(4.0, 2.0), egui::Sense::hover());
                let d = rect.min.to_vec2() + Vec2::new(2.0, 1.0);
                if ui.is_rect_visible(rect) {
                    ui.painter().extend(r.shapes.iter().cloned().map(|mut s| {
                        s.translate(d);
                        s
                    }));
                }
                true
            }
            None => false,
        }
    }
}


/// Binary operators and relations KaTeX may break a line after (`buildHTML` puts an allowed break after an
/// `mbin` or `mrel` at the top level of an inline formula).
const BREAK_AFTER: &[&str] = &[
    "pm", "mp", "cdot", "times", "div", "ast", "star", "circ", "bullet", "cap", "cup", "wedge", "vee", "oplus", "ominus", "otimes",
    "setminus", "le", "leq", "ge", "geq", "ne", "neq", "approx", "sim", "simeq", "cong", "equiv", "propto", "in", "ni", "notin",
    "subset", "supset", "subseteq", "supseteq", "to", "rightarrow", "leftarrow", "Rightarrow", "Leftarrow", "Longrightarrow",
    "longrightarrow", "mapsto", "leftrightarrow", "Leftrightarrow", "iff", "implies", "perp", "parallel", "mid", "ll", "gg", "prec",
    "succ",
];

/// Split an inline formula where a browser could wrap it (css `.katex`: breaks only between top-level atoms,
/// after a binary operator or relation, never inside a group, fraction or `\left ... \right`). Every piece but
/// the last ends with `{}` so the operator keeps the space TeX sets around it when the pieces share a line.
/// A formula with no such place comes back whole.
pub fn split_breaks(tex: &str) -> Vec<String> {
    #[derive(PartialEq, Clone, Copy)]
    enum Prev {
        Start,
        Ord,
        Open,
        Op,
    }
    if tex.contains("\\begin") || tex.contains("\\\\") {
        return vec![tex.to_string()];
    }
    let b = tex.as_bytes();
    let mut cuts: Vec<usize> = vec![];
    let (mut depth, mut i, mut prev) = (0i32, 0usize, Prev::Start);
    // after `\left` / `\right` / `^` / `_` the next token belongs to the atom before it
    let mut skip_token = false;
    while i < b.len() {
        let c = b[i] as char;
        let token_end;
        let mut op = false;
        let mut open = false;
        if c == '\\' {
            let mut j = i + 1;
            if j < b.len() && (b[j] as char).is_ascii_alphabetic() {
                while j < b.len() && (b[j] as char).is_ascii_alphabetic() {
                    j += 1;
                }
            } else if j < b.len() {
                j += tex[j..].chars().next().map_or(1, |c| c.len_utf8());
            }
            token_end = j;
            let name = &tex[i + 1..j];
            match name {
                "left" | "right" => {
                    if name == "left" {
                        depth += 1;
                    } else {
                        depth -= 1;
                    }
                    skip_token = true;
                    i = j;
                    continue;
                }
                " " | "," | ";" | ":" | "!" | "quad" | "qquad" => {
                    i = j;
                    continue;
                }
                "{" => open = true,
                _ => op = BREAK_AFTER.contains(&name),
            }
        } else if c.is_whitespace() {
            i += 1;
            continue;
        } else if c == '{' {
            // a whole group is one atom
            let mut d = 0;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'\\' => j += 1,
                    b'{' => d += 1,
                    b'}' => {
                        d -= 1;
                        if d == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            i = (j + 1).min(b.len());
            if skip_token {
                skip_token = false;
            } else {
                prev = Prev::Ord;
            }
            continue;
        } else {
            token_end = i + tex[i..].chars().next().map_or(1, |c| c.len_utf8());
            match c {
                '+' | '-' | '=' | '<' | '>' => op = true,
                '(' | '[' | ',' | ';' => open = true,
                '^' | '_' => {
                    skip_token = true;
                    i = token_end;
                    continue;
                }
                _ => {}
            }
        }
        if skip_token {
            skip_token = false;
            i = token_end;
            continue;
        }
        if depth == 0 && op && matches!(prev, Prev::Ord) {
            cuts.push(token_end);
            prev = Prev::Op;
        } else if open || (op && depth == 0) {
            prev = Prev::Open;
        } else if op {
            // inside \left..\right: not a place to break
        } else {
            prev = Prev::Ord;
        }
        i = token_end;
    }
    // a break at the very end is no break
    while cuts.last().is_some_and(|&e| tex[e..].trim().is_empty()) {
        cuts.pop();
    }
    if cuts.is_empty() {
        return vec![tex.to_string()];
    }
    let mut out = vec![];
    let mut at = 0;
    for e in cuts {
        out.push(format!("{}{{}}", tex[at..e].trim()));
        at = e;
    }
    out.push(tex[at..].trim().to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesets_and_falls_back() {
        let mut c = MathCache::new();
        assert!(c.get("\\frac{a}{b}", false, 22.0, Color32::BLACK).is_some());
        assert!(c.get("\\bar{x} \\pm 1.96", false, 22.0, Color32::BLACK).is_some());
        // known gap from S7: \pmod is unsupported, the caller shows the source
        assert!(c.get("a \\pmod{n}", false, 22.0, Color32::BLACK).is_none());
        assert_eq!(c.failed, vec!["a \\pmod{n}".to_string()]);
    }

    #[test]
    fn breaks_come_after_binary_operators_and_relations_at_the_top_level() {
        let one = |s: &str| split_breaks(s);
        assert_eq!(one("a + b"), vec!["a +{}", "b"]);
        assert_eq!(one("-x + 1"), vec!["-x +{}", "1"]);
        assert_eq!(one("\\frac{a+b}{c}"), vec!["\\frac{a+b}{c}"]);
        assert_eq!(one("x^{2+1}"), vec!["x^{2+1}"]);
        assert_eq!(one("x^- + 1").len(), 2);
        assert_eq!(one("\\left(a+b\\right) = c"), vec!["\\left(a+b\\right) ={}", "c"]);
        assert_eq!(one("\\bar{x} \\pm 1.96 \\cdot \\frac{s}{\\sqrt{n}} = 21.25").len(), 4);
        assert_eq!(one("a ="), vec!["a ="]);
        assert_eq!(one("x"), vec!["x"]);
    }

    /// The pieces put back together are as wide as the formula whole: the empty group keeps the space the
    /// operator has in TeX, so a formula that fits on a line looks the same split or not.
    #[test]
    fn pieces_are_as_wide_as_the_whole() {
        let mut c = MathCache::new();
        for tex in ["a + b", "\\bar{x} \\pm 1.96 \\cdot \\frac{s}{\\sqrt{n}} = 21.25 \\pm 1.96", "x \\le y - 3"] {
            let whole = c.get(tex, false, 26.0, Color32::BLACK).unwrap().size.x;
            let parts = split_breaks(tex);
            assert!(parts.len() > 1);
            let sum: f32 = parts.iter().map(|p| c.get(p, false, 26.0, Color32::BLACK).unwrap_or_else(|| panic!("{p}")).size.x).sum();
            assert!((whole - sum).abs() < 1.0, "{tex}: whole {whole}, pieces {sum}");
        }
    }

    /// The web path: the font arrives as bytes (fetched), and typesetting is identical to the embedded one.
    #[test]
    fn a_supplied_font_typesets_the_same_shapes() {
        let mut embedded = MathCache::new();
        let mut supplied = MathCache::new();
        supplied.set_font(latex_rust::STIX_TWO_MATH_OTF.to_vec());
        assert!(!supplied.wants_font());
        let a = embedded.get("\\frac{a}{b} + x^2", false, 22.0, Color32::BLACK).map(|r| r.size);
        let b = supplied.get("\\frac{a}{b} + x^2", false, 22.0, Color32::BLACK).map(|r| r.size);
        assert!(a.is_some() && a == b);
        // bytes that are not a font: nothing typesets, the source fallback applies
        let mut broken = MathCache::new();
        broken.set_font(vec![1, 2, 3]);
        assert!(broken.get("x", false, 22.0, Color32::BLACK).is_none());
    }
}
