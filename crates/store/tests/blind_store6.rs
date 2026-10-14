//! Blind critic phase 2, round 7. Fixture from `fixtures/gen_blind6.mjs` (JS JSON.stringify, new seed and
//! atoms: marker lookalikes, U+2028, integer-like keys, ...).
use serde_json::{json, Value};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use vishion_core::model::Project;
use vishion_store::fs::FsStore;
use vishion_store::projects::Projects;
use vishion_store::Store;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("future suspended"),
    }
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("vishion-blind6-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn fx() -> Value {
    serde_json::from_str(include_str!("fixtures/blind6.json")).unwrap()
}

#[test]
fn open_save_and_edit_pages_match_js_bytes() {
    let fx = fx();
    let pages = fx["pages"].as_array().unwrap();
    let dir = tmp("pages");
    let pr = Projects::new(FsStore::new(&dir));
    let ids: Vec<String> = pages.iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    block_on(pr.store.put("q/project.json", format!("{{\"name\":\"Q\",\"order\":{}}}", serde_json::to_string(&ids).unwrap()).as_bytes())).unwrap();
    for c in pages {
        block_on(pr.store.put(&format!("q/pages/{}.json", c["id"].as_str().unwrap()), c["text"].as_str().unwrap().as_bytes())).unwrap();
    }
    let mut o = block_on(pr.open_project("q", false, 1)).unwrap().unwrap();
    let mut bad = vec![];
    for u in &o.unreadable {
        bad.push(format!("unreadable {} {}", u.id, u.error));
    }
    for c in pages {
        let id = c["id"].as_str().unwrap();
        let Some(p) = o.assignment.pages.get(id) else { continue };
        block_on(pr.save_page("q", p)).unwrap();
        let got = String::from_utf8(block_on(pr.store.get(&format!("q/pages/{id}.json"))).unwrap().unwrap()).unwrap();
        if got != c["afterOpen"].as_str().unwrap() {
            bad.push(format!("resave {id}: {got:?} != {:?}", c["afterOpen"]));
        }
    }
    for c in pages {
        let id = c["id"].as_str().unwrap();
        let Some(p) = o.assignment.pages.get_mut(id) else { continue };
        p.set("corrected", json!(c["edit"].as_str().unwrap()));
        let p = p.clone();
        block_on(pr.save_page("q", &p)).unwrap();
        let got = String::from_utf8(block_on(pr.store.get(&format!("q/pages/{id}.json"))).unwrap().unwrap()).unwrap();
        if got != c["afterEdit"].as_str().unwrap() {
            bad.push(format!("edit {id}: {got:?} != {:?}", c["afterEdit"]));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(bad.is_empty(), "{} diffs; first 3: {:#?}", bad.len(), &bad[..bad.len().min(3)]);
}

#[test]
fn project_json_normalize_and_rename_match_js_bytes() {
    let fx = fx();
    let mut bad = vec![];
    let mut n = 0;
    for c in fx["metas"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        if text.contains("\"__proto__\"") {
            continue;
        }
        n += 1;
        let dir = tmp("meta");
        let pr = Projects::new(FsStore::new(&dir));
        block_on(pr.store.put("q/project.json", text.as_bytes())).unwrap();
        // normalize + pretty (what saveMeta with the fixed clock would write)
        match Project::from_json(text) {
            Ok(m) => {
                let got = m.normalized(7).to_json_pretty().unwrap();
                if got != c["normPretty"].as_str().unwrap() {
                    bad.push(format!("norm {text:?}\n got  {got:?}\n want {:?}", c["normPretty"]));
                }
            }
            Err(e) => bad.push(format!("parse {text:?}: {e}")),
        }
        // rename through the project layer: JS renameProject = readMeta + name + updated, plain write
        match block_on(pr.rename_project("q", "ren\u{e9}", 99)) {
            Ok(true) => {
                let got = String::from_utf8(block_on(pr.store.get("q/project.json")).unwrap().unwrap()).unwrap();
                // created/updated defaults use `now`; JS fixture used 7 for created when absent -> only compare when both present
                let want = c["renamedPretty"].as_str().unwrap();
                let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
                let has_created = parsed.get("created").is_some();
                // names in the fixture are 'rené<i>'; compare after normalising the index
                let w = regex_free_strip(want);
                let g = regex_free_strip(&got);
                if has_created && w != g {
                    bad.push(format!("rename {text:?}\n got  {got:?}\n want {want:?}"));
                }
            }
            Ok(false) => bad.push(format!("rename returned false for {text:?}")),
            Err(e) => bad.push(format!("rename err {text:?}: {e}")),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(n > 200);
    assert!(bad.is_empty(), "{} diffs; first: {}", bad.len(), bad.first().cloned().unwrap_or_default());
}

/// Drop digits of the `ren<e-acute><i>` name so the per-case index does not matter.
fn regex_free_strip(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.split("ren\u{e9}");
    out.push_str(it.next().unwrap());
    for part in it {
        out.push_str("ren\u{e9}");
        out.push_str(part.trim_start_matches(|c: char| c.is_ascii_digit()));
    }
    out
}

/// The JS app's own seeded data (S3 / store-oracle snapshots, string entries only) replayed through the
/// native store: every document must come back byte-identical after open + save of everything.
#[test]
fn js_seeded_snapshots_round_trip_natively() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut checked = 0;
    for rel in [
        "tools/store-oracle/out/snapshot-idb.json",
        "tools/store-oracle/out/snapshot-folder.json",
        "spikes/s3-storage-compat/out/snapshot-idb.json",
        "spikes/s3-storage-compat/out/snapshot-folder.json",
    ] {
        let Ok(t) = std::fs::read_to_string(root.join(rel)) else { continue };
        let snap: Value = serde_json::from_str(&t).unwrap();
        let dir = tmp("snap");
        let pr = Projects::new(FsStore::new(&dir));
        let mut docs = vec![];
        for (k, v) in snap["files"].as_object().unwrap() {
            if v["type"] == "string" && !k.ends_with(".bak") {
                if block_on(pr.store.put(k, v["text"].as_str().unwrap().as_bytes())).is_ok() {
                    docs.push((k.clone(), v["text"].as_str().unwrap().to_string()));
                }
            }
        }
        assert!(docs.len() > 3, "{rel}: {} docs", docs.len());
        let slugs: Vec<String> = block_on(pr.store.children("")).unwrap().into_iter().filter(|(_, d)| *d).map(|(n, _)| n).collect();
        for s in &slugs {
            let mut o = block_on(pr.open_project(s, false, 5)).unwrap().unwrap();
            assert!(o.unreadable.is_empty(), "{rel} {s}: {:?}", o.unreadable);
            for p in o.assignment.pages.values() {
                block_on(pr.save_page(s, p)).unwrap();
            }
            block_on(pr.save_meta(s, &mut o.assignment.meta, None)).unwrap();
        }
        if let Some(m) = block_on(pr.load_settings()).unwrap() {
            block_on(pr.save_settings(&vishion_core::model::Settings(m))).unwrap();
        }
        for (k, text) in &docs {
            let got = String::from_utf8(block_on(pr.store.get(k)).unwrap().unwrap()).unwrap();
            // a page JS had left as `transcribing` is recovered in memory only
            let mut same = got == *text || (got.replace("\"pending\"", "\"transcribing\"") == *text);
            // a page saved after opening carries `order` = its position: `fs.openProject` ends with `S.reindex()`
            // and JS writes whatever is in memory
            if !same && k.contains("/pages/") {
                let slug = k.split('/').next().unwrap();
                let id = k.rsplit('/').next().unwrap().trim_end_matches(".json");
                let pos = docs
                    .iter()
                    .find(|(dk, _)| *dk == format!("{slug}/project.json"))
                    .and_then(|(_, pt)| serde_json::from_str::<Value>(pt).ok())
                    .and_then(|v| v["order"].as_array().and_then(|a| a.iter().position(|x| x.as_str() == Some(id))));
                if let (Some(i), Ok(mut want), Ok(have)) = (pos, serde_json::from_str::<Value>(text), serde_json::from_str::<Value>(&got)) {
                    want["order"] = serde_json::json!(i);
                    if want["status"] == "transcribing" {
                        want["status"] = serde_json::json!("pending");
                    }
                    same = want == have;
                }
            }
            assert!(same, "{rel}: {k} changed:\n  was {text:?}\n  now {got:?}");
            checked += 1;
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(checked > 10, "only {checked} documents checked");
}

/// A hand-edited project.json with a typo fails to parse; `open_project(.., restore = true)` (what the home
/// screen does, as `readMeta(slug, true)` in JS) overwrites it with the older `.bak`. The damaged bytes
/// (the user's newer edits) are then gone from disk: a failed parse must not destroy the only copy.
#[test]
fn restoring_from_bak_must_not_destroy_the_damaged_project_json() {
    let dir = tmp("restore");
    let pr = Projects::new(FsStore::new(&dir));
    let damaged = "{\"name\":\"Edited by hand\",\"order\":[\"a\",\"b\",\"c\"],\"topic\":\"Chem\" \"prompt\":\"p\"}";
    block_on(pr.store.put("q/project.json", damaged.as_bytes())).unwrap();
    block_on(pr.store.put("q/project.json.bak", b"{\"name\":\"Old\",\"order\":[\"a\"]}")).unwrap();
    let o = block_on(pr.open_project("q", true, 1)).unwrap().unwrap();
    assert!(o.restored);
    let mut found = false;
    fn walk(d: &std::path::Path, damaged: &str, found: &mut bool) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, damaged, found);
            } else if std::fs::read_to_string(&p).map(|t| t == damaged).unwrap_or(false) {
                *found = true;
            }
        }
    }
    walk(&dir, damaged, &mut found);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found, "the damaged project.json text no longer exists anywhere under the project");
}

/// JS `JSON.parse` opens a settings.json that holds U+10FFFE followed by four surrogate hex digits; the
/// project layer refuses it (`lenient::parse` instead of `parse_marked`), so settings are silently
/// dropped while project.json/pages of the same shape open fine.
#[test]
fn settings_with_marker_lookalike_text_load() {
    let dir = tmp("setlook");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("settings.json", "{\"model\":\"a\",\"note\":\"\u{10FFFE}d800\",\"x\":\"\\ud800\"}".as_bytes())).unwrap();
    let r = block_on(pr.load_settings());
    let _ = std::fs::remove_dir_all(&dir);
    assert!(r.is_ok(), "settings.json unreadable: {:?}", r.err());
}
