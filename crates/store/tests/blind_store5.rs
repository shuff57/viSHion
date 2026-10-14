//! Blind critic round 5 (phase 2 store): seeded round-trip fuzz. Driven by env FUZZ_DIR (a tree written by
//! a JS generator using JSON.stringify): open every project, save every page and project.json untouched,
//! every file must stay byte-identical.
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use vishion_store::fs::FsStore;
use vishion_store::projects::Projects;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("future suspended"),
    }
}

#[test]
fn js_written_tree_round_trips_byte_identical() {
    let Ok(dir) = std::env::var("FUZZ_DIR") else { return };
    let root = std::path::PathBuf::from(dir);
    let pr = Projects::new(FsStore::new(&root));
    let mut bad = vec![];
    let mut n = 0;
    for e in std::fs::read_dir(&root).unwrap() {
        let slug = e.unwrap().file_name().into_string().unwrap();
        let before: Vec<(std::path::PathBuf, Vec<u8>)> = walk(&root.join(&slug));
        match block_on(pr.open_project(&slug, false, 1)) {
            Ok(Some(mut o)) => {
                for id in o.assignment.meta.order() {
                    if let Some(p) = o.assignment.pages.get(&id) {
                        block_on(pr.save_page(&slug, p)).unwrap();
                    } else {
                        bad.push(format!("{slug}/{id}: unreadable {:?}", o.unreadable.iter().find(|u| u.id == id).map(|u| u.error.clone())));
                    }
                }
                block_on(pr.save_meta(&slug, &mut o.assignment.meta, None)).unwrap();
            }
            other => bad.push(format!("{slug}: open -> {:?}", other.map(|o| o.is_some()))),
        }
        for (p, b) in before {
            if p.file_name().unwrap() == "project.json.bak" { continue; }
            let after = std::fs::read(&p).unwrap();
            n += 1;
            if after != b {
                bad.push(format!("{}: bytes changed\n  before {}\n  after  {}", p.display(), String::from_utf8_lossy(&b), String::from_utf8_lossy(&after)));
            }
        }
    }
    assert!(bad.is_empty(), "{n} files, {} bad:\n{}", bad.len(), bad.iter().take(5).cloned().collect::<Vec<_>>().join("\n"));
}

fn walk(d: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = vec![];
    for e in std::fs::read_dir(d).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { out.extend(walk(&p)); } else { let b = std::fs::read(&p).unwrap(); out.push((p, b)); }
    }
    out
}

/// Edit `corrected` of every page to a string from FUZZ_DIR/edits.json (written by the JS generator), save,
/// and let node verify (JSON.parse of the file must give that string and otherwise the same page).
#[test]
fn edits_with_marker_lookalikes_survive() {
    let Ok(dir) = std::env::var("FUZZ_DIR") else { return };
    let root = std::path::PathBuf::from(dir);
    let edits: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("edits.json")).unwrap()).unwrap();
    let pr = Projects::new(FsStore::new(&root));
    for e in std::fs::read_dir(&root).unwrap() {
        let e = e.unwrap();
        if !e.path().is_dir() { continue; }
        let slug = e.file_name().into_string().unwrap();
        let mut o = block_on(pr.open_project(&slug, false, 1)).unwrap().unwrap();
        let ids: Vec<String> = o.assignment.pages.keys().cloned().collect();
        for id in ids {
            let s = edits[&id].as_str().unwrap().to_string();
            let p = o.assignment.pages.get_mut(&id).unwrap();
            p.set("corrected", serde_json::json!(s));
            block_on(pr.save_page(&slug, p)).unwrap();
        }
        // reopen: Rust itself must read back what it wrote
        let o2 = block_on(pr.open_project(&slug, false, 1)).unwrap().unwrap();
        assert!(o2.unreadable.is_empty(), "{slug}: unreadable after own write: {:?}", o2.unreadable);
    }
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("vishion-blind5-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn edit_and_read(tag: &str, page_json: &str, corrected: &str) -> serde_json::Value {
    let dir = tmp(tag);
    let pr = Projects::new(FsStore::new(&dir));
    std::fs::create_dir_all(dir.join("p/pages")).unwrap();
    std::fs::write(dir.join("p/project.json"), r#"{"name":"p","order":["a"]}"#).unwrap();
    std::fs::write(dir.join("p/pages/a.json"), page_json).unwrap();
    let mut o = block_on(pr.open_project("p", false, 1)).unwrap().unwrap();
    let p = o.assignment.pages.get_mut("a").unwrap();
    p.set("corrected", serde_json::json!(corrected));
    block_on(pr.save_page("p", p)).unwrap();
    let txt = std::fs::read_to_string(dir.join("p/pages/a.json")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    serde_json::Value::String(txt)
}

/// The user's text is data: a U+10FFFE followed by hex digits typed or pasted into a page that also holds a
/// lone surrogate must be written as that literal text, not turned into a lone surrogate escape.
#[test]
fn edit_text_looking_like_marker_is_not_turned_into_lone_surrogate() {
    let typed = "\u{10FFFE}d800";
    let out = edit_and_read("one", r#"{"id":"a","raw":"\ud800","status":"done"}"#, typed);
    let txt = out.as_str().unwrap();
    // the edited field must hold the 7 characters typed: U+10FFFE (raw or as the valid pair) + d800
    let corrected = txt.split("\"corrected\":\"").nth(1).unwrap();
    assert!(
        corrected.starts_with("\u{10FFFE}d800\""),
        "typed text mangled: {corrected}"
    );
}

#[test]
fn edit_text_looking_like_marker_in_page_without_lone_surrogates() {
    let out = edit_and_read("two", r#"{"id":"a","raw":"x","status":"done"}"#, "\u{10FFFE}d800");
    let corrected = out.as_str().unwrap().split("\"corrected\":\"").nth(1).unwrap().to_string();
    assert!(corrected.starts_with("\u{10FFFE}d800\""), "typed text mangled: {corrected}");
}

/// A zip made by the JS app in browser-storage mode may hold names Windows cannot create (`?`, `*`, `"`,
/// `<`, `>`, `|`). import_files must skip-and-report them (or refuse before writing), not abort halfway
/// and leave a half-imported project behind.
#[test]
fn import_with_windows_illegal_name_is_not_left_half_written() {
    let dir = tmp("imp-illegal");
    let pr = Projects::new(FsStore::new(&dir));
    let entries = vec![
        ("x/project.json".to_string(), br#"{"name":"Imp","order":["a","b"]}"#.to_vec()),
        ("x/pages/a.json".to_string(), br#"{"id":"a"}"#.to_vec()),
        ("x/pages/we?ird.json".to_string(), br#"{"id":"w"}"#.to_vec()),
        ("x/pages/b.json".to_string(), br#"{"id":"b"}"#.to_vec()),
    ];
    let r = block_on(pr.import_files_report(&entries));
    let b_present = std::fs::read_to_string(dir.join("imp/pages/b.json")).is_ok();
    let left = dir.exists();
    let _ = std::fs::remove_dir_all(&dir);
    match r {
        Ok((_, skipped)) => assert!(b_present && skipped.iter().any(|s| s.contains("we?ird")), "ok but b missing / not reported: {skipped:?}"),
        Err(e) => assert!(!left, "import failed with {e} but left a partial project on disk"),
    }
}

/// JS orders integer-like keys first (ascending) in every object, so `JSON.stringify(normalize(meta))` of a
/// project.json with an unknown key "5" starts with "5". Rewriting untouched must be byte-identical to
/// what the JS app writes.
#[test]
fn integer_like_extra_meta_key_keeps_js_order() {
    let dir = tmp("intkey");
    let pr = Projects::new(FsStore::new(&dir));
    std::fs::create_dir_all(dir.join("p")).unwrap();
    std::fs::write(dir.join("p/project.json"), "{\n  \"5\": 1,\n  \"schemaVersion\": 1,\n  \"name\": \"p\"\n}").unwrap();
    let mut o = block_on(pr.open_project("p", false, 1)).unwrap().unwrap();
    block_on(pr.save_meta("p", &mut o.assignment.meta, None)).unwrap();
    let txt = std::fs::read_to_string(dir.join("p/project.json")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(txt.starts_with("{\n  \"5\": 1,"), "integer key moved after the defaults: {txt}");
}
