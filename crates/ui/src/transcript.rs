//! Transcript text to drawable runs: the pure half of `R.renderFormatted` + `W.wrap` (js/render.js,
//! js/words.js). A marked page text (flagged words wrapped in U+E000 key U+E001 text U+E002) is split into
//! display units; each unit becomes text runs with their markdown style, held math, and flagged words as
//! controls. Everything platform-free so the same lines feed egui and the web twin.
use vishion_core::jsx;
use vishion_core::render::{self, Math, Piece};
use vishion_core::words::{S0, S1, S2};

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Text { s: String, st: Style },
    Math(usize),
    /// A flagged word. `key` is the item key (`c:0`, `i:1`), `text` what the transcript shows for it.
    Word { key: String, text: String, st: Style },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub index: usize,
    pub runs: Vec<Run>,
    pub math: Vec<Math>,
    /// A `Margin:` line (lead-in stripped; drawn as a block with a bold "Margin note").
    pub margin: bool,
}

impl Line {
    /// Display math standing alone on its line (centred block).
    pub fn display_only(&self) -> Option<&Math> {
        match self.runs.as_slice() {
            [Run::Math(n)] if self.math[*n].display => Some(&self.math[*n]),
            _ => None,
        }
    }

    /// Plain text of the line for assistive tech: words as text, math as its TeX.
    pub fn plain(&self) -> String {
        let mut s = String::new();
        for r in &self.runs {
            match r {
                Run::Text { s: t, .. } => s.push_str(t),
                Run::Word { text, .. } => s.push_str(text),
                Run::Math(n) => s.push_str(&self.math[*n].tex),
            }
        }
        s
    }
}

fn strip_margin(unit: &str) -> String {
    let t = unit.trim_start_matches(jsx::is_ws);
    // "Margin:" is ASCII, so the byte offset is safe
    let rest = &t[7..];
    rest.trim_start_matches(jsx::is_ws).to_string()
}

/// Split a marked page text into drawable lines.
pub fn lines(marked: &str) -> Vec<Line> {
    render::units(marked)
        .into_iter()
        .enumerate()
        .map(|(index, unit)| {
            let margin = render::is_margin(&unit);
            let src = if margin { strip_margin(&unit) } else { unit };
            let (pieces, math) = render::unit_pieces(&src);
            Line { index, runs: collapse(runs(&pieces)), math, margin }
        })
        .collect()
}

/// HTML white-space: runs of spaces (across run boundaries too) draw as one, and a line starts with none.
fn collapse(runs: Vec<Run>) -> Vec<Run> {
    let mut prev_space = true;
    let mut out = Vec::with_capacity(runs.len());
    for r in runs {
        match r {
            Run::Text { s, st } => {
                let mut t = String::with_capacity(s.len());
                for c in s.chars() {
                    if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                        if !prev_space {
                            t.push(' ');
                        }
                        prev_space = true;
                    } else {
                        t.push(c);
                        prev_space = false;
                    }
                }
                if !t.is_empty() {
                    out.push(Run::Text { s: t, st });
                }
            }
            other => {
                prev_space = false;
                out.push(other);
            }
        }
    }
    out
}

#[derive(PartialEq)]
enum St {
    Plain,
    Key,
    Word,
}

fn runs(pieces: &[Piece]) -> Vec<Run> {
    let mut out = vec![];
    let (mut state, mut key, mut word) = (St::Plain, String::new(), String::new());
    let mut word_st = Style { bold: false, italic: false, strike: false, code: false };
    for p in pieces {
        match p {
            Piece::Math(n) => match state {
                St::Plain => out.push(Run::Math(*n)),
                // math inside a marked word cannot happen (markers never land inside TeX); keep the text
                _ => {}
            },
            Piece::Text { s, bold, italic, strike, code, .. } => {
                let st = Style { bold: *bold, italic: *italic, strike: *strike, code: *code };
                let mut plain = String::new();
                let flush = |plain: &mut String, out: &mut Vec<Run>, st: &Style| {
                    if !plain.is_empty() {
                        out.push(Run::Text { s: std::mem::take(plain), st: st.clone() });
                    }
                };
                for c in s.chars() {
                    match (&state, c) {
                        (St::Plain, S0) => {
                            flush(&mut plain, &mut out, &st);
                            state = St::Key;
                            key.clear();
                            word.clear();
                            word_st = st.clone();
                        }
                        (St::Key, S1) => state = St::Word,
                        (St::Key, c) => key.push(c),
                        (St::Word, S2) => {
                            out.push(Run::Word { key: std::mem::take(&mut key), text: std::mem::take(&mut word), st: word_st.clone() });
                            state = St::Plain;
                        }
                        (St::Word, c) => word.push(c),
                        (St::Plain, c) => plain.push(c),
                    }
                }
                flush(&mut plain, &mut out, &st);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_controls_in_reading_order() {
        let m = format!("The {}c:0{}confidance{} interval $x$\n\n{}i:0{}[illegible]{} min", S0, S1, S2, S0, S1, S2);
        let l = lines(&m);
        assert_eq!(l.len(), 2);
        let kinds: Vec<_> = l[0].runs.iter().map(|r| match r {
            Run::Text { s, .. } => format!("t:{s}"),
            Run::Word { key, text, .. } => format!("w:{key}={text}"),
            Run::Math(n) => format!("m:{n}"),
        }).collect();
        assert_eq!(kinds, vec!["t:The ", "w:c:0=confidance", "t: interval ", "m:0"]);
        assert!(matches!(&l[1].runs[0], Run::Word { key, text, .. } if key == "i:0" && text == "[illegible]"));
        assert_eq!(l[1].plain(), "[illegible] min");
    }

    #[test]
    fn margin_lead_in_is_stripped_and_strike_kept() {
        let l = lines("Margin:  check my units!\nhello ~~old~~ new");
        assert!(l[0].margin);
        assert_eq!(l[0].plain(), "check my units!");
        assert!(l[1].runs.iter().any(|r| matches!(r, Run::Text { s, st } if s == "old" && st.strike)));
    }

    #[test]
    fn display_math_alone_is_a_block() {
        let l = lines("$$\\frac{a}{b}$$");
        assert!(l[0].display_only().is_some());
        assert!(lines("so $$x$$ here")[0].display_only().is_none());
    }

    /// Blind critic, review screen round 3: the JS app renders HTML, so runs of spaces collapse to one
    /// ("Last  line   with  gaps" reads as "Last line with gaps"). Here the runs are drawn at full width.
    #[test]
    fn blind3_runs_of_spaces_collapse_like_html() {
        let l = lines("Last  line   with  gaps");
        assert_eq!(l[0].plain(), "Last line with gaps");
    }
}
