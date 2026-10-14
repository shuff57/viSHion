//! Browser transport: `fetch` + a byte stream into the same `SseParser`. Stop and the timeout are checked
//! between chunks (an in-flight request that has sent no byte yet ends when the server answers or the
//! browser gives up; ponytail: wire an AbortController if that wait is ever long in practice).
use crate::slots;
use crate::sse::SseParser;
use crate::{body, http_error, tokens, unreachable_message, verdict_error, Cancel, Done, Error, Progress, Request};
use futures_util::future::{select, Either};
use futures_util::StreamExt;
use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, RequestInit, Response};

fn now() -> f64 {
    js_sys::Date::now()
}

async fn sleep(ms: i32) {
    let global = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
    let p = js_sys::Promise::new(&mut |resolve, _| {
        let _ = global.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(p).await;
}

/// One `GET /slots`; `None` for anything but a prompt being read (the endpoint is optional).
async fn slots_percent(url: &str) -> Option<u32> {
    let global = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
    let res: Response = JsFuture::from(global.fetch_with_str(url)).await.ok()?.unchecked_into();
    if !res.ok() {
        return None;
    }
    let text = JsFuture::from(res.text().ok()?).await.ok()?.as_string()?;
    slots::reading_percent(&text)
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
    // While the answer has not started, ask /slots every 400 ms how far the model has read the picture.
    let mut fetch = Box::pin(JsFuture::from(global.fetch_with_request(&request)));
    let poll = req.poll_slots && req.endpoint.serves_slots();
    let slots_url = req.endpoint.slots_url();
    let res: Response = loop {
        if !poll {
            break fetch.await.map_err(|_| network())?.unchecked_into();
        }
        match select(fetch, Box::pin(sleep(slots::EVERY.as_millis() as i32))).await {
            Either::Left((res, _)) => break res.map_err(|_| network())?.unchecked_into(),
            Either::Right((_, pending)) => {
                fetch = pending;
                if cancel.is_set() {
                    return Err(Error::Aborted);
                }
                if let Some(pct) = slots_percent(&slots_url).await {
                    on(Progress::Status(&slots::status(pct)));
                }
            }
        }
    };
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
