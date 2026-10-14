//! Blind critic, phase 2 re-check. Fixtures come from `tests/fixtures/gen_blind2.mjs` (JS `JSON.stringify`,
//! seed unused by the builder). Also: crash safety under SIGKILL-style process kill, hostile keys.
use serde_json::Value;
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

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("vishion-blind2-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/blind2.json")).unwrap()
}

#[test]
fn js_written_pages_survive_open_and_save_byte_identical() {
    let fx = fixtures();
    let pages = fx["pages"].as_array().unwrap();
    let dir = tmp("pages");
    let pr = Projects::new(FsStore::new(&dir));
    let ids: Vec<String> = pages.iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
    let order = serde_json::to_string(&ids).unwrap();
    block_on(pr.store.put("q/project.json", format!("{{\"name\":\"Q\",\"order\":{order}}}").as_bytes())).unwrap();
    for c in pages {
        let id = c["id"].as_str().unwrap();
        block_on(pr.store.put(&format!("q/pages/{id}.json"), c["text"].as_str().unwrap().as_bytes())).unwrap();
    }
    let o = block_on(pr.open_project("q", false, 1)).unwrap().unwrap();
    let mut bad = vec![];
    // documented limitation: literal U+10FFFE directly followed by surrogate-looking hex is refused
    for u in &o.unreadable {
        if !u.error.contains("unrepresentable") {
            bad.push(format!("unreadable: {} {}", u.id, u.error));
        }
    }
    for c in pages {
        let id = c["id"].as_str().unwrap();
        let Some(p) = o.assignment.pages.get(id) else { continue };
        block_on(pr.save_page("q", p)).unwrap();
        let got = String::from_utf8(block_on(pr.store.get(&format!("q/pages/{id}.json"))).unwrap().unwrap()).unwrap();
        // JS stringify(parse(text)) of a JS-written text is the text itself
        assert_eq!(c["resave"], c["text"], "generator sanity");
        if got != c["text"].as_str().unwrap() {
            bad.push(format!("{id}: {:?} != {:?}", got, c["text"]));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(bad.is_empty(), "{} diffs, first: {:?}", bad.len(), &bad[..bad.len().min(3)]);
}

#[test]
fn normalized_project_json_matches_js_pretty_output() {
    use vishion_core::model::Project;
    let fx = fixtures();
    let mut bad = vec![];
    for c in fx["metas"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        // JS reorders integer-like keys and Object.assign drops an own __proto__: out of scope here
        if text.contains("\"__proto__\"") || text.contains("\"10\"") || text.contains("\"2\"") || text.contains("\"4294967")  {
            continue;
        }
        let Ok(m) = Project::from_json(text) else {
            if !text.contains('􏿾') {
                bad.push(format!("parse failed: {text:?}"));
            }
            continue;
        };
        let got = m.normalized(7).to_json_pretty().unwrap();
        if got != c["normPretty"].as_str().unwrap() {
            bad.push(format!("{text:?}\n  got  {got:?}\n  want {:?}", c["normPretty"]));
        }
    }
    assert!(bad.is_empty(), "{} diffs, first: {}", bad.len(), bad.first().cloned().unwrap_or_default());
}

#[test]
fn damaged_documents_are_never_written() {
    let dir = tmp("dmg");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("q/project.json", b"{\"name\":\"Q\",\"order\":[\"a\",\"b\",\"c\",\"d\"]}")).unwrap();
    block_on(pr.store.put("q/pages/a.json", b"{\"id\":\"a\",\"raw\":\"ok\"}")).unwrap();
    block_on(pr.store.put("q/pages/b.json", b"{\"id\":\"b\",\"raw\":")).unwrap(); // truncated
    block_on(pr.store.put("q/pages/c.json", b"[1,2]")).unwrap(); // valid, not an object
    block_on(pr.store.put("q/pages/d.json", b"null")).unwrap();
    let snap = |dir: &std::path::Path| {
        let mut v = vec![];
        fn w(d: &std::path::Path, v: &mut Vec<(String, Vec<u8>)>) {
            for e in std::fs::read_dir(d).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    w(&e.path(), v)
                } else {
                    v.push((e.path().to_string_lossy().into(), std::fs::read(e.path()).unwrap()))
                }
            }
        }
        w(dir, &mut v);
        v.sort();
        v
    };
    let before = snap(&dir);
    let o = block_on(pr.open_project("q", true, 1)).unwrap().unwrap();
    assert_eq!(o.assignment.meta.order(), vec!["a", "b", "c", "d"], "unreadable pages must stay in order");
    assert_eq!(o.unreadable.len(), 3);
    assert_eq!(snap(&dir), before, "open must not write");
    // a damaged project.json with no backup: error and untouched
    block_on(pr.store.put("z/project.json", b"{\"name\":")).unwrap();
    let before = snap(&dir);
    assert!(block_on(pr.open_project("z", true, 1)).is_err());
    assert!(block_on(pr.rename_project("z", "n", 1)).is_err());
    assert!(block_on(pr.duplicate_project("z", 1)).is_err());
    assert!(block_on(pr.list_projects(1)).unwrap().iter().all(|s| s.slug != "z"));
    assert_eq!(snap(&dir), before, "failed parse must not write");
    // both damaged
    block_on(pr.store.put("z/project.json.bak", b"nope")).unwrap();
    let before = snap(&dir);
    assert!(block_on(pr.open_project("z", true, 1)).is_err());
    assert_eq!(snap(&dir), before);
    // saving meta over a damaged project.json must not replace a good .bak with garbage
    block_on(pr.store.put("y/project.json", b"{\"name\":\"Y\"}")).unwrap();
    let mut m = block_on(pr.meta("y", 1)).unwrap().unwrap();
    block_on(pr.save_meta("y", &mut m, Some(2))).unwrap();
    block_on(pr.store.put("y/project.json", b"GARBAGE")).unwrap();
    block_on(pr.save_meta("y", &mut m, Some(3))).unwrap();
    let bak = block_on(pr.store.get("y/project.json.bak")).unwrap().unwrap();
    assert!(Value::from(serde_json::from_slice::<Value>(&bak).is_ok()).as_bool().unwrap(), "bak replaced by garbage");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_keys_cannot_escape_root() {
    let base = tmp("esc");
    let root = base.join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(base.join("secret.txt"), b"s").unwrap();
    let pr = Projects::new(FsStore::new(&root));
    let hostile = [
        "../secret.txt", "a/../../secret.txt", "..\\secret.txt", "a/..\\..\\secret.txt", "C:/Windows/x", "C:\\x", "/abs", "\\\\server\\share\\x",
        "\\\\?\\C:\\x", "a/b:stream", "a/b::$DATA", "a/CON", "a/con.json", "a/Nul.", "a/ aux", "a/COM1.txt", "a/LPT1", "a/\u{0}", "a/.vishion-tmp-1-2",
        "a/..", "a/./b", "a/b/", "~", "a/CLOCK$", "a/CONIN$", "a/COM\u{b9}", "a/PRN .x", "a/A\u{202e}b",
    ];
    for k in hostile {
        let r = block_on(pr.store.put(k, b"pwn"));
        let _ = block_on(pr.store.get(k));
        let _ = block_on(pr.store.delete(k));
        let _ = block_on(pr.store.delete_tree(k));
        let _ = block_on(pr.store.list(k));
        let _ = block_on(pr.store.children(k));
        let _ = r;
    }
    // hostile ids / slugs through the project layer
    let _ = block_on(pr.delete_page("q", "../../../secret"));
    let _ = block_on(pr.delete_page("..", "x"));
    let _ = block_on(pr.delete_project(".."));
    let _ = block_on(pr.delete_project("../root"));
    let _ = block_on(pr.save_image("../..", "x", b"pwn"));
    let _ = block_on(pr.save_original("q", "..", "jpg/../../../x", b"pwn"));
    let _ = block_on(pr.write_export("q", "../../../pwn.md", b"pwn"));
    let entries = vec![
        ("p/project.json".to_string(), b"{\"name\":\"I\"}".to_vec()),
        ("p/../../pwn.txt".to_string(), b"pwn".to_vec()),
        ("p/..\\..\\pwn2.txt".to_string(), b"pwn".to_vec()),
        ("p/C:\\pwn3.txt".to_string(), b"pwn".to_vec()),
    ];
    let _ = block_on(pr.import_files(&entries));
    assert_eq!(std::fs::read(base.join("secret.txt")).unwrap(), b"s");
    let mut leaked = vec![];
    for e in std::fs::read_dir(&base).unwrap() {
        leaked.push(e.unwrap().file_name().to_string_lossy().to_string());
    }
    leaked.sort();
    assert_eq!(leaked, vec!["root", "secret.txt"], "files created outside root");
    // everything created is below root and a plain file/dir
    fn count(d: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap() {
            let e = e.unwrap();
            out.push(e.path().to_string_lossy().into());
            if e.file_type().unwrap().is_dir() {
                count(&e.path(), out)
            }
        }
    }
    let mut all = vec![];
    count(&root, &mut all);
    let _ = std::fs::remove_dir_all(&base);
    eprintln!("created under root: {all:?}");
}

#[test]
fn mem_store_rejects_what_fs_rejects_for_project_layer_paths() {
    // MemStore / IdbStore use check_key only. Names the native store refuses (and Windows aliases) become
    // real keys there: the same project exported from IDB can then fail to import natively.
    let pr = Projects::new(MemStore::new());
    for k in ["p/a.", "p/NUL", "p/a:b"] {
        let _ = block_on(pr.store.put(k, b"x"));
    }
    // informational only
    eprintln!("mem keys: {:?}", block_on(pr.store.list("")).unwrap());
}

/// Child half of the kill test: hammer one project.json with two alternating big documents.
#[test]
fn kill_child_writer() {
    let Ok(dir) = std::env::var("BLIND2_KILL_DIR") else { return };
    let s = FsStore::new(dir);
    let a = format!("{{\"v\":\"A\",\"pad\":\"{}\"}}", "a".repeat(300_000));
    let b = format!("{{\"v\":\"B\",\"pad\":\"{}\"}}", "b".repeat(300_000));
    let pr = Projects::new(s);
    let mut i = 0u64;
    loop {
        let t = if i % 2 == 0 { &a } else { &b };
        block_on(pr.safe_json("k/project.json", t)).unwrap();
        i += 1;
    }
}

#[test]
#[ignore = "slow: spawns and kills a writer 6 times; run with --ignored (passed in phase 2 critic)"]
fn killed_writer_never_leaves_a_torn_project_json() {
    let dir = tmp("kill");
    let exe = std::env::current_exe().unwrap();
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("k/project.json", b"{\"v\":\"A\",\"pad\":\"\"}")).unwrap();
    let mut seed = 0x9e3779b9u32;
    for round in 0..6 {
        let mut child = std::process::Command::new(&exe)
            .args(["kill_child_writer", "--exact", "--nocapture", "--test-threads=1"])
            .env("BLIND2_KILL_DIR", &dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        std::thread::sleep(std::time::Duration::from_millis(150 + (seed % 400) as u64));
        eprintln!("round {round}: killing");
        child.kill().unwrap();
        child.wait().unwrap();
        for f in ["k/project.json", "k/project.json.bak"] {
            let Ok(Some(b)) = block_on(pr.store.get(f)) else { continue };
            let v: Result<Value, _> = serde_json::from_slice(&b);
            assert!(v.is_ok(), "round {round}: {f} is torn ({} bytes)", b.len());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// On a case-insensitive filesystem (Windows, default macOS) a hand-made or imported folder `Quiz` and the
/// slug `quiz` are the same directory, but `unique_slug` compares names case-sensitively, so
/// `create_project("Quiz")` writes a blank project.json over the existing project.
#[test]
#[cfg(any(windows, target_os = "macos"))]
fn create_project_must_not_overwrite_a_case_aliased_project() {
    let dir = tmp("case");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("Quiz/project.json", b"{\"name\":\"Mine\",\"order\":[\"a\"]}")).unwrap();
    let slug = block_on(pr.create_project("Quiz", 5)).unwrap();
    let after = String::from_utf8(block_on(pr.store.get("Quiz/project.json")).unwrap().unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(after.contains("\"Mine\""), "existing project overwritten via slug {slug:?}: {after}");
}

/// `list` returns every file the OS lists, `get` refuses names the native key rules reject (here a
/// Windows device name created with the `\?\` prefix, on unix a ':' name). One such file makes
/// duplicate_project and project_files (project zip) fail for the whole project.
#[test]
fn one_unreadable_name_must_not_abort_duplicate_or_zip() {
    let dir = tmp("weird");
    let pr = Projects::new(FsStore::new(&dir));
    block_on(pr.store.put("q/project.json", b"{\"name\":\"Q\",\"order\":[]}")).unwrap();
    let bs = '\\';
    let verbatim = format!("{bs}{bs}?{bs}{}", dir.display());
    let weird = if cfg!(windows) { format!("{verbatim}{bs}q{bs}aux.txt") } else { format!("{}/q/a:b.txt", dir.display()) };
    std::fs::write(&weird, b"x").unwrap();
    let files = block_on(pr.project_files("q"));
    let dup = block_on(pr.duplicate_project("q", 9));
    let _ = std::fs::remove_dir_all(&verbatim);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(files.is_ok(), "project_files aborted: {:?}", files.err());
    assert!(dup.is_ok(), "duplicate_project aborted: {:?}", dup.err());
}

/// A project.json that is valid JSON (the JS app opens it) but that `lenient` refuses (literal U+10FFFE
/// directly followed by `d800`..`dfff` hex digits, e.g. in `prompt`) is treated as damaged: `read_meta` falls
/// back to the older `.bak`, and with `restore` (what `open_project(.., true)` does, as the JS app) the good
/// file is replaced by the stale backup; the project then shows `order: []`. Found by tools/store-oracle/
/// blind2.mjs (SEED=424243). A refusal that is not a syntax error must surface as an error, never as a restore.
#[test]
fn unrepresentable_but_valid_project_json_must_not_be_replaced_by_the_backup() {
    let dir = tmp("unrep");
    let pr = Projects::new(FsStore::new(&dir));
    let good = "{\"name\":\"N\",\"prompt\":\"\u{10FFFE}d800\",\"order\":[\"a\",\"b\"]}";
    block_on(pr.store.put("q/project.json", good.as_bytes())).unwrap();
    block_on(pr.store.put("q/project.json.bak", b"{\"name\":\"N\",\"prompt\":\"\",\"order\":[]}")).unwrap();
    let r = block_on(pr.open_project("q", true, 1));
    let now = String::from_utf8(block_on(pr.store.get("q/project.json")).unwrap().unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(now, good, "project.json replaced by the stale backup (open result ok={})", r.is_ok());
}
