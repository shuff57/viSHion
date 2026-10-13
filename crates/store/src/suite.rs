//! The project-layer suite. Every backend must pass it: native `FsStore` and `MemStore` in `cargo test`,
//! `IdbStore` and `FsaStore` inside Chrome through the store oracle (tools/store-oracle). It needs an
//! empty store and reports the first failed check as `Err(message)` (no panics, so a wasm trap never
//! hides which check failed).
use crate::projects::{Projects, SETTINGS_PATH};
use crate::{Result, Store, StoreError};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use vishion_core::import::{self, Options};
use vishion_core::model::{Assignment, Page, Project, Settings};

const T0: i64 = 1_760_000_000_000;

macro_rules! check {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

fn e<T>(r: Result<T>) -> std::result::Result<T, String> {
    r.map_err(|x: StoreError| x.to_string())
}

async fn ea<T>(f: impl std::future::Future<Output = Result<T>>) -> std::result::Result<T, String> {
    e(f.await)
}

fn page_of(id: &str, raw: &str) -> Page {
    let mut p = Assignment::new_page(id, &format!("{id}.jpg"));
    p.set("raw", json!(raw));
    p.set("corrected", json!(raw));
    p.set("status", json!("done"));
    p
}

/// Run the whole suite against an empty store.
pub async fn run<S: Store>(store: S) -> std::result::Result<(), String> {
    let pr = Projects::new(store);

    // ---- raw store behaviour
    ea(pr.store.put("zz-raw/a.bin", &[0, 1, 2, 250, 255])).await?;
    check!(e(pr.store.get("zz-raw/a.bin").await)? == Some(vec![0, 1, 2, 250, 255]), "binary round trip");
    check!(e(pr.store.get("zz-raw/none").await)?.is_none(), "missing key is None");
    for bad in ["", "/", "a//b", "../x", "a/../b", "a\\b"] {
        check!(pr.store.put(bad, b"x").await.is_err(), "key {bad:?} must be rejected");
    }
    ea(pr.store.put("zz-raw/d/ab.txt", b"1")).await?;
    ea(pr.store.put("zz-raw/d/a/b.txt", b"2")).await?;
    check!(
        e(pr.store.children("zz-raw").await)? == vec![("a.bin".to_string(), false), ("d".to_string(), true)],
        "children"
    );
    check!(e(pr.store.list("zz-raw/d/a/").await)? == vec!["zz-raw/d/a/b.txt".to_string()], "list prefix is a path prefix");
    ea(pr.store.delete_tree("zz-raw/d/a")).await?;
    check!(e(pr.store.list("zz-raw/d/").await)? == vec!["zz-raw/d/ab.txt".to_string()], "delete_tree must not touch a sibling that shares a name prefix");
    ea(pr.store.delete_tree("zz-raw")).await?;
    check!(e(pr.store.list("").await)?.is_empty(), "delete_tree empties the tree");
    check!(e(pr.list_projects(T0).await)?.is_empty(), "no projects in an empty store");

    // ---- create
    let slug = e(pr.create_project("Algebra: Quadratics", T0).await)?;
    check!(slug == "algebra-quadratics", "slug {slug}");
    check!(e(pr.create_project("Algebra: Quadratics", T0).await)? == "algebra-quadratics-2", "unique slug");
    check!(e(pr.create_project("", T0).await)? == "project", "empty name slug");
    let created = e(pr.read_text("algebra-quadratics/project.json").await)?.ok_or("project.json missing")?;
    let want = "{\n  \"schemaVersion\": 1,\n  \"name\": \"Algebra: Quadratics\",\n  \"topic\": \"General\",\n  \"prompt\": \"\",\n  \"order\": [],\n  \"created\": 1760000000000,\n  \"updated\": 1760000000000,\n  \"ui\": {},\n  \"stats\": {\n    \"pages\": 0,\n    \"done\": 0\n  }\n}";
    check!(created == want, "created project.json differs:\n{created}");
    check!(e(pr.store.get("algebra-quadratics/project.json.bak").await)?.is_none(), "no .bak at creation");

    // ---- open, add pages, save (bak on every save after creation)
    let mut o = e(pr.open_project(&slug, true, T0).await)?.ok_or("open failed")?;
    check!(o.assignment.pages.is_empty() && o.unreadable.is_empty(), "empty project");
    let mut ids = vec![];
    for (i, raw) in ["first page", "second \u{1F600} é", "third"].iter().enumerate() {
        let id = format!("pg{}", i + 1);
        let mut p = page_of(&id, raw);
        p.set("boxes", json!([{"x": 0, "y": 0.1, "w": 1, "h": 0.30000000000000004, "r": 0}]));
        p.set("futureField", json!({"z": [1, 2, {"a": null}], "n": 1e21}));
        o.assignment.add_pages(vec![p]);
        ids.push(id);
    }
    for id in &ids {
        e(pr.save_page(&slug, &o.assignment.pages[id]).await)?;
        e(pr.save_image(&slug, id, &[0xff, 0xd8, 0xff, 0xd9, id.len() as u8]).await)?;
    }
    let first = e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap();
    e(pr.save_meta(&slug, &mut o.assignment.meta, Some(T0 + 1)).await)?;
    let bak = e(pr.read_text(&format!("{slug}/project.json.bak")).await)?.ok_or(".bak missing after a save")?;
    check!(bak == first, ".bak must hold the previous project.json");
    let o2 = e(pr.open_project(&slug, true, T0).await)?.ok_or("reopen failed")?;
    check!(o2.assignment == o.assignment, "reopened project differs from the saved one");
    check!(o2.assignment.meta.order() == ids, "order");
    check!(o2.assignment.meta.get("updated") == Some(&json!(T0 + 1)), "updated");
    check!(o2.selected_id().as_deref() == Some("pg1"), "selected id");
    let pt = e(pr.read_text(&format!("{slug}/pages/pg1.json")).await)?.unwrap();
    check!(pt.contains("\"h\":0.30000000000000004") && pt.contains("\"n\":1e+21") && !pt.contains('\n'), "page text kept verbatim: {pt}");
    // pages: rewriting an untouched page is byte-identical
    e(pr.save_page(&slug, &o2.assignment.pages["pg1"]).await)?;
    check!(e(pr.read_text(&format!("{slug}/pages/pg1.json")).await)?.unwrap() == pt, "untouched page rewrite must be byte-identical");
    check!(e(pr.load_image(&slug, "pg2").await)? == Some(vec![0xff, 0xd8, 0xff, 0xd9, 3]), "image");

    // ---- summaries
    let sums = e(pr.list_projects(T0).await)?;
    check!(sums.len() == 3, "three projects listed, got {}", sums.len());
    check!(sums[0].slug == "algebra-quadratics" && sums[0].pages == 3 && sums[0].name == "Algebra: Quadratics", "newest first with page count: {:?}", sums[0]);

    // ---- crash recovery is in memory only
    let mut t = page_of("pg3", "third");
    t.set("status", json!("transcribing"));
    e(pr.save_page(&slug, &t).await)?;
    let tx = e(pr.read_text(&format!("{slug}/pages/pg3.json")).await)?.unwrap();
    let o3 = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(o3.assignment.pages["pg3"].status() == "pending", "transcribing becomes pending in memory");
    check!(e(pr.read_text(&format!("{slug}/pages/pg3.json")).await)?.unwrap() == tx, "opening must not write");

    // ---- lone surrogates: load, edit, write back, untouched content byte-identical
    let lone = "{\"id\":\"pg2\",\"name\":\"pg2.jpg\",\"order\":1,\"status\":\"done\",\"raw\":\"lone \\ud800 and \\udfff end\",\"corrected\":\"lone \\ud800 and \\udfff end\",\"corrections\":[],\"boxes\":[],\"timings\":{\"ms\":0,\"tokens\":0},\"error\":\"\",\"rotation\":0,\"student\":null,\"reviewed\":false,\"resolved\":{},\"edits\":[],\"guesses\":{\"k\\ud83d\":1}}";
    ea(pr.write_text(&format!("{slug}/pages/pg2.json"), lone)).await?;
    let mut o4 = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(o4.unreadable.is_empty() && o4.assignment.pages.contains_key("pg2"), "lone-surrogate page must load: {:?}", o4.unreadable);
    e(pr.save_page(&slug, &o4.assignment.pages["pg2"]).await)?;
    check!(e(pr.read_text(&format!("{slug}/pages/pg2.json")).await)?.unwrap() == lone, "lone-surrogate page must write back byte-identical");
    let mut edited = o4.assignment.pages["pg2"].clone();
    let corrected = format!("{}!", edited.corrected());
    edited.set_carried("corrected", json!(corrected));
    edited.set("reviewed", json!(true));
    e(pr.save_page(&slug, &edited).await)?;
    let after = e(pr.read_text(&format!("{slug}/pages/pg2.json")).await)?.unwrap();
    let expect = lone
        .replace("\"corrected\":\"lone \\ud800 and \\udfff end\"", "\"corrected\":\"lone \\ud800 and \\udfff end!\"")
        .replace("\"reviewed\":false", "\"reviewed\":true");
    check!(after == expect, "edit of a lone-surrogate page: untouched content must stay byte-identical:\n{after}");
    // meta with a lone surrogate
    let m_lone = "{\n  \"schemaVersion\": 1,\n  \"name\": \"n\\ud800\",\n  \"topic\": \"General\",\n  \"prompt\": \"\",\n  \"order\": [\n    \"pg1\"\n  ],\n  \"created\": 1,\n  \"updated\": 2,\n  \"ui\": {},\n  \"stats\": null\n}";
    ea(pr.write_text(&format!("{slug}/project.json"), m_lone)).await?;
    let mut o5 = e(pr.open_project(&slug, true, T0).await)?.ok_or("lone-surrogate project.json must open")?;
    check!(!o5.from_backup, "lone-surrogate project.json is not damaged");
    ea(pr.save_meta(&slug, &mut o5.assignment.meta, None)).await?;
    check!(e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap() == m_lone, "lone-surrogate project.json byte-identical");
    check!(e(pr.read_text(&format!("{slug}/project.json.bak")).await)?.unwrap() == m_lone, "bak made from a lone-surrogate project.json");
    // restore the three-page project
    o4.assignment.meta.set_order(&ids);
    ea(pr.save_meta(&slug, &mut o4.assignment.meta, Some(T0 + 2))).await?;

    // ---- an unparsable page is kept in the order and never rewritten
    let garbage = "{\"id\":\"pg3\",\"raw\": \"trunc";
    ea(pr.write_text(&format!("{slug}/pages/pg3.json"), garbage)).await?;
    let mut o6 = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(o6.unreadable.len() == 1 && o6.unreadable[0].id == "pg3", "unreadable page reported");
    check!(o6.assignment.meta.order() == ids, "unreadable page stays in meta.order");
    check!(!o6.assignment.pages.contains_key("pg3"), "unreadable page is not loaded");
    ea(pr.save_meta(&slug, &mut o6.assignment.meta, Some(T0 + 3))).await?;
    check!(e(pr.read_text(&format!("{slug}/pages/pg3.json")).await)?.unwrap() == garbage, "unreadable page untouched");
    check!(
        e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap().contains("\"pg3\""),
        "saved meta keeps the unreadable id"
    );
    ea(pr.save_page(&slug, &page_of("pg3", "third"))).await?;

    // ---- a missing page is dropped from the order (like JS)
    ea(pr.store.delete(&format!("{slug}/pages/pg3.json"))).await?;
    let o7 = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(o7.assignment.meta.order() == ["pg1", "pg2"], "missing page dropped: {:?}", o7.assignment.meta.order());
    ea(pr.save_page(&slug, &page_of("pg3", "third"))).await?;

    // ---- damaged project.json: backup used, nothing overwritten unless restore
    let good = e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap();
    ea(pr.write_text(&format!("{slug}/project.json.bak"), &good)).await?;
    ea(pr.write_text(&format!("{slug}/project.json"), "{\"name\": \"Alg")).await?;
    let ob = e(pr.open_project(&slug, false, T0).await)?.unwrap();
    check!(ob.from_backup && !ob.restored, "opened from backup without restoring");
    check!(e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap() == "{\"name\": \"Alg", "no restore, no write");
    let ob = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(ob.from_backup && ob.restored, "restored");
    check!(e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap() == good, "restored bytes");
    check!(
        e(pr.read_text(&format!("{slug}/project.json.damaged")).await)?.is_none(),
        "JS writes no .damaged copy"
    );
    // marker-lookalike text (U+10FFFE + hex digits) opens via a per-document marker, never replaced
    let unrep = "{\"name\":\"N\",\"prompt\":\"\u{10FFFE}d800\",\"order\":[\"a\"]}";
    ea(pr.write_text(&format!("{slug}/project.json"), unrep)).await?;
    let ou = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(!ou.from_backup && ou.assignment.meta.prompt() == "\u{10FFFE}d800", "lookalike project opens as is");
    check!(e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap() == unrep, "unrepresentable project.json untouched");
    ea(pr.write_text(&format!("{slug}/project.json"), "{\"name\": \"Alg")).await?;
    // slug taken case-insensitively: never overwrite
    ea(pr.write_text("CaseKeep/project.json", "{\"name\":\"Mine\",\"order\":[]}")).await?;
    let ck = e(pr.create_project("CaseKeep", T0).await)?;
    check!(ck.to_lowercase() != "casekeep", "slug {ck:?} aliases CaseKeep");
    check!(e(pr.read_text("CaseKeep/project.json").await)?.unwrap().contains("Mine"), "existing project kept");
    ea(pr.delete_project("CaseKeep")).await?;
    ea(pr.delete_project(&ck)).await?;
    // both damaged: error, nothing written
    ea(pr.write_text(&format!("{slug}/project.json"), "{oops")).await?;
    ea(pr.write_text(&format!("{slug}/project.json.bak"), "{also oops")).await?;
    check!(matches!(pr.open_project(&slug, true, T0).await, Err(StoreError::Parse { .. })), "unusable project is an error");
    check!(e(pr.read_text(&format!("{slug}/project.json")).await)?.unwrap() == "{oops", "unusable project.json untouched");
    check!(e(pr.list_projects(T0).await)?.iter().all(|s| s.slug != slug), "unusable project not listed");
    ea(pr.write_text(&format!("{slug}/project.json"), &good)).await?;
    ea(pr.store.delete(&format!("{slug}/project.json.damaged"))).await?;

    // ---- bak is not refreshed from a damaged project.json
    let mut om = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    ea(pr.write_text(&format!("{slug}/project.json"), "{broken")).await?;
    ea(pr.write_text(&format!("{slug}/project.json.bak"), &good)).await?;
    ea(pr.save_meta(&slug, &mut om.assignment.meta, None)).await?;
    check!(e(pr.read_text(&format!("{slug}/project.json.bak")).await)?.unwrap() == good, "damaged copy must not replace the backup");

    // ---- originals + delete_page
    ea(pr.save_original(&slug, "pg1", "png", &[1, 2, 3])).await?;
    ea(pr.save_original(&slug, "pg10", "png", &[4])).await?;
    ea(pr.delete_page(&slug, "pg1")).await?;
    check!(e(pr.store.get(&format!("{slug}/pages/pg1.json")).await)?.is_none(), "page file deleted");
    check!(e(pr.load_image(&slug, "pg1").await)?.is_none(), "image deleted");
    check!(e(pr.store.get(&format!("{slug}/originals/pg1.png")).await)?.is_none(), "original deleted");
    check!(e(pr.store.get(&format!("{slug}/originals/pg10.png")).await)?.is_some(), "other original kept");

    // ---- rename (plain write, no .bak refresh), exports, duplicate, zip round trip, delete
    ea(pr.write_text(&format!("{slug}/project.json.bak"), "keep")).await?;
    check!(e(pr.rename_project(&slug, "Algebra II", T0 + 9).await)?, "rename");
    check!(e(pr.read_text(&format!("{slug}/project.json.bak")).await)?.unwrap() == "keep", "rename writes no .bak");
    let mut o8 = e(pr.open_project(&slug, true, T0).await)?.unwrap();
    check!(o8.assignment.meta.name() == "Algebra II" && o8.assignment.meta.get("updated") == Some(&json!(T0 + 9)), "renamed");
    ea(pr.write_export(&slug, "out.md", b"# hi\n")).await?;
    let dup = e(pr.duplicate_project(&slug, T0 + 20).await)?.ok_or("duplicate")?;
    check!(dup == "algebra-quadratics-copy", "duplicate slug {dup}");
    check!(e(pr.duplicate_project(&slug, T0 + 21).await)?.unwrap() == "algebra-quadratics-copy-2", "second duplicate slug");
    let od = e(pr.open_project(&dup, true, T0).await)?.unwrap();
    check!(od.assignment.meta.name() == "Algebra II copy", "copy name");
    check!(od.assignment.meta.get("created") == Some(&json!(T0 + 20)), "copy created");
    check!(od.assignment.pages.len() == 2 && e(pr.load_image(&dup, "pg2").await)?.is_some(), "copy has pages and images");
    check!(e(pr.store.get(&format!("{dup}/exports/out.md")).await)?.is_some(), "duplicate copies exports (as JS)");
    let files = e(pr.project_files(&slug).await)?;
    check!(files.iter().all(|(k, _)| !k.contains("/exports/")), "zip excludes exports");
    check!(files.iter().any(|(k, _)| k.ends_with("/project.json")), "zip has project.json");
    let mut with_export = files.clone();
    with_export.push((format!("{slug}/exports/x.md"), b"x".to_vec()));
    let imp = e(pr.import_files(&with_export).await)?;
    check!(imp == "algebra-ii", "import slug {imp}");
    check!(e(pr.store.get(&format!("{imp}/exports/x.md")).await)?.is_none(), "import skips exports");
    let oi = e(pr.open_project(&imp, true, T0).await)?.unwrap();
    check!(oi.assignment.pages == o8.assignment.pages, "imported pages equal");
    check!(
        matches!(pr.import_files(&[("a/b.txt".into(), vec![1])]).await, Err(StoreError::Invalid(m)) if m.starts_with("No project.json found")),
        "import without project.json"
    );
    ea(pr.delete_project(&dup)).await?;
    check!(e(pr.store.list(&format!("{dup}/")).await)?.is_empty(), "delete_project removes everything");
    check!(e(pr.list_projects(T0).await)?.iter().all(|s| s.slug != dup), "deleted project not listed");
    o8.assignment.remove_page("pg2");
    check!(o8.assignment.meta.order() == ["pg3"], "remove_page (sanity)");

    // ---- settings.json
    let mut st = Settings::defaults();
    st.set("theme", json!("dark"));
    st.set("apiKey", json!("sk-secret"));
    st.set("rustExtra", json!({"b": 1, "a": 0.5}));
    ea(pr.save_settings(&st)).await?;
    let text = e(pr.read_text(SETTINGS_PATH).await)?.unwrap();
    check!(!text.contains("sk-secret") && !text.contains("\"apiKey\"") && !text.contains("noticeSeen") && !text.contains("\"storage\""), "secrets stripped");
    check!(text.starts_with("{\n  \"theme\": \"dark\",") && !text.ends_with('\n'), "settings pretty");
    let back = e(pr.load_settings().await)?.ok_or("settings missing")?;
    check!(back.get("theme") == Some(&json!("dark")) && back.get("rustExtra") == Some(&json!({"b": 1, "a": 0.5})), "settings back");
    ea(pr.write_text(SETTINGS_PATH, "{nope")).await?;
    check!(matches!(pr.load_settings().await, Err(StoreError::Parse { .. })), "damaged settings is an error");
    check!(e(pr.read_text(SETTINGS_PATH).await)?.unwrap() == "{nope", "damaged settings untouched");
    ea(pr.store.delete(SETTINGS_PATH)).await?;
    check!(e(pr.load_settings().await)?.is_none(), "no settings");

    // ---- legacy import
    let sessions = vec![
        json!({"id": "s1", "name": "Legacy one", "created": 1700000000000i64, "lastRaw": "legacy transcript \u{2192} one", "preRaw": "legacy raw", "hasImage": true, "topic": "General", "prompt": "p", "appliedDiff": [{"orig": "teh", "term": "the"}]}),
        json!({"id": "s2", "name": "Legacy \u{e9}", "created": 1700000001000i64, "lastRaw": "two", "hasImage": false}),
        json!({"id": "s3", "created": 1700000002000i64, "hasImage": true}),
    ];
    let mut images = BTreeMap::new();
    images.insert("s1".to_string(), "data:image/png;base64,iVBORw0KGgo=".to_string());
    images.insert("s3".to_string(), "data:image/png;base64,iVBORw0KGgo=".to_string());
    let topics = match Settings::defaults().get("topics") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let mut n = 0;
    let plan = import::plan(
        &sessions,
        &images,
        &Options { name: None, topics: &topics, now: T0 + 50 },
        &mut || {
            n += 1;
            format!("leg{n}")
        },
        &mut |u, _| import::decode_data_url(u).map(|(_, b)| b),
        &mut |_, _| vec![],
    )?;
    let imp = e(pr.apply_legacy(&plan, T0 + 50).await)?;
    check!(imp.pages == 3 && imp.with_image == 2, "legacy counts {imp:?}");
    let ol = e(pr.open_project(&imp.slug, true, T0).await)?.unwrap();
    check!(ol.assignment.meta.name() == import::DEFAULT_NAME, "legacy name");
    check!(ol.assignment.meta.order() == ["leg1", "leg2", "leg3"], "legacy order");
    check!(ol.assignment.pages["leg1"].corrected() == "legacy transcript \u{2192} one", "legacy corrected");
    check!(e(pr.load_image(&imp.slug, "leg1").await)?.is_some() && e(pr.load_image(&imp.slug, "leg2").await)?.is_none(), "legacy images");
    check!(e(pr.store.get(&format!("{}/project.json.bak", imp.slug)).await)?.is_none(), "legacy import writes no .bak");

    // ---- Project type sanity: normalised key order of a hand-written meta
    let hand = Project::from_json("{\"name\":\"h\",\"order\":[],\"extra\":1}").map_err(|x| x.to_string())?.normalized(7);
    check!(
        hand.to_json().map_err(|x| x.to_string())?
            == "{\"schemaVersion\":1,\"name\":\"h\",\"topic\":\"General\",\"prompt\":\"\",\"order\":[],\"created\":7,\"updated\":7,\"ui\":{},\"stats\":null,\"extra\":1}",
        "normalized order"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("store future suspended"),
        }
    }

    #[test]
    fn mem_store_passes_the_suite() {
        block_on(run(crate::mem::MemStore::new())).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_fs_store_passes_the_suite() {
        let dir = std::env::temp_dir().join(format!("vishion-suite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = block_on(run(crate::fs::FsStore::new(&dir)));
        let leftovers: Vec<_> = walk(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        r.unwrap();
        assert!(leftovers.iter().all(|n| !n.contains(".vishion-tmp-")), "temp files left: {leftovers:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn walk(d: &std::path::Path) -> Vec<String> {
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                out.push(e.file_name().to_string_lossy().into_owned());
                if e.path().is_dir() {
                    out.extend(walk(&e.path()));
                }
            }
        }
        out
    }
}
