//! S7: can egui render viSHion transcripts (inline markdown + KaTeX-style math) on native AND wasm?
//! Mirrors js/render.js: units() split, $..$ / $$..$$ held as placeholders, inline markdown, [illegible].
use egui::{Color32, RichText, Shape, Vec2};
use latex_rust::{latex_to_shapes, EguiOptions, MathFont};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

#[derive(serde::Deserialize, Clone)]
pub struct Case {
    pub name: String,
    pub text: String,
}

pub fn corpus() -> Vec<Case> {
    serde_json::from_str(include_str!("../corpus.json")).unwrap()
}

/// Same rule as R.units: blank lines dropped, a multi-line $$ block stays one unit.
pub fn units(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.replace('\r', "").leak().split('\n').collect();
    let (mut out, mut i) = (vec![], 0);
    while i < lines.len() {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if t.starts_with("$$") && !t[2..].trim_end().ends_with("$$") {
            let mut buf = vec![lines[i]];
            i += 1;
            while i < lines.len() {
                buf.push(lines[i]);
                let end = lines[i].trim_end().ends_with("$$");
                i += 1;
                if end {
                    break;
                }
            }
            out.push(buf.join("\n"));
        } else {
            out.push(lines[i].to_string());
            i += 1;
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct Math {
    pub tex: String,
    pub display: bool,
}

/// Pull $$..$$ then $..$ out of a unit, leaving ⟦n⟧ (as render.js does).
pub fn hold_math(src: &str) -> (String, Vec<Math>) {
    let mut math = vec![];
    let mut out = String::new();
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '$' && b.get(i + 1) == Some(&'$') {
            if let Some(j) = (i + 2..b.len().saturating_sub(1)).find(|&j| b[j] == '$' && b[j + 1] == '$') {
                if j > i + 2 {
                    let tex: String = b[i + 2..j].iter().collect();
                    math.push(Math { tex: tex.trim().into(), display: true });
                    out.push_str(&format!("⟦{}⟧", math.len() - 1));
                    i = j + 2;
                    continue;
                }
            }
        } else if b[i] == '$' {
            if let Some(j) = (i + 1..b.len()).find(|&j| b[j] == '$' || b[j] == '\n') {
                let tex: String = b[i + 1..j].iter().collect();
                if b[j] == '$' && !tex.is_empty() && !tex.starts_with(char::is_whitespace) && !tex.ends_with(char::is_whitespace) {
                    math.push(Math { tex, display: false });
                    out.push_str(&format!("⟦{}⟧", math.len() - 1));
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    (out, math)
}

#[derive(Clone, Debug)]
pub enum Piece {
    Text { s: String, bold: bool, italic: bool, strike: bool, code: bool, illegible: bool },
    Math(usize),
}

/// Inline markdown -> flat pieces. Placeholders ⟦n⟧ become Piece::Math(n).
pub fn pieces(held: &str, nmath: usize) -> Vec<Piece> {
    let (mut bold, mut italic, mut strike) = (0, 0, 0);
    let mut out = vec![];
    let push_text = |s: &str, bold: i32, italic: i32, strike: i32, code: bool, out: &mut Vec<Piece>| {
        let mut rest = s;
        while let Some(a) = rest.find('⟦') {
            let tail = &rest[a + '⟦'.len_utf8()..];
            let parsed = tail.find('⟧').and_then(|e| tail[..e].parse::<usize>().ok().filter(|n| *n < nmath).map(|n| (n, e)));
            match parsed {
                Some((n, e)) => {
                    if a > 0 {
                        out.push(Piece::Text { s: rest[..a].into(), bold: bold > 0, italic: italic > 0, strike: strike > 0, code, illegible: false });
                    }
                    out.push(Piece::Math(n));
                    rest = &tail[e + '⟧'.len_utf8()..];
                }
                None => {
                    let cut = a + '⟦'.len_utf8();
                    out.push(Piece::Text { s: rest[..cut].into(), bold: bold > 0, italic: italic > 0, strike: strike > 0, code, illegible: false });
                    rest = &rest[cut..];
                }
            }
        }
        if !rest.is_empty() {
            let low = rest.to_lowercase();
            let ill = low == "[illegible]";
            out.push(Piece::Text { s: rest.into(), bold: bold > 0, italic: italic > 0, strike: strike > 0, code, illegible: ill });
        }
    };
    let held = &swap_illegible(held);
    let opts = Options::ENABLE_STRIKETHROUGH;
    for ev in Parser::new_ext(held, opts) {
        match ev {
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold -= 1,
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic -= 1,
            Event::Start(Tag::Strikethrough) => strike += 1,
            Event::End(TagEnd::Strikethrough) => strike -= 1,
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                let mut s = t.as_ref();
                while let Some(a) = s.find('') {
                    if a > 0 {
                        push_text(&s[..a], bold, italic, strike, false, &mut out);
                    }
                    out.push(Piece::Text { s: "[illegible]".into(), bold: bold > 0, italic: italic > 0, strike: strike > 0, code: false, illegible: true });
                    s = &s[a + ''.len_utf8()..];
                }
                push_text(s, bold, italic, strike, false, &mut out);
            }
            Event::Code(t) => push_text(&t, bold, italic, strike, true, &mut out),
            Event::SoftBreak | Event::HardBreak => push_text(" ", bold, italic, strike, false, &mut out),
            _ => {}
        }
    }
    out
}

fn swap_illegible(s: &str) -> String {
    let low = s.to_lowercase();
    let (mut out, mut i) = (String::new(), 0);
    while let Some(a) = low[i..].find("[illegible]") {
        out.push_str(&s[i..i + a]);
        out.push('');
        i += a + 11;
    }
    out.push_str(&s[i..]);
    out
}

pub struct Rendered {
    pub shapes: Vec<Shape>,
    pub size: Vec2,
}

pub struct MathCache {
    font: Option<MathFont>,
    map: HashMap<(String, bool, u32), Option<Rendered>>,
    pub errors: Vec<String>,
}

impl MathCache {
    pub fn new() -> Self {
        Self { font: MathFont::stix_two_math().ok(), map: HashMap::new(), errors: vec![] }
    }
    pub fn get(&mut self, m: &Math, pt: u32, color: Color32) -> Option<&Rendered> {
        let key = (m.tex.clone(), m.display, pt);
        if !self.map.contains_key(&key) {
            let r = self.font.as_ref().and_then(|f| {
                let mut o = EguiOptions::new();
                o.font_size_pt = latex_rust::Dim::from_i64(pt as i64);
                o.color = latex_rust::Color::rgb(color.r(), color.g(), color.b());
                o.display = m.display;
                match latex_to_shapes(&m.tex, f, &o, egui::Pos2::ZERO, 1.0) {
                    Ok((shapes, rect)) => Some(Rendered { shapes, size: rect.size() }),
                    Err(_) => None,
                }
            });
            if r.is_none() {
                self.errors.push(m.tex.clone());
            }
            self.map.insert(key.clone(), r);
        }
        self.map[&key].as_ref()
    }
}

/// Draw one unit as a wrapped flow of text runs and math boxes.
pub fn show_unit(ui: &mut egui::Ui, cache: &mut MathCache, src: &str) {
    let (held, math) = hold_math(src);
    let ps = pieces(&held, math.len());
    let pt = 16u32;
    let ink = ui.visuals().text_color();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for p in &ps {
            match p {
                Piece::Text { s, bold, italic, strike, code, illegible } => {
                    let mut t = RichText::new(s.as_str()).size(pt as f32);
                    if *bold { t = t.strong(); }
                    if *italic { t = t.italics(); }
                    if *strike { t = t.strikethrough(); }
                    if *code { t = t.monospace().background_color(ui.visuals().faint_bg_color); }
                    if *illegible { t = t.color(Color32::from_rgb(180, 90, 20)).italics(); }
                    ui.label(t);
                }
                Piece::Math(n) => {
                    let m = &math[*n];
                    match cache.get(m, pt, ink) {
                        Some(r) => {
                            let (rect, _) = ui.allocate_exact_size(r.size + Vec2::new(4.0, 2.0), egui::Sense::hover());
                            let d = rect.min.to_vec2() + Vec2::new(2.0, 1.0);
                            ui.painter().extend(r.shapes.iter().cloned().map(|mut s| { s.translate(d); s }));
                        }
                        // same fallback as render.js: <code class="math-fb">tex</code>
                        None => {
                            ui.label(RichText::new(&m.tex).monospace().color(Color32::from_rgb(160, 40, 40)));
                        }
                    }
                }
            }
        }
    });
}

pub struct App {
    cases: Vec<Case>,
    sel: usize,
    mirrored: Option<usize>,
    cache: MathCache,
}

impl App {
    pub fn new() -> Self {
        Self { cases: corpus(), sel: 0, mirrored: None, cache: MathCache::new() }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        if self.mirrored != Some(self.sel) {
            self.mirrored = Some(self.sel);
            #[cfg(target_arch = "wasm32")]
            a11y_mirror(&self.cases[self.sel].text);
        }
        egui::SidePanel::left("list").show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, c) in self.cases.iter().enumerate() {
                    if ui.selectable_label(self.sel == i, &c.name).clicked() {
                        self.sel = i;
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let text = self.cases[self.sel].text.clone();
                for u in units(&text) {
                    show_unit(ui, &mut self.cache, &u);
                    ui.add_space(6.0);
                }
            });
        });
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    pub async fn start(canvas_id: String) -> Result<(), JsValue> {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        eframe::WebRunner::new()
            .start(&canvas_id, eframe::WebOptions::default(), Box::new(|_| Ok(Box::new(super::App::new()))))
            .await
    }
}

/// Hidden DOM twin of the transcript so assistive tech has something to read. Plain DOM, no egui involved.
#[cfg(target_arch = "wasm32")]
fn a11y_mirror(text: &str) {
    use wasm_bindgen::JsCast;
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Some(host) = doc.get_element_by_id("a11y") else { return };
    host.set_text_content(None);
    for u in units(text) {
        let (held, math) = hold_math(&u);
        let Ok(p) = doc.create_element("p") else { continue };
        for piece in pieces(&held, math.len()) {
            let Ok(span) = doc.create_element("span") else { continue };
            match piece {
                Piece::Text { s, .. } => span.set_text_content(Some(&s)),
                Piece::Math(n) => {
                    let _ = span.set_attribute("role", "math");
                    span.set_text_content(Some(&math[n].tex));
                }
            }
            let _ = p.append_child(&span);
        }
        let _ = host.append_child(&p);
    }
    let _ = host.dyn_ref::<web_sys::HtmlElement>();
}
