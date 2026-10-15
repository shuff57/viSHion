//! Native transport: the request runs on its own thread (ureq is blocking) and the caller polls a channel,
//! so Stop and the timeout are noticed within 100 ms even while the model is still reading the page.
use crate::sse::SseParser;
use crate::slots;
use crate::{complete_body_with, content_of, body, http_error, tokens, unreachable_message, verdict_error, Cancel, Done, Error, Progress, Request};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

struct StopOnDrop(Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

enum Msg {
    Bytes(Vec<u8>),
    End,
    Failed(Error),
    /// `/slots`: how much of the picture the model has read.
    Slots(u32),
    /// The connection dropped mid-stream.
    Dropped,
}

fn run(url: String, key: String, payload: String, origin: String, secs: u64, tx: mpsc::Sender<Msg>) {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(secs.max(1)))
        .build();
    let mut req = agent.post(&url).set("Content-Type", "application/json");
    if !key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    let res = match req.send_string(&payload) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let mut s = String::new();
            let _ = r.into_reader().take(4096).read_to_string(&mut s);
            let _ = tx.send(Msg::Failed(http_error(code, &s)));
            return;
        }
        Err(_) => {
            let _ = tx.send(Msg::Failed(Error::Network(unreachable_message(&origin))));
            return;
        }
    };
    let mut r = res.into_reader();
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(Msg::End);
                return;
            }
            Ok(n) => {
                if tx.send(Msg::Bytes(buf[..n].to_vec())).is_err() {
                    return; // the caller gave up (stopped or timed out)
                }
            }
            Err(_) => {
                let _ = tx.send(Msg::Dropped);
                return;
            }
        }
    }
}

/// Polls `/slots` until `stop` is set; every failure is ignored, as in js/ocr.js (`/slots` is optional).
fn poll(url: String, stop: Arc<AtomicBool>, tx: mpsc::Sender<Msg>) {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build();
    while !stop.load(Ordering::SeqCst) {
        if let Ok(r) = agent.get(&url).call() {
            let mut s = String::new();
            if r.into_reader().take(1 << 16).read_to_string(&mut s).is_ok() {
                if let Some(p) = slots::reading_percent(&s) {
                    if tx.send(Msg::Slots(p)).is_err() {
                        return;
                    }
                }
            }
        }
        std::thread::sleep(slots::EVERY);
    }
}

/// `GET url` with the API key as a bearer token. Any HTTP status is a `Reply`; only a connection that could not
/// be made is an error. Blocks the calling thread for at most `timeout_secs`.
pub async fn get(url: &str, api_key: &str, timeout_secs: u64) -> Result<crate::Reply, Error> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout_secs.max(1))).build();
    let mut r = agent.get(url);
    if !api_key.is_empty() {
        r = r.set("Authorization", &format!("Bearer {api_key}"));
    }
    match r.call() {
        Ok(res) => {
            let status = res.status();
            let mut s = String::new();
            let _ = res.into_reader().take(1 << 20).read_to_string(&mut s);
            Ok(crate::Reply { status, body: s })
        }
        Err(ureq::Error::Status(status, res)) => {
            let mut s = String::new();
            let _ = res.into_reader().take(1 << 16).read_to_string(&mut s);
            Ok(crate::Reply { status, body: s })
        }
        Err(_) => Err(Error::Network(format!("Can\u{2019}t reach {url}."))),
    }
}

/// `POST url` with a JSON body (the Ollama `/api/show` question). Any HTTP status is a `Reply`.
pub async fn post_json(url: &str, api_key: &str, body: &str, timeout_secs: u64) -> Result<crate::Reply, Error> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout_secs.max(1))).build();
    let mut r = agent.post(url).set("Content-Type", "application/json");
    if !api_key.is_empty() {
        r = r.set("Authorization", &format!("Bearer {api_key}"));
    }
    match r.send_string(body) {
        Ok(res) => {
            let status = res.status();
            let mut s = String::new();
            let _ = res.into_reader().take(1 << 20).read_to_string(&mut s);
            Ok(crate::Reply { status, body: s })
        }
        Err(ureq::Error::Status(status, res)) => {
            let mut s = String::new();
            let _ = res.into_reader().take(1 << 16).read_to_string(&mut s);
            Ok(crate::Reply { status, body: s })
        }
        Err(_) => Err(Error::Network(format!("Can\u{2019}t reach {url}."))),
    }
}

/// One non-streamed answer (`W.guess`): the model's text. Blocks the calling thread (the host's worker),
/// for at most `timeout_secs`.
pub async fn complete(req: Request<'_>) -> Result<String, Error> {
    complete_opts(req, 0.2, 500).await
}

/// [`complete`] with the temperature and answer length of the caller.
pub async fn complete_opts(req: Request<'_>, temperature: f64, max_tokens: u32) -> Result<String, Error> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(req.timeout_secs.max(1)))
        .build();
    let mut r = agent.post(&req.endpoint.chat_completions_url()).set("Content-Type", "application/json");
    if !req.api_key.is_empty() {
        r = r.set("Authorization", &format!("Bearer {}", req.api_key));
    }
    match r.send_string(&complete_body_with(req.model, req.image, req.prompt, temperature, max_tokens)) {
        Ok(res) => res.into_string().map(|t| content_of(&t)).map_err(|_| Error::Network(unreachable_message(&req.endpoint.base_url))),
        Err(ureq::Error::Status(code, res)) => {
            let mut s = String::new();
            let _ = res.into_reader().take(4096).read_to_string(&mut s);
            Err(http_error(code, &s))
        }
        Err(_) => Err(Error::Network(unreachable_message(&req.endpoint.base_url))),
    }
}

/// `O.transcribe` minus demo mode. Blocks the calling thread (the host's worker).
pub async fn transcribe(req: Request<'_>, cancel: &Cancel, on: &mut dyn FnMut(Progress)) -> Result<Done, Error> {
    let t0 = Instant::now();
    on(Progress::Status("reading document\u{2026}"));
    let (tx, rx) = mpsc::channel();
    let payload = body(req.model, req.image, req.prompt);
    let origin = req.endpoint.base_url.clone();
    let (url, key, secs) = (req.endpoint.chat_completions_url(), req.api_key.to_string(), req.timeout_secs);
    let stop_poll = Arc::new(AtomicBool::new(false));
    if req.poll_slots && req.endpoint.serves_slots() {
        let (u, st, t) = (req.endpoint.slots_url(), stop_poll.clone(), tx.clone());
        std::thread::spawn(move || poll(u, st, t));
    }
    let _stop = StopOnDrop(stop_poll);
    std::thread::spawn(move || run(url, key, payload, origin, secs, tx));

    let deadline = t0 + Duration::from_secs(req.timeout_secs.max(1));
    let mut p = SseParser::new();
    let mut writing = false;
    loop {
        if cancel.is_set() {
            return Err(Error::Aborted);
        }
        if Instant::now() >= deadline {
            return Err(Error::TimedOut);
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) | Ok(Msg::Dropped) => {
                return Err(Error::Stream { message: "The connection to the OCR server dropped mid-page.".into(), partial: p.text });
            }
            Ok(Msg::Failed(e)) => return Err(e),
            Ok(Msg::Slots(pct)) => {
                if !writing {
                    on(Progress::Status(&slots::status(pct)));
                }
            }
            Ok(Msg::Bytes(b)) => p.push(&b, &mut |chunk, full, n| {
                writing = true;
                on(Progress::Text { chunk, full });
                on(Progress::Status(&format!("writing\u{2026} {n} tok")));
            }),
            Ok(Msg::End) => {
                p.end(&mut |_, _, _| {});
                if let Some(e) = verdict_error(&p) {
                    return Err(e);
                }
                let ms = t0.elapsed().as_millis() as u32;
                return Ok(Done { tokens: tokens(&p), text: p.text, ms });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Endpoint;
    use std::io::{Read as _, Write};
    use std::net::TcpListener;

    fn pending<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// A one-shot server: reads the request, writes `reply` (split in two with a pause), closes.
    fn serve(reply: &'static [u8]) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 65536];
            let _ = s.read(&mut b);
            let mid = reply.len() / 2;
            s.write_all(&reply[..mid]).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            s.write_all(&reply[mid..]).unwrap();
            // Take the rest of the request before closing: closing with unread bytes sends a reset, which
            // the client sees as a dropped connection.
            let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
            while matches!(s.read(&mut b), Ok(n) if n > 0) {}
        });
        format!("http://{addr}")
    }

    fn req<'a>(url: &str) -> Request<'a> {
        Request { endpoint: Endpoint::new(url), api_key: "", model: "", prompt: "p", image: b"x", timeout_secs: 5, poll_slots: false }
    }

    #[test]
    fn streams_text_and_counts_tokens() {
        let url = serve(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"usage\":{\"completion_tokens\":2}}\n\ndata: [DONE]\n\n");
        let mut seen = vec![];
        let r = pending(transcribe(req(&url), &Cancel::default(), &mut |p| {
            if let Progress::Text { full, .. } = p {
                seen.push(full.to_string());
            }
        }))
        .unwrap();
        assert_eq!((r.text.as_str(), r.tokens), ("Hello", 2));
        assert_eq!(seen, ["Hel", "Hello"]);
    }

    #[test]
    fn a_stream_without_done_fails_and_keeps_the_partial() {
        let url = serve(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n");
        let e = pending(transcribe(req(&url), &Cancel::default(), &mut |_| {})).unwrap_err();
        assert!(matches!(e, Error::Stream { partial, .. } if partial == "Hi"));
    }

    #[test]
    fn http_503_is_unavailable_and_a_dead_port_is_network() {
        let url = serve(b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 7\r\n\r\nloading");
        let e = pending(transcribe(req(&url), &Cancel::default(), &mut |_| {})).unwrap_err();
        assert_eq!(e, Error::Unavailable("The OCR model is not loaded yet. loading".into()));
        let e = pending(transcribe(req("http://127.0.0.1:1"), &Cancel::default(), &mut |_| {})).unwrap_err();
        assert!(matches!(e, Error::Network(m) if m.contains("Cannot reach OCR server")));
    }

    #[test]
    fn complete_returns_the_message_content() {
        let j = r#"{"choices":[{"message":{"content":"[{\"n\":1}]"}}]}"#;
        let reply = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{j}", j.len());
        let url = serve(Box::leak(reply.into_boxed_str()).as_bytes());
        let t = pending(complete(req(&url))).unwrap();
        assert_eq!(t, r#"[{"n":1}]"#);
    }

    #[test]
    fn stop_wins_over_a_silent_server() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let c = Cancel::default();
        let c2 = c.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.stop();
        });
        let e = pending(transcribe(req(&url), &c, &mut |_| {})).unwrap_err();
        assert_eq!(e, Error::Aborted);
        drop(l);
    }

    #[test]
    fn slots_progress_shows_while_the_model_reads() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for conn in l.incoming() {
                let Ok(mut s) = conn else { return };
                std::thread::spawn(move || {
                    let mut b = vec![0u8; 65536];
                    let n = s.read(&mut b).unwrap_or(0);
                    let head = String::from_utf8_lossy(&b[..n]).to_string();
                    if head.starts_with("GET /slots") {
                        let j = r#"[{"is_processing":true,"n_prompt_tokens":100,"n_prompt_tokens_processed":40}]"#;
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{j}", j.len());
                    } else {
                        std::thread::sleep(Duration::from_millis(900));
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: [DONE]\n\n");
                        let _ = s.set_read_timeout(Some(Duration::from_millis(100)));
                        while matches!(s.read(&mut b), Ok(n) if n > 0) {}
                    }
                });
            }
        });
        let mut statuses = vec![];
        let mut r = req(&url);
        r.poll_slots = true;
        let d = pending(transcribe(r, &Cancel::default(), &mut |p| {
            if let Progress::Status(s) = p {
                statuses.push(s.to_string());
            }
        }))
        .unwrap();
        assert_eq!(d.text, "Hi");
        assert!(statuses.iter().any(|s| s == "reading document\u{2026} 40%"), "{statuses:?}");
    }
}
