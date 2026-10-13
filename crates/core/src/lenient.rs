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
    encode_impl(text, true, SENT)
}

/// Marker candidates in preference order: the plain default first, so a document that never collides
/// keeps the historical marker.
fn candidates() -> impl Iterator<Item = char> {
    let astral = (1u32..=0x10).rev().flat_map(|p| [(p << 16) | 0xFFFE, (p << 16) | 0xFFFF]);
    astral.chain([0xFFFE, 0xFFFF]).chain(0xFDD0..=0xFDEF).filter_map(char::from_u32)
}

/// The per-document marker: the first candidate that occurs nowhere in `text`, neither literally nor as a
/// `\uXXXX` escape. Falls back to [`SENT`], whose collisions [`parse`] then refuses.
pub fn pick_marker(text: &str) -> char {
    let b = text.as_bytes();
    // every escape value >= D800 (surrogates and the BMP noncharacters)
    let mut esc: Vec<u32> = vec![];
    let mut i = 0;
    while i + 6 <= b.len() {
        if b[i] == b'\\' {
            if b[i + 1] == b'u' {
                if let Some(v) = hex4(text.get(i + 2..i + 6)).filter(|v| *v >= 0xD800) {
                    esc.push(v);
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    candidates()
        .find(|&c| {
            if text.contains(c) {
                return false;
            }
            let u = c as u32;
            if u > 0xFFFF {
                let hi = 0xD800 + ((u - 0x10000) >> 10);
                let lo = 0xDC00 + ((u - 0x10000) & 0x3FF);
                !(esc.contains(&hi) && esc.contains(&lo))
            } else {
                !esc.contains(&u)
            }
        })
        .unwrap_or(SENT)
}

fn encode_impl(text: &str, strict: bool, marker: char) -> Result<Cow<'_, str>, ()> {
    let mut mbuf = [0u8; 4];
    let mfirst = marker.encode_utf8(&mut mbuf).as_bytes()[0];
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
                            if strict && 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00) == marker as u32 && text.get(i + 12..).is_some_and(|r| surrogate_hex(r).is_some()) {
                                return Err(());
                            }
                            i += 12;
                            continue;
                        }
                    }
                    if high || low {
                        touched = true;
                        out.push_str(&text[last..i]);
                        out.push(marker);
                        out.push_str(&text[i + 2..i + 6]);
                        i += 6;
                        last = i;
                        continue;
                    }
                }
            }
            i += 2;
        } else if b[i] == mfirst && text[i..].starts_with(marker) {
            i += marker.len_utf8();
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
    match encode_impl(text, false, SENT) {
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
    decode_with(s, SENT)
}

/// [`decode`] for a document whose marker is `sent` (see [`pick_marker`]).
pub fn decode_with(s: String, sent: char) -> String {
    if !s.contains(sent) {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(pos) = rest.find(sent) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + sent.len_utf8()..];
        match surrogate_hex(after) {
            Some(h) => {
                out.push_str("\\u");
                out.push_str(h);
                rest = &after[4..];
            }
            None => {
                out.push(sent);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// True when `s` carries an escaped lone surrogate (`SENT` + 4 surrogate hex digits).
pub fn has_lone(s: &str) -> bool {
    has_lone_with(s, SENT)
}

pub fn has_lone_with(s: &str, marker: char) -> bool {
    s.match_indices(marker).any(|(i, _)| surrogate_hex(&s[i + marker.len_utf8()..]).is_some())
}

/// Text for display: lone-surrogate markers become U+FFFD.
pub fn display(s: &str) -> Cow<'_, str> {
    display_with(s, SENT)
}

pub fn display_with(s: &str, marker: char) -> Cow<'_, str> {
    if !has_lone_with(s, marker) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(marker) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + marker.len_utf8()..];
        if surrogate_hex(after).is_some() {
            out.push('\u{FFFD}');
            rest = &after[4..];
        } else {
            out.push(marker);
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

/// [`parse`] with a per-document marker: the returned char is the marker chosen for this text (a char
/// absent from it), so a document holding U+10FFFE followed by hex digits still opens. Pass it to
/// [`to_string_with`] when writing the document back.
pub fn parse_marked<T: DeserializeOwned>(text: &str) -> Result<(T, char), serde_json::Error> {
    use serde::de::Error;
    let marker = pick_marker(text);
    let enc = encode_impl(text, true, marker).map_err(|_| {
        serde_json::Error::custom("document uses every lone-surrogate marker character (unrepresentable)")
    })?;
    Ok((serde_json::from_str(&enc)?, marker))
}

/// [`to_string`] for a document parsed with [`parse_marked`].
pub fn to_string_with<T: Serialize + ?Sized>(v: &T, pretty: bool, marker: char) -> Result<String, serde_json::Error> {
    let s = if pretty { serde_json::to_string_pretty(v)? } else { serde_json::to_string(v)? };
    Ok(decode_with(s, marker))
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
    fn marked_parse_opens_lookalikes_and_round_trips() {
        for src in [
            "{\"a\":\"x\u{10FFFE}dc00y\"}",
            "{\"a\":\"\u{10FFFE}d800 \\ud800 \u{10FFFF}dc00\"}",
        ] {
            let (v, m): (Value, char) = parse_marked(src).unwrap();
            assert_ne!(m, SENT, "{src}");
            assert_eq!(to_string_with(&v, false, m).unwrap(), src);
        }
        // an escaped pair spelling the default marker also moves the marker
        let (v, m): (Value, char) = parse_marked("{\"a\":\"\\udbff\\udffeDC00 \\udfff\"}").unwrap();
        assert_ne!(m, SENT);
        assert_eq!(to_string_with(&v, false, m).unwrap(), "{\"a\":\"\u{10FFFE}DC00 \\udfff\"}");
        let (_, m): (Value, char) = parse_marked("{\"a\":\"plain \\ud800\"}").unwrap();
        assert_eq!(m, SENT);
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
