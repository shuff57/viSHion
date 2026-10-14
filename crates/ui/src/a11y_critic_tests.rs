// blind-critic round 13: spoken-math misreads found by reading ~190 invented expressions.
// Ignored so the workspace stays green; run `cargo test -p vishion-ui critic_round13 -- --ignored` to see them.
use crate::a11y::tex_speech;

#[test]
#[ignore = "known gaps, see failure list"]
fn critic_round13_spoken_math_gaps() {
    // (tex, text that must not appear, text that must appear)
    let cases: [(&str, &str, &str); 8] = [
        (r"\boldsymbol{\beta}", r"\", "beta"),
        (r"\leqslant", "leqslant", "less than or equal"),
        (r"\geqslant", "geqslant", "greater than or equal"),
        (r"\mathbb{R}^{2\times 2}", "to the power of 2 times 2", "2 by 2"),
        (r"\overrightarrow{AB}", "vector", "ray"),
        (r"-b\pm\sqrt{b^2-4ac}\over 2a", "end root over 2 a", "the fraction"),
        (r"\mathbf{u}\times\mathbf{v}", "u cross v", "bold"),
        (r"\forall \epsilon>0\ \exists \delta>0", "epsilon is greater than 0, there is some delta is", "such that"),
    ];
    let bad: Vec<String> = cases
        .iter()
        .filter(|(t, no, yes)| {
            let s = tex_speech(t);
            s.contains(no) || !s.contains(yes)
        })
        .map(|(t, ..)| format!("{t} => {}", tex_speech(t)))
        .collect();
    assert!(bad.is_empty(), "misreads:\n{}", bad.join("\n"));
}
