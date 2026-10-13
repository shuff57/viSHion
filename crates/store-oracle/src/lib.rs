//! Browser harness for the store oracle (tools/store-oracle/run.mjs). Everything the Rust side does to
//! the data the JS app wrote goes through the same `vishion_store` API the real app uses; the driver
//! then reloads the unmodified JS app and compares. Compiles to nothing on native targets.
#![cfg(target_arch = "wasm32")]

use serde_json::{json, Value};
use vishion_core::import::{self, Options};
use vishion_core::model::{Assignment, Settings};
use vishion_store::fsa::FsaStore;
use vishion_store::idb::IdbStore;
use vishion_store::projects::Projects;
use vishion_store::{legacy_web, suite, Result as SResult, Store};
use wasm_bindgen::prelude::*;

/// Either web backend behind one `Store`.
enum Any {
    Idb(IdbStore),
    Fsa(FsaStore),
}

impl Store for Any {
    async fn get(&self, key: &str) -> SResult<Option<Vec<u8>>> {
        match self {
            Any::Idb(s) => s.get(key).await,
            Any::Fsa(s) => s.get(key).await,
        }
    }
    async fn put(&self, key: &str, value: &[u8]) -> SResult<()> {
        match self {
            Any::Idb(s) => s.put(key, value).await,
            Any::Fsa(s) => s.put(key, value).await,
        }
    }
    async fn list(&self, prefix: &str) -> SResult<Vec<String>> {
        match self {
            Any::Idb(s) => s.list(prefix).await,
            Any::Fsa(s) => s.list(prefix).await,
        }
    }
    async fn delete(&self, key: &str) -> SResult<()> {
        match self {
            Any::Idb(s) => s.delete(key).await,
            Any::Fsa(s) => s.delete(key).await,
        }
    }
    async fn delete_tree(&self, path: &str) -> SResult<()> {
        match self {
            Any::Idb(s) => s.delete_tree(path).await,
            Any::Fsa(s) => s.delete_tree(path).await,
        }
    }
    async fn children(&self, dir: &str) -> SResult<Vec<(String, bool)>> {
        match self {
            Any::Idb(s) => s.children(dir).await,
            Any::Fsa(s) => s.children(dir).await,
        }
    }
}

fn err<E: ToString>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// `"idb"`: `ocr_fs/files`. `"folder"`: the handle the JS app stored as `ocr_fs/kv/root`.
async fn open(mode: &str) -> std::result::Result<Projects<Any>, JsValue> {
    let idb = IdbStore::open().await.map_err(err)?;
    match mode {
        "idb" => Ok(Projects::new(Any::Idb(idb))),
        "folder" => {
            let f = FsaStore::from_ocr_fs(&idb).await.map_err(err)?.ok_or_else(|| err("kv/root is empty"))?;
            idb.close();
            Ok(Projects::new(Any::Fsa(f)))
        }
        m => Err(err(format!("unknown mode {m}"))),
    }
}

fn now() -> i64 {
    js_sys::Date::now() as i64
}

fn b36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut s = vec![];
    while n > 0 {
        s.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// `OCR.uid`.
fn uid() -> String {
    let r: String = (0..5).map(|_| b36((js_sys::Math::random() * 36.0) as u64)).collect();
    format!("{}{}", b36(now() as u64), r)
}

fn out(v: Value) -> std::result::Result<String, JsValue> {
    serde_json::to_string(&v).map_err(err)
}

/// Run the project-layer suite on a fresh `ocr_fs` (created by Rust at version 1).
#[wasm_bindgen]
pub async fn suite_idb() -> std::result::Result<String, JsValue> {
    let s = IdbStore::open().await.map_err(err)?;
    match suite::run(s).await {
        Ok(()) => Ok("ok".into()),
        Err(m) => Err(err(m)),
    }
}

/// Run the suite on an (empty) directory handle, e.g. `navigator.storage.getDirectory()`.
#[wasm_bindgen]
pub async fn suite_handle(handle: JsValue) -> std::result::Result<String, JsValue> {
    let s = FsaStore::new(handle).map_err(err)?;
    match suite::run(s).await {
        Ok(()) => Ok("ok".into()),
        Err(m) => Err(err(m)),
    }
}

/// Folder permission as the JS app would see it (`granted` for OPFS).
#[wasm_bindgen]
pub async fn folder_permission() -> std::result::Result<String, JsValue> {
    let idb = IdbStore::open().await.map_err(err)?;
    let f = FsaStore::from_ocr_fs(&idb).await.map_err(err)?.ok_or_else(|| err("kv/root is empty"))?;
    f.permission().await.map_err(err)
}

/// Open every project through the project layer and write everything back untouched: pages, project.json
/// (with its `.bak`), settings.json. The driver then checks bytes, value types and the reloaded JS state.
#[wasm_bindgen]
pub async fn rewrite_all(mode: &str) -> std::result::Result<String, JsValue> {
    let pr = open(mode).await?;
    let t = now();
    let (mut projects, mut pages, mut unusable, mut unreadable, mut from_backup) = (0, 0, vec![], vec![], vec![]);
    for (name, is_dir) in pr.store.children("").await.map_err(err)? {
        if !is_dir || name.starts_with('.') {
            continue;
        }
        match pr.open_project(&name, false, t).await {
            Ok(Some(mut o)) => {
                projects += 1;
                if o.from_backup {
                    from_backup.push(name.clone());
                }
                for u in &o.unreadable {
                    unreadable.push(format!("{name}/{}: {}", u.id, u.error));
                }
                for p in o.assignment.pages.values() {
                    pr.save_page(&name, p).await.map_err(err)?;
                    pages += 1;
                }
                pr.save_meta(&name, &mut o.assignment.meta, None).await.map_err(err)?;
            }
            Ok(None) => {}
            Err(e) => unusable.push(format!("{name}: {e}")),
        }
    }
    let mut settings = false;
    if let Some(m) = pr.load_settings().await.map_err(err)? {
        pr.save_settings(&Settings(m)).await.map_err(err)?;
        settings = true;
    }
    out(json!({"projects": projects, "pages": pages, "unusable": unusable, "unreadable": unreadable, "fromBackup": from_backup, "settings": settings}))
}

/// Edit one page the way the Rust UI would (corrected text, reviewed) and bump `meta.updated`.
#[wasm_bindgen]
pub async fn edit_page(mode: &str, slug: &str, id: &str, suffix: &str) -> std::result::Result<String, JsValue> {
    let pr = open(mode).await?;
    let t = now();
    let mut o = pr.open_project(slug, true, t).await.map_err(err)?.ok_or_else(|| err("no such project"))?;
    let p = o.assignment.pages.get_mut(id).ok_or_else(|| err("no such page"))?;
    let c = format!("{}{}", p.corrected(), suffix);
    p.set("corrected", json!(c));
    p.set("reviewed", json!(true));
    let p = p.clone();
    pr.save_page(slug, &p).await.map_err(err)?;
    pr.save_meta(slug, &mut o.assignment.meta, Some(t)).await.map_err(err)?;
    Ok("ok".into())
}

/// A brand-new assignment with one page and a JPEG, as `createProject` + `addPages` would write it.
#[wasm_bindgen]
pub async fn create_project(mode: &str, name: &str, jpeg: Vec<u8>, transcript: &str) -> std::result::Result<String, JsValue> {
    let pr = open(mode).await?;
    let t = now();
    let slug = pr.create_project(name, t).await.map_err(err)?;
    let mut o = pr.open_project(&slug, true, t).await.map_err(err)?.ok_or_else(|| err("created project does not open"))?;
    let id = uid();
    let mut p = Assignment::new_page(&id, "rust-made-page.jpg");
    p.set("status", json!("done"));
    p.set("raw", json!(transcript));
    p.set("corrected", json!(transcript));
    pr.save_image(&slug, &id, &jpeg).await.map_err(err)?;
    o.assignment.add_pages(vec![p]);
    pr.save_page(&slug, &o.assignment.pages[&id]).await.map_err(err)?;
    pr.save_meta(&slug, &mut o.assignment.meta, Some(t)).await.map_err(err)?;
    out(json!({"slug": slug, "id": id}))
}

/// rename + export + duplicate (twice) + delete one copy + delete a page of the other copy.
#[wasm_bindgen]
pub async fn lifecycle(mode: &str, slug: &str, new_name: &str) -> std::result::Result<String, JsValue> {
    let pr = open(mode).await?;
    let t = now();
    pr.write_export(slug, "rust-export.md", b"# exported by rust\n").await.map_err(err)?;
    let dup = pr.duplicate_project(slug, t).await.map_err(err)?.ok_or_else(|| err("duplicate failed"))?;
    let dup2 = pr.duplicate_project(slug, t).await.map_err(err)?.ok_or_else(|| err("duplicate 2 failed"))?;
    if !pr.rename_project(&dup, new_name, t).await.map_err(err)? {
        return Err(err("rename failed"));
    }
    pr.delete_project(&dup2).await.map_err(err)?;
    let o = pr.open_project(&dup, true, t).await.map_err(err)?.ok_or_else(|| err("copy does not open"))?;
    let first = o.assignment.meta.order().first().cloned();
    let mut o = o;
    if let Some(id) = first {
        pr.delete_page(&dup, &id).await.map_err(err)?;
        o.assignment.remove_page(&id);
        pr.save_meta(&dup, &mut o.assignment.meta, Some(t)).await.map_err(err)?;
    }
    out(json!({"dup": dup, "deleted": dup2}))
}

fn sniff_image(b: &[u8]) -> bool {
    let png = b.len() >= 33 && b.starts_with(&[0x89, b'P', b'N', b'G', 13, 10, 26, 10]) && &b[12..16] == b"IHDR";
    let jpeg = b.len() > 4 && b.starts_with(&[0xff, 0xd8, 0xff]);
    png || jpeg
}

/// `fs.importLegacy` from `ocr_sessions_v2` + `ocr_images`. Pictures are stored as decoded (the JS app
/// re-encodes through a canvas; that step is the UI's job), boxes are left empty.
#[wasm_bindgen]
pub async fn import_legacy(mode: &str, name: &str) -> std::result::Result<String, JsValue> {
    let pr = open(mode).await?;
    let t = now();
    let sessions = import::parse_sessions(legacy_web::sessions_json().as_deref());
    let images = legacy_web::images().await;
    let settings = match pr.load_settings().await.map_err(err)? {
        Some(m) => Settings::load(&Value::Object(m)),
        None => Settings::defaults(),
    };
    let topics = match settings.get("topics") {
        Some(Value::Object(m)) => m.clone(),
        _ => Default::default(),
    };
    let plan = import::plan(
        &sessions,
        &images,
        &Options { name: if name.is_empty() { None } else { Some(name) }, topics: &topics, now: t },
        &mut || uid(),
        &mut |u, _| import::decode_data_url(u).map(|(_, b)| b).filter(|b| sniff_image(b)),
        &mut |_, _| vec![],
    )
    .map_err(err)?;
    let r = pr.apply_legacy(&plan, t).await.map_err(err)?;
    out(json!({"slug": r.slug, "pages": r.pages, "withImage": r.with_image}))
}
