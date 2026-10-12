use crate::pipeline::{self, Math};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// `window.markdownit` (UMD global), called as a plain function: `markdownit(options)`.
    #[wasm_bindgen(js_name = markdownit)]
    fn markdownit(opts: &JsValue) -> MarkdownIt;
    pub type MarkdownIt;
    #[wasm_bindgen(method, catch, js_name = renderInline)]
    fn render_inline(this: &MarkdownIt, src: &str) -> Result<String, JsValue>;
    /// `katex.renderToString(tex, opts)`; `catch` turns a JS throw into Err instead of a wasm trap.
    #[wasm_bindgen(js_namespace = katex, js_name = renderToString, catch)]
    fn katex_render(tex: &str, opts: &JsValue) -> Result<String, JsValue>;
}

fn obj(pairs: &[(&str, bool)]) -> JsValue {
    let o = Object::new();
    for (k, v) in pairs { Reflect::set(&o, &JsValue::from_str(k), &JsValue::from_bool(*v)).unwrap(); }
    o.into()
}
fn katex_present() -> bool {
    Reflect::get(&js_sys::global(), &JsValue::from_str("katex")).map(|v| v.is_truthy()).unwrap_or(false)
}

#[wasm_bindgen]
pub struct Renderer { md: MarkdownIt, inline: JsValue, display: JsValue }

#[wasm_bindgen]
impl Renderer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Renderer {
        Renderer {
            md: markdownit(&obj(&[("html", false), ("linkify", false), ("breaks", true)])),
            inline: obj(&[("displayMode", false), ("throwOnError", false)]),
            display: obj(&[("displayMode", true), ("throwOnError", false)]),
        }
    }

    /// R.unit(src)
    pub fn unit(&self, src: &str) -> Result<String, JsValue> {
        let mut math = |m: &Math| -> String {
            if katex_present() {
                if let Ok(h) = katex_render(&m.tex, if m.display { &self.display } else { &self.inline }) { return h; }
            }
            format!("<code class=\"math-fb\">{}</code>", pipeline::esc(&m.tex))
        };
        pipeline::unit(src, &mut |t| self.md.render_inline(t), &mut math)
    }

    /// R.units(text)
    pub fn units(&self, text: &str) -> Array {
        pipeline::units(text).into_iter().map(|s| JsValue::from_str(&s)).collect()
    }

    /// Whole transcript -> HTML of every unit (what renderFormatted feeds innerHTML), for the glue-cost benchmark.
    pub fn render_all(&self, text: &str) -> Result<Array, JsValue> {
        let out = Array::new();
        for u in pipeline::units(text) { out.push(&JsValue::from_str(&self.unit(&u)?)); }
        Ok(out)
    }

    /// Bare boundary crossings, for measuring glue overhead without pipeline logic.
    pub fn bare_inline(&self, s: &str) -> Result<String, JsValue> { self.md.render_inline(s) }
    pub fn bare_katex(&self, s: &str) -> Result<String, JsValue> { katex_render(s, &self.inline) }
}
