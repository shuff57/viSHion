//! Native transport: the request runs on its own thread (ureq is blocking) and the caller polls a channel,
//! so Stop and the timeout are noticed within 100 ms even while the model is still reading the page.
use crate::sse::SseParser;
use crate::{body, http_error, tokens, unreachable_message, verdict_error, Cancel, Done, Error, Progress, Request};
use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

enum Msg {
    Bytes(Vec<u8>),
    End,
    Failed(Error),
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

/// `O.transcribe` minus demo mode. Blocks the calling thread (the host's worker).
pub async fn transcribe(req: Request<'_>, cancel: &Cancel, on: &mut dyn FnMut(Progress)) -> Result<Done, Error> {
    let t0 = Instant::now();
    on(Progress::Status("reading document\u{2026}"));
    let (tx, rx) = mpsc::channel();
    let payload = body(req.model, req.image, req.prompt);
    let origin = req.endpoint.base_url.clone();
    let (url, key, secs) = (req.endpoint.chat_completions_url(), req.api_key.to_string(), req.timeout_secs);
    std::thread::spawn(move || run(url, key, payload, origin, secs, tx));

    let deadline = t0 + Duration::from_secs(req.timeout_secs.max(1));
    let mut p = SseParser::new();
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
            Ok(Msg::Bytes(b)) => p.push(&b, &mut |chunk, full, n| {
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
        });
        format!("http://{addr}")
    }

    fn req<'a>(url: &str) -> Request<'a> {
        Request { endpoint: Endpoint::new(url), api_key: "", model: "", prompt: "p", image: b"x", timeout_secs: 5 }
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
}
