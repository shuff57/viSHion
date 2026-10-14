//! Pure SSE state machine, a line-for-line behavioural port of the read loop in js/ocr.js
//! (`O.transcribe`). No web types, no I/O: bytes in, callbacks out. Runs natively under `cargo test`.
//!
//! Bug-compat points (all deliberate, each pinned by a unit test below):
//!  * lines split on `\n` ONLY (a lone `\r` is not a terminator); each line is JS-`trim()`ed.
//!  * an unterminated final line is DROPPED (so `data: [DONE]` with no trailing newline = "unfinished").
//!    Set `flush_tail = true` to opt out of that (the "fix"); default replicates the JS.
//!  * `data:` with an empty payload is a JSON.parse failure => `malformed`.
//!  * a JSON `null` payload is a TypeError inside the JS try block => `malformed`; other scalars are ignored.
//!  * `[DONE]` only sets `finished`; later lines are still processed.
//!  * `malformed` is sticky and wins over `finished`.
//!  * usage / timings keep the LAST truthy value; content is appended only when truthy.

use serde_json::Value;

/// JS `String.prototype.trim` whitespace: WhiteSpace + LineTerminator. NOT the same set as Rust's
/// `char::is_whitespace` (Rust trims U+0085, JS does not; JS trims U+FEFF, Rust does not).
pub fn is_js_ws(c: char) -> bool {
    matches!(c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | '\u{20}' | '\u{a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
pub fn js_trim(s: &str) -> &str { s.trim_matches(is_js_ws) }

/// Incremental UTF-8 decoder matching `new TextDecoder().decode(chunk, {stream:true})`:
/// U+FFFD per maximal invalid subpart, incomplete tail held back, one leading BOM stripped.
#[derive(Default)]
pub struct Utf8Stream { pending: Vec<u8>, bom_done: bool }
impl Utf8Stream {
    pub fn push(&mut self, bytes: &[u8], out: &mut String) {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        let mut s = String::new();
        let mut rest: &[u8] = &data;
        loop {
            match std::str::from_utf8(rest) {
                Ok(v) => { s.push_str(v); break; }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    s.push_str(std::str::from_utf8(good).unwrap());
                    match e.error_len() {
                        Some(n) => { s.push('\u{FFFD}'); rest = &bad[n..]; }
                        None => { self.pending = bad.to_vec(); break; }
                    }
                }
            }
        }
        if !self.bom_done && !s.is_empty() {
            self.bom_done = true;
            if let Some(r) = s.strip_prefix('\u{FEFF}') { out.push_str(r); return; }
        }
        out.push_str(&s);
    }
}

/// JS truthiness of a parsed JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// Best-effort `'' + value` for the (never seen in practice) non-string truthy content case.
fn js_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i128),
            Some(f) => format!("{}", f),
            None => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(js_to_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Verdict { Ok, Unfinished, Malformed }

#[derive(Default)]
pub struct SseParser {
    dec: Utf8Stream,
    buf: String,
    pub flush_tail: bool,
    pub text: String,
    pub chunks: u32,
    pub usage: Option<Value>,
    pub timings: Option<Value>,
    pub finished: bool,
    pub malformed: bool,
}

pub type OnText<'a> = &'a mut dyn FnMut(&str, &str, u32);

impl SseParser {
    pub fn new() -> Self { Self::default() }

    /// Feed raw network bytes. `on_text(delta, full_text_so_far, chunk_count)` fires once per non-empty
    /// content delta, in order, exactly where the JS calls `opt.onText` then `onStatus('writing… N tok')`.
    pub fn push(&mut self, bytes: &[u8], on_text: OnText) {
        self.dec.push(bytes, &mut self.buf);
        while let Some(i) = self.buf.find('\n') {
            let line: String = self.buf[..i].to_string();
            self.buf.drain(..=i);
            self.line(&line, on_text);
        }
    }

    /// Call once on clean end-of-stream (reader `done`). With the JS default this discards the tail.
    pub fn end(&mut self, on_text: OnText) {
        if self.flush_tail && !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            self.line(&line, on_text);
        } else {
            self.buf.clear();
        }
    }

    fn line(&mut self, raw: &str, on_text: OnText) {
        let line = js_trim(raw);
        if !line.starts_with("data:") { return; }
        let payload = js_trim(&line[5..]);
        if payload == "[DONE]" { self.finished = true; return; }
        let j: Value = match serde_json::from_str(payload) { Ok(v) => v, Err(_) => { self.malformed = true; return; } };
        if j.is_null() { self.malformed = true; return; } // `null.choices` throws in JS
        // j.choices && j.choices[0] && j.choices[0].delta && j.choices[0].delta.content
        let content = j.get("choices").filter(|c| truthy(c)).and_then(|c| match c {
            Value::Array(a) => a.first(),
            Value::Object(m) => m.get("0"),
            _ => None,
        }).and_then(|c0| c0.get("delta")).and_then(|d| d.get("content"));
        if let Some(t) = content.filter(|t| truthy(t)) {
            let s = js_to_string(t);
            self.text.push_str(&s);
            self.chunks += 1;
            on_text(&s, &self.text, self.chunks);
        }
        if let Some(u) = j.get("usage").filter(|u| truthy(u)) { self.usage = Some(u.clone()); }
        if let Some(t) = j.get("timings").filter(|u| truthy(u)) { self.timings = Some(t.clone()); }
    }

    pub fn verdict(&self) -> Verdict {
        if self.malformed { Verdict::Malformed } else if !self.finished { Verdict::Unfinished } else { Verdict::Ok }
    }

    /// `(usage && usage.completion_tokens) || (timings && timings.predicted_n) || chunks`
    pub fn tokens(&self) -> Value {
        let pick = |o: &Option<Value>, k: &str| o.as_ref().and_then(|v| v.get(k)).filter(|v| truthy(v)).cloned();
        pick(&self.usage, "completion_tokens").or_else(|| pick(&self.timings, "predicted_n")).unwrap_or_else(|| Value::from(self.chunks))
    }
    /// `timings && timings.predicted_per_second` (None when no timings; Some(Null) when the field is missing).
    pub fn tps(&self) -> Option<Value> {
        self.timings.as_ref().map(|t| t.get("predicted_per_second").cloned().unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(chunks: &[&[u8]], flush: bool) -> (SseParser, Vec<String>) {
        let mut p = SseParser::new();
        p.flush_tail = flush;
        let mut got = vec![];
        for c in chunks { p.push(c, &mut |d, _, _| got.push(d.to_string())); }
        p.end(&mut |d, _, _| got.push(d.to_string()));
        (p, got)
    }
    #[test] fn normal() {
        let (p, got) = run(&[b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"}}],\"usage\":{\"completion_tokens\":7}}\n\ndata: [DONE]\n\n"], false);
        assert_eq!(got, ["a", "b"]); assert_eq!(p.verdict(), Verdict::Ok); assert_eq!(p.tokens(), Value::from(7));
    }
    #[test] fn crlf_and_split_everywhere() {
        let s = b"data: {\"choices\":[{\"delta\":{\"content\":\"h\xc3\xa9\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n";
        let one_by_one: Vec<&[u8]> = s.chunks(1).collect();
        let (p, got) = run(&one_by_one, false);
        assert_eq!(got, ["hé"]); assert_eq!(p.verdict(), Verdict::Ok);
    }
    #[test] fn done_without_trailing_newline_is_unfinished_like_js() {
        let (p, _) = run(&[b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: [DONE]"], false);
        assert_eq!(p.verdict(), Verdict::Unfinished);
        let (p, _) = run(&[b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: [DONE]"], true);
        assert_eq!(p.verdict(), Verdict::Ok);
    }
    #[test] fn empty_data_and_null_are_malformed() {
        assert!(run(&[b"data:\n"], false).0.malformed);
        assert!(run(&[b"data: null\n"], false).0.malformed);
        assert!(!run(&[b"data: 123\ndata: \"x\"\ndata: []\ndata: true\n"], false).0.malformed);
        assert!(!run(&[b": keep-alive\nevent: ping\n\n"], false).0.malformed);
    }
    #[test] fn lone_cr_is_not_a_terminator() {
        let (p, got) = run(&[b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\rdata: [DONE]\r\n"], false);
        assert!(got.is_empty()); assert!(p.malformed);
    }
    #[test] fn js_trim_set() {
        assert_eq!(js_trim("\u{feff} x \u{a0}\u{2028}"), "x");
        assert_eq!(js_trim("\u{85}x\u{85}"), "\u{85}x\u{85}"); // NEL: Rust trims it, JS does not
        assert_eq!("\u{85}x".trim(), "x");
    }
    #[test] fn utf8_matches_textdecoder() {
        let mut d = Utf8Stream::default(); let mut o = String::new();
        d.push(&[0xEF, 0xBB], &mut o); d.push(&[0xBF, b'a', 0xFF, 0xE2, 0x82], &mut o); d.push(&[0xAC, 0xF0, 0x9F], &mut o);
        assert_eq!(o, "a\u{FFFD}\u{20AC}"); // BOM stripped, FF -> FFFD, trailing incomplete F0 9F held back
    }
    #[test] fn after_done_still_processed_and_last_usage_wins() {
        let (p, got) = run(&[b"data: [DONE]\ndata: {\"choices\":[{\"delta\":{\"content\":\"z\"}}],\"usage\":{\"completion_tokens\":1}}\ndata: {\"usage\":{\"completion_tokens\":2},\"timings\":{\"predicted_per_second\":3.5,\"predicted_n\":9}}\n"], false);
        assert_eq!(got, ["z"]); assert!(p.finished); assert_eq!(p.tokens(), Value::from(2)); assert_eq!(p.tps(), Some(Value::from(3.5)));
    }
}
