//! Port of `O.clean` from js/ocr.js: removes model wrappers (a code fence, "Here is the transcription:",
//! a closing note) only at the very start or end, and leaves the handwriting itself alone.
//! The regexes are matched by hand; every `i` flag is ASCII-only, as in JS without the `u` flag.
use crate::jsx::{boundary, is_ws, starts_with_ci, trim};

fn is_term(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/\s*:?\s*$/` matches all of `s`.
fn only_ws_colon(s: &str) -> bool {
    let t = s.trim_start_matches(is_ws);
    let t = t.strip_prefix(':').unwrap_or(t);
    t.trim_start_matches(is_ws).is_empty()
}

/// `.*` then `rest` to the end of input, where `.` excludes line terminators:
/// `tail` is the text after the part already matched, `rest_ok` tests what must follow `.*`.
fn dot_star_then(tail: &str, rest_ok: &dyn Fn(&str) -> bool) -> bool {
    match tail.find(is_term) {
        None => true, // `.*` takes everything; the rest matches empty
        Some(cut) => rest_ok(&tail[cut..]),
    }
}

/// Word boundary between `s[..i]` and `s[i..]` (byte index).
fn bound_at(s: &str, i: usize) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let ci = s[..i].chars().count();
    boundary(&chars, ci)
}

/// The "lead" line regex of `O.clean`.
fn is_lead(line: &str) -> bool {
    let mut starts: Vec<usize> = vec![];
    let p0 = line.len() - line.trim_start_matches(is_ws).len();
    starts.push(p0);
    for w in ["sure", "okay", "ok", "certainly", "of course"] {
        if starts_with_ci(&line[p0..], w) {
            starts.push(p0 + w.len());
        }
    }
    for st in starts {
        let mut p = st;
        while line[p..].starts_with([',', '!', '.', ' ']) {
            p += 1;
        }
        p += line[p..].len() - line[p..].trim_start_matches(is_ws).len();
        let r = &line[p..];
        // here(?:'s| is| are)\b.*
        if starts_with_ci(r, "here") {
            for suf in ["'s", " is", " are"] {
                if starts_with_ci(&r[4..], suf) {
                    let e = p + 4 + suf.len();
                    if bound_at(line, e) && dot_star_then(&line[e..], &only_ws_colon) {
                        return true;
                    }
                }
            }
        }
        // the (?:transcription|transcript|text)\b.*
        if starts_with_ci(r, "the ") {
            for w in ["transcription", "transcript", "text"] {
                if starts_with_ci(&r[4..], w) {
                    let e = p + 4 + w.len();
                    if bound_at(line, e) && dot_star_then(&line[e..], &only_ws_colon) {
                        return true;
                    }
                }
            }
        }
        // transcription | transcript | handwritten text, then \s*:?\s*$
        for w in ["transcription", "transcript", "handwritten text"] {
            if starts_with_ci(r, w) && only_ws_colon(&r[w.len()..]) {
                return true;
            }
        }
    }
    false
}

/// The closing-remark ("tail") line regex of `O.clean`.
fn is_tail(line: &str) -> bool {
    let p0 = line.len() - line.trim_start_matches(is_ws).len();
    let free = |s: &str| !s.chars().any(is_term);
    let lit = |s: &str, w: &str| -> Option<usize> {
        // literal prefix `w`, then \b, then `.*$`
        if starts_with_ci(s, w) {
            let e = w.len();
            let full_e = line.len() - s.len() + e;
            if bound_at(line, full_e) && free(&line[full_e..]) {
                return Some(e);
            }
        }
        None
    };
    let s = &line[p0..];
    if lit(s, "let me know").is_some() || lit(s, "i hope").is_some() || lit(s, "please let me know").is_some() {
        return true;
    }
    // this (?:is|was) (?:a |the )?transcription\b.*
    if starts_with_ci(s, "this ") {
        let r = &s[5..];
        for v in ["is ", "was "] {
            if starts_with_ci(r, v) {
                let r2 = &r[v.len()..];
                for art in ["a ", "the ", ""] {
                    if starts_with_ci(r2, art) && lit(&r2[art.len()..], "transcription").is_some() {
                        return true;
                    }
                }
            }
        }
    }
    // \(?\s*notes?\b.*\b(?:transcri|illegible|handwrit|unclear|hard to read|difficult|image|the page)\b.*
    let mut q = p0;
    if line[q..].starts_with('(') {
        q += 1;
    }
    q += line[q..].len() - line[q..].trim_start_matches(is_ws).len();
    if starts_with_ci(&line[q..], "note") {
        let mut e = q + 4;
        if starts_with_ci(&line[e..], "s") && bound_at(line, e + 1) {
            e += 1;
        } else if !bound_at(line, e) {
            return false;
        }
        if !free(&line[e..]) {
            return false;
        }
        let rest = &line[e..];
        for k in rest.char_indices().map(|(i, _)| i) {
            for kw in ["transcri", "illegible", "handwrit", "unclear", "hard to read", "difficult", "image", "the page"] {
                if starts_with_ci(&rest[k..], kw) && bound_at(line, e + k) && bound_at(line, e + k + kw.len()) {
                    return true;
                }
            }
        }
    }
    false
}

/// `O.clean(t)`.
pub fn clean(t: &str) -> String {
    let mut s: String = t.replace('\r', "");
    if let Some(r) = s.strip_prefix('\u{FEFF}') {
        s = r.to_string();
    }
    // .replace(/^\s*```[a-z]*\s*\n/i, '')
    {
        let p = s.len() - s.trim_start_matches(is_ws).len();
        if s[p..].starts_with("```") {
            let mut q = p + 3;
            while q < s.len() && s.as_bytes()[q].is_ascii_alphabetic() {
                q += 1;
            }
            let ws_end = q + (s[q..].len() - s[q..].trim_start_matches(is_ws).len());
            if let Some(nl) = s[q..ws_end].rfind('\n') {
                s = s[q + nl + 1..].to_string();
            }
        }
    }
    // .replace(/\n\s*```\s*$/, '')
    {
        let end = trim_end_ws(&s).len();
        if s[..end].ends_with("```") {
            let fence = end - 3;
            let head = &s[..fence];
            let run_start = head.trim_end_matches(is_ws).len();
            if let Some(nl) = s[run_start..fence].find('\n') {
                s.truncate(run_start + nl);
            }
        }
    }
    let mut lines: Vec<String> = s.split('\n').map(String::from).collect();
    while !lines.is_empty() {
        let first_lead = is_lead(&lines[0]);
        let first_blank = trim(&lines[0]).is_empty();
        if !(first_lead || first_blank) {
            break;
        }
        let second = lines.len() > 1 && is_lead(&lines[1]);
        if !(first_lead || second) {
            break;
        }
        lines.remove(0);
    }
    while lines.last().map_or(false, |l| trim(l).is_empty()) {
        lines.pop();
    }
    let mut k = lines.len();
    while k > 1 {
        k -= 1;
        if trim(&lines[k - 1]).is_empty() && is_tail(&lines[k]) {
            lines.truncate(k);
            break;
        }
        if !trim(&lines[k]).is_empty() && !is_tail(&lines[k]) {
            break;
        }
    }
    while lines.last().map_or(false, |l| trim(l).is_empty()) {
        lines.pop();
    }
    let joined = lines.join("\n");
    // .replace(/\n{3,}/g, '\n\n')
    let mut c = String::with_capacity(joined.len());
    let mut run = 0;
    for ch in joined.chars() {
        if ch == '\n' {
            run += 1;
            if run <= 2 {
                c.push('\n');
            }
        } else {
            run = 0;
            c.push(ch);
        }
    }
    // .replace(/[ \t]+$/gm, '')
    let cs: Vec<char> = c.chars().collect();
    let mut out = String::with_capacity(c.len());
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == ' ' || cs[i] == '\t' {
            let mut j = i;
            while j < cs.len() && (cs[j] == ' ' || cs[j] == '\t') {
                j += 1;
            }
            if j == cs.len() || is_term(cs[j]) {
                i = j;
                continue;
            }
            out.extend(&cs[i..j]);
            i = j;
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
    trim(&out).to_string()
}

fn trim_end_ws(s: &str) -> &str {
    s.trim_end_matches(is_ws)
}
