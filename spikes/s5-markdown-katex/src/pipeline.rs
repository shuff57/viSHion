//! Pure (no web types) port of the string plumbing in js/render.js: math hold/restore, unit splitting,
//! `[illegible]` marking. Everything that touches markdown-it / KaTeX is injected by the caller.
//! Byte-for-byte compat is the goal, so JS whitespace and regex semantics are reproduced by hand.

#[derive(Debug, Clone, PartialEq)]
pub struct Math { pub tex: String, pub display: bool }

/// JS `\s` / `trim()` set (WhiteSpace + LineTerminator). Differs from `char::is_whitespace` (U+0085, U+FEFF).
pub fn is_js_ws(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | '\u{20}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
pub fn js_trim(s: &str) -> &str { s.trim_matches(is_js_ws) }

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c { '&' => o.push_str("&amp;"), '<' => o.push_str("&lt;"), '>' => o.push_str("&gt;"), '"' => o.push_str("&quot;"), '\'' => o.push_str("&#39;"), _ => o.push(c) }
    }
    o
}

fn hold(math: &mut Vec<Math>, tex: &str, display: bool) -> String {
    math.push(Math { tex: tex.to_string(), display });
    format!("\u{27E6}{}\u{27E7}", math.len() - 1)
}

/// `src.replace(/\$\$([\s\S]+?)\$\$/g, hold(a.trim(), true))` then
/// `.replace(/\$([^$\n]+?)\$/g, (m,a) => /^\s|\s$/.test(a) ? m : hold(a,false))`
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
                    let ph = hold(&mut math, js_trim(&src[after..j]), true);
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
            if let Some(p) = rest.find(|c| c == '$' || c == '\n') {
                if p > 0 && rest.as_bytes()[p] == b'$' {
                    let a = &rest[..p];
                    let padded = a.chars().next().map_or(false, is_js_ws) || a.chars().next_back().map_or(false, is_js_ws);
                    if padded { out.push_str(&t[i..i + 1 + p + 1]); } else { let ph = hold(&mut math, a, false); out.push_str(&ph); }
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

/// `html.replace(/\[illegible\]/gi, '<span class="illegible">[illegible]</span>')`
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
        } else { i += 1; }
    }
    out.push_str(&html[last..]);
    out
}

/// `html.replace(/⟦(\d+)⟧/g, (m,i) => math[+i] ? mathHtml(math[+i]) : '')`
pub fn restore_math(html: &str, math: &[Math], render: &mut dyn FnMut(&Math) -> String) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(p) = rest.find('\u{27E6}') {
        out.push_str(&rest[..p]);
        let after = &rest[p + '\u{27E6}'.len_utf8()..];
        let nd = after.bytes().take_while(|b| b.is_ascii_digit()).count();
        if nd > 0 && after[nd..].starts_with('\u{27E7}') {
            if let Some(m) = after[..nd].parse::<usize>().ok().and_then(|ix| math.get(ix)) { out.push_str(&render(m)); }
            rest = &after[nd + '\u{27E7}'.len_utf8()..];
        } else {
            out.push('\u{27E6}');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `R.units`: display units, a multi-line `$$` block stays one unit.
pub fn units(text: &str) -> Vec<String> {
    let cleaned = text.replace('\r', "");
    let lines: Vec<&str> = cleaned.split('\n').collect();
    let ends_dd = |s: &str| s.trim_end_matches(is_js_ws).ends_with("$$");
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = js_trim(l);
        if t.is_empty() { i += 1; continue; }
        if t.starts_with("$$") && !ends_dd(&t[2..]) {
            let mut buf = vec![l];
            i += 1;
            while i < lines.len() {
                buf.push(lines[i]);
                if ends_dd(lines[i]) { break; }
                i += 1;
            }
            out.push(buf.join("\n"));
        } else { out.push(l.to_string()); }
        i += 1;
    }
    out
}

/// `R.unit` with the inline renderer and math renderer injected.
pub fn unit<E>(src: &str, inline: &mut dyn FnMut(&str) -> Result<String, E>, math: &mut dyn FnMut(&Math) -> String) -> Result<String, E> {
    let (t, held) = hold_math(src);
    let html = mark_illegible(&inline(&t)?);
    Ok(restore_math(&html, &held, math))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn hold() {
        let (t, m) = hold_math("a $x$ and $$ y $$ b $ z $ c $$");
        assert_eq!(t, "a \u{27E6}1\u{27E7} and \u{27E6}0\u{27E7} b $ z $ c $$");
        assert_eq!(m[0], Math { tex: "y".into(), display: true });
    }
    #[test] fn units_multiline() {
        assert_eq!(units("a\r\n\r\n$$\nx\ny$$\nb"), vec!["a", "$$\nx\ny$$", "b"]);
    }
}
