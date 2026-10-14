//! Blind-critic gaps: expected values are node Intl.Collator(numeric, base) results (js=-1/0/1).
use std::cmp::Ordering;
#[test]
fn nat_blind_gaps() {
    let cases: &[(&str, &str, i32)] = &[
        ("\u{10d3}", "\u{636}", -1), // Georgian vs Arabic
        ("\u{10d0}", "\u{5ea}", -1), // Georgian vs Hebrew
        ("\u{561}", "\u{10d0}", 1),  // Armenian vs Georgian
        ("\u{3042}", "\u{30a2}", 0), // hiragana vs katakana
        ("\u{3c2}", "\u{3c3}", 0),   // final sigma
        ("\u{5da}", "\u{5db}", 0),   // final kaf
        ("\u{5dd}", "\u{5de}", 0),   // final mem
        ("\u{439}", "\u{438}", 1),   // й vs и
        ("\u{491}", "\u{434}", -1),  // ґ vs д
        ("\u{623}", "\u{627}", -1),  // أ vs ا
        ("a\u{ad}", "a", 0),         // soft hyphen ignorable
        ("a\u{200b}", "a", 0),       // zero-width space ignorable
        ("\u{bf}", "\u{2014}", 1),   // ¿ vs em dash
        ("\u{bf}", "\u{2026}", -1),  // ¿ vs ellipsis
        ("\u{ab}", "\u{b7}", 1),     // « vs ·
        ("\u{ab}", "\u{bf}", 1),     // « vs ¿
    ];
    let mut bad = vec![];
    for &(a, b, e) in cases {
        let g = match vishion_core::jsx::nat_compare(a, b) { Ordering::Less => -1, Ordering::Equal => 0, _ => 1 };
        if g != e { bad.push((a, b, e, g)); }
    }
    assert!(bad.is_empty(), "{:?}", bad);
}
