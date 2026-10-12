//! JavaScript semantics the JS engine relies on, reproduced by hand so ports stay byte-identical:
//! whitespace sets, `\w`/`\b`, UTF-16 lengths, `Number()` / `String()` coercion, `Math.round`,
//! `Number#toFixed(1)` ties, regex escaping and `String#localeCompare(numeric, base)` for ASCII-ish text.
use serde_json::{Number, Value};
use std::cmp::Ordering;
use unicode_normalization::UnicodeNormalization;

/// JS `\s` / `String#trim` set (WhiteSpace + LineTerminator). Differs from `char::is_whitespace`.
pub fn is_ws(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | '\u{20}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

pub fn trim(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// Length in UTF-16 code units (JS `.length`).
pub fn u16len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.split(/\s+/).filter(Boolean)`.
pub fn split_ws(s: &str) -> Vec<&str> {
    s.split(is_ws).filter(|t| !t.is_empty()).collect()
}

/// `s.replace(/\s+/g, ' ')`.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if is_ws(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            in_ws = false;
            out.push(c);
        }
    }
    out
}

/// Regex `\w` (no `u` flag): ASCII only.
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Regex `\b` at char index `i` of `chars`.
pub fn boundary(chars: &[char], i: usize) -> bool {
    let a = i > 0 && is_word(chars[i - 1]);
    let b = i < chars.len() && is_word(chars[i]);
    a != b
}

/// `s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')`.
pub fn esc_regex(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// `Math.round`.
pub fn round(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 {
        r + 1.0
    } else {
        r
    }
}

/// JS `Number(string)`.
pub fn string_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    for (p, r) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(d) = t.strip_prefix(p) {
            if d.is_empty() || !d.chars().all(|c| c.is_digit(r)) {
                return f64::NAN;
            }
            return d.chars().fold(0.0, |a, c| a * r as f64 + c.to_digit(r).unwrap() as f64);
        }
    }
    let (sign, body) = match t.as_bytes()[0] {
        b'+' => (1.0, &t[1..]),
        b'-' => (-1.0, &t[1..]),
        _ => (1.0, t),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    // StrDecimalLiteral: digits [. digits?] | . digits, then optional exponent.
    let b = body.as_bytes();
    let mut i = 0;
    let d1 = b.iter().take_while(|c| c.is_ascii_digit()).count();
    i += d1;
    let mut d2 = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        d2 = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += d2;
    }
    if d1 + d2 == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if e == 0 {
            return f64::NAN;
        }
        i += e;
    }
    if i != b.len() {
        return f64::NAN;
    }
    sign * body.parse::<f64>().unwrap_or(f64::NAN)
}

/// A JSON number as the f64 JS would hold (`1e999` is Infinity).
fn num_f64(n: &Number) -> f64 {
    n.as_f64().or_else(|| n.to_string().parse::<f64>().ok()).unwrap_or(f64::NAN)
}

/// JS `Number(value)` for a JSON value.
pub fn number(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => *b as u8 as f64,
        Value::Number(n) => num_f64(n),
        Value::String(s) => string_to_number(s),
        Value::Array(_) => string_to_number(&to_string(v)),
        Value::Object(_) => f64::NAN,
    }
}

/// JS `String(number)` (exponent forms for very large / small values).
pub fn num_str(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if f == 0.0 {
        return "0".into();
    }
    let a = f.abs();
    if !(1e-6..1e21).contains(&a) {
        let s = format!("{:e}", f);
        return match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{}e+{}", m, e),
            _ => s,
        };
    }
    format!("{}", f)
}

/// JS `String(value)` for a JSON value.
pub fn to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => num_str(num_f64(n)),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|x| if x.is_null() { String::new() } else { to_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JS truthiness of a JSON value (`None` is `undefined`).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map_or(true, |f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// A JS number as JSON: integral values print without a fraction, NaN/Infinity become `null`.
pub fn num_value(f: f64) -> Value {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 {
        Value::from(f as i64)
    } else {
        Number::from_f64(f).map_or(Value::Null, Value::Number)
    }
}

/// `x.toFixed(1)` for finite x. JS rounds exact ties up; Rust formatting rounds them to even.
pub fn to_fixed1(x: f64) -> String {
    if x < 0.0 {
        return format!("-{}", to_fixed1(-x));
    }
    let t = x * 4.0;
    if t.fract() == 0.0 && (t as u64) % 2 == 1 && t < 9.0e15 {
        // x = t/4 with t odd: exact tie between two tenths; take the larger.
        let n = (5 * (t as u64) + 1) / 2;
        return format!("{}.{}", n / 10, n % 10);
    }
    format!("{:.1}", x)
}

/// Compare like `a.localeCompare(b, undefined, {numeric: true, sensitivity: 'base'})` for Latin text:
/// space < punctuation < symbols < digits < letters, case and accents ignored, digit runs by value.
/// Non-Latin scripts fall back to code point order (documented deviation).
pub fn nat_compare(a: &str, b: &str) -> Ordering {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum K {
        Num(String),
        Ch(u8, u32),
    }
    const PUNCT: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    fn keys(s: &str) -> Vec<K> {
        use crate::natfold::{EXPAND, OWN};
        // Exception table first (Đ, ß, æ, ı, þ, circled digits, ...), then NFKD and mark stripping.
        enum Piece {
            C(char),
            Own(u8, u32),
        }
        let mut cs: Vec<Piece> = vec![];
        for c in s.chars() {
            if let Ok(i) = EXPAND.binary_search_by_key(&c, |e| e.0) {
                cs.extend(EXPAND[i].1.chars().map(Piece::C));
            } else if let Ok(i) = OWN.binary_search_by_key(&c, |e| e.0) {
                cs.push(Piece::Own(OWN[i].1, OWN[i].2));
            } else {
                cs.extend(
                    std::iter::once(c)
                        .nfkd()
                        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
                        .map(Piece::C),
                );
            }
        }
        let mut out = vec![];
        let mut i = 0;
        while i < cs.len() {
            let c = match cs[i] {
                Piece::Own(k, v) => {
                    out.push(K::Ch(k, v));
                    i += 1;
                    continue;
                }
                Piece::C(c) => c,
            };
            if c.is_ascii_digit() {
                let mut d = String::new();
                while let Some(Piece::C(x)) = cs.get(i) {
                    if !x.is_ascii_digit() {
                        break;
                    }
                    d.push(*x);
                    i += 1;
                }
                let d = d.trim_start_matches('0').to_string();
                // length first so that numeric order = (len, lexicographic)
                out.push(K::Num(format!("{:08}{}", d.len(), d)));
                continue;
            }
            i += 1;
            if c.is_control() {
                continue; // ignorable
            }
            let k = if c.is_whitespace() {
                K::Ch(0, 0)
            } else if let Some(p) = PUNCT.find(c) {
                K::Ch(1, p as u32)
            } else if c.is_alphabetic() {
                K::Ch(4, c.to_lowercase().next().unwrap_or(c) as u32 * 1000)
            } else {
                K::Ch(2, c as u32)
            };
            out.push(k);
        }
        out
    }
    let (ka, kb) = (keys(a), keys(b));
    for (x, y) in ka.iter().zip(kb.iter()) {
        let o = match (x, y) {
            (K::Num(p), K::Num(q)) => p.cmp(q),
            (K::Num(_), K::Ch(c, _)) => {
                if *c >= 3 {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (K::Ch(c, _), K::Num(_)) => {
                if *c >= 3 {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (K::Ch(c1, v1), K::Ch(c2, v2)) => (c1, v1).cmp(&(c2, v2)),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ka.len().cmp(&kb.len())
}

/// ASCII case-insensitive prefix test (regex `i` flag without `u` only folds ASCII).
pub fn starts_with_ci(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes()) && s.is_char_boundary(p.len())
}
