//! Lone-surrogate tolerant JSON (S3 rule 3, mirrors the spike's `lenient_in` / `lenient_out`).
//!
//! `JSON.stringify` writes a lone UTF-16 surrogate as a `\udXXX` escape. `serde_json` rejects those and a
//! Rust `String` cannot hold one, so a document containing one cannot be parsed directly. Before parsing,
//! each lone escape is replaced by `SENT` followed by its four hex digits; after serialising, `SENT`
//! directly followed by four hex digits of a surrogate value (D800..DFFF) is turned back into the
//! original `\udXXX` escape (hex digits copied verbatim, so case is kept).
//!
//! A literal U+10FFFE (raw, or the valid pair `\udbff\udffe`) stays one character in memory and is
//! written back as a literal, so text typed or pasted after load is never mangled. The one spelling that
//! cannot be told apart from a marker, a literal U+10FFFE directly followed by four surrogate hex digits
//! as text, is refused by `parse` (the document is never rewritten). Documents without lone surrogates
//! take the borrowed fast path and are untouched.
//!
//! In memory a lone surrogate is therefore `SENT` + 4 hex digits inside the string. `display` maps those
//! to U+FFFD for the UI; a string that never had one is returned unchanged.
use serde::{de::DeserializeOwned, Serialize};
use std::borrow::Cow;

/// Private-use noncharacter that marks an escaped lone surrogate.
pub const SENT: char = '\u{10FFFE}';

fn hex4(s: Option<&str>) -> Option<u32> {
    let s = s?;
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        u32::from_str_radix(s, 16).ok()
    } else {
        None
    }
}

/// The 4 ASCII hex digits at the start of `s` when their value is a surrogate (D800..DFFF).
fn surrogate_hex(s: &str) -> Option<&str> {
    let h = s.get(..4)?;
    hex4(Some(h)).filter(|c| (0xD800..0xE000).contains(c)).map(|_| h)
}

/// Replace lone `\udXXX` escapes so the text parses with serde_json. `Err(())` when the document holds a
/// literal U+10FFFE directly followed by surrogate-looking hex digits (unrepresentable, see module docs).
fn encode_checked(text: &str) -> Result<Cow<'_, str>, ()> {
    encode_impl(text, true)
}

fn encode_impl(text: &str, strict: bool) -> Result<Cow<'_, str>, ()> {
    let b = text.as_bytes();
    let mut out = String::new();
    let (mut i, mut last, mut touched) = (0usize, 0usize, false);
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'u' {
                if let Some(cp) = hex4(text.get(i + 2..i + 6)) {
                    let high = (0xD800..0xDC00).contains(&cp);
                    let low = (0xDC00..0xE000).contains(&cp);
                    if high && b.get(i + 6) == Some(&b'\\') && b.get(i + 7) == Some(&b'u') {
                        if let Some(lo) = hex4(text.get(i + 8..i + 12)).filter(|c| (0xDC00..0xE000).contains(c)) {
                            // a valid pair: serde_json handles it. The pair that decodes to the marker
                            // character must not be followed by marker-looking text.
                            if strict && cp == 0xDBFF && lo == 0xDFFE && text.get(i + 12..).is_some_and(|r| surrogate_hex(r).is_some()) {
                                return Err(());
                            }
                            i += 12;
                            continue;
                        }
                    }
                    if high || low {
                        touched = true;
                        out.push_str(&text[last..i]);
                        out.push(SENT);
                        out.push_str(&text[i + 2..i + 6]);
                        i += 6;
                        last = i;
                        continue;
                    }
                }
            }
            i += 2;
        } else if b[i] == 0xF4 && text[i..].starts_with(SENT) {
            i += SENT.len_utf8();
            if strict && surrogate_hex(&text[i..]).is_some() {
                return Err(());
            }
        } else {
            i += 1;
        }
    }
    if !touched {
        return Ok(Cow::Borrowed(text));
    }
    out.push_str(&text[last..]);
    Ok(Cow::Owned(out))
}

/// True when `text` is syntactically valid JSON as `JSON.parse` sees it, even if this crate cannot
/// represent it (U+10FFFE directly followed by surrogate-like hex digits). Lets callers tell "damaged"
/// from "valid but unrepresentable".
pub fn is_valid_json(text: &str) -> bool {
    match encode_impl(text, false) {
        Ok(enc) => serde_json::from_str::<serde::de::IgnoredAny>(&enc).is_ok(),
        Err(()) => false,
    }
}

/// Infallible form of the encoder (an unrepresentable document is passed through unchanged).
pub fn encode(text: &str) -> Cow<'_, str> {
    encode_checked(text).unwrap_or(Cow::Borrowed(text))
}

/// Inverse of [`encode`] for serialised output.
pub fn decode(s: String) -> String {
    if !s.contains(SENT) {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(pos) = rest.find(SENT) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + SENT.len_utf8()..];
        match surrogate_hex(after) {
            Some(h) => {
                out.push_str("\\u");
                out.push_str(h);
                rest = &after[4..];
            }
            None => {
                out.push(SENT);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// True when `s` carries an escaped lone surrogate (`SENT` + 4 surrogate hex digits).
pub fn has_lone(s: &str) -> bool {
    s.match_indices(SENT).any(|(i, _)| surrogate_hex(&s[i + SENT.len_utf8()..]).is_some())
}

/// Text for display: lone-surrogate markers become U+FFFD.
pub fn display(s: &str) -> Cow<'_, str> {
    if !has_lone(s) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(SENT) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + SENT.len_utf8()..];
        if surrogate_hex(after).is_some() {
            out.push('\u{FFFD}');
            rest = &after[4..];
        } else {
            out.push(SENT);
            rest = after;
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// `serde_json::from_str` that tolerates lone-surrogate escapes.
pub fn parse<T: DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    use serde::de::Error;
    let enc = encode_checked(text).map_err(|_| {
        serde_json::Error::custom("document holds U+10FFFE followed by surrogate-like hex digits (unrepresentable)")
    })?;
    serde_json::from_str(&enc)
}

/// `serde_json::to_string(_pretty)` that restores lone-surrogate escapes.
pub fn to_string<T: Serialize + ?Sized>(v: &T, pretty: bool) -> Result<String, serde_json::Error> {
    let s = if pretty { serde_json::to_string_pretty(v)? } else { serde_json::to_string(v)? };
    Ok(decode(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn valid_json_even_when_unrepresentable() {
        assert!(is_valid_json("{\"a\":\"\u{10FFFE}d800\"}"));
        assert!(is_valid_json("{\"a\":\"\\udbff\\udffed800\"}"));
        assert!(is_valid_json("{\"a\":\"\\ud800\"}"));
        assert!(!is_valid_json("{\"a\":\"\u{10FFFE}d800\""));
        assert!(!is_valid_json("{\"a\":"));
    }

    fn rt(src: &str, pretty: bool) -> String {
        let v: Value = parse(src).unwrap();
        to_string(&v, pretty).unwrap()
    }

    #[test]
    fn lone_surrogates_round_trip_byte_identical() {
        for src in [
            r#"{"raw":"a\ud800b"}"#,
            r#"{"raw":"x\udfffy","k\udc00":1}"#,
            "{\"a\":\"\\ud800\u{1F600} \\udfff tail\"}",
            r#"{"a":"\udc00\ud800"}"#,
            r#"{"a":"\\ud800 is text"}"#,
            "{\"a\":\"\u{10FFFE}abcd and \u{10FFFE}\u{10FFFE}\"}",
            "{\"a\":\"\u{10FFFE}\\ud800\u{10FFFE}\"}",
            r#"{"a":"\uD800"}"#,
        ] {
            assert_eq!(rt(src, false), src, "{src}");
        }
        let pretty = "{\n  \"raw\": \"\\ud800\"\n}";
        assert_eq!(rt(pretty, true), pretty);
    }

    #[test]
    fn marker_lookalike_documents_are_refused_not_corrupted() {
        assert!(parse::<Value>("{\"a\":\"\u{10FFFE}d800\"}").is_err());
        assert!(parse::<Value>("{\"a\":\"\\udbff\\udffeDC00\"}").is_err());
        assert!(parse::<Value>("{\"a\":\"\\udbff\\udffe xyz\"}").is_ok());
    }

    #[test]
    fn plain_documents_take_the_borrowed_path() {
        assert!(matches!(encode(r#"{"a":"caf\u00e9 \\u0041"}"#), Cow::Borrowed(_)));
    }

    #[test]
    fn valid_pair_is_a_real_char_and_lone_is_marked() {
        let v: Value = parse(r#"{"a":"\ud83d\ude00","b":"\ud800"}"#).unwrap();
        assert_eq!(v["a"], "😀");
        assert!(has_lone(v["b"].as_str().unwrap()));
        assert!(!has_lone(v["a"].as_str().unwrap()));
        assert_eq!(display(v["b"].as_str().unwrap()), "\u{FFFD}");
    }
}
