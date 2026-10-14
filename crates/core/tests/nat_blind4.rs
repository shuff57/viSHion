//! Blind critic 4: random multi-script pairs vs node collator. Set NAT_PAIRS to a jsonl file of [a,b,expected].
use std::cmp::Ordering;
#[test]
fn nat_random_pairs() {
    let Ok(path) = std::env::var("NAT_PAIRS") else { return };
    let txt = std::fs::read_to_string(path).unwrap();
    let (mut bad, mut n) = (vec![], 0);
    for l in txt.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let (a, b, e) = (v[0].as_str().unwrap(), v[1].as_str().unwrap(), v[2].as_i64().unwrap());
        let g = match vishion_core::jsx::nat_compare(a, b) { Ordering::Less => -1, Ordering::Equal => 0, _ => 1 };
        n += 1;
        if g != e { bad.push((a.to_string(), b.to_string(), e, g)); }
    }
    for b in bad.iter().take(60) { eprintln!("{:?}", b); }
    assert!(bad.is_empty(), "{} of {} pairs differ", bad.len(), n);
}

/// Gap: characters whose compatibility decomposition is a Han ideograph (Kangxi radicals, CJK radicals
/// supplement, circled/parenthesized ideographs, ideographic telegraph) compare equal to that ideograph in
/// node (primary strength) but not in Rust, because hanfold maps char-for-char and does not follow the expansion.
#[test]
fn nat_han_compat_equals_ideograph() {
    let cases: &[(&str, &str)] = &[("\u{2F01}", "\u{4E28}"), ("\u{2F46}", "\u{65E0}"), ("\u{2E9F}", "\u{6BCD}"), ("\u{32A5}", "\u{4E2D}"), ("\u{2F53}", "\u{6C14}"), ("\u{2FD3}", "\u{9F8D}"), ("\u{2EF3}", "\u{9F9F}")];
    let bad: Vec<_> = cases.iter().filter(|(a, b)| vishion_core::jsx::nat_compare(a, b) != Ordering::Equal).collect();
    assert!(bad.is_empty(), "{:?}", bad);
}

/// Same cause, ordering against other Han (expected values from node: 2F46 vs 5200 is 1, 2F53 vs 9F8D is -1).
#[test]
fn nat_han_compat_order() {
    let c = |a: u32, b: u32| match vishion_core::jsx::nat_compare(&char::from_u32(a).unwrap().to_string(), &char::from_u32(b).unwrap().to_string()) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        _ => 1,
    };
    for (a, b, e) in [(0x2F46, 0x5200, 1), (0x2F53, 0x9F8D, -1), (0x3281, 0x4E2D, 1), (0x328B, 0x23394, 1), (0x33FF, 0x4E2D, -1), (0x32A5, 0x4E2D, 0)] {
        assert_eq!(c(a, b), e, "{a:X} vs {b:X}");
    }
}
