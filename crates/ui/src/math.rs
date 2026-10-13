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
}

impl MathCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shapes for `tex` at `px` in `color`, `None` when it cannot be typeset. The STIX font is parsed
    /// on first use, so a screen without math never pays for it.
    pub fn get(&mut self, tex: &str, display: bool, px: f32, color: Color32) -> Option<&Rendered> {
        let key = Key { tex: tex.into(), display, px: px.round() as u32, rgba: color.to_array() };
        if !self.map.contains_key(&key) {
            let font = self.font.get_or_insert_with(|| MathFont::stix_two_math().ok());
            let r = font.as_ref().and_then(|f| {
                let mut o = EguiOptions::new();
                o.font_size_pt = Dim::from_i64(key.px as i64);
                o.color = Color::rgb(color.r(), color.g(), color.b());
                o.display = display;
                latex_to_shapes(tex, f, &o, Pos2::ZERO, 1.0).ok().map(|(shapes, rect)| Rendered { shapes, size: rect.size() })
            });
            if r.is_none() {
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
}
