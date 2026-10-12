//! Pure parts of js/guess.js: numbering the unreadable words for the prompt and cleaning the model's
//! answer into `page.guesses = { "i:<n>": [ { text, percent } ] }`.
use crate::jsx::{self, collapse_ws, trim, u16len};
use crate::words::math_spans;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

const ILL: &str = "[illegible]";

/// `numbered(raw)`: the transcript with each unreadable word numbered (`[illegible #k]`, outside math),
/// in the same order words.rs counts them, and how many there were.
pub fn numbered(raw: &str) -> (String, usize) {
    let mut n = 0usize;
    let mut num = |seg: &str, out: &mut String| {
        let b = seg.as_bytes();
        let (mut i, mut last) = (0, 0);
        while i + 11 <= b.len() {
            if b[i] == b'[' && b[i..i + 11].eq_ignore_ascii_case(ILL.as_bytes()) {
                out.push_str(&seg[last..i]);
                n += 1;
                out.push_str(&format!("[illegible #{}]", n));
                i += 11;
                last = i;
            } else {
                i += 1;
            }
        }
        out.push_str(&seg[last..]);
    };
    let mut out = String::new();
    let mut last = 0;
    for (a, b) in math_spans(raw) {
        num(&raw[last..a], &mut out);
        out.push_str(&raw[a..b]);
        last = b;
    }
    num(&raw[last..], &mut out);
    (out, n)
}

/// `firstArray`: first JSON array in a reply that may also contain notes or code fences.
pub fn first_array(t: &str) -> Option<Value> {
    let s = t.find('[')?;
    let mut depth = 0i32;
    for (i, c) in t[s..].char_indices() {
        if c == '[' {
            depth += 1;
        } else if c == ']' {
            depth -= 1;
            if depth == 0 {
                return serde_json::from_str(&t[s..s + i + 1]).ok();
            }
        }
    }
    None
}

/// `String(x)` where `x == null` gives `''` (the JS does `String((o && o.text) == null ? '' : o.text)`).
fn text_of(o: &Value) -> String {
    // (o && o.text): a falsy `o` yields `o` itself
    let t: Option<&Value> = if jsx::truthy(Some(o)) { o.get("text") } else { Some(o) };
    match t {
        None | Some(Value::Null) => String::new(),
        Some(v) => jsx::to_string(v),
    }
}

fn percent_of(o: &Value) -> f64 {
    let raw = if jsx::truthy(Some(o)) {
        let p = o.get("percent");
        match p {
            Some(v) if !v.is_null() => v.clone(),
            _ => o.get("certainty").cloned().unwrap_or(Value::Null),
        }
    } else {
        o.clone()
    };
    let n = jsx::number(&raw);
    let n = if n == 0.0 || n.is_nan() { 0.0 } else { n };
    jsx::round(n).clamp(0.0, 100.0)
}

/// `W.parseGuesses(reply, count)`: ranked, capped at three, bounded, de-duplicated.
pub fn parse_guesses(reply: &str, count: usize) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(Value::Array(arr)) = first_array(reply) else { return out };
    for g in &arr {
        let n = if jsx::truthy(Some(g)) { jsx::number(g.get("n").unwrap_or(&Value::Null)) } else { jsx::number(g) };
        let n = if g.is_object() && g.get("n").is_none() { f64::NAN } else { n };
        let options = g.get("options").and_then(Value::as_array);
        if !(n >= 1.0 && n <= count as f64) || options.is_none() {
            continue;
        }
        let mut seen = HashSet::new();
        let mut opts: Vec<(String, f64)> = options
            .unwrap()
            .iter()
            .map(|o| (trim(&collapse_ws(&text_of(o))).to_string(), percent_of(o)))
            .filter(|(t, p)| {
                if t.is_empty() || u16len(t) > 40 || *p <= 0.0 {
                    return false;
                }
                let low = t.to_lowercase();
                if low.starts_with("illegible") || low.starts_with("[illegible") {
                    return false;
                }
                seen.insert(low)
            })
            .collect();
        opts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        opts.truncate(3);
        let sum: f64 = opts.iter().map(|o| o.1).sum();
        if sum > 100.0 {
            for o in opts.iter_mut() {
                o.1 = jsx::round(o.1 * 100.0 / sum).max(1.0);
            }
        }
        if !opts.is_empty() {
            let arr: Vec<Value> = opts.iter().map(|(t, p)| json!({"text": t, "percent": jsx::num_value(*p)})).collect();
            // n is an integer-valued index only when Number() gave one; otherwise the key is 'i:' + (n - 1)
            out.insert(format!("i:{}", jsx::num_str(n - 1.0)), Value::Array(arr));
        }
    }
    out
}
