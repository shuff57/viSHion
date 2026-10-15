//! `GET /slots` progress of js/ocr.js `poll`: llama.cpp reports how much of the prompt (the picture) it has
//! read, which the page shows as "reading document… 40%" until the first word arrives.
use std::time::Duration;

/// js/ocr.js ticks every 400 ms.
pub const EVERY: Duration = Duration::from_millis(400);

/// The percentage for a `/slots` body: the first slot (or the object itself), only while it is processing
/// a prompt; 99 at most, so 100 is left to the first word.
pub fn reading_percent(body: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let slot = if v.is_array() { v.get(0)? } else { &v };
    if slot.get("is_processing")?.as_bool() != Some(true) {
        return None;
    }
    let total = slot.get("n_prompt_tokens")?.as_f64()?;
    if total <= 0.0 {
        return None;
    }
    let done = slot.get("n_prompt_tokens_processed").and_then(|d| d.as_f64()).unwrap_or(0.0);
    Some(((100.0 * done / total).round() as u32).min(99))
}

pub fn status(pct: u32) -> String {
    format!("reading document\u{2026} {pct}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_of_the_first_slot() {
        let b = r#"[{"is_processing":true,"n_prompt_tokens":200,"n_prompt_tokens_processed":50},{"is_processing":false}]"#;
        assert_eq!(reading_percent(b), Some(25));
    }

    #[test]
    fn single_object_and_cap_at_99() {
        let b = r#"{"is_processing":true,"n_prompt_tokens":10,"n_prompt_tokens_processed":10}"#;
        assert_eq!(reading_percent(b), Some(99));
    }

    #[test]
    fn idle_or_unreadable_slots_say_nothing() {
        assert_eq!(reading_percent(r#"[{"is_processing":false,"n_prompt_tokens":5}]"#), None);
        assert_eq!(reading_percent(r#"[{"is_processing":true,"n_prompt_tokens":0}]"#), None);
        assert_eq!(reading_percent("not json"), None);
        assert_eq!(reading_percent("[]"), None);
    }

    #[test]
    fn missing_processed_count_is_zero() {
        assert_eq!(reading_percent(r#"{"is_processing":true,"n_prompt_tokens":8}"#), Some(0));
    }
}
