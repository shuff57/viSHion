//! Demo mode of js/ocr.js and js/guess.js: sample transcripts instead of a model, so the app can be tried
//! without a server. Pure data; the host decides when demo mode is on (the `demo` setting, or the sample
//! assignment) and answers `Transcribe` and `Guess` with these.
use serde_json::{json, Map, Value};

/// `DEMO` of js/ocr.js: picked by the page's position in the run, `DEMO[index % 3]`.
pub const TEXTS: [&str; 3] = [
    "Lesson 4: Quadratics\nSolve x^2 - 5x + 6 = 0 by factoring\n$x^2 - 5x + 6 = (x-2)(x-3)$\nso $x = 2$ or $x = 3$\nMargin: check with the discriminant\n~~complete the square~~ not needed here\nVertex form:\n$$y = a(x-h)^2 + k$$\nStudents struggled w/ [illegible] on #7",
    "Pt seen 10/3 c/o headache x2 days\nBP 142/90, HR 88, afebrile\nHx of hypertension, on lisinopril 10mg qd\nMargin: recheck BP in 2 wks\nPlan: continue meds, low sodium diet\n~~start metformin~~ not indicated\nF/U prn",
    "Staff meeting notes\n1. Robotics budget due Friday\n2. Pathways grant report, draft by 10/20\n3. [illegible] to order sensors\nMargin: ask about the second cart\nMean of the quiz scores: $\\bar{x} = \\frac{\\sum x_i}{n}$",
];

/// The sample text for the page at `index` of a run.
pub fn text(index: usize) -> &'static str {
    TEXTS[index % TEXTS.len()]
}

/// What demo mode "reads": the sample text, and how many words/tokens and milliseconds the streamed answer
/// would have taken (`demo()` in js/ocr.js: six progress ticks of 90 ms, then 30 ms per streamed part).
pub struct Read {
    pub text: String,
    pub tokens: u32,
    pub ms: u32,
}

pub fn read(index: usize) -> Read {
    let raw = text(index);
    // `text.split(/(\s+)/)`: words and the whitespace between them are streamed one part at a time
    let mut parts = 0u32;
    let mut tokens = 0u32;
    let mut in_ws: Option<bool> = None;
    for c in raw.chars() {
        let ws = c.is_whitespace();
        if in_ws != Some(ws) {
            parts += 1;
            if !ws {
                tokens += 1;
            }
            in_ws = Some(ws);
        }
    }
    Read { text: crate::clean::clean(raw), tokens, ms: 6 * 90 + 30 * parts }
}

/// `ask()` of js/guess.js in demo mode: the same three guesses for every unreadable word of the page.
pub fn guesses(count: usize) -> Map<String, Value> {
    let mut d = Map::new();
    for i in 0..count {
        d.insert(
            format!("i:{i}"),
            json!([{"text": "the discriminant", "percent": 55}, {"text": "discriminant", "percent": 25}, {"text": "a derivative", "percent": 10}]),
        );
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_picks_the_sample_and_counts_words() {
        assert!(read(0).text.starts_with("Lesson 4: Quadratics"));
        assert!(read(4).text.starts_with("Pt seen"));
        assert!(read(2).text.contains("\\bar{x}"));
        assert_eq!(read(1).tokens, text(1).split_whitespace().count() as u32);
    }

    #[test]
    fn guesses_cover_each_unreadable_word() {
        let g = guesses(2);
        assert_eq!(g.len(), 2);
        assert_eq!(g["i:1"][0]["text"], "the discriminant");
        assert!(guesses(0).is_empty());
    }
}
