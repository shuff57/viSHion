//! Browser shell around `sse::SseParser`: fetch + wasm-streams + AbortSignal. Mirrors O.transcribe in js/ocr.js.
use crate::sse::{SseParser, Verdict};
use futures_util::StreamExt;
use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_streams::ReadableStream;
use web_sys::{AbortSignal, Headers, Request, RequestInit, Response};

fn set(o: &Object, k: &str, v: impl Into<JsValue>) { let _ = Reflect::set(o, &JsValue::from_str(k), &v.into()); }
fn name_of(e: &JsValue) -> String {
    Reflect::get(e, &JsValue::from_str("name")).ok().and_then(|n| n.as_string()).unwrap_or_default()
}
fn is_abort(e: &JsValue) -> bool { name_of(e) == "AbortError" }
/// `Object.assign(new Error(msg), { kind }, extra)`
fn fail(kind: &str, msg: &str, partial: Option<&str>) -> JsValue {
    let e = js_sys::Error::new(msg);
    set(&e, "kind", kind);
    if let Some(p) = partial { set(&e, "partial", p); }
    e.into()
}
fn emit(cb: &js_sys::Function, build: impl FnOnce(&Object)) -> Result<(), JsValue> {
    let o = Object::new();
    build(&o);
    cb.call1(&JsValue::NULL, &o).map(|_| ())
}
fn global_fetch(req: &Request) -> js_sys::Promise {
    // `fetch` lives on Window, WorkerGlobalScope and Node's globalThis alike; web_sys::window() is None
    // outside a document, so go through the global object (works in window, worker and Node).
    js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>().fetch_with_request(req)
}

/// Same contract as `OCR.ocr.transcribe`'s network half. `on_event` receives
/// `{type:'status', message}` and `{type:'text', chunk, full}`. Resolves to `{text, tokens, ms, tps}` (raw,
/// un-cleaned text); rejects with an Error carrying `.kind` (`network|unavailable|http|stream`) and `.partial`,
/// or the original AbortError.
#[wasm_bindgen]
pub async fn transcribe(origin: String, body_json: String, api_key: String, signal: Option<AbortSignal>, on_event: js_sys::Function, flush_tail: bool) -> Result<JsValue, JsValue> {
    let t0 = js_sys::Date::now();
    let _ = emit(&on_event, |o| { set(o, "type", "status"); set(o, "message", "reading document\u{2026}"); });

    let headers = Headers::new()?;
    headers.set("Content-Type", "application/json")?;
    if !api_key.is_empty() { headers.set("Authorization", &format!("Bearer {api_key}"))?; }
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_headers(&headers);
    init.set_body(&JsValue::from_str(&body_json));
    if let Some(s) = &signal { init.set_signal(Some(s)); }
    let url = format!("{}/v1/chat/completions", origin.trim_end_matches('/'));
    let req = Request::new_with_str_and_init(&url, &init)?;

    let res: Response = match JsFuture::from(global_fetch(&req)).await {
        Ok(r) => r.unchecked_into(),
        Err(e) => {
            if is_abort(&e) { return Err(e); }
            return Err(fail("network", &format!("Cannot reach OCR server at {}. Is llama-server running with CORS enabled?", origin.trim_end_matches('/')), None));
        }
    };
    if !res.ok() {
        // keep the detail as a JS string: slice(0,160) counts UTF-16 units and may cut an emoji in half,
        // leaving a lone surrogate that a Rust String cannot hold (the JS client does exactly that)
        let detail: js_sys::JsString = match res.text() {
            Ok(p) => match JsFuture::from(p).await { Ok(v) if v.is_string() => v.unchecked_into::<js_sys::JsString>().slice(0, 160), _ => js_sys::JsString::from("") },
            Err(_) => js_sys::JsString::from(""),
        };
        let (kind, prefix) = match res.status() {
            503 => ("unavailable".to_string(), "The OCR model is not loaded yet. ".to_string()),
            n => ("http".to_string(), format!("OCR server returned {n}. ")),
        };
        let msg = js_sys::JsString::from(prefix).concat(&detail);
        let e = js_sys::Error::new("");
        set(&e, "message", msg);
        set(&e, "kind", kind.as_str());
        return Err(e.into());
    }

    let body = res.body().ok_or_else(|| JsValue::from(js_sys::TypeError::new("response has no body")))?;
    let mut stream = ReadableStream::from_raw(body.unchecked_into()).into_stream();
    let mut p = SseParser::new();
    p.flush_tail = flush_tail;
    let mut cb_err: Option<JsValue> = None;
    let mut read_err: Option<JsValue> = None;
    'read: while let Some(item) = stream.next().await {
        match item {
            Err(e) => { read_err = Some(e); break 'read; }
            Ok(v) => {
                let bytes = v.unchecked_into::<Uint8Array>().to_vec();
                p.push(&bytes, &mut |delta, full, n| {
                    if cb_err.is_some() { return; }
                    // a throwing callback lands in the same try/catch as the read loop in the JS
                    if let Err(e) = emit(&on_event, |o| { set(o, "type", "text"); set(o, "chunk", delta); set(o, "full", full); }) { cb_err = Some(e); return; }
                    if let Err(e) = emit(&on_event, |o| { set(o, "type", "status"); set(o, "message", format!("writing\u{2026} {n} tok")); }) { cb_err = Some(e); }
                });
                if cb_err.is_some() { read_err = cb_err.take(); break 'read; }
            }
        }
    }
    if let Some(e) = read_err {
        drop(stream); // IntoStream cancels the underlying body on drop (the JS leaves the reader dangling)
        if is_abort(&e) { return Err(e); }
        return Err(fail("stream", "The connection to the OCR server dropped mid-page.", Some(&p.text)));
    }
    p.end(&mut |_, _, _| {});
    match p.verdict() {
        Verdict::Ok => {}
        Verdict::Malformed => return Err(fail("stream", "The OCR server sent an unreadable response.", Some(&p.text))),
        Verdict::Unfinished => return Err(fail("stream", "The OCR server stopped before finishing this page.", Some(&p.text))),
    }
    let out = Object::new();
    let js_json = |v: &serde_json::Value| js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL);
    set(&out, "text", p.text.as_str());
    set(&out, "tokens", js_json(&p.tokens()));
    set(&out, "ms", js_sys::Date::now() - t0);
    set(&out, "tps", p.tps().map(|v| js_json(&v)).unwrap_or(JsValue::NULL));
    Ok(out.into())
}
