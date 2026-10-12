//! Network edge. The reqwest-based streaming client (wasm + native) lands in Phase 1 with the SSE parser
//! rules from S4 (flush the tail, ignore empty `data:` payloads). Phase 0 only fixes the crate boundary.

/// Where a chat-completions endpoint lives. `connect-src` in the CSP already allows https and localhost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
}

impl Endpoint {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_join_ignores_trailing_slash() {
        assert_eq!(
            Endpoint::new("http://localhost:1234/v1/").chat_completions_url(),
            "http://localhost:1234/v1/chat/completions"
        );
    }
}
