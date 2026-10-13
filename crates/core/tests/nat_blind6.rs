//! Blind critic 6: pairs / whole-list sorts / real js/groups.js replays with seeds the builder did not use.
//! NAT6_PAIRS = jsonl [a,b,sign]; NAT6_GROUPS = jsonl [pages,[[key,name,[ids]]]]. Lists reuse NAT_LISTS (nat_blind5).
use vishion_core::{groups, jsx, model::Page};

#[test]
fn pairs_match_node() {
    let Ok(path) = std::env::var("NAT6_PAIRS") else { return };
    let txt = std::fs::read_to_string(path).unwrap();
    let (mut bad, mut n) = (0, 0);
    for l in txt.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let (a, b, c) = (v[0].as_str().unwrap(), v[1].as_str().unwrap(), v[2].as_i64().unwrap());
        let g = jsx::nat_compare(a, b) as i64;
        n += 1;
        if g != c {
            bad += 1;
            if bad < 15 {
                eprintln!("PAIR a={:?} b={:?} got={} exp={}", a, b, g, c);
            }
        }
    }
    assert!(bad == 0, "{} of {} pairs differ", bad, n);
}

#[test]
fn groups_match_real_js() {
    let Ok(path) = std::env::var("NAT6_GROUPS") else { return };
    let txt = std::fs::read_to_string(path).unwrap();
    let (mut bad, mut n) = (0, 0);
    for l in txt.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        let pages: Vec<Page> = v[0].as_array().unwrap().iter().map(|p| Page::from_value(p.clone()).unwrap()).collect();
        let refs: Vec<&Page> = pages.iter().collect();
        let got: Vec<(String, String, Vec<String>)> =
            groups::list(&refs, &|_| false).into_iter().map(|g| (g.key, g.name, g.pages)).collect();
        let exp: Vec<(String, String, Vec<String>)> = v[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                (
                    g[0].as_str().unwrap().to_string(),
                    g[1].as_str().unwrap().to_string(),
                    g[2].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect(),
                )
            })
            .collect();
        n += 1;
        if got != exp {
            bad += 1;
            if bad < 4 {
                let g: Vec<_> = got.iter().map(|x| &x.1).collect();
                let e: Vec<_> = exp.iter().map(|x| &x.1).collect();
                eprintln!("GROUPS DIFF\n got={:?}\n exp={:?}", g, e);
                for (x, y) in got.iter().zip(exp.iter()) {
                    if x != y {
                        eprintln!(" first diff got={:?} exp={:?}", x, y);
                        break;
                    }
                }
            }
        }
    }
    assert!(bad == 0, "{} of {} group sets differ", bad, n);
}

/// Contraction sweep: for every prefix P and every code point X, sign(nat_compare(P+X+"q", P+"q")).
/// NAT6_CSWEEP = binary (125 prefixes x 0x110000 bytes, 0 lt / 1 eq / 2 gt / 9 skip), NAT6_CPREFIX = prefixes json.
#[test]
fn contraction_sweep_matches_node() {
    let (Ok(bp), Ok(pp)) = (std::env::var("NAT6_CSWEEP"), std::env::var("NAT6_CPREFIX")) else { return };
    let buf = std::fs::read(bp).unwrap();
    let pre: Vec<String> = serde_json::from_str(&std::fs::read_to_string(pp).unwrap()).unwrap();
    let mut bad = 0;
    let mut by: std::collections::BTreeMap<String, Vec<u32>> = Default::default();
    for (i, p) in pre.iter().enumerate() {
        for cp in 0..0x110000u32 {
            let e = buf[i * 0x110000 + cp as usize];
            let Some(c) = char::from_u32(cp) else { continue };
            if e == 9 { continue; }
            let a = format!("{}{}q", p, c);
            let b = format!("{}q", p);
            let g = match jsx::nat_compare(&a, &b) { std::cmp::Ordering::Less => 0, std::cmp::Ordering::Equal => 1, _ => 2 };
            if g != e {
                bad += 1;
                by.entry(format!("{:?}", p)).or_default().push(cp);
            }
        }
    }
    for (p, v) in &by {
        let s: Vec<String> = v.iter().take(12).map(|c| format!("U+{:04X}", c)).collect();
        eprintln!("prefix {} : {} code points differ e.g. {}", p, v.len(), s.join(" "));
    }
    assert!(bad == 0, "{} (prefix,cp) pairs differ", bad);
}

/// Gap (blind critic 6): root collation has the Catalan contractions "L·" / "l·" (U+00B7, U+0387); node ignores the
/// dot at primary strength. ICU4X as configured treats it as a non-ignorable char. Expected values come from node.
#[test]
fn nat_catalan_middle_dot_contraction() {
    use std::cmp::Ordering::*;
    let cases: [(&str, &str, std::cmp::Ordering); 7] = [
        ("L\u{b7}z", "Lz", Equal),
        ("l\u{387}a", "la", Equal),
        ("Col\u{b7}legi", "Collegi", Equal),
        ("Pol\u{b7}len", "Pollen", Equal),
        ("Col\u{b7}legi", "Colkegi", Greater),
        ("Paral\u{b7}lel B", "Parallel A", Greater),
        ("L\u{b7}\u{eda}", "L\u{eda}", Equal),
    ];
    let bad: Vec<_> = cases.iter().filter(|(a, b, e)| jsx::nat_compare(a, b) != *e).collect();
    assert!(bad.is_empty(), "differ from node: {:?}", bad);
    // groups::list order: node gives [Parallel A, Paral·lel A, Paral·lel B, Parallel B, Zed]
    let names = ["Zed", "Paral\u{b7}lel B", "Parallel A", "Paral\u{b7}lel A", "Parallel B"];
    let pages: Vec<Page> = names
        .iter()
        .enumerate()
        .map(|(i, n)| Page::from_value(serde_json::json!({"id": format!("p{i}"), "name": "x.jpg", "status": "pending", "student": {"name": n}})).unwrap())
        .collect();
    let refs: Vec<&Page> = pages.iter().collect();
    let got: Vec<String> = groups::list(&refs, &|_| false).into_iter().map(|g| g.name).collect();
    assert_eq!(got, ["Parallel A", "Paral\u{b7}lel A", "Paral\u{b7}lel B", "Parallel B", "Zed"]);
}
