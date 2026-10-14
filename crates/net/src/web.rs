//! Browser transport: `fetch` + a byte stream into the same `SseParser`. Stop and the timeout are checked
//! between chunks (an in-flight request that has sent no byte yet ends when the server answers or the
//! browser gives up; ponytail: wire an AbortController if that wait is ever long in practice).
use crate::sse::SseParser;
use crate::{body, http_error, tokens, unreachable_message, verdict_error, Cancel, Done, Error, Progress, Request};
use futures_util::StreamExt;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, RequestInit, Response};

fn now() -> f64 {
    js_sys::Date::now()
}

pub async fn transcribe(req: Request<'_>, cancel: &Cancel, on: &mut dyn FnMut(Progress)) -> Result<Done, Error> {
    let t0 = now();
    on(Progress::Status("reading document\u{2026}"));
    let network = || Error::Network(unreachable_message(&req.endpoint.base_url));
    let headers = Headers::new().map_err(|_| network())?;
    let _ = headers.set("Content-Type", "application/json");
    if !req.api_key.is_empty() {
        let _ = headers.set("Authorization", &format!("Bearer {}", req.api_key));
    }
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_headers(&headers);
    init.set_body(&wasm_bindgen::JsValue::from_str(&body(req.model, req.image, req.prompt)));
    let request = web_sys::Request::new_with_str_and_init(&req.endpoint.chat_completions_url(), &init).map_err(|_| network())?;
    let global = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
    let res: Response = JsFuture::from(global.fetch_with_request(&request)).await.map_err(|_| network())?.unchecked_into();
    if cancel.is_set() {
        return Err(Error::Aborted);
    }
    if !res.ok() {
        let text = match res.text() {
            Ok(p) => JsFuture::from(p).await.ok().and_then(|v| v.as_string()).unwrap_or_default(),
            Err(_) => String::new(),
        };
        return Err(http_error(res.status(), &text));
    }
    let Some(stream) = res.body() else { return Err(network()) };
    let mut stream = wasm_streams::ReadableStream::from_raw(stream.unchecked_into()).into_stream();
    let mut p = SseParser::new();
    let limit = req.timeout_secs.max(1) as f64 * 1000.0;
    while let Some(item) = stream.next().await {
        if cancel.is_set() {
            return Err(Error::Aborted); // dropping `stream` cancels the body
        }
        if now() - t0 > limit {
            return Err(Error::TimedOut);
        }
        let Ok(v) = item else {
            return Err(Error::Stream { message: "The connection to the OCR server dropped mid-page.".into(), partial: p.text });
        };
        p.push(&v.unchecked_into::<Uint8Array>().to_vec(), &mut |chunk, full, n| {
            on(Progress::Text { chunk, full });
            on(Progress::Status(&format!("writing\u{2026} {n} tok")));
        });
    }
    p.end(&mut |_, _, _| {});
    if let Some(e) = verdict_error(&p) {
        return Err(e);
    }
    Ok(Done { tokens: tokens(&p), ms: (now() - t0) as u32, text: p.text })
}
