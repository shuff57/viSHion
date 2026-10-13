//! Regression tests for the phase 2 critic gaps that are not in blind_store.rs.
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
    let d = std::env::temp_dir().join(format!("vishion-p2fix-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn trailing_dot_and_device_names_are_rejected_not_aliased() {
    let dir = tmp("alias");
    let s = FsStore::new(&dir);
    for k in ["p/a.", "p/a ", "p/NUL", "p/aux.txt", "p/COM3", "p/lpt9.x"] {
        assert!(block_on(s.put(k, b"x")).is_err(), "{k}");
        assert!(block_on(s.get(k)).is_err(), "{k}");
    }
    assert!(block_on(s.put("p/com0.txt", b"x")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_skips_unstorable_names_instead_of_aborting() {
    let dir = tmp("import");
    let pr = Projects::new(FsStore::new(&dir));
    let entries = vec![
        ("zz/project.json".to_string(), br#"{"name":"Zed","order":["a"]}"#.to_vec()),
        ("zz/pages/NUL".to_string(), b"junk".to_vec()),
        ("zz/pages/a.json".to_string(), br#"{"id":"a","raw":"x"}"#.to_vec()),
    ];
    let slug = block_on(pr.import_files(&entries)).unwrap();
    assert!(block_on(pr.store.get(&format!("{slug}/pages/a.json"))).unwrap().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn empty_directories_are_visible_to_unique_slug() {
    let dir = tmp("emptydir");
    std::fs::create_dir_all(dir.join("quiz")).unwrap();
    let pr = Projects::new(FsStore::new(&dir));
    assert_eq!(block_on(pr.store.children("")).unwrap(), vec![("quiz".to_string(), true)]);
    assert_eq!(block_on(pr.create_project("Quiz", 1)).unwrap(), "quiz-2");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_tree_on_fs_rejects_empty_path() {
    let dir = tmp("wipe");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("a/project.json", b"{}")).unwrap();
    assert!(block_on(pr.delete_project("")).is_err());
    assert!(block_on(pr.store.delete_tree("/")).is_err());
    assert_eq!(block_on(pr.store.list("")).unwrap(), vec!["a/project.json"]);
    let _ = std::fs::remove_dir_all(&dir);
}
