//! Who answers a page: the seam between the app and a model API. `OpenAiCompatible` is the chat-completions
//! client every provider of the settings speaks (a local llama.cpp or Ollama, Ollama Cloud, OpenRouter, a
//! custom endpoint); `Mock` answers from memory, for tests and for trying the seam. The settings pick one by
//! id (`Provider::for_setting`), and the rest of the app only calls the methods here.
use crate::{Cancel, Done, Error, Progress, Request};

/// What a mock provider says.
#[derive(Debug, Clone, PartialEq)]
pub struct Mock {
    /// The transcript `transcribe` returns (streamed word by word).
    pub text: String,
    /// The answer of `complete` (a name reply, a guess list).
    pub reply: String,
    /// When set, every call fails with this network message.
    pub fail: Option<String>,
}

impl Default for Mock {
    fn default() -> Self {
        Mock { text: "A mock transcript.".into(), reply: "[]".into(), fail: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    OpenAiCompatible,
    Mock(Mock),
}

impl Provider {
    /// The provider for a `settings.provider` value. Every provider the app lists speaks chat completions;
    /// `mock` is the in-memory one.
    pub fn for_setting(id: &str) -> Provider {
        match id {
            "mock" => Provider::Mock(Mock::default()),
            _ => Provider::OpenAiCompatible,
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            Provider::OpenAiCompatible => "openai-compatible",
            Provider::Mock(_) => "mock",
        }
    }

    /// Whether `/slots` can be asked how far the model has read (llama.cpp only).
    pub fn polls_slots(&self) -> bool {
        matches!(self, Provider::OpenAiCompatible)
    }

    /// Read one page, streaming the text to `on`.
    pub async fn transcribe(&self, req: Request<'_>, cancel: &Cancel, on: &mut dyn FnMut(Progress)) -> Result<Done, Error> {
        match self {
            Provider::OpenAiCompatible => crate::transcribe(req, cancel, on).await,
            Provider::Mock(m) => {
                if let Some(msg) = &m.fail {
                    return Err(Error::Network(msg.clone()));
                }
                let mut full = String::new();
                let mut n = 0u32;
                for part in m.text.split_inclusive(' ') {
                    if cancel.is_set() {
                        return Err(Error::Aborted);
                    }
                    full.push_str(part);
                    n += 1;
                    on(Progress::Text { chunk: part, full: &full });
                }
                Ok(Done { text: full, tokens: n, ms: 0 })
            }
        }
    }

    /// A plain `GET` (the model list, the health check).
    pub async fn get(&self, url: &str, api_key: &str, timeout_secs: u64) -> Result<crate::Reply, Error> {
        match self {
            Provider::OpenAiCompatible => crate::get(url, api_key, timeout_secs).await,
            Provider::Mock(m) => match &m.fail {
                Some(msg) => Err(Error::Network(msg.clone())),
                None if url.ends_with("/health") => Ok(crate::Reply { status: 200, body: "ok".into() }),
                None => Ok(crate::Reply { status: 200, body: "{\"data\":[{\"id\":\"mock-vision\"}]}".into() }),
            },
        }
    }

    /// A `POST` with a JSON body.
    pub async fn post_json(&self, url: &str, api_key: &str, body: &str, timeout_secs: u64) -> Result<crate::Reply, Error> {
        match self {
            Provider::OpenAiCompatible => crate::post_json(url, api_key, body, timeout_secs).await,
            Provider::Mock(m) => match &m.fail {
                Some(msg) => Err(Error::Network(msg.clone())),
                None if url.ends_with("/v1/chat/completions") => {
                    Ok(crate::Reply { status: 200, body: "{\"choices\":[{\"message\":{\"content\":\"OK\"}}]}".into() })
                }
                None => Ok(crate::Reply { status: 200, body: "{\"capabilities\":[\"completion\",\"vision\"]}".into() }),
            },
        }
    }

    /// `M.probe`: one tiny picture and a one-word question to each model, with the key. A model that answers
    /// accepts images with this key; one that is refused is left out. `progress(done, total)` after each.
    pub async fn probe(&self, base: &str, api_key: &str, ids: &[String], progress: &dyn Fn(usize, usize)) -> (Vec<String>, Vec<(String, String)>) {
        const TINY: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAN0lEQVR4nO3RwQ0AMAjDwJT9d05HMB9+vgGCZF7bXJrT9XhgwR8gEyETIRMhEyETIRMhEyEThXzH8QM9OMM6fAAAAABJRU5ErkJggg==";
        let url = format!("{}/v1/chat/completions", base.trim_end_matches('/'));
        let (mut ok, mut failed) = (vec![], vec![]);
        for (i, id) in ids.iter().enumerate() {
            let body = serde_json::json!({
                "model": id, "stream": false, "max_tokens": 8, "temperature": 0,
                "messages": [{"role": "user", "content": [
                    {"type": "image_url", "image_url": {"url": TINY}},
                    {"type": "text", "text": "Reply with the single word OK."}]}]
            })
            .to_string();
            match self.post_json(&url, api_key, &body, 60).await {
                Ok(r) if r.status == 200 => {
                    let answered = serde_json::from_str::<serde_json::Value>(&r.body)
                        .ok()
                        .is_some_and(|v| v["choices"][0]["message"]["content"].is_string());
                    if answered {
                        ok.push(id.clone());
                    } else {
                        failed.push((id.clone(), "empty answer".to_string()));
                    }
                }
                Ok(r) => failed.push((id.clone(), format!("HTTP {}", r.status))),
                Err(_) => failed.push((id.clone(), "no connection".to_string())),
            }
            progress(i + 1, ids.len());
        }
        (ok, failed)
    }

    /// `M.list` (before the picker's tidying): the rows the server lists, each with what `/api/show` says about
    /// vision when the provider is a local Ollama. Errors are the sentences Settings shows.
    pub async fn list_models(&self, base: &str, api_key: &str, shows: bool, relay: bool) -> Result<Vec<(crate::models::ModelRow, Option<bool>)>, String> {
        let base = base.trim_end_matches('/');
        if base.is_empty() {
            return Err("Enter the server address first.".into());
        }
        let r = match self.get(&format!("{base}/v1/models"), api_key, 15).await {
            Ok(r) => r,
            Err(_) if relay => return Err("The relay is not running. Start it once with: node tools/relay.js".into()),
            Err(_) => return Err(format!("Can\u{2019}t reach {base}. Is it running, and does it allow this page (CORS)?")),
        };
        if r.status == 401 || r.status == 403 {
            return Err("The server did not accept the API key.".into());
        }
        if r.status != 200 {
            return Err(format!("The server answered with {}.", r.status));
        }
        let rows = crate::models::parse_rows(&r.body);
        let router = rows.iter().any(|m| m.router);
        let mut out = vec![];
        for m in rows {
            let mut shown = None;
            // a local Ollama can tell which of its models see images; llama.cpp has no /api/show
            if shows && !router {
                let body = serde_json::json!({ "model": m.id }).to_string();
                if let Ok(s) = self.post_json(&format!("{base}/api/show"), api_key, &body, 15).await {
                    if s.status == 200 {
                        if let Some(caps) = serde_json::from_str::<serde_json::Value>(&s.body).ok().and_then(|v| v.get("capabilities").and_then(|c| c.as_array().cloned())) {
                            shown = Some(caps.iter().any(|c| c.as_str() == Some("vision")));
                        }
                    }
                }
            }
            out.push((m, shown));
        }
        Ok(out)
    }

    /// One answer that is not streamed (name reading, guesses).
    pub async fn complete(&self, req: Request<'_>, temperature: f64, max_tokens: u32) -> Result<String, Error> {
        match self {
            Provider::OpenAiCompatible => crate::complete_opts(req, temperature, max_tokens).await,
            Provider::Mock(m) => match &m.fail {
                Some(msg) => Err(Error::Network(msg.clone())),
                None => Ok(m.reply.clone()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Endpoint;

    fn req<'a>() -> Request<'a> {
        Request { endpoint: Endpoint::new("http://unused"), api_key: "", model: "", prompt: "p", image: b"x", timeout_secs: 1, poll_slots: false }
    }

    fn pending<F: std::future::Future>(f: F) -> F::Output {
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    #[test]
    fn settings_pick_a_provider_by_id() {
        for id in ["local", "ollama-cloud", "openrouter", "custom", "", "something-new"] {
            assert_eq!(Provider::for_setting(id), Provider::OpenAiCompatible, "{id}");
        }
        assert_eq!(Provider::for_setting("mock").id(), "mock");
        assert!(Provider::OpenAiCompatible.polls_slots() && !Provider::for_setting("mock").polls_slots());
    }

    #[test]
    fn the_mock_streams_words_and_answers() {
        let p = Provider::Mock(Mock { text: "one two three".into(), reply: "[{\"n\":1}]".into(), fail: None });
        let mut seen = vec![];
        let d = pending(p.transcribe(req(), &Cancel::default(), &mut |e| {
            if let Progress::Text { full, .. } = e {
                seen.push(full.to_string());
            }
        }))
        .unwrap();
        assert_eq!((d.text.as_str(), d.tokens), ("one two three", 3));
        assert_eq!(seen, ["one ", "one two ", "one two three"]);
        assert_eq!(pending(p.complete(req(), 0.1, 60)).unwrap(), "[{\"n\":1}]");
    }

    #[test]
    fn the_mock_stops_and_fails_like_a_server() {
        let c = Cancel::default();
        c.stop();
        let p = Provider::for_setting("mock");
        assert_eq!(pending(p.transcribe(req(), &c, &mut |_| {})).unwrap_err(), Error::Aborted);
        let f = Provider::Mock(Mock { fail: Some("down".into()), ..Mock::default() });
        assert_eq!(pending(f.complete(req(), 0.1, 60)).unwrap_err(), Error::Network("down".into()));
    }
}
