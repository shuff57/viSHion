//! Byte-for-byte parity of `vishion-core` against the unmodified JS engine.
//!
//! `tools/parity/fixture.json` is produced by `node tools/parity/run.js`, which loads js/*.js with a stub
//! window (the pattern of tests/*.test.js and spikes/s5-markdown-katex/run.js) and records, for each case,
//! the input and what the JS returned. This test replays every case through the Rust crate and compares the
//! serialised results as strings, so key order and number text count.
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use vishion_core::export::{self, Export, Opts};
use vishion_core::model::{Assignment, Page, Project, Settings};
use vishion_core::{clean, groups, guess, judge, jsx, render, student, words};

// ---- SHA-256 (std only; used to compare exported archives without storing them) ----
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{:08x}", x)).collect()
}

fn digest(b: &[u8]) -> Value {
    json!({"len": b.len(), "sha256": sha256_hex(b)})
}

// ---- helpers ----
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn ser(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}
fn strs(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| s(x).to_string()).collect()
}
fn page_of(v: &Value) -> Page {
    Page::from_value(v.clone()).unwrap()
}

struct Tally {
    n: usize,
    kinds: HashMap<String, usize>,
    fails: Vec<String>,
}
impl Tally {
    fn check(&mut self, kind: &str, idx: usize, got: &Value, want: &Value) {
        self.n += 1;
        *self.kinds.entry(kind.to_string()).or_default() += 1;
        let (g, w) = (ser(got), ser(want));
        if g != w && self.fails.len() < 25 {
            let cut = |x: &str| x.chars().take(if kind == "nat" { 1_000_000 } else { 700 }).collect::<String>();
            self.fails.push(format!("[{} #{}]\n  rust: {}\n  js:   {}", kind, idx, cut(&g), cut(&w)));
        } else if g != w {
            self.fails.push(String::new());
        }
    }
}

fn found_json(f: &[judge::Found]) -> Value {
    Value::Array(
        f.iter()
            .map(|c| json!({"word": c.word, "term": c.term, "conf": jsx::num_value(c.conf)}))
            .collect(),
    )
}

fn snap(p: &Page, settings: &Settings) -> Map<String, Value> {
    let items = words::items(p);
    let mut keys: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    keys.push("i:7".into());
    keys.push("zz".into());
    let ctx: Vec<Value> = keys
        .iter()
        .map(|k| {
            let c = words::context_for(p, k).map_or(Value::Null, |(b, a)| json!({"before": b, "after": a}));
            json!([k, c])
        })
        .collect();
    let v = json!({
        "effective": words::effective(p),
        "marked": words::marked(p, false),
        "original": words::marked(p, true),
        "items": items.iter().map(|i| i.to_value()).collect::<Vec<_>>(),
        "count": words::count(p),
        "ctx": ctx,
        "page": p.to_value(),
        "reference": settings.reference(),
    });
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

fn run_page_case(input: &Value, want: &Value) -> Value {
    let mut page = page_of(&input["page"]);
    let topic = s(&input["topic"]).to_string();
    let mut st = Settings(Map::new());
    st.set("reference", json!([]));
    if let Some(m) = input["settings"].as_object() {
        for (k, v) in m {
            st.set(k, v.clone());
        }
    }
    let mut steps = vec![];
    for op in input["ops"].as_array().unwrap() {
        let now = op["now"].as_i64().unwrap();
        let key = s(&op["key"]).to_string();
        let has_corr = matches!(page.get("corrections"), Some(Value::Array(_)));
        let mut ret = Value::Null;
        let mut threw = false;
        match s(&op["op"]) {
            "choose" => {
                let ok = words::choose(&mut page, &key, s(&op["via"]), s(&op["text"]), &mut st, &topic, now);
                if ok && key.starts_with('c') && !has_corr {
                    threw = true; // JS: judge.toggle reads p.corrections[idx] and throws
                } else {
                    ret = json!(ok);
                }
            }
            "reopen" => words::reopen(&mut page, &key),
            "useAll" => words::use_all(&mut page, &mut st, &topic, now),
            "toggle" => {
                judge::toggle(&mut page, op["idx"].as_f64().unwrap(), op["applied"].as_bool().unwrap());
                threw = !has_corr;
            }
            "applyAll" => {
                judge::apply_all(&mut page);
                threw = !has_corr;
            }
            o => panic!("unknown op {}", o),
        }
        let mut m = snap(&page, &st);
        m.insert("ret".into(), ret);
        m.insert("threw".into(), json!(threw));
        steps.push(Value::Object(m));
    }
    let initial = snap(&page_of(&input["page"]), &{
        let mut s0 = Settings(Map::new());
        s0.set("reference", json!([]));
        if let Some(m) = input["settings"].as_object() {
            for (k, v) in m {
                s0.set(k, v.clone());
            }
        }
        s0
    });
    let _ = want;
    json!({"initial": Value::Object(initial), "steps": steps})
}

fn run_groups(input: &Value) -> Value {
    let pages: Vec<Page> = input["pages"].as_array().unwrap().iter().map(page_of).collect();
    let refs: Vec<&Page> = pages.iter().collect();
    let queued: Vec<String> = strs(&input["queued"]);
    let q = |id: &str| queued.iter().any(|x| x == id);
    let list = groups::list(&refs, &q);
    let t = groups::tally(&list);
    let matches: Vec<Value> = list
        .iter()
        .map(|g| {
            json!(["all", "processing", "ready", "check", "failed", "queued", "waiting"]
                .iter()
                .map(|f| groups::matches(g, f))
                .collect::<Vec<_>>())
        })
        .collect();
    json!({
        "list": list.iter().map(|g| g.to_value()).collect::<Vec<_>>(),
        "tally": {"total": t.total, "ready": t.ready, "check": t.check, "failed": t.failed, "processing": t.processing},
        "names": groups::names(&refs, &list),
        "matches": matches,
        "of": pages.iter().map(|p| groups::of(p, &list).map_or(Value::Null, |g| json!(g.key))).collect::<Vec<_>>(),
        "nameOf": pages.iter().map(|p| json!(groups::name_of(p))).collect::<Vec<_>>(),
        "keyOf": pages.iter().map(|p| json!(groups::key_of(p))).collect::<Vec<_>>(),
    })
}

fn run_export(input: &Value) -> Value {
    let pages: Vec<Page> = input["pages"].as_array().unwrap().iter().map(page_of).collect();
    let list: Vec<&Page> = pages.iter().collect();
    let meta = Project::from_json(&ser(&input["meta"])).unwrap();
    let exported = s(&input["exported"]).to_string();
    let texts: Vec<Value> = ["raw", "corrected", "both", "x"]
        .iter()
        .map(|m| json!([m, list.iter().map(|p| export::page_text(p, m)).collect::<Vec<_>>()]))
        .collect();
    let per_page: Map<String, Value> =
        export::per_page(&list, "corrected").into_iter().map(|(k, v)| (k, json!(v))).collect();
    let mut runs = vec![];
    for mode in ["raw", "corrected", "both"] {
        for sep in ["label", "break", "none"] {
            for fmt in ["txt", "md", "docx"] {
                let o = Opts::new(mode, sep, fmt);
                runs.push(json!({
                    "o": {"mode": mode, "sep": sep, "fmt": fmt},
                    "combined": export::combined(&list, &o),
                    "docx": digest(&export::docx(&list, &o)),
                }));
            }
        }
    }
    let mut deliveries = vec![];
    for kind in ["combined", "pages", "json", "csv", "copy", "all"] {
        for (mode, sep, fmt) in [("corrected", "label", "md"), ("both", "break", "docx"), ("raw", "none", "txt")] {
            let o = Opts::new(mode, sep, fmt);
            let got = match export::run(kind, &list, &o, &meta, &exported).unwrap() {
                Export::File { name, mime, bytes } => json!({"name": name, "mime": mime, "bytes": digest(&bytes)}),
                Export::Copy(t) => json!({"copy": t}),
            };
            deliveries.push(json!({"kind": kind, "o": {"mode": mode, "sep": sep, "fmt": fmt}, "got": got}));
        }
    }
    json!({
        "texts": texts,
        "labels": list.iter().map(|p| json!(export::label(p))).collect::<Vec<_>>(),
        "csv": export::csv(&list),
        "json": export::json(&list, &meta, &exported),
        "exported": exported,
        "perPage": Value::Object(per_page),
        "runs": runs,
        "deliveries": deliveries,
    })
}

fn run_assignment(input: &Value, want: &Value) -> Value {
    let mut a = Assignment::new(Project::from_json(r#"{"name":"A","order":[],"futureField":{"z":1}}"#).unwrap());
    for op in input["ops"].as_array().unwrap() {
        match s(&op["op"]) {
            "add" => {
                let names = strs(&op["names"]);
                let ids = strs(&op["ids"]);
                a.add_pages(ids.iter().zip(names.iter()).map(|(i, n)| Assignment::new_page(i, n)).collect());
            }
            "remove" => a.remove_page(s(&op["id"])),
            "move" => a.move_page(op["from"].as_i64().unwrap(), op["to"].as_i64().unwrap()),
            "update" => a.update_page(s(&op["id"]), op["patch"].as_object().unwrap().clone()),
            o => panic!("unknown op {}", o),
        }
    }
    // JS `pages` is an insertion-ordered object; Rust keeps a sorted map, so emit in the JS key order.
    let mut pages = Map::new();
    for k in want["pages"].as_object().unwrap().keys() {
        if let Some(p) = a.page(k) {
            pages.insert(k.clone(), p.to_value());
        }
    }
    let c = a.counts();
    json!({
        "meta": Value::Object(a.meta.doc().clone()),
        "pages": Value::Object(pages),
        "order": a.ordered().iter().map(|p| json!(p.id())).collect::<Vec<_>>(),
        "counts": {"total": c.total, "done": c.done, "error": c.error, "pending": c.pending, "transcribing": c.transcribing},
    })
}

#[test]
fn rust_matches_js_on_every_fixture_case() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/parity/fixture.json");
    let text = std::fs::read_to_string(path).expect("fixture missing: run `node tools/parity/run.js`");
    let fx: Value = serde_json::from_str(&text).unwrap();
    let mut t = Tally { n: 0, kinds: HashMap::new(), fails: vec![] };
    for (idx, c) in fx["cases"].as_array().unwrap().iter().enumerate() {
        let kind = s(&c["kind"]);
        let (input, want) = (&c["input"], &c["expected"]);
        let got: Value = match kind {
            "page" => run_page_case(input, want),
            "heuristic" => found_json(&judge::heuristic(s(&input["text"]), &strs(&input["list"]))),
            "accept" => json!(judge::accept(&input["x"], &strs(&input["list"]), s(&input["text"]))),
            "remote" => match judge::parse_remote(s(&input["reply"]), &strs(&input["list"]), s(&input["text"]), input["strict"].as_bool().unwrap()) {
                Ok(f) => json!({"ok": found_json(&f)}),
                Err(_) => json!({"error": true}),
            },
            "student_reply" => student::parse_reply(s(&input["reply"])).map_or(Value::Null, |n| json!(n)),
            "student_demo" => student::demo_name(s(&input["hint"])).map_or(Value::Null, |n| json!(n)),
            "guesses" => Value::Object(guess::parse_guesses(s(&input["reply"]), input["count"].as_u64().unwrap() as usize)),
            "numbered" => {
                let raw = s(&input["raw"]);
                let (txt, n) = guess::numbered(raw);
                if raw.to_ascii_lowercase().contains("[illegible]") && n > 0 { json!(txt) } else { Value::Null }
            }
            "groups" => run_groups(input),
            "nat" => Value::Array(
                input["pairs"].as_array().unwrap().iter()
                    .map(|p| json!(match jsx::nat_compare(s(&p[0]), s(&p[1])) { std::cmp::Ordering::Less => -1, std::cmp::Ordering::Equal => 0, _ => 1 }))
                    .collect(),
            ),
            "natsort" => Value::Array(
                input["lists"].as_array().unwrap().iter()
                    .map(|l| {
                        let mut v = strs(l);
                        vishion_core::v8sort::sort_by(&mut v, |a, b| jsx::nat_compare(a, b));
                        json!(v)
                    })
                    .collect(),
            ),
            "slug" => Value::Array(strs(&input["slugs"]).iter().map(|x| json!(export::slug(x))).collect()),
            "number_str" => Value::Array(strs(&input["strs"]).iter().map(|x| json!(jsx::num_str(jsx::string_to_number(x)))).collect()),
            "to_fixed" => Value::Array(input["xs"].as_array().unwrap().iter().map(|x| json!(jsx::to_fixed1(x.as_f64().unwrap()))).collect()),
            "csv_secs" => Value::Array(
                input["ms"].as_array().unwrap().iter()
                    .map(|x| { let v = x.as_f64().unwrap() / 1000.0; json!(jsx::to_fixed1(if v == 0.0 || v.is_nan() { 0.0 } else { v })) })
                    .collect(),
            ),
            "clean" => Value::Array(
                input["inputs"].as_array().unwrap().iter()
                    .map(|x| {
                        let t = match x { Value::Null => String::new(), Value::Object(_) => String::new(), v => jsx::to_string(v) };
                        json!(clean::clean(&t))
                    })
                    .collect(),
            ),
            "bands" => Value::Array(render::map_bands(input["bands"].as_array().unwrap(), input["n"].as_u64().unwrap() as usize)),
            "render" => Value::Array(
                strs(&input["texts"]).iter()
                    .map(|t| {
                        let u = render::units(t);
                        json!({"units": u, "html": u.iter().map(|x| render::unit_basic(x)).collect::<Vec<_>>()})
                    })
                    .collect(),
            ),
            "export" => run_export(input),
            "settings" => Value::Object(Settings::load(&input["saved"]).0),
            "new_page" => Assignment::new_page("id1", s(&input["name"])).to_value(),
            "assignment" => run_assignment(input, want),
            k => panic!("unknown case kind {}", k),
        };
        t.check(kind, idx, &got, want);
    }

    // markdown-it + KaTeX recorded from the real libraries: replay by lookup through the Rust glue.
    let rl = &fx["render_libs"];
    if rl.is_object() {
        let inline: HashMap<String, String> =
            rl["inline"].as_array().unwrap().iter().map(|p| (s(&p[0]).to_string(), s(&p[1]).to_string())).collect();
        let maths: HashMap<String, String> =
            rl["maths"].as_array().unwrap().iter().map(|p| (s(&p[0]).to_string(), s(&p[1]).to_string())).collect();
        for (i, text) in strs(&rl["texts"]).iter().enumerate() {
            let html: Vec<Value> = render::units(text)
                .iter()
                .map(|u| {
                    let h = render::unit::<String>(
                        u,
                        &mut |t| inline.get(t).cloned().ok_or_else(|| format!("no recorded renderInline for {:?}", t)),
                        &mut |m| {
                            let key = ser(&json!([m.tex, m.display]));
                            maths.get(&key).cloned().unwrap_or_else(|| render::math_fallback(m))
                        },
                    );
                    json!(h.unwrap_or_else(|e| format!("ERR {}", e)))
                })
                .collect();
            t.check("render_libs", i, &Value::Array(html), &rl["html"][i]);
        }
    } else {
        panic!("fixture has no render_libs section; run `node tools/parity/run.js` with the S5 vendor files present");
    }

    let summary: Vec<String> = {
        let mut k: Vec<_> = t.kinds.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        k.sort();
        k
    };
    println!("parity: {} comparisons ({})", t.n, summary.join(", "));
    let bad = t.fails.iter().filter(|f| !f.is_empty()).cloned().collect::<Vec<_>>();
    assert!(
        t.fails.is_empty(),
        "{} of {} comparisons differ from the JS engine; first {}:\n{}",
        t.fails.len(),
        t.n,
        bad.len(),
        bad.join("\n")
    );
}

/// The flat pieces (pulldown-cmark, block syntax neutralised) against markdown-it's `renderInline` output for
/// every inline string the S5 corpus produced: same visible characters (ignoring whitespace and the
/// `[illegible]` case, which render.js normalises) with the same bold / italic / strike / code flags.
#[test]
fn pieces_match_markdown_it_inline() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/parity/fixture.json");
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    type Flags = (char, bool, bool, bool, bool);
    let from_html = |h: &str| -> Vec<Flags> {
        let (mut out, mut depth) = (vec![], [0i32; 4]); // strong, em, s, code
        let mut rest = h;
        while !rest.is_empty() {
            if let Some(t) = rest.strip_prefix('<') {
                let end = t.find('>').unwrap();
                let tag = &t[..end];
                let (close, name) = match tag.strip_prefix('/') {
                    Some(n) => (true, n),
                    None => (false, tag.split([' ', '/']).next().unwrap()),
                };
                let ix = match name {
                    "strong" => Some(0),
                    "em" => Some(1),
                    "s" | "del" => Some(2),
                    "code" => Some(3),
                    _ => None,
                };
                if let Some(i) = ix {
                    depth[i] += if close { -1 } else { 1 };
                }
                rest = &t[end + 1..];
                continue;
            }
            let (txt, n) = if let Some(r) = ["&lt;", "&gt;", "&quot;", "&#39;", "&amp;"].iter().zip(['<', '>', '"', '\'', '&']).find(|(e, _)| rest.starts_with(**e)) {
                (r.1, r.0.len())
            } else {
                let c = rest.chars().next().unwrap();
                (c, c.len_utf8())
            };
            if !jsx::is_ws(txt) {
                out.push((txt.to_ascii_lowercase(), depth[0] > 0, depth[1] > 0, depth[2] > 0, depth[3] > 0));
            }
            rest = &rest[n..];
        }
        out
    };
    let from_pieces = |held: &str| -> Vec<Flags> {
        let n = held.matches('⟦').count();
        let mut out = vec![];
        for p in render::pieces(held, n + 100) {
            match p {
                render::Piece::Text { s, bold, italic, strike, code, .. } => {
                    for c in s.chars().filter(|c| !jsx::is_ws(*c)) {
                        out.push((c.to_ascii_lowercase(), bold, italic, strike, code));
                    }
                }
                render::Piece::Math(i) => {
                    for c in format!("⟦{}⟧", i).chars() {
                        out.push((c, false, false, false, false));
                    }
                }
            }
        }
        out
    };
    let (mut same, mut total) = (0, 0);
    let mut diffs = vec![];
    for p in fx["render_libs"]["inline"].as_array().unwrap() {
        let (held, html) = (s(&p[0]), s(&p[1]));
        total += 1;
        // `Piece::Math` carries no style, so placeholder characters are compared without flags
        let mut b = from_html(html);
        let mut i = 0;
        while i < b.len() {
            if b[i].0 == '⟦' {
                let mut j = i;
                while j < b.len() && b[j].0 != '⟧' {
                    j += 1;
                }
                let end = (j + 1).min(b.len());
                for x in b[i..end].iter_mut() {
                    x.1 = false;
                    x.2 = false;
                    x.3 = false;
                    x.4 = false;
                }
                i = j;
            }
            i += 1;
        }
        let a = from_pieces(held);
        if a == b {
            same += 1;
        } else if diffs.len() < 8 {
            diffs.push(format!("{:?}
    pieces: {:?}
    md-it:  {:?}", held, a, b));
        }
    }
    println!("pieces vs markdown-it renderInline (chars + flags): {}/{} identical", same, total);
    assert!(total > 100);
    assert!(diffs.is_empty(), "{} of {} differ:
{}", total - same, total, diffs.join("
"));
}
