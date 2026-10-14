//! Visual parity round: what is persisted must be what the JS app persists after the same actions.
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

/// `fs.openProject` ends with `S.reindex()`: `page.order` follows `meta.order` in memory, so the first save of
/// a page after opening (a decision, Done) writes its position, not whatever the file said.
#[test]
fn opening_a_project_reindexes_page_order_in_memory() {
    let dir = std::env::temp_dir().join(format!("vishion-vp-reindex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pr = Projects::new(FsStore::new(&dir));
    let entries = vec![
        ("zz/project.json".to_string(), br#"{"name":"Zed","order":["a","b","c"]}"#.to_vec()),
        ("zz/pages/a.json".to_string(), br#"{"id":"a","order":0,"raw":"x"}"#.to_vec()),
        ("zz/pages/b.json".to_string(), br#"{"id":"b","order":0,"raw":"y"}"#.to_vec()),
        ("zz/pages/c.json".to_string(), br#"{"id":"c","raw":"z"}"#.to_vec()),
    ];
    let slug = block_on(pr.import_files(&entries)).unwrap();
    let o = block_on(pr.open_project(&slug, true, 1)).unwrap().unwrap();
    let order = |id: &str| o.assignment.pages[id].get("order").cloned();
    assert_eq!(order("a"), Some(serde_json::json!(0)));
    assert_eq!(order("b"), Some(serde_json::json!(1)));
    assert_eq!(order("c"), Some(serde_json::json!(2)));
    // nothing was written by opening
    let text = String::from_utf8(block_on(pr.store.get(&format!("{slug}/pages/b.json"))).unwrap().unwrap()).unwrap();
    assert!(text.contains("\"order\":0"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
