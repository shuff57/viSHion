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
