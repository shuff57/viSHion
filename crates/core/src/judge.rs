//! Port of js/judge.js (pure parts): offline heuristic, acceptance rule, `corrected` rebuild, toggle.
//! The `fetch` to the judge model is I/O and lives in `net`; `parse_remote` is its pure tail.
use crate::jsx::{self, boundary, num_value, u16len};
use crate::model::Page;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// Levenshtein distance over UTF-16 code units (as the JS does).
pub fn lev(a: &str, b: &str) -> usize {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let sub = prev[j - 1] + usize::from(a[i - 1] != b[j - 1]);
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(sub));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// `text.match(/[A-Za-z][A-Za-z'-]{3,}/g)`.
fn candidate_words(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let tail = |c: u8| c.is_ascii_alphabetic() || c == b'\'' || c == b'-';
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_alphabetic() {
            let run = b[i + 1..].iter().take_while(|c| tail(**c)).count();
            if run >= 3 {
                out.push(&text[i..i + 1 + run]);
                i += 1 + run;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A suggestion as stored in `page.corrections` (`applied` is added by `apply_found`).
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub word: String,
    pub term: String,
    pub conf: f64,
}

/// Offline mode: flag words that are one or two edits away from a glossary term.
pub fn heuristic(text: &str, list: &[String]) -> Vec<Found> {
    let single: Vec<&String> = list.iter().filter(|t| !t.chars().any(jsx::is_ws)).collect();
    let known: HashSet<String> = list.iter().map(|t| t.to_lowercase()).collect();
    let mut seen = HashSet::new();
    let mut out = vec![];
    for w in candidate_words(text) {
        let lw = w.to_lowercase();
        if seen.contains(&lw) || known.contains(&lw) {
            continue;
        }
        seen.insert(lw.clone());
        let mut best: Option<&String> = None;
        let mut bd = 99usize;
        for t in &single {
            let d = lev(&lw, &t.to_lowercase());
            let lim = if u16len(&lw) >= 8 { 2 } else { 1 };
            if d <= lim && d < bd && u16len(&lw).abs_diff(u16len(t)) <= 2 {
                best = Some(t);
                bd = d;
            }
        }
        if let Some(t) = best {
            let conf = (1.0 - bd as f64 / (u16len(w).max(3) as f64)).max(0.3);
            out.push(Found { word: w.to_string(), term: t.clone(), conf });
        }
    }
    out
}

/// `J.run` tail: `page.corrections = found.map(c => ({ applied: false, ...c }))`.
pub fn apply_found(p: &mut Page, found: &[Found]) {
    let arr = found
        .iter()
        .map(|c| json!({"applied": false, "word": c.word, "term": c.term, "conf": num_value(c.conf)}))
        .collect();
    p.set_carried("corrections", Value::Array(arr));
}

/// A suggestion is only a spelling fix: the replacement must be a glossary term, close to the written word
/// by edit distance, and the written word must not already be a glossary term.
pub fn accept(x: &Value, list: &[String], text: &str) -> bool {
    let (Some(word), Some(term)) = (
        x.get("word").and_then(Value::as_str),
        x.get("term").and_then(Value::as_str),
    ) else {
        return false;
    };
    if !text.contains(word) {
        return false;
    }
    let (w, t) = (word.to_lowercase(), term.to_lowercase());
    let known: Vec<String> = list.iter().map(|s| s.to_lowercase()).collect();
    if w == t || !known.contains(&t) || known.contains(&w) {
        return false;
    }
    let lim = 1usize.max(3usize.min(u16len(&w).max(u16len(&t)) / 4));
    lev(&w, &t) <= lim
}

/// The question `J.remote` puts to the model: the glossary, then the transcript.
pub fn remote_prompt(list: &[String], text: &str) -> String {
    format!(
        "Check a handwriting transcript against a glossary. Return only a JSON array of {{\"word\",\"term\",\"conf\"}} for words in the transcript that are probably misspellings of a glossary term. Fix spelling only: never reword, never change grammar, numbers or meaning, and only use a term exactly as it appears in the glossary. conf is 0 to 1. Return [] when nothing needs fixing.\nGlossary: {}\nTranscript:\n{}",
        list.join(", "),
        text
    )
}

/// Pure tail of `J.remote`: the model's reply to the accepted suggestions.
/// Errors carry the JS error message; `strict` is typesafe mode.
pub fn parse_remote(reply: &str, list: &[String], text: &str, strict: bool) -> Result<Vec<Found>, String> {
    // t.match(/\[[^\[\]]*\]/): the first bracket pair with no brackets inside.
    let chars: Vec<(usize, char)> = reply.char_indices().collect();
    let mut cand: Option<&str> = None;
    'o: for (k, (i, c)) in chars.iter().enumerate() {
        if *c != '[' {
            continue;
        }
        for (j, d) in chars[k + 1..].iter() {
            if *d == '[' {
                break;
            }
            if *d == ']' {
                cand = Some(&reply[*i..j + 1]);
                break 'o;
            }
        }
    }
    let src = cand.unwrap_or(reply);
    let v: Value = serde_json::from_str(src).map_err(|e| e.to_string())?;
    let Value::Array(arr) = v else {
        return Err("judge returned no array".into());
    };
    let ok: Vec<Found> = arr
        .iter()
        .filter(|x| accept(x, list, text))
        .map(|x| {
            let c = jsx::number(x.get("conf").unwrap_or(&Value::Null));
            let c = if c == 0.0 || c.is_nan() { 0.5 } else { c };
            Found {
                word: x["word"].as_str().unwrap().to_string(),
                term: x["term"].as_str().unwrap().to_string(),
                conf: c.max(0.0).min(1.0),
            }
        })
        .collect();
    if strict && ok.len() != arr.len() {
        return Err("judge output failed validation".into());
    }
    Ok(ok)
}

/// Expands a JS replacement string (`$$`, `$&`, `` $` ``, `$'`) for a regex with no capture groups.
fn expand(repl: &str, matched: &str, before: &str, after: &str, out: &mut String) {
    let cs: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '$' && i + 1 < cs.len() {
            match cs[i + 1] {
                '$' => out.push('$'),
                '&' => out.push_str(matched),
                '`' => out.push_str(before),
                '\'' => out.push_str(after),
                _ => {
                    out.push('$');
                    i += 1;
                    continue;
                }
            }
            i += 2;
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
}

/// `t.replace(new RegExp('\\b' + esc(word) + '\\b', 'g'), term)`.
fn replace_word(t: &str, word: &str, term: &str) -> String {
    let ch: Vec<char> = t.chars().collect();
    let w: Vec<char> = word.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut last = 0;
    let s = |a: usize, b: usize| -> String { ch[a..b].iter().collect() };
    while i <= ch.len() {
        let e = i + w.len();
        if e <= ch.len() && ch[i..e] == w[..] && boundary(&ch, i) && boundary(&ch, e) {
            out.push_str(&s(last, i));
            expand(term, &s(i, e), &s(0, i), &s(e, ch.len()), &mut out);
            last = e;
            if w.is_empty() {
                // empty match: JS advances one code unit; the skipped char is copied by `last`
                i += 1;
            } else {
                i = e;
            }
        } else {
            i += 1;
        }
    }
    out.push_str(&s(last.min(ch.len()), ch.len()));
    out
}

/// `corrected` = raw with every applied suggestion swapped in, so undo is exact.
pub fn rebuild(raw: &str, corrections: &[Value]) -> String {
    let mut t = raw.to_string();
    for c in corrections {
        if !jsx::truthy(c.get("applied")) {
            continue;
        }
        let word = jsx::to_string(c.get("word").unwrap_or(&Value::Null));
        let term = c.get("term").map_or("undefined".to_string(), jsx::to_string);
        t = replace_word(&t, &word, &term);
    }
    t
}

/// `J.toggle`: set one suggestion applied/undone and refresh `corrected`. `idx` is a JS number.
pub fn toggle(p: &mut Page, idx: f64, applied: bool) {
    if idx.is_nan() || idx < 0.0 || idx.fract() != 0.0 {
        return;
    }
    let i = idx as usize;
    let cur = p.corrections();
    if i >= cur.len() || !jsx::truthy(Some(&cur[i])) {
        return;
    }
    let next: Vec<Value> = cur
        .iter()
        .enumerate()
        .map(|(k, c)| {
            if k == i {
                let mut m: Map<String, Value> = c.as_object().cloned().unwrap_or_default();
                m.insert("applied".into(), Value::Bool(applied));
                Value::Object(m)
            } else {
                c.clone()
            }
        })
        .collect();
    let corrected = rebuild(p.raw(), &next);
    p.set_carried("corrections", Value::Array(next));
    p.set_carried("corrected", Value::String(corrected));
}

/// `J.applyAll`.
pub fn apply_all(p: &mut Page) {
    // JS: `p.corrections.map` throws on a page without the array, leaving the page untouched
    if !matches!(p.get("corrections"), Some(Value::Array(_))) {
        return;
    }
    let next: Vec<Value> = p
        .corrections()
        .iter()
        .map(|c| {
            let mut m: Map<String, Value> = c.as_object().cloned().unwrap_or_default();
            m.insert("applied".into(), Value::Bool(true));
            Value::Object(m)
        })
        .collect();
    let corrected = rebuild(p.raw(), &next);
    p.set_carried("corrections", Value::Array(next));
    p.set_carried("corrected", Value::String(corrected));
}
