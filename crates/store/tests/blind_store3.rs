//! Blind critic round 3 (phase 2 store). Concrete gaps, found by running the code.
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
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
    let d = std::env::temp_dir().join(format!("vishion-blind3-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// JS `normalize(JSON.parse("null"))` is a valid empty project (no restore from .bak). Rust treats a
/// project.json that is valid JSON but not an object as damaged and overwrites it from .bak.
#[test]
fn valid_json_non_object_project_json_is_not_restored_over() {
    for (i, txt) in ["null", "[]", "5", "\"x\"", "true", "{}"].iter().enumerate() {
        let dir = tmp(&format!("nul{i}"));
        let pr = Projects::new(FsStore::new(&dir));
        block_on(pr.store.put("q/project.json", txt.as_bytes())).unwrap();
        block_on(pr.store.put("q/project.json.bak", b"{\"name\":\"OLD\",\"order\":[]}")).unwrap();
        let r = block_on(pr.open_project("q", true, 5));
        let after = String::from_utf8(block_on(pr.store.get("q/project.json")).unwrap().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(after, *txt, "project.json {txt:?} was rewritten on open (result ok={})", r.is_ok());
    }
}

/// Every JS-written document of the oracle seed (tools/store-oracle/out/snapshot-folder.json) must
/// survive open + save of every page and project.json through the native store byte-identically.
#[test]
fn oracle_seed_documents_round_trip_natively() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/store-oracle/out/snapshot-folder.json");
    let Ok(txt) = std::fs::read_to_string(&p) else { return };
    let snap: serde_json::Value = serde_json::from_str(&txt).unwrap();
    let dir = tmp("seed");
    let pr = Projects::new(FsStore::new(&dir));
    let mut texts = std::collections::BTreeMap::new();
    for (k, v) in snap["files"].as_object().unwrap() {
        if v["type"] == "string" {
            let t = v["text"].as_str().unwrap().to_string();
            block_on(pr.store.put(k, t.as_bytes())).unwrap();
            texts.insert(k.clone(), t);
        }
    }
    let mut n = 0;
    for (k, _) in texts.iter().filter(|(k, _)| k.ends_with("/project.json")) {
        let slug = k.trim_end_matches("/project.json");
        let mut o = block_on(pr.open_project(slug, false, 1)).unwrap().unwrap();
        assert!(o.unreadable.is_empty(), "{slug}: {:?}", o.unreadable);
        for (_, page) in o.assignment.pages.clone() {
            block_on(pr.save_page(slug, &page)).unwrap();
            n += 1;
        }
        block_on(pr.save_meta(slug, &mut o.assignment.meta, None)).unwrap();
    }
    let mut bad = vec![];
    for (k, t) in &texts {
        if k.ends_with(".bak") { continue; }
        let got = String::from_utf8(block_on(pr.store.get(k)).unwrap().unwrap()).unwrap();
        if &got != t { bad.push(k.clone()); }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(n > 5);
    assert!(bad.is_empty(), "changed: {bad:?}");
}

/// A path whose parent is a FILE must read as "not there" (JS folder backend: TypeMismatchError is
/// "missing"; IDB: no such key), not as an I/O error that aborts open/list.
#[test]
fn file_in_place_of_directory_reads_as_missing() {
    let dir = tmp("filedir");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("settings.json", b"{}")).unwrap();
    let g = block_on(pr.store.get("settings.json/project.json"));
    let l = block_on(pr.store.list("settings.json/"));
    let c = block_on(pr.store.children("settings.json"));
    let o = block_on(pr.open_project("settings.json", false, 1));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(matches!(g, Ok(None)), "get: {g:?}");
    assert!(matches!(&l, Ok(v) if v.is_empty()), "list: {l:?}");
    assert!(matches!(&c, Ok(v) if v.is_empty()), "children: {c:?}");
    assert!(matches!(o, Ok(None)), "open: {:?}", o.map(|_| ()));
}

/// Hostile page ids in meta.order never create or touch anything outside the project root and never
/// abort the open.
#[test]
fn hostile_order_ids_are_contained() {
    let base = tmp("host");
    let root = base.join("root");
    let pr = Projects::new(FsStore::new(&root));
    let ids = ["../../outside", "..", "a/../../b", r"C:\x", "c:x", "con", "NUL.json", "", r"a\b", "/abs", "x/y", ".vishion-tmp-1-1", "p.", "q "];
    let order = serde_json::to_string(&ids).unwrap();
    block_on(pr.store.put("q/project.json", format!("{{\"name\":\"Q\",\"order\":{order}}}").as_bytes())).unwrap();
    std::fs::write(base.join("outside.json"), "SECRET").unwrap();
    let o = block_on(pr.open_project("q", true, 1)).unwrap().unwrap();
    for id in ids {
        let _ = block_on(pr.delete_page("q", id));
        let _ = block_on(pr.save_image("q", id, b"img"));
        let _ = block_on(pr.save_original("q", id, "png", b"img"));
        let _ = block_on(pr.save_original("q", "x", id, b"img"));
        let _ = block_on(pr.write_export("q", id, b"x"));
    }
    let _ = o;
    fn walk(d: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap() {
            let e = e.unwrap();
            out.push(e.path().to_string_lossy().replace(char::from(92u8), "/"));
            if e.file_type().unwrap().is_dir() { walk(&e.path(), out); }
        }
    }
    let mut all = vec![];
    walk(&base, &mut all);
    let outside: Vec<_> = all.iter().filter(|p| !p.contains("/root/") && !p.ends_with("/root")).collect();
    let secret = std::fs::read_to_string(base.join("outside.json")).unwrap();
    let _ = std::fs::remove_dir_all(&base);
    assert_eq!(secret, "SECRET");
    assert_eq!(outside.len(), 1, "files outside root: {outside:?}");
}

/// JS opens any valid JSON. A page whose text holds the noncharacter U+10FFFE followed by four hex
/// digits (d800..dfff) is "unreadable" here because that spelling collides with the lone-surrogate
/// marker. A per-document marker (a char absent from the text) removes the collision.
#[test]
fn page_with_marker_lookalike_text_opens() {
    let dir = tmp("marker");
    let pr = Projects::new(FsStore::new(&dir));
    let page = format!("{{\"id\":\"a\",\"raw\":\"x\u{10FFFE}dc00y\"}}");
    block_on(pr.store.put("q/project.json", br#"{"name":"Q","order":["a"]}"#)).unwrap();
    block_on(pr.store.put("q/pages/a.json", page.as_bytes())).unwrap();
    let o = block_on(pr.open_project("q", true, 1)).unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(o.unreadable.is_empty(), "valid JSON page reported unreadable: {:?}", o.unreadable);
}
