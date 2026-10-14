//! Blind critic 5: whole-list sorts vs node Array.sort(Intl.Collator numeric base). NAT_LISTS = jsonl of [list, sorted].
use std::cmp::Ordering;
#[test]
fn nat_list_sorts() {
    let Ok(path) = std::env::var("NAT_LISTS") else { return };
    let txt = std::fs::read_to_string(path).unwrap();
    let (mut bad, mut n) = (0, 0);
    for l in txt.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let list: Vec<String> = v[0].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        let exp: Vec<String> = v[1].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        let mut got = list.clone();
        let r = std::panic::catch_unwind(move || { vishion_core::v8sort::sort_by(&mut got, |a, b| vishion_core::jsx::nat_compare(a, b)); got });
        n += 1;
        match r {
            Ok(g) if g == exp => {}
            Ok(g) => { bad += 1; if bad < 6 { eprintln!("DIFF list={:?}\n got={:?}\n exp={:?}", list, g, exp); } }
            Err(_) => { bad += 1; if bad < 6 { eprintln!("PANIC list={:?}", list); } }
        }
    }
    let _ = Ordering::Equal;
    assert!(bad == 0, "{} of {} lists differ", bad, n);
}

/// Gap: ICU's identical-prefix skip makes the comparator non-transitive for mixed BMP/supplementary digit runs
/// (A>D>B>A below). node's TimSort yields [0j, D, A, B]; Rust's stable sort yields a different order.
#[test]
fn nat_sort_cyclic_comparator_matches_v8() {
    let mut l: Vec<String> = ["\u{96f}\u{1d7d8}00\u{133}e", "\u{96f}\u{1d7e3}\u{1fb3}0\u{c5}\u{0}f", "0j\u{1d6a8}".trim_end_matches('\u{1d6a8}'), "\u{1d7d3}\u{1d7d6}\u{1d7d1}\u{1d400}"].iter().map(|s| s.to_string()).collect();
    l[2] = "0j\u{1d2e8}".to_string();
    let exp = ["0j\u{1d2e8}", "\u{1d7d3}\u{1d7d6}\u{1d7d1}\u{1d400}", "\u{96f}\u{1d7d8}00\u{133}e", "\u{96f}\u{1d7e3}\u{1fb3}0\u{c5}\u{0}f"];
    vishion_core::v8sort::sort_by(&mut l, |a, b| vishion_core::jsx::nat_compare(a, b));
    assert_eq!(l, exp);
}

/// Gap: groups::list sorts with `sort_by(nat_compare)`; with the non-transitive comparator Rust's sort can panic
/// ("user-provided comparison function does not correctly implement a total order") on >20 names, JS never throws.
#[test]
fn nat_sort_must_not_panic() {
    let l = ["𐒩٦", "𞥙0", "Å𝟮𝟬𝟮🯳", "7𝟟", "ĳ𝟶4", "e𝟽𐒦７", "a٢𝟱𐒧٧", "x𝟮𐒣९", "a𐒣", "ĳ٧𝟕", "𝟓00", "Å𝟛𝟟2४", "x9𞥓𝟏𞥑", "٠𝟱𝟫𝟣a", "𝟷𝟔", "٩🯴𝟘𝟫", "𞥙٨𝟽", "९𝟰𝟚𝟠", "𝟝8１𝟻", "Å𝟼", "Å𝟪٠a", "x𐒤𝟾𐒠２", "x٧8", "a0𝟬９a", "٠000", "🯸🯲", "७4e", "９𝟗0", "𝟷𝟐𝟢00", "9२", "Å𝟎9", "𝟱", "𐒠🯵", "Å0e", "𝟯𞥗١", "a٨𝟻٧𝟻", "𝟐🯹00", "ĳ🯵𐒧𝟔", "𐒥𐒠𐒠𐒥", "e2𞥓", "५🯶", "x٠", "e🯳𝟹", "٠𝟗١", "Å9５0", "e𐒦६𐒢𝟒", "७𝟠𝟼00", "٢５", "𝟠٣e", "٠e", "e𝟥𝟫𞥗a", "ĳ🯶८", "🯳𝟩", "Å𐒧𝟘𝟦𝟝", "𝟚", "e𝟯𝟐e", "６٥𝟠", "x٤𝟠𐒥𝟫𝟯𞥖e", "a𝟥١𝟼", "x𐒣𐒨a", "3３🯳٦1", "٩", "x𝟥𝟬", "ĳ𝟯", "𝟙３"];
    let mut v: Vec<String> = l.iter().map(|s| s.to_string()).collect();
    vishion_core::v8sort::sort_by(&mut v, |a, b| vishion_core::jsx::nat_compare(a, b));
}
