//! Math isolation (plan risks table, rule 4): the only module that knows `latex-rust`. A cache of shape
//! lists per (tex, display, size, colour). A layout error is not an error for the screen: the source is
//! shown in red monospace, exactly as `render.js` does with `<code class="math-fb">`.
use egui::{Color32, Pos2, Shape, Vec2};
use latex_rust::{latex_to_shapes, Color, Dim, EguiOptions, MathFont};
use std::collections::HashMap;

pub struct Rendered {
    shapes: Vec<Shape>,
    pub size: Vec2,
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
                latex_to_shapes(tex, f, &o, Pos2::ZERO, 1.0).ok().map(|(shapes, rect)| Rendered { shapes, size: rect.size() })
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
