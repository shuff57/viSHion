//! Network edge: the OCR client of js/ocr.js (`O.transcribe`). `sse` is the pure parser (bytes in, text
//! out); `native` (ureq, blocking) and `web` (fetch) only move bytes. Both expose the same
//! `async fn transcribe`.

pub mod models;
pub mod provider;
pub mod slots;
pub mod sse;

pub use provider::{Mock, Provider};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{complete, complete_opts, get, transcribe};
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{complete, complete_opts, get, transcribe};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Where a chat-completions endpoint lives. `connect-src` in the CSP already allows https and localhost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
}

impl Endpoint {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into() }
    }

    /// llama.cpp's progress endpoint, polled while a page is read.
    pub fn slots_url(&self) -> String {
        format!("{}/slots", self.base_url.trim_end_matches('/'))
    }

    /// js/ocr.js asks `/slots` of llama.cpp only, never of hosted endpoints: those are https, a llama-server
    /// on this machine or the local network is not.
    pub fn serves_slots(&self) -> bool {
        self.base_url.starts_with("http://")
    }

    /// js/ocr.js posts to `origin + '/v1/chat/completions'`, origin without trailing slashes.
    pub fn chat_completions_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

/// An answer to a plain `GET`: the status and the text, whatever the status was.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: String,
}

/// One page to read.
pub struct Request<'a> {
    pub endpoint: Endpoint,
    pub api_key: &'a str,
    pub model: &'a str,
    pub prompt: &'a str,
    /// The stored page picture (JPEG).
    pub image: &'a [u8],
    pub timeout_secs: u64,
    /// Ask `/slots` how far the model is while it reads (llama.cpp only).
    pub poll_slots: bool,
}

/// What the page showed while it was read.
pub enum Progress<'a> {
    Status(&'a str),
    /// A new piece of text and everything so far.
    Text { chunk: &'a str, full: &'a str },
}

#[derive(Debug)]
pub struct Done {
    /// Raw model output; run `vishion_core::clean::clean` over it.
    pub text: String,
    pub tokens: u32,
    pub ms: u32,
}

/// `queue.js` outcomes: `Aborted` is a person pressing Stop, `Network`/`Unavailable` pause the run, the rest
/// fail the page (`Stream` keeps what was read).
#[derive(Debug, PartialEq)]
pub enum Error {
    Aborted,
    TimedOut,
    Network(String),
    Unavailable(String),
    Http(String),
    Stream { message: String, partial: String },
}

/// Set from any thread to stop the page being read (the in-flight request is dropped).
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The chat-completions body of `O.transcribe`.
pub fn body(model: &str, image_jpeg: &[u8], prompt: &str) -> String {
    use base64::Engine;
    let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(image_jpeg));
    serde_json::json!({
        "model": if model.is_empty() { "ocr" } else { model },
        "stream": true, "temperature": 0, "max_tokens": 2000, "stream_options": {"include_usage": true},
        "messages": [{"role": "user", "content": [
            {"type": "image_url", "image_url": {"url": url}},
            {"type": "text", "text": prompt}]}]
    })
    .to_string()
}

/// The body of js/guess.js `ask`: one answer, not streamed, a little warmer than reading, 500 tokens at most.
pub fn complete_body(model: &str, image_jpeg: &[u8], prompt: &str) -> String {
    complete_body_with(model, image_jpeg, prompt, 0.2, 500)
}

/// [`complete_body`] with the temperature and the answer length of the caller (the name reader: 0.1, 60).
pub fn complete_body_with(model: &str, image_jpeg: &[u8], prompt: &str, temperature: f64, max_tokens: u32) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&body(model, image_jpeg, prompt)).expect("body is json");
    let o = v.as_object_mut().expect("object");
    o.insert("stream".into(), false.into());
    o.remove("stream_options");
    o.insert("temperature".into(), temperature.into());
    o.insert("max_tokens".into(), max_tokens.into());
    v.to_string()
}

/// `j.choices[0].message.content`, or empty.
pub(crate) fn content_of(reply: &str) -> String {
    serde_json::from_str::<serde_json::Value>(reply)
        .ok()
        .and_then(|j| j["choices"][0]["message"]["content"].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub(crate) fn unreachable_message(origin: &str) -> String {
    format!("Cannot reach OCR server at {}. Is llama-server running with CORS enabled?", origin.trim_end_matches('/'))
}

/// First 160 UTF-16 units, like `text.slice(0, 160)`; a Rust string cannot end in half a pair, so it cuts
/// before one.
pub(crate) fn detail(s: &str) -> String {
    let mut n = 0;
    let mut out = String::new();
    for c in s.chars() {
        n += c.len_utf16();
        if n > 160 {
            break;
        }
        out.push(c);
    }
    out
}

pub(crate) fn http_error(status: u16, body: &str) -> Error {
    let d = detail(body);
    if status == 503 {
        Error::Unavailable(format!("The OCR model is not loaded yet. {d}"))
    } else {
        Error::Http(format!("OCR server returned {status}. {d}"))
    }
}

pub(crate) fn verdict_error(p: &sse::SseParser) -> Option<Error> {
    let message = match p.verdict() {
        sse::Verdict::Ok => return None,
        sse::Verdict::Malformed => "The OCR server sent an unreadable response.",
        sse::Verdict::Unfinished => "The OCR server stopped before finishing this page.",
    };
    Some(Error::Stream { message: message.into(), partial: p.text.clone() })
}

pub(crate) fn tokens(p: &sse::SseParser) -> u32 {
    p.tokens().as_u64().unwrap_or(p.chunks as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_join_ignores_trailing_slash() {
        assert_eq!(Endpoint::new("http://localhost:1234/").chat_completions_url(), "http://localhost:1234/v1/chat/completions");
    }

    #[test]
    fn body_has_the_image_and_prompt() {
        let v: serde_json::Value = serde_json::from_str(&body("", b"\xff\xd8", "Read.")).unwrap();
        assert_eq!(v["model"], "ocr");
        assert_eq!(v["stream"], true);
        let c = &v["messages"][0]["content"];
        assert_eq!(c[0]["image_url"]["url"], "data:image/jpeg;base64,/9g=");
        assert_eq!(c[1]["text"], "Read.");
    }

    #[test]
    fn complete_body_is_one_warm_short_answer() {
        let v: serde_json::Value = serde_json::from_str(&complete_body("m", b"x", "Q")).unwrap();
        assert_eq!((v["stream"].as_bool(), v["max_tokens"].as_u64(), v["temperature"].as_f64()), (Some(false), Some(500), Some(0.2)));
        assert!(v.get("stream_options").is_none());
        assert_eq!(v["model"], "m");
    }

    #[test]
    fn content_of_takes_the_first_choice() {
        assert_eq!(content_of(r#"{"choices":[{"message":{"content":"[]"}}]}"#), "[]");
        assert_eq!(content_of("nope"), "");
    }

    #[test]
    fn http_errors_follow_queue_kinds() {
        assert!(matches!(http_error(503, "loading"), Error::Unavailable(m) if m == "The OCR model is not loaded yet. loading"));
        assert!(matches!(http_error(401, "no"), Error::Http(m) if m == "OCR server returned 401. no"));
    }
}
