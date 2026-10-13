//! Blind critic round 4 (phase 2 store). Concrete gaps, found by running the code.
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
    let d = std::env::temp_dir().join(format!("vishion-blind4-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A project named "Con" / "Aux" / "Nul" / "Prn" / "Com1" / "Lpt1" slugs to a Windows device name. The JS app
/// in browser storage (IDB) creates it fine (slug `con`). FsStore rejects the slug on every OS, so
/// create_project fails; it must pick a usable slug (e.g. `con-2`) instead.
#[test]
fn project_named_like_a_device_can_be_created() {
    for name in ["Con", "Aux", "Nul", "Prn", "Com1", "Lpt1", "COM9"] {
        let dir = tmp("dev");
        let pr = Projects::new(FsStore::new(&dir));
        let r = block_on(pr.create_project(name, 5));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(r.is_ok(), "create_project({name:?}) failed: {:?}", r.err().map(|e| e.to_string()));
    }
}

/// import_files of a zip made by the JS app for a project named "Nul": every file is silently dropped
/// (InvalidKey ignored), the import reports success and the project is empty.
#[test]
fn import_of_device_named_project_must_not_silently_drop_files() {
    let dir = tmp("impdev");
    let pr = Projects::new(FsStore::new(&dir));
    let entries = vec![
        ("nul/project.json".to_string(), br#"{"name":"Nul","order":["a"]}"#.to_vec()),
        ("nul/pages/a.json".to_string(), br#"{"id":"a","status":"done"}"#.to_vec()),
    ];
    let slug = block_on(pr.import_files(&entries));
    let ok = match &slug {
        Ok(s) => block_on(pr.store.get(&format!("{s}/pages/a.json"))).ok().flatten().is_some(),
        Err(_) => true, // refusing loudly is acceptable
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "import returned {slug:?} but the page file was silently dropped");
}

/// Crash safety: a writer process killed at a random moment must leave project.json as a complete old or
/// new version (never truncated / mixed), and `.bak` must also be complete.
#[test]
#[ignore = "slow (spawns and kills writer processes); run with --ignored"]
fn killed_writer_never_corrupts_project_json() {
    if let Ok(dir) = std::env::var("BLIND4_CHILD_DIR") {
        let pr = Projects::new(FsStore::new(dir));
        for i in 0u64.. {
            let pad = "x".repeat(3_000_000 + (i % 2) as usize * 1000);
            let doc = format!("{{\"name\":\"v{}\",\"order\":[],\"pad\":\"{pad}\"}}", i % 2);
            block_on(pr.safe_json("q/project.json", &doc)).unwrap();
        }
        return;
    }
    let dir = tmp("kill");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("q/project.json", b"{\"name\":\"v1\",\"order\":[]}")).unwrap();
    for round in 0..4u64 {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["killed_writer_never_corrupts_project_json", "--exact", "--nocapture"])
            .env("BLIND4_CHILD_DIR", &dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150 + (round * 37) % 400));
        let _ = child.kill();
        let _ = child.wait();
        for f in ["q/project.json", "q/project.json.bak"] {
            if let Some(b) = block_on(pr.store.get(f)).unwrap() {
                assert!(serde_json::from_slice::<serde_json::Value>(&b).is_ok(), "{f} corrupt after kill in round {round} ({} bytes)", b.len());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Valid JSON nested deeper than serde_json's recursion limit (128) is valid for `JSON.parse`. The Rust
/// store calls it damaged and, with restore, overwrites it from `.bak` (data loss); an unreadable page
/// must at least be left alone.
#[test]
fn deeply_nested_but_valid_project_json_is_not_restored_over() {
    let dir = tmp("deep");
    let pr = Projects::new(FsStore::new(&dir));
    let deep = format!("{{\"name\":\"NEW\",\"order\":[],\"x\":{}1{}}}", "[".repeat(300), "]".repeat(300));
    block_on(pr.store.put("q/project.json", deep.as_bytes())).unwrap();
    block_on(pr.store.put("q/project.json.bak", br#"{"name":"OLD","order":[]}"#)).unwrap();
    let _ = block_on(pr.open_project("q", true, 5));
    let after = String::from_utf8(block_on(pr.store.get("q/project.json")).unwrap().unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(after, deep, "valid project.json was overwritten from .bak");
}

/// `meta.order` entries that are not strings (hand edited / other tools): JS builds the path with string
/// concatenation and loads `pages/1.json`. Rust skips the entry on load and, as soon as another id is
/// dropped, rewrites `order` without it.
#[test]
fn non_string_order_entries_are_not_dropped_from_project_json() {
    let dir = tmp("numid");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("q/project.json", br#"{"name":"Q","order":[1,"gone","b"]}"#)).unwrap();
    block_on(pr.store.put("q/pages/1.json", br#"{"id":"1","status":"done"}"#)).unwrap();
    block_on(pr.store.put("q/pages/b.json", br#"{"id":"b","status":"done"}"#)).unwrap();
    let mut o = block_on(pr.open_project("q", true, 5)).unwrap().unwrap();
    block_on(pr.save_meta("q", &mut o.assignment.meta, None)).unwrap();
    let after = String::from_utf8(block_on(pr.store.get("q/project.json")).unwrap().unwrap()).unwrap();
    let loaded = o.assignment.pages.contains_key("1");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(loaded, "page 1 (order entry 1) not loaded though pages/1.json exists");
    assert!(after.contains('1') && after.contains("\"order\": [\n    1"), "order entry 1 dropped on save: {after}");
}
