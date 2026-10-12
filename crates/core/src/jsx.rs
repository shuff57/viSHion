//! JavaScript semantics the JS engine relies on, reproduced by hand so ports stay byte-identical:
//! whitespace sets, `\w`/`\b`, UTF-16 lengths, `Number()` / `String()` coercion, `Math.round`,
//! `Number#toFixed(1)` ties, regex escaping and `String#localeCompare(numeric, base)` (ICU4X root collation).
use serde_json::{Number, Value};
use std::cmp::Ordering;

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

/// `a.localeCompare(b, undefined, {numeric: true, sensitivity: 'base'})`: CLDR root collation at primary
/// strength with numeric ordering, via ICU4X (the same algorithm and data family V8 uses).
pub fn nat_compare(a: &str, b: &str) -> Ordering {
    let (fa, fb) = (crate::hanfold::fold(a), crate::hanfold::fold(b));
    let (fa, fb) = icu_equal_prefix_skip(a, b, &fa, &fb);
    nat_compare_raw(fa, fb)
}

/// Lead-surrogate blocks (`(cp - 0x10000) >> 10`) that ICU treats as "unsafe backward" (the rest are not).
fn lead_unsafe(block: u32) -> bool {
    matches!(block, 0..=8 | 0xb | 0xc | 0x11 | 0x18 | 0x1a | 0x1b | 0x20 | 0x23 | 0x2c | 0x2f | 0x34 | 0x38..=0x3a)
}

/// BMP code points ICU treats as unsafe backward (contraction/digit/mark context), probed from node's ICU.
const UNSAFE_BMP: &[(u32, u32)] = &[
    (0x4c, 0x4c), (0x300, 0x34e), (0x350, 0x36f), (0x3a9, 0x3a9), (0x3e2, 0x3e2), (0x42f, 0x42f),
    (0x483, 0x487), (0x531, 0x531), (0x591, 0x5bd), (0x5bf, 0x5bf), (0x5c1, 0x5c2), (0x5c4, 0x5c5),
    (0x5c7, 0x5c7), (0x5d0, 0x5d0), (0x610, 0x61a), (0x628, 0x628), (0x64b, 0x65f), (0x670, 0x670),
    (0x6d6, 0x6dc), (0x6df, 0x6e4), (0x6e7, 0x6e8), (0x6ea, 0x6ed), (0x710, 0x711), (0x730, 0x74a),
    (0x78c, 0x78c), (0x7ca, 0x7ca), (0x7eb, 0x7f3), (0x7fd, 0x7fd), (0x800, 0x800), (0x816, 0x819),
    (0x81b, 0x823), (0x825, 0x827), (0x829, 0x82d), (0x840, 0x840), (0x859, 0x85b), (0x897, 0x89f),
    (0x8ca, 0x8e1), (0x8e3, 0x8ff), (0x905, 0x905), (0x93c, 0x93c), (0x94d, 0x94d), (0x951, 0x954),
    (0x995, 0x995), (0x9bc, 0x9bc), (0x9be, 0x9be), (0x9cd, 0x9cd), (0x9d7, 0x9d7), (0x9fe, 0x9fe),
    (0xa15, 0xa15), (0xa3c, 0xa3c), (0xa4d, 0xa4d), (0xa95, 0xa95), (0xabc, 0xabc), (0xacd, 0xacd),
    (0xb15, 0xb15), (0xb3c, 0xb3c), (0xb3e, 0xb3e), (0xb4d, 0xb4d), (0xb56, 0xb57), (0xb95, 0xb95),
    (0xbbe, 0xbbe), (0xbcd, 0xbcd), (0xbd7, 0xbd7), (0xc15, 0xc15), (0xc3c, 0xc3c), (0xc4d, 0xc4d),
    (0xc55, 0xc56), (0xc95, 0xc95), (0xcbc, 0xcbc), (0xcc2, 0xcc2), (0xccd, 0xccd), (0xcd5, 0xcd6),
    (0xd15, 0xd15), (0xd3b, 0xd3c), (0xd3e, 0xd3e), (0xd4d, 0xd4d), (0xd57, 0xd57), (0xd85, 0xd85),
    (0xdca, 0xdca), (0xdcf, 0xdcf), (0xddf, 0xddf), (0xe01, 0xe2e), (0xe32, 0xe32), (0xe38, 0xe3a),
    (0xe48, 0xe4b), (0xe81, 0xe82), (0xe84, 0xe84), (0xe86, 0xe8a), (0xe8c, 0xea3), (0xea5, 0xea5),
    (0xea7, 0xeae), (0xeb2, 0xeb2), (0xeb8, 0xeba), (0xec8, 0xecb), (0xedc, 0xedf), (0xf18, 0xf19),
    (0xf35, 0xf35), (0xf37, 0xf37), (0xf39, 0xf39), (0xf40, 0xf40), (0xf71, 0xf75), (0xf7a, 0xf7d),
    (0xf80, 0xf84), (0xf86, 0xf87), (0xfc6, 0xfc6), (0x1000, 0x1000), (0x102e, 0x102e),
    (0x1037, 0x1037), (0x1039, 0x103a), (0x108d, 0x108d), (0x10d3, 0x10d3), (0x12a0, 0x12a0),
    (0x135d, 0x135f), (0x13c4, 0x13c4), (0x14c0, 0x14c0), (0x168f, 0x168f), (0x16a0, 0x16a0),
    (0x1703, 0x1703), (0x1714, 0x1715), (0x1723, 0x1723), (0x1734, 0x1734), (0x1743, 0x1743),
    (0x1763, 0x1763), (0x1780, 0x1780), (0x17d2, 0x17d2), (0x17dd, 0x17dd), (0x1826, 0x1826),
    (0x18a9, 0x18a9), (0x1900, 0x1900), (0x1939, 0x193b), (0x1950, 0x1950), (0x1980, 0x19ab),
    (0x1a00, 0x1a00), (0x1a17, 0x1a18), (0x1a20, 0x1a20), (0x1a60, 0x1a60), (0x1a75, 0x1a7c),
    (0x1a7f, 0x1a7f), (0x1ab0, 0x1abd), (0x1abf, 0x1add), (0x1ae0, 0x1aeb), (0x1b05, 0x1b05),
    (0x1b34, 0x1b35), (0x1b44, 0x1b44), (0x1b6b, 0x1b73), (0x1b83, 0x1b83), (0x1baa, 0x1bab),
    (0x1bc0, 0x1bc0), (0x1be6, 0x1be6), (0x1bf2, 0x1bf3), (0x1c00, 0x1c00), (0x1c37, 0x1c37),
    (0x1c5a, 0x1c5a), (0x1cd0, 0x1cd2), (0x1cd4, 0x1ce0), (0x1ce2, 0x1ce8), (0x1ced, 0x1ced),
    (0x1cf4, 0x1cf4), (0x1cf8, 0x1cf9), (0x1dc0, 0x1dff), (0x20d0, 0x20dc), (0x20e1, 0x20e1),
    (0x20e5, 0x20f0), (0x2c00, 0x2c00), (0x2cef, 0x2cf1), (0x2d30, 0x2d30), (0x2d7f, 0x2d7f),
    (0x2de0, 0x2dff), (0x302a, 0x302f), (0x304b, 0x304b), (0x3099, 0x309a), (0x30ab, 0x30ab),
    (0x3105, 0x3105), (0x5b57, 0x5b57), (0xa288, 0xa288), (0xa4d0, 0xa4d0), (0xa549, 0xa549),
    (0xa66f, 0xa66f), (0xa674, 0xa67d), (0xa69e, 0xa6a0), (0xa6f0, 0xa6f1), (0xa800, 0xa800),
    (0xa806, 0xa806), (0xa82c, 0xa82c), (0xa840, 0xa840), (0xa882, 0xa882), (0xa8c4, 0xa8c4),
    (0xa8e0, 0xa8f1), (0xa90a, 0xa90a), (0xa92b, 0xa92d), (0xa930, 0xa930), (0xa953, 0xa953),
    (0xa984, 0xa984), (0xa9b3, 0xa9b3), (0xa9c0, 0xa9c0), (0xaa00, 0xaa00), (0xaa80, 0xaab0),
    (0xaab2, 0xaab4), (0xaab7, 0xaab8), (0xaabe, 0xaabf), (0xaac1, 0xaac1), (0xaaf6, 0xaaf6),
    (0xabc0, 0xabc0), (0xabed, 0xabed), (0xac00, 0xac00), (0xfb1e, 0xfb1e), (0xfdd0, 0xfdd0),
    (0xfe20, 0xfe2f),
];

/// BMP decimal digits above U+065F (ICU's numeric mode makes digits "unsafe backward"; ASCII handled inline).
const BMP_ND: &[(u32, u32)] = &[
    (0x660, 0x669), (0x6f0, 0x6f9), (0x7c0, 0x7c9), (0x966, 0x96f), (0x9e6, 0x9ef), (0xa66, 0xa6f), (0xae6, 0xaef),
    (0xb66, 0xb6f), (0xbe6, 0xbef), (0xc66, 0xc6f), (0xce6, 0xcef), (0xd66, 0xd6f), (0xde6, 0xdef), (0xe50, 0xe59),
    (0xed0, 0xed9), (0xf20, 0xf29), (0x1040, 0x1049), (0x1090, 0x1099), (0x17e0, 0x17e9), (0x1810, 0x1819),
    (0x1946, 0x194f), (0x19d0, 0x19d9), (0x1a80, 0x1a89), (0x1a90, 0x1a99), (0x1b50, 0x1b59), (0x1bb0, 0x1bb9),
    (0x1c40, 0x1c49), (0x1c50, 0x1c59), (0xa620, 0xa629), (0xa8d0, 0xa8d9), (0xa900, 0xa909), (0xa9d0, 0xa9d9),
    (0xa9f0, 0xa9f9), (0xaa50, 0xaa59), (0xabf0, 0xabf9), (0xff10, 0xff19),
];

fn in_ranges(t: &[(u32, u32)], u: u32) -> bool {
    t.binary_search_by(|&(lo, hi)| if u < lo { Ordering::Greater } else if u > hi { Ordering::Less } else { Ordering::Equal }).is_ok()
}

/// ICU `CollationData::isUnsafeBackward(unit, numeric = true)` for one UTF-16 code unit: trail surrogates, lead
/// surrogates of unsafe blocks, contraction/mark characters, and (numeric mode) BMP decimal digits.
fn unit_unsafe(u: u16) -> bool {
    let u = u as u32;
    match u {
        0xd800..=0xdbff => lead_unsafe(u - 0xd800),
        0xdc00..=0xdfff => true,
        0x30..=0x39 => true,
        _ => in_ranges(UNSAFE_BMP, u) || in_ranges(BMP_ND, u),
    }
}

/// ICU `RuleBasedCollator::doCompare` skips the identical UTF-16 prefix and only backs up to the start of a
/// contraction/digit run when the next code unit on either side is "unsafe backward". A lead surrogate of a
/// "safe" block (U+1CCxx, U+1D7xx, U+1FBxx Nd) is not unsafe, so a digit run before it is cut and the tails are
/// compared alone. ICU4X always merges, so strip exactly the prefix ICU strips (the back-up loop only inspects
/// code units of the common prefix). `fa`/`fb` are the char-for-char Han-folded forms of `a`/`b`.
fn icu_equal_prefix_skip<'a>(a: &str, b: &str, fa: &'a str, fb: &'a str) -> (&'a str, &'a str) {
    let ua: Vec<u16> = a.encode_utf16().collect();
    let ub: Vec<u16> = b.encode_utf16().collect();
    let mut eq = ua.iter().zip(ub.iter()).take_while(|(x, y)| x == y).count();
    if eq == 0 {
        return (fa, fb);
    }
    if (eq != ua.len() && unit_unsafe(ua[eq])) || (eq != ub.len() && unit_unsafe(ub[eq])) {
        loop {
            eq -= 1;
            if !(eq > 0 && unit_unsafe(ua[eq])) {
                break;
            }
        }
    }
    if eq == 0 {
        return (fa, fb);
    }
    // eq is on a code point boundary here; convert the UTF-16 offset to a char count, then to a byte offset.
    let (mut units, mut chars) = (0usize, 0usize);
    for c in a.chars() {
        if units >= eq {
            break;
        }
        units += c.len_utf16();
        chars += 1;
    }
    let off = |s: &'a str| s.char_indices().nth(chars).map_or(s.len(), |(i, _)| i);
    (&fa[off(fa)..], &fb[off(fb)..])
}

/// ICU4X root collation without the Han reordering of `hanfold` (used by the table generator).
#[doc(hidden)]
pub fn nat_compare_raw(a: &str, b: &str) -> Ordering {
    use icu_collator::options::{CollatorOptions, Strength};
    use icu_collator::preferences::CollationNumericOrdering;
    use icu_collator::{Collator, CollatorPreferences};
    thread_local! {
        static C: Collator = {
            let mut prefs = CollatorPreferences::default();
            prefs.numeric_ordering = Some(CollationNumericOrdering::True);
            let mut o = CollatorOptions::default();
            o.strength = Some(Strength::Primary);
            Collator::try_new(prefs, o).expect("root collation data is compiled in").static_to_owned()
        };
    }
    C.with(|c| c.as_borrowed().compare(a, b))
}

/// ASCII case-insensitive prefix test (regex `i` flag without `u` only folds ASCII).
pub fn starts_with_ci(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes()) && s.is_char_boundary(p.len())
}
