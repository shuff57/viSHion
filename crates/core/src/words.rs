//! Port of js/words.js (pure parts): flagged items, effective text, marked text, decisions, reference
//! library. The regexes of the JS file are reproduced by hand; see `build` for the one that matters.
use crate::jsx::{self, boundary, esc_regex, is_ws, split_ws, u16len};
use crate::model::{Page, Settings};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const S0: char = '\u{E000}';
pub const S1: char = '\u{E001}';
pub const S2: char = '\u{E002}';
const ILL: &str = "[illegible]";
const REF_MAX: usize = 300;

/// Byte spans of every `/\$\$[\s\S]+?\$\$|\$[^$\n]+?\$/g` match, left to right.
pub fn math_spans(t: &str) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut i = 0;
    while i < t.len() {
        let rest = &t[i..];
        if rest.starts_with('$') {
            // alt 1: $$ + at least one char (lazy) + $$
            if rest.starts_with("$$") {
                let after = i + 2;
                if let Some(first) = t[after..].chars().next() {
                    let from = after + first.len_utf8();
                    if let Some(rel) = t[from..].find("$$") {
                        let end = from + rel + 2;
                        out.push((i, end));
                        i = end;
                        continue;
                    }
                }
            }
            // alt 2: $ + at least one non-$/non-newline char + $
            let tail = &t[i + 1..];
            if let Some(p) = tail.find(['$', '\n']) {
                if p > 0 && tail.as_bytes()[p] == b'$' {
                    let end = i + 1 + p + 1;
                    out.push((i, end));
                    i = end;
                    continue;
                }
            }
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    out
}

/// `mapText`: runs `f` on the text outside `$...$` math so markers never land inside TeX.
fn map_text(t: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (a, b) in math_spans(t) {
        out.push_str(&f(&t[last..a]));
        out.push_str(&t[a..b]);
        last = b;
    }
    out.push_str(&f(&t[last..]));
    out
}

/// One flagged word. `to_value` gives the exact JSON the JS object serialises to (key order included).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub key: String,
    /// "suggest" or "illegible"
    pub kind: &'static str,
    pub index: usize,
    pub word: Option<Value>,
    pub term: Option<Value>,
    pub conf: Option<Value>,
    pub base: Option<String>,
    pub via: Option<String>,
    pub shown: Value,
}

impl Item {
    pub fn is_suggest(&self) -> bool {
        self.kind == "suggest"
    }
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("key".into(), json!(self.key));
        m.insert("kind".into(), json!(self.kind));
        m.insert("index".into(), json!(self.index));
        if let Some(w) = &self.word {
            m.insert("word".into(), w.clone());
        }
        if let Some(t) = &self.term {
            m.insert("term".into(), t.clone());
        }
        if let Some(c) = &self.conf {
            m.insert("conf".into(), c.clone());
        }
        if let Some(b) = &self.base {
            m.insert("base".into(), json!(b));
        }
        m.insert("via".into(), self.via.as_ref().map_or(Value::Null, |v| json!(v)));
        m.insert("shown".into(), self.shown.clone());
        Value::Object(m)
    }
}

fn edit_text(p: &Page, key: &str) -> Option<Value> {
    p.edits()
        .iter()
        .find(|e| e.get("key").and_then(Value::as_str) == Some(key))
        .map(|e| e.get("text").cloned().unwrap_or(Value::Null))
}

/// Suggestions whose current text repeats an earlier one are dropped so every item has a unique place.
fn suggest_items(p: &Page, plain: bool) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for (i, c) in p.corrections().iter().enumerate() {
        let get = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("");
        let applied = jsx::truthy(c.get("applied"));
        let shown = if plain || !applied { get("word") } else { get("term") };
        if shown.is_empty() || !seen.insert(shown.to_string()) {
            continue;
        }
        out.push(Item {
            key: format!("c:{}", i),
            kind: "suggest",
            index: i,
            word: c.get("word").cloned(),
            term: c.get("term").cloned(),
            conf: c.get("conf").cloned(),
            base: Some(shown.to_string()),
            via: None,
            shown: Value::Null,
        });
    }
    out
}

/// One pass over the base text: apply typed edits, and (when `mark`) wrap each flagged word in markers.
fn build(p: &Page, base: &str, plain: bool, mark: bool) -> String {
    let items = suggest_items(p, plain);
    // JS: items.forEach(map.set) then alternation sorted by escaped UTF-16 length, descending (stable).
    let mut alts: Vec<(String, &Item)> = vec![];
    let mut lookup: Vec<(&str, &Item)> = vec![];
    for it in &items {
        let b = it.base.as_deref().unwrap();
        lookup.push((b, it));
        alts.push((esc_regex(b), it));
    }
    alts.sort_by(|a, b| u16len(&b.0).cmp(&u16len(&a.0)));
    let words: Vec<&str> = alts.iter().map(|(_, it)| it.base.as_deref().unwrap()).collect();
    let mut n = 0usize;
    map_text(base, |seg| {
        let ch: Vec<char> = seg.chars().collect();
        let mut out = String::with_capacity(seg.len());
        let mut i = 0;
        while i < ch.len() {
            // alternative 1: [illegible], ASCII case-insensitive
            let mut hit: Option<(usize, String, String)> = None; // (len in chars, key, text)
            if ch[i] == '[' && i + 11 <= ch.len() {
                let cand: String = ch[i..i + 11].iter().collect();
                if cand.eq_ignore_ascii_case(ILL) {
                    hit = Some((11, String::new(), cand));
                }
            }
            if hit.is_none() && boundary(&ch, i) {
                for w in &words {
                    let wc: Vec<char> = w.chars().collect();
                    let e = i + wc.len();
                    if e <= ch.len() && ch[i..e] == wc[..] && boundary(&ch, e) {
                        let it = lookup.iter().find(|(b, _)| b == w).unwrap().1;
                        hit = Some((wc.len(), it.key.clone(), w.to_string()));
                        break;
                    }
                }
            }
            match hit {
                None => {
                    out.push(ch[i]);
                    i += 1;
                }
                Some((len, mut key, mut text)) => {
                    if key.is_empty() {
                        key = format!("i:{}", n);
                        n += 1;
                    }
                    if !plain {
                        if let Some(e) = edit_text(p, &key) {
                            text = jsx::to_string(&e);
                        }
                    }
                    if mark {
                        out.push(S0);
                        out.push_str(&key);
                        out.push(S1);
                        out.push_str(&text);
                        out.push(S2);
                    } else {
                        out.push_str(&text);
                    }
                    i += len;
                }
            }
        }
        out
    })
}

/// Text with typed fixes applied. This is what the review screen and every export show.
pub fn effective(p: &Page) -> String {
    let base = p.base();
    if p.edits().is_empty() {
        base.to_string()
    } else {
        build(p, base, false, false)
    }
}

/// Same text with markers, for the transcript view. `original` selects the raw text.
pub fn marked(p: &Page, original: bool) -> String {
    if original {
        build(p, p.raw(), true, true)
    } else {
        build(p, p.base(), false, true)
    }
}

/// Every flagged word on a page, in reading order, with its decision (`via`) when there is one.
pub fn items(p: &Page) -> Vec<Item> {
    if p.status() != "done" || p.raw().is_empty() {
        return vec![];
    }
    let res = p.resolved();
    let via_of = |k: &str| match res.get(k) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let mut out = vec![];
    for mut it in suggest_items(p, false) {
        it.via = via_of(&it.key);
        it.shown = edit_text(p, &it.key).unwrap_or_else(|| json!(it.base));
        out.push(it);
    }
    let mut n = 0usize;
    map_text(p.base(), |seg| {
        let b = seg.as_bytes();
        let mut i = 0;
        while i + 11 <= b.len() {
            if b[i] == b'[' && b[i..i + 11].eq_ignore_ascii_case(ILL.as_bytes()) {
                let key = format!("i:{}", n);
                n += 1;
                out.push(Item {
                    via: via_of(&key),
                    shown: edit_text(p, &key).unwrap_or_else(|| json!(ILL)),
                    key,
                    kind: "illegible",
                    index: n - 1,
                    word: Some(json!(ILL)),
                    term: None,
                    conf: None,
                    base: None,
                });
                i += 11;
            } else {
                i += 1;
            }
        }
        String::new()
    });
    out
}

/// Items with no decision yet.
pub fn open(p: &Page) -> Vec<Item> {
    items(p).into_iter().filter(|i| i.via.is_none()).collect()
}

pub fn count(p: &Page) -> usize {
    open(p).len()
}

/// `clean` of js/words.js: the text a teacher typed, with markers, `$`, newlines and `[illegible]` removed.
pub fn clean_typed(s: &str) -> String {
    let mut t = String::with_capacity(s.len());
    for c in s.chars() {
        t.push(if matches!(c, '\u{E000}'..='\u{E002}' | '$' | '\n' | '\r') { ' ' } else { c });
    }
    // .replace(/\[illegible\]/gi, '')
    let b = t.as_bytes();
    let mut out = String::new();
    let (mut i, mut last) = (0, 0);
    while i + 11 <= b.len() {
        if b[i] == b'[' && b[i..i + 11].eq_ignore_ascii_case(ILL.as_bytes()) {
            out.push_str(&t[last..i]);
            i += 11;
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&t[last..]);
    jsx::trim(&jsx::collapse_ws(&out)).to_string()
}

/// Up to five words either side of the n-th unreadable word (outside math), for the reference library.
/// Returns `(before, after)`.
pub fn context_for(p: &Page, key: &str) -> Option<(String, String)> {
    let base: Vec<char> = p.base().chars().collect();
    let base_s: String = base.iter().collect();
    let want = jsx::string_to_number(key.chars().skip(2).collect::<String>().as_str());
    // masked: every char of a math span replaced by one space (same char positions as `base`).
    let mut masked = base.clone();
    let spans = math_spans(&base_s);
    if !spans.is_empty() {
        // convert byte spans to char spans
        let mut byte_to_char = std::collections::HashMap::new();
        let mut bi = 0;
        for (ci, c) in base.iter().enumerate() {
            byte_to_char.insert(bi, ci);
            bi += c.len_utf8();
        }
        byte_to_char.insert(bi, base.len());
        for (a, b) in spans {
            for m in masked.iter_mut().take(byte_to_char[&b]).skip(byte_to_char[&a]) {
                *m = ' ';
            }
        }
    }
    let mut n = 0f64;
    let mut at: Option<usize> = None;
    let mut i = 0;
    while i + 11 <= masked.len() {
        let cand: String = masked[i..i + 11].iter().collect();
        if masked[i] == '[' && cand.eq_ignore_ascii_case(ILL) {
            let hit = n == want;
            n += 1.0;
            if hit {
                at = Some(i);
                break;
            }
            i += 11;
        } else {
            i += 1;
        }
    }
    let at = at?;
    let plain = |t: &str| {
        let mut o = String::new();
        let mut last = 0;
        for (a, b) in math_spans(t) {
            o.push_str(&t[last..a]);
            o.push(' ');
            last = b;
        }
        o.push_str(&t[last..]);
        o
    };
    let words = |t: &str| -> Vec<String> {
        let mut s = String::new();
        let b = t.as_bytes();
        let (mut i, mut last) = (0, 0);
        while i + 11 <= b.len() {
            if b[i] == b'[' && b[i..i + 11].eq_ignore_ascii_case(ILL.as_bytes()) {
                s.push_str(&t[last..i]);
                s.push_str(" \u{2026} ");
                i += 11;
                last = i;
            } else {
                i += 1;
            }
        }
        s.push_str(&t[last..]);
        let s: String = s.chars().map(|c| if c == '~' || c == '$' { ' ' } else { c }).collect();
        split_ws(&s).into_iter().map(String::from).collect()
    };
    let before_src: String = base[..at].iter().collect();
    let after_src: String = base[at + 11..].iter().collect();
    let bw = words(&plain(&before_src));
    let aw = words(&plain(&after_src));
    let before = bw[bw.len().saturating_sub(5)..].join(" ");
    let after = aw[..aw.len().min(5)].join(" ");
    Some((before, after))
}

/// `W.remember`: adds a fix the teacher typed for an unreadable word to `settings.reference`.
pub fn remember(p: &Page, key: &str, text: &str, settings: &mut Settings, topic: &str, now: i64) {
    if !settings.remember_fixes() || text.is_empty() {
        return;
    }
    let Some((before, after)) = context_for(p, key) else { return };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(String::from);
    let mut list: Vec<Value> = settings
        .reference()
        .into_iter()
        .filter(|r| {
            !(s(r, "word").as_deref() == Some(text)
                && s(r, "before").as_deref() == Some(before.as_str())
                && s(r, "after").as_deref() == Some(after.as_str()))
        })
        .collect();
    list.push(json!({"word": text, "before": before, "after": after, "topic": topic, "at": now}));
    let start = list.len().saturating_sub(REF_MAX);
    settings.set("reference", Value::Array(list[start..].to_vec()));
}

fn ref_str<'a>(r: &'a Value, k: &str) -> &'a str {
    r.get(k).and_then(Value::as_str).unwrap_or("")
}

/// `W.referenceWords(topic, max)`.
pub fn reference_words(settings: &Settings, topic: &str, max: Option<usize>) -> Vec<String> {
    let max = max.filter(|m| *m > 0).unwrap_or(30);
    let mut seen = HashSet::new();
    let mut out = vec![];
    for r in settings.reference().iter().rev() {
        let w = ref_str(r, "word");
        let k = w.to_lowercase();
        let rt = ref_str(r, "topic");
        if (!topic.is_empty() && !rt.is_empty() && rt != topic)
            || seen.contains(&k)
            || out.len() >= max
            || (w.chars().any(is_ws) && u16len(w) > 30)
        {
            continue;
        }
        seen.insert(k);
        out.push(w.to_string());
    }
    out
}

/// `W.referenceExamples(topic, max)`.
pub fn reference_examples(settings: &Settings, topic: &str, max: Option<usize>) -> Vec<String> {
    let max = max.filter(|m| *m > 0).unwrap_or(10);
    let all: Vec<Value> = settings
        .reference()
        .into_iter()
        .filter(|r| topic.is_empty() || ref_str(r, "topic").is_empty() || ref_str(r, "topic") == topic)
        .collect();
    let start = all.len().saturating_sub(max);
    all[start..]
        .iter()
        .map(|r| {
            let parts: Vec<&str> = [ref_str(r, "before"), "___", ref_str(r, "after")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            format!("\u{201C}{}\u{201D} was {}", parts.join(" "), ref_str(r, "word"))
        })
        .collect()
}

/// `W.choose`: records a decision. `via`: suggested | written | typed | marked. `text` only for typed.
/// Returns false (page untouched) when a typed fix is empty after cleaning.
pub fn choose(p: &mut Page, key: &str, via: &str, text: &str, settings: &mut Settings, topic: &str, now: i64) -> bool {
    let mut resolved = p.resolved();
    let mut edits: Vec<Value> = p
        .edits()
        .iter()
        .filter(|e| e.get("key").and_then(Value::as_str) != Some(key))
        .cloned()
        .collect();
    let mut typed = String::new();
    if via == "typed" {
        typed = clean_typed(text);
        if typed.is_empty() {
            return false;
        }
        edits.push(json!({"key": key, "text": typed}));
    }
    resolved.insert(key.to_string(), json!(via));
    if via == "typed" && key.starts_with('i') {
        remember(p, key, &typed, settings, topic, now);
    }
    p.set_carried("resolved", Value::Object(resolved));
    p.set_carried("edits", Value::Array(edits));
    p.set_carried("reviewed", Value::Bool(false));
    if key.starts_with('c') {
        let idx = jsx::string_to_number(key.chars().skip(2).collect::<String>().as_str());
        crate::judge::toggle(p, idx, via == "suggested");
    }
    true
}

/// `W.reopen`: takes back a decision on one word.
pub fn reopen(p: &mut Page, key: &str) {
    let mut resolved = p.resolved();
    resolved.shift_remove(key);
    let edits: Vec<Value> = p
        .edits()
        .iter()
        .filter(|e| e.get("key").and_then(Value::as_str) != Some(key))
        .cloned()
        .collect();
    p.set_carried("resolved", Value::Object(resolved));
    p.set_carried("edits", Value::Array(edits));
    p.set_carried("reviewed", Value::Bool(false));
}

/// `W.useAll` for one page: use every suggestion that has no decision yet.
pub fn use_all(p: &mut Page, settings: &mut Settings, topic: &str, now: i64) {
    for it in items(p).into_iter().filter(|i| i.is_suggest() && i.via.is_none()) {
        choose(p, &it.key, "suggested", "", settings, topic, now);
    }
}

/// `O.buildPrompt`: the assignment's own instructions (or the default), then the topic's spelling hints and
/// the words this teacher has fixed before.
pub fn build_prompt(settings: &Settings, meta: &crate::model::Project) -> String {
    let own = meta.prompt().trim_matches(is_ws);
    let default = settings.get("defaultPrompt").and_then(Value::as_str).unwrap_or(crate::model::DEFAULT_PROMPT);
    let base = if own.is_empty() { default } else { own };
    let terms = settings.terms(meta.topic());
    let hints = if terms.is_empty() {
        String::new()
    } else {
        format!("Spelling hints (use only when the handwriting matches): {}.", terms.join(", "))
    };
    let mine = reference_words(settings, meta.topic(), Some(30));
    let earlier = if mine.is_empty() {
        String::new()
    } else {
        format!("Words this teacher has fixed before (use only when the handwriting matches): {}.", mine.join(", "))
    };
    [base.to_string(), hints, earlier].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod prompt_tests {
    use super::*;
    use crate::model::{Project, Settings};

    #[test]
    fn default_prompt_then_topic_hints() {
        let s = Settings::defaults();
        let meta = Project::from_json("{}").unwrap();
        assert_eq!(build_prompt(&s, &meta), crate::model::DEFAULT_PROMPT);
        let mut m = Project::from_json("{}").unwrap();
        m.set("topic", json!("Medical"));
        m.set("prompt", json!("  Read it.  "));
        let p = build_prompt(&s, &m);
        assert!(p.starts_with("Read it.\nSpelling hints (use only when the handwriting matches): hypertension, "));
    }
}
