//! Blind-critic 2 gaps (expected = node Intl.Collator numeric/base, js=-1/0/1): supplementary-plane
//! decimal digits (U+1D7CE.., U+1CCF0.., U+1FBF0..) after a digit run order differently in ICU4X.
use std::cmp::Ordering;
#[test]
fn nat_supplementary_digits_after_digit_run() {
    let cases: &[(&str, &str, i32)] = &[
        ("1\u{1D7CF}", "1a", -1),
        ("\u{662}\u{1D7CF}", "\u{662}\u{16B2}", -1),
        ("2\u{1200}", "2\u{1D7CF}", 1),
        ("0\u{1CCF0}", "0!", 1),
        ("1\u{1CCF0}", "1a", -1),
        ("1\u{1FBF1}", "1a", -1),
    ];
    let mut bad = vec![];
    for &(a, b, e) in cases {
        let g = match vishion_core::jsx::nat_compare(a, b) { Ordering::Less => -1, Ordering::Equal => 0, _ => 1 };
        if g != e { bad.push((a, b, e, g)); }
    }
    assert!(bad.is_empty(), "{:?}", bad);
}
