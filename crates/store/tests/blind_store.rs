//! Blind critic (phase 2 store). Each test is a concrete gap found by running the code, not by reading
//! the commit message. They fail on purpose until the product code is fixed.
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
use vishion_store::fs::FsStore;
use vishion_store::mem::MemStore;
use vishion_store::projects::Projects;
use vishion_store::Store;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("future suspended"),
    }
}

/// `IdbStore::delete_tree` and the default `Store::delete_tree` (MemStore) do not validate the path.
/// An empty / "/" slug turns `dir_prefix` into "" and every key of the store matches, so one bad call
/// wipes ALL projects. FsStore and FsaStore reject it (InvalidKey); the JS app's `fs.remove('')` removes
/// nothing. Repro on MemStore (same code path as IdbStore: idb.rs `delete_tree`).
#[test]
fn delete_tree_with_empty_path_must_not_wipe_the_store() {
    let pr = Projects::new(MemStore::new());
    block_on(pr.store.put("a/project.json", b"{}")).unwrap();
    block_on(pr.store.put("b/project.json", b"{}")).unwrap();
    let _ = block_on(pr.delete_project(""));
    let _ = block_on(pr.store.delete_tree("/"));
    assert_eq!(
        block_on(pr.store.list("")).unwrap(),
        vec!["a/project.json", "b/project.json"],
        "delete_project(\"\") / delete_tree(\"/\") deleted other projects"
    );
}

/// `Projects::save_page` names the file after the page's own `id` field, the JS app names it after the
/// `meta.order` key (`fs.savePage(id)` -> `slug/pages/<id>.json`). A page file without `id` (or whose
/// `id` differs from its file name) is therefore saved to `pages/.json` / `pages/<other>.json`: the
/// edit is written somewhere the project never reads and is lost on reopen.
#[test]
fn save_page_must_write_the_file_the_page_was_loaded_from() {
    let dir = std::env::temp_dir().join(format!("vishion-blind-noid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("q/project.json", br#"{"name":"Q","order":["a","b"]}"#)).unwrap();
    block_on(pr.store.put("q/pages/a.json", br#"{"name":"x.jpg","status":"done","raw":"hello"}"#)).unwrap();
    block_on(pr.store.put("q/pages/b.json", br#"{"id":"zzz","name":"y.jpg","status":"done","raw":"hi"}"#)).unwrap();
    let mut o = block_on(pr.open_project("q", true, 5)).unwrap().unwrap();
    for id in ["a", "b"] {
        let p = o.assignment.pages.get_mut(id).unwrap();
        p.set("corrected", serde_json::json!("EDITED"));
        let p = p.clone();
        block_on(pr.save_page("q", &p)).unwrap();
    }
    let keys = block_on(pr.store.list("")).unwrap();
    let a = block_on(pr.store.get("q/pages/a.json")).unwrap().unwrap();
    let b = block_on(pr.store.get("q/pages/b.json")).unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(String::from_utf8_lossy(&a).contains("EDITED"), "edit of page a lost; keys: {keys:?}");
    assert!(String::from_utf8_lossy(&b).contains("EDITED"), "edit of page b lost; keys: {keys:?}");
    assert!(keys.iter().all(|k| k != "q/pages/.json" && k != "q/pages/zzz.json"), "stray files: {keys:?}");
}
