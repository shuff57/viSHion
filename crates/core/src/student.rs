//! Pure parts of js/student.js: turning the model's answer into a usable student name, the demo-mode
//! name picker, and the one-line answer parse. The strip crop and the request are I/O (`net` / `ui`).
use crate::jsx::{self, is_ws, starts_with_ci, u16len};
use serde_json::Value;

/// Top fraction of the page sent to the name reader.
pub const STRIP: f64 = 0.14;
/// The question put to the top strip of a page (js/student.js `ASK`).
pub const ASK: &str = "This is the top strip of a handwritten student page. If a student name is written there, read it. Answer with JSON only, like {\"name\": \"Maya R.\"}. If no name is written, answer {\"name\": null}. Do not guess and do not add anything else.";

/// `I.check` of js/image.js by file name and size: why a file cannot be added, or `None`.
pub fn check_file(name: &str, size: usize) -> Option<&'static str> {
    let n = name.to_lowercase();
    let ext = n.rsplit('.').next().filter(|_| n.contains('.')).unwrap_or("");
    if matches!(ext, "heic" | "heif") {
        return Some("HEIC images are not supported here. Convert the file to JPG or PNG first.");
    }
    if ext == "pdf" {
        return Some("PDFs are not supported here. Convert each page to JPG or PNG first.");
    }
    if !matches!(ext, "png" | "jpg" | "jpeg" | "webp") {
        return Some("Unsupported file type. Use PNG, JPG or WebP.");
    }
    if size > 40 * 1024 * 1024 {
        return Some("The file is larger than 40 MB.");
    }
    None
}

pub const DEMO_NAMES: [Option<&str>; 6] =
    [Some("Maya R."), Some("Devon T."), Some("Sofia V."), Some("Aaliyah M."), None, Some("Caleb P.")];

/// `/^\s*(student\s*)?name\s*[:\-]\s*/i`: length in bytes of the match, if any.
fn prefix_len(s: &str) -> Option<usize> {
    let skip_ws = |t: &str| t.len() - t.trim_start_matches(is_ws).len();
    let start = skip_ws(s);
    let tries: [bool; 2] = [true, false];
    for with_student in tries {
        let mut pos = start;
        if with_student {
            if !starts_with_ci(&s[pos..], "student") {
                continue;
            }
            pos += 7;
            pos += skip_ws(&s[pos..]);
        }
        if !starts_with_ci(&s[pos..], "name") {
            continue;
        }
        pos += 4;
        pos += skip_ws(&s[pos..]);
        match s[pos..].chars().next() {
            Some(':') | Some('-') => pos += 1,
            _ => continue,
        }
        pos += skip_ws(&s[pos..]);
        return Some(pos);
    }
    None
}

/// `ST.clean`: the model's answer as a usable name, or `None`.
pub fn clean(v: &Value) -> Option<String> {
    let Value::String(v) = v else { return None };
    let rest = match prefix_len(v) {
        Some(n) => &v[n..],
        None => v.as_str(),
    };
    let no_quotes: String = rest.chars().filter(|c| !matches!(c, '"' | '\'' | '`')).collect();
    let n = jsx::trim(&jsx::collapse_ws(&no_quotes)).to_string();
    let n = n.trim_end_matches([',', ';', ':']).to_string();
    let len = u16len(&n);
    if !(2..=40).contains(&len) {
        return None;
    }
    let l = n.to_ascii_lowercase();
    if ["null", "none", "n/a", "unknown", "not found", "no name"].contains(&l.as_str()) {
        return None;
    }
    if !n.chars().any(|c| c.is_ascii_alphabetic() || ('\u{C0}'..='\u{24F}').contains(&c)) {
        return None;
    }
    Some(n)
}

/// Demo mode: the same file name always gets the same name (`h = h * 31 + code unit`, as u32).
pub fn demo_name(hint: &str) -> Option<&'static str> {
    let mut h: u32 = 0;
    for u in hint.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as u32);
    }
    DEMO_NAMES[(h as usize) % DEMO_NAMES.len()]
}

/// Parses the reader's reply (`text.match(/\{[\s\S]*\}/)` then `JSON.parse(...).name`) into a name.
pub fn parse_reply(text: &str) -> Option<String> {
    let src = match (text.find('{'), text.rfind('}')) {
        (Some(a), Some(b)) if b > a => &text[a..=b],
        _ => text,
    };
    let v: Value = serde_json::from_str(src).ok()?;
    clean(v.get("name").unwrap_or(&Value::Null))
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn files_are_checked_like_image_check() {
        assert_eq!(check_file("a.JPG", 10), None);
        assert_eq!(check_file("scan.webp", 10), None);
        assert!(check_file("a.heic", 10).unwrap().starts_with("HEIC"));
        assert!(check_file("a.pdf", 10).unwrap().starts_with("PDFs"));
        assert!(check_file("a.gif", 10).unwrap().starts_with("Unsupported"));
        assert!(check_file("noext", 10).unwrap().starts_with("Unsupported"));
        assert!(check_file("a.png", 41 * 1024 * 1024).unwrap().contains("40 MB"));
    }
}

