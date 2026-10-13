//! Lone-surrogate tolerant JSON (S3 rule 3, mirrors the spike's `lenient_in` / `lenient_out`).
//!
//! `JSON.stringify` writes a lone UTF-16 surrogate as a `\udXXX` escape. `serde_json` rejects those and a
//! Rust `String` cannot hold one, so a document containing one cannot be parsed directly. Before parsing,
//! each lone escape is replaced by `SENT` followed by its four hex digits; after serialising the sentinel
//! sequence is turned back into the original `\udXXX` escape (hex digits copied verbatim, so case is
//! kept). A raw U+10FFFE character in the input is doubled so the mapping is unambiguous. Documents
//! without lone surrogates and without U+10FFFE take the borrowed fast path and are untouched.
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

/// Replace lone `\udXXX` escapes (and double raw `SENT`) so the text parses with serde_json.
pub fn encode(text: &str) -> Cow<'_, str> {
    let b = text.as_bytes();
    let mut out = String::new();
    let (mut i, mut last, mut touched) = (0usize, 0usize, false);
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'u' {
                if let Some(cp) = hex4(text.get(i + 2..i + 6)) {
                    let high = (0xD800..0xDC00).contains(&cp);
                    let low = (0xDC00..0xE000).contains(&cp);
                    if high
                        && b.get(i + 6) == Some(&b'\\')
                        && b.get(i + 7) == Some(&b'u')
                        && hex4(text.get(i + 8..i + 12)).is_some_and(|c| (0xDC00..0xE000).contains(&c))
                    {
                        i += 12; // a valid pair: serde_json handles it
                        continue;
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
            // raw U+10FFFE (F4 8F BF BE): double it
            touched = true;
            out.push_str(&text[last..i]);
            out.push(SENT);
            out.push(SENT);
            i += SENT.len_utf8();
            last = i;
        } else {
            i += 1;
        }
    }
    if !touched {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[last..]);
    Cow::Owned(out)
}

/// Inverse of [`encode`] for serialised output.
pub fn decode(s: String) -> String {
    if !s.contains(SENT) {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != SENT {
            out.push(c);
        } else if it.peek() == Some(&SENT) {
            it.next();
            out.push(SENT);
        } else {
            out.push_str("\\u");
            for _ in 0..4 {
                if let Some(h) = it.next() {
                    out.push(h);
                }
            }
        }
    }
    out
}

/// True when `s` carries an escaped lone surrogate (`SENT` + hex, not a doubled `SENT`).
pub fn has_lone(s: &str) -> bool {
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == SENT {
            if it.peek() == Some(&SENT) {
                it.next();
            } else {
                return true;
            }
        }
    }
    false
}

/// Text for display: lone-surrogate markers become U+FFFD, doubled `SENT` becomes one `SENT`.
pub fn display(s: &str) -> Cow<'_, str> {
    if !s.contains(SENT) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != SENT {
            out.push(c);
        } else if it.peek() == Some(&SENT) {
            it.next();
            out.push(SENT);
        } else {
            for _ in 0..4 {
                it.next();
            }
            out.push('\u{FFFD}');
        }
    }
    Cow::Owned(out)
}

/// `serde_json::from_str` that tolerates lone-surrogate escapes.
pub fn parse<T: DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(&encode(text))
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
            r#"{"a":"\uD800"}"#,
        ] {
            assert_eq!(rt(src, false), src, "{src}");
        }
        let pretty = "{\n  \"raw\": \"\\ud800\"\n}";
        assert_eq!(rt(pretty, true), pretty);
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
