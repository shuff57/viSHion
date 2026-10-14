//! Port of js/render.js: the string plumbing of the transcript renderer, and the flat "pieces" the egui
//! UI draws (spikes/s7-egui). markdown-it and KaTeX are not here: `unit` takes the inline renderer and the
//! math renderer as closures, so web can inject the JS libraries and native can inject pulldown-cmark.
//! Byte-for-byte compatibility with JS is the goal, so JS whitespace and regex semantics are reproduced.
use crate::jsx::{is_ws, trim};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Math {
    pub tex: String,
    pub display: bool,
}

/// `OCR.esc`.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn hold(math: &mut Vec<Math>, tex: &str, display: bool) -> String {
    math.push(Math { tex: tex.to_string(), display });
    format!("\u{27E6}{}\u{27E7}", math.len() - 1)
}

/// `src.replace(/\$\$([\s\S]+?)\$\$/g, hold(a.trim(), true))` then
/// `.replace(/\$([^$\n]+?)\$/g, (m,a) => /^\s|\s$/.test(a) ? m : hold(a,false))`.
pub fn hold_math(src: &str) -> (String, Vec<Math>) {
    let mut math = Vec::new();
    // pass 1: $$...$$ (lazy, at least one char, may span newlines)
    let mut t = String::with_capacity(src.len());
    let mut i = 0;
    while i < src.len() {
        if src[i..].starts_with("$$") {
            let after = i + 2;
            if let Some(first) = src[after..].chars().next() {
                let from = after + first.len_utf8();
                if let Some(rel) = src[from..].find("$$") {
                    let j = from + rel;
                    let ph = hold(&mut math, trim(&src[after..j]), true);
                    t.push_str(&ph);
                    i = j + 2;
                    continue;
                }
            }
        }
        let c = src[i..].chars().next().unwrap();
        t.push(c);
        i += c.len_utf8();
    }
    // pass 2: $...$ (no `$`/newline inside; rejected when padded by whitespace, and the match is still consumed)
    let mut out = String::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        if t.as_bytes()[i] == b'$' {
            let rest = &t[i + 1..];
            if let Some(p) = rest.find(['$', '\n']) {
                if p > 0 && rest.as_bytes()[p] == b'$' {
                    let a = &rest[..p];
                    let padded = a.chars().next().map_or(false, is_ws) || a.chars().next_back().map_or(false, is_ws);
                    if padded {
                        out.push_str(&t[i..i + 1 + p + 1]);
                    } else {
                        let ph = hold(&mut math, a, false);
                        out.push_str(&ph);
                    }
                    i += 1 + p + 1;
                    continue;
                }
            }
            out.push('$');
            i += 1;
            continue;
        }
        let c = t[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    (out, math)
}

/// `html.replace(/\[illegible\]/gi, '<span class="illegible">[illegible]</span>')`.
pub fn mark_illegible(html: &str) -> String {
    const NEEDLE: &str = "[illegible]";
    let b = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let (mut i, mut last) = (0, 0);
    while i + NEEDLE.len() <= b.len() {
        if b[i] == b'[' && b[i..i + NEEDLE.len()].eq_ignore_ascii_case(NEEDLE.as_bytes()) {
            out.push_str(&html[last..i]);
            out.push_str("<span class=\"illegible\">[illegible]</span>");
            i += NEEDLE.len();
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&html[last..]);
    out
}

/// `html.replace(/⟦(\d+)⟧/g, (m,i) => math[+i] ? mathHtml(math[+i]) : '')`.
pub fn restore_math(html: &str, math: &[Math], render: &mut dyn FnMut(&Math) -> String) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(p) = rest.find('\u{27E6}') {
        out.push_str(&rest[..p]);
        let after = &rest[p + '\u{27E6}'.len_utf8()..];
        let nd = after.bytes().take_while(|b| b.is_ascii_digit()).count();
        if nd > 0 && after[nd..].starts_with('\u{27E7}') {
            if let Some(m) = after[..nd].parse::<usize>().ok().and_then(|ix| math.get(ix)) {
                out.push_str(&render(m));
            }
            rest = &after[nd + '\u{27E7}'.len_utf8()..];
        } else {
            out.push('\u{27E6}');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `R.units`: display units. A multi-line `$$` block stays one unit; blank lines are dropped.
pub fn units(text: &str) -> Vec<String> {
    let cleaned = text.replace('\r', "");
    let lines: Vec<&str> = cleaned.split('\n').collect();
    let ends_dd = |s: &str| s.trim_end_matches(is_ws).ends_with("$$");
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = trim(l);
        if t.is_empty() {
            i += 1;
            continue;
        }
        if t.starts_with("$$") && !ends_dd(&t[2..]) {
            let mut buf = vec![l];
            i += 1;
            while i < lines.len() {
                buf.push(lines[i]);
                if ends_dd(lines[i]) {
                    break;
                }
                i += 1;
            }
            out.push(buf.join("\n"));
        } else {
            out.push(l.to_string());
        }
        i += 1;
    }
    out
}

/// `/^\s*Margin:/i`: the unit is a margin note (rendered with the `margin` class).
pub fn is_margin(unit: &str) -> bool {
    crate::jsx::starts_with_ci(unit.trim_start_matches(is_ws), "margin:")
}

/// `basic(t)`: the fallback used when markdown-it did not load: `esc` plus `~~strike~~`.
pub fn basic(t: &str) -> String {
    let e: Vec<char> = esc(t).chars().collect();
    let term = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    let mut out = String::new();
    let mut i = 0;
    while i < e.len() {
        if e[i] == '~' && e.get(i + 1) == Some(&'~') && e.get(i + 2).map_or(false, |c| !term(*c)) {
            // `.+?` is lazy: try "~~" at k, otherwise consume e[k] (never a line terminator) and go on
            let mut k = i + 3;
            let mut found = None;
            while k + 1 < e.len() {
                if e[k] == '~' && e[k + 1] == '~' {
                    found = Some(k);
                    break;
                }
                if term(e[k]) {
                    break;
                }
                k += 1;
            }
            if let Some(k) = found {
                out.push_str("<del>");
                out.extend(&e[i + 2..k]);
                out.push_str("</del>");
                i = k + 2;
                continue;
            }
        }
        out.push(e[i]);
        i += 1;
    }
    out
}

/// `mathHtml` without KaTeX: `<code class="math-fb">tex</code>`.
pub fn math_fallback(m: &Math) -> String {
    format!("<code class=\"math-fb\">{}</code>", esc(&m.tex))
}

/// `R.unit` with the inline renderer and the math renderer injected.
pub fn unit<E>(
    src: &str,
    inline: &mut dyn FnMut(&str) -> Result<String, E>,
    math: &mut dyn FnMut(&Math) -> String,
) -> Result<String, E> {
    let (t, held) = hold_math(src);
    let html = mark_illegible(&inline(&t)?);
    Ok(restore_math(&html, &held, math))
}

/// `R.unit` with neither markdown-it nor KaTeX available (the JS fallbacks).
pub fn unit_basic(src: &str) -> String {
    unit::<()>(src, &mut |t| Ok(basic(t)), &mut |m| math_fallback(m)).unwrap()
}

/// `R.mapBands`: fit detected ink bands to the number of transcript lines. Bands are JSON objects with
/// numeric `x y w h` (and optional `r`, `s`); extra keys are carried through like `Object.assign`.
pub fn map_bands(bands: &[Value], n: usize) -> Vec<Value> {
    let mut b: Vec<Map<String, Value>> = bands.iter().filter_map(|v| v.as_object().cloned()).collect();
    if n == 0 || b.is_empty() {
        return vec![];
    }
    let f = |m: &Map<String, Value>, k: &str| m.get(k).map_or(f64::NAN, crate::jsx::number);
    let nv = crate::jsx::num_value;
    while b.len() > n {
        if b.iter().all(|x| x.get("s").map_or(false, Value::is_number)) {
            // too many: the weakest band is the stray one (an edge, a smudge), not two lines to merge
            let mut w = 0;
            for i in 1..b.len() {
                if f(&b[i], "s") < f(&b[w], "s") {
                    w = i;
                }
            }
            b.remove(w);
            continue;
        }
        let (mut k, mut g) = (0, 1e9);
        for i in 0..b.len() - 1 {
            let gap = f(&b[i + 1], "y") - (f(&b[i], "y") + f(&b[i], "h"));
            if gap < g {
                g = gap;
                k = i;
            }
        }
        let (a, c) = (&b[k], &b[k + 1]);
        let x = f(a, "x").min(f(c, "x"));
        let x2 = (f(a, "x") + f(a, "w")).max(f(c, "x") + f(c, "w"));
        let mut m = Map::new();
        m.insert("x".into(), nv(x));
        m.insert("y".into(), nv(f(a, "y")));
        m.insert("w".into(), nv(x2 - x));
        m.insert("h".into(), nv(f(c, "y") + f(c, "h") - f(a, "y")));
        m.insert("r".into(), json!(0));
        b.splice(k..k + 2, [m]);
    }
    while b.len() < n {
        let mut k = 0;
        for i in 1..b.len() {
            if f(&b[i], "h") > f(&b[k], "h") {
                k = i;
            }
        }
        let a = b[k].clone();
        let h = f(&a, "h") / 2.0;
        let mut first = a.clone();
        first.insert("h".into(), nv(h));
        let mut second = a.clone();
        second.insert("y".into(), nv(f(&a, "y") + h));
        second.insert("h".into(), nv(h));
        b.splice(k..k + 1, [first, second]);
    }
    b.into_iter().map(Value::Object).collect()
}

/// The reliability test of `R.computeBoxes`: when the band count is too far from the line count, draw nothing.
pub fn bands_reliable(nbands: usize, nlines: usize) -> bool {
    let tol = 2f64.max(crate::jsx::round(nlines as f64 * 0.35));
    nbands > 0 && (nbands as f64 - nlines as f64).abs() <= tol
}

// ---- flat pieces for the egui renderer (spikes/s7-egui) ----

/// Private-use stand-in for `[illegible]` while pulldown-cmark runs.
const ILL_MARK: char = '\u{E010}';
/// Put at every line start so pulldown-cmark's block parser sees a plain paragraph: the JS only calls
/// `renderInline`, so `1. `, `# `, `- `, `> `, fences and indented code stay literal text there. U+1680 is
/// Unicode whitespace (flanking rules see a line start) and never occurs in handwriting transcripts.
const LINE_MARK: char = '\u{1680}';

fn inline_only(s: &str) -> String {
    if s.contains(LINE_MARK) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push(LINE_MARK);
        out.push_str(line);
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text { s: String, bold: bool, italic: bool, strike: bool, code: bool, illegible: bool },
    Math(usize),
}

fn swap_illegible(s: &str) -> String {
    const N: &str = "[illegible]";
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut i, mut last) = (0, 0);
    while i + N.len() <= b.len() {
        if b[i] == b'[' && b[i..i + N.len()].eq_ignore_ascii_case(N.as_bytes()) {
            out.push_str(&s[last..i]);
            out.push(ILL_MARK);
            i += N.len();
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

/// Inline markdown to flat pieces. Placeholders `⟦n⟧` become `Piece::Math(n)`; `[illegible]` its own piece.
pub fn pieces(held: &str, nmath: usize) -> Vec<Piece> {
    let (mut bold, mut italic, mut strike) = (0i32, 0i32, 0i32);
    let mut out: Vec<Piece> = vec![];
    fn push_text(s: &str, st: (i32, i32, i32), code: bool, nmath: usize, out: &mut Vec<Piece>) {
        let mk = |s: &str| Piece::Text {
            s: s.into(),
            bold: st.0 > 0,
            italic: st.1 > 0,
            strike: st.2 > 0,
            code,
            illegible: false,
        };
        let mut rest = s;
        while let Some(a) = rest.find('\u{27E6}') {
            let tail = &rest[a + '\u{27E6}'.len_utf8()..];
            let parsed = tail.find('\u{27E7}').and_then(|e| {
                let d = &tail[..e];
                (!d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| d.parse::<usize>().ok())
                    .flatten()
                    .filter(|n| *n < nmath)
                    .map(|n| (n, e))
            });
            match parsed {
                Some((n, e)) => {
                    if a > 0 {
                        out.push(mk(&rest[..a]));
                    }
                    out.push(Piece::Math(n));
                    rest = &tail[e + '\u{27E7}'.len_utf8()..];
                }
                None => {
                    let cut = a + '\u{27E6}'.len_utf8();
                    out.push(mk(&rest[..cut]));
                    rest = &rest[cut..];
                }
            }
        }
        if !rest.is_empty() {
            out.push(mk(rest));
        }
    }
    // text with the [illegible] stand-in split out into its own pieces
    fn text_run(t: &str, st: (i32, i32, i32), nmath: usize, out: &mut Vec<Piece>) {
        let cleaned = t.replace(LINE_MARK, "");
        let mut s = cleaned.as_str();
        while let Some(a) = s.find(ILL_MARK) {
            if a > 0 {
                push_text(&s[..a], st, false, nmath, out);
            }
            out.push(Piece::Text {
                s: "[illegible]".into(),
                bold: st.0 > 0,
                italic: st.1 > 0,
                strike: st.2 > 0,
                code: false,
                illegible: true,
            });
            s = &s[a + ILL_MARK.len_utf8()..];
        }
        push_text(s, st, false, nmath, out);
    }
    // markdown-it's validateLink: these schemes are never turned into links, the source stays text
    fn unsafe_link(dest: &str) -> bool {
        let d = dest.trim().to_ascii_lowercase();
        if d.starts_with("data:") {
            return !["data:image/gif", "data:image/png", "data:image/jpeg", "data:image/webp"].iter().any(|p| d.starts_with(p));
        }
        d.starts_with("javascript:") || d.starts_with("vbscript:") || d.starts_with("file:")
    }
    let swapped = inline_only(&swap_illegible(held));
    let mut skip_link = 0;
    for (ev, range) in Parser::new_ext(&swapped, Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
        let st = (bold, italic, strike);
        if skip_link > 0 {
            match ev {
                Event::Start(Tag::Link { .. }) => skip_link += 1,
                Event::End(TagEnd::Link) => skip_link -= 1,
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::Link { dest_url, .. }) if unsafe_link(&dest_url) => {
                text_run(&swapped[range], st, nmath, &mut out);
                skip_link = 1;
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold -= 1,
            Event::Start(Tag::Emphasis) => italic += 1,
            Event::End(TagEnd::Emphasis) => italic -= 1,
            Event::Start(Tag::Strikethrough) => strike += 1,
            Event::End(TagEnd::Strikethrough) => strike -= 1,
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => text_run(&t, st, nmath, &mut out),
            Event::Code(t) => push_text(&t.replace(LINE_MARK, ""), st, true, nmath, &mut out),
            Event::SoftBreak | Event::HardBreak => push_text(" ", st, false, nmath, &mut out),
            _ => {}
        }
    }
    out
}

/// Convenience: a unit as pieces plus its held math.
pub fn unit_pieces(src: &str) -> (Vec<Piece>, Vec<Math>) {
    let (held, math) = hold_math(src);
    (pieces(&held, math.len()), math)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_keep_multiline_display_math_together() {
        let u = units("a\n\n$$\nx\n$$\nb\r\n");
        assert_eq!(u, vec!["a", "$$\nx\n$$", "b"]);
    }

    #[test]
    fn hold_math_rejects_padded_inline() {
        let (t, m) = hold_math("$ a $ and $b$ and $$c$$");
        assert_eq!(t, "$ a $ and \u{27E6}1\u{27E7} and \u{27E6}0\u{27E7}");
        assert_eq!(m[0], Math { tex: "c".into(), display: true });
        assert_eq!(m[1], Math { tex: "b".into(), display: false });
    }

    #[test]
    fn pieces_split_math_and_illegible() {
        let (p, m) = unit_pieces("see $x$ and [Illegible] **b**");
        assert_eq!(m.len(), 1);
        assert!(p.contains(&Piece::Math(0)));
        assert!(p.iter().any(|x| matches!(x, Piece::Text { illegible: true, .. })));
        assert!(p.iter().any(|x| matches!(x, Piece::Text { bold: true, s, .. } if s == "b")));
    }

    #[test]
    fn basic_strike_and_escape() {
        assert_eq!(basic("a ~~b~~ <c>"), "a <del>b</del> &lt;c&gt;");
        assert_eq!(basic("~~~~"), "~~~~");
    }
}
