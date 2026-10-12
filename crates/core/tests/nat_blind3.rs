//! Blind-critic 3 gaps (expected = node Intl.Collator numeric/base, js=-1/0/1): supplementary-plane
//! decimal digits mixed with a preceding digit run still order differently from node's ICU.
use std::cmp::Ordering;
#[test]
fn nat_supplementary_digit_mix() {
    let cases: &[(&str, &str, i32)] = &[
        ("100", "1\u{1D7CE}", 1),
        ("1\u{1D7CE}0", "10", 1),
        ("1\u{1D7CE}0", "1\u{1D7CE}", 0),
        ("100", "1\u{1CCF9}", 1),
        ("1\u{1D7CE}0", "17", 1),
        ("\u{4E43}\u{967}\u{668}\u{2B9}", "\u{4E43}\u{967}\u{1CCF0}\u{1CCF6}\u{1D431}\u{16A62}", -1),
    ];
    let mut bad = vec![];
    for &(a, b, e) in cases {
        let g = match vishion_core::jsx::nat_compare(a, b) { Ordering::Less => -1, Ordering::Equal => 0, _ => 1 };
        if g != e { bad.push((a, b, e, g)); }
    }
    assert!(bad.is_empty(), "{:?}", bad);
}
