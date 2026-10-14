//! Shared wasm helpers for the web backends: promise wrappers over IndexedDB requests, error mapping,
//! and the `ocr_fs` database opener (version 1 exactly, S3 rule 4).
use crate::StoreError;
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

pub const DB_NAME: &str = "ocr_fs";
pub const DB_VERSION: u32 = 1;

/// `name: message` of a thrown value (DOMException, Error or string).
pub fn describe(e: &JsValue) -> String {
    let get = |k: &str| Reflect::get(e, &JsValue::from_str(k)).ok().and_then(|v| v.as_string());
    match (get("name"), get("message")) {
        (Some(n), Some(m)) if !m.is_empty() => format!("{n}: {m}"),
        (Some(n), _) => n,
        _ => e.as_string().unwrap_or_else(|| format!("{e:?}")),
    }
}

pub fn berr(e: JsValue) -> StoreError {
    StoreError::Backend(describe(&e))
}

/// `NotFoundError` / `TypeMismatchError`: the entry is simply not there.
pub fn is_missing(e: &JsValue) -> bool {
    let n = Reflect::get(e, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
    n == "NotFoundError" || n == "TypeMismatchError"
}

pub fn obj(pairs: &[(&str, JsValue)]) -> JsValue {
    let o = js_sys::Object::new();
    for (k, v) in pairs {
        let _ = Reflect::set(&o, &JsValue::from_str(k), v);
    }
    o.into()
}

pub fn req_promise(r: &IdbRequest) -> Promise {
    let (a, b) = (r.clone(), r.clone());
    Promise::new(&mut move |res: Function, rej: Function| {
        let (a2, b2) = (a.clone(), b.clone());
        let ok = Closure::once_into_js(move |_e: Event| {
            let _ = res.call1(&JsValue::NULL, &a2.result().unwrap_or(JsValue::UNDEFINED));
        });
        let er = Closure::once_into_js(move |_e: Event| {
            let e = b2.error().ok().flatten().map(JsValue::from).unwrap_or_else(|| JsValue::from_str("idb request error"));
            let _ = rej.call1(&JsValue::NULL, &e);
        });
        a.set_onsuccess(Some(ok.unchecked_ref()));
        a.set_onerror(Some(er.unchecked_ref()));
    })
}

/// Resolves on the transaction's `complete` (writes are durable then, S3 rule 6), not on request success.
pub fn tx_done(t: &IdbTransaction) -> Promise {
    let t = t.clone();
    Promise::new(&mut move |res: Function, rej: Function| {
        let ok = Closure::once_into_js(move |_e: Event| {
            let _ = res.call0(&JsValue::NULL);
        });
        let rej2 = rej.clone();
        let er = Closure::once_into_js(move |_e: Event| {
            let _ = rej2.call1(&JsValue::NULL, &JsValue::from_str("transaction error"));
        });
        let ab = Closure::once_into_js(move |_e: Event| {
            let _ = rej.call1(&JsValue::NULL, &JsValue::from_str("transaction aborted"));
        });
        t.set_oncomplete(Some(ok.unchecked_ref()));
        t.set_onerror(Some(er.unchecked_ref()));
        t.set_onabort(Some(ab.unchecked_ref()));
    })
}

/// Open `ocr_fs` at version 1 exactly. If the database does not exist, the upgrade creates the `kv` and
/// `files` stores like js/fs.js does. A database already at a higher version is an error (never upgrade).
pub async fn open_ocr_fs() -> Result<IdbDatabase, StoreError> {
    let win = web_sys::window().ok_or_else(|| StoreError::Backend("no window".into()))?;
    let f = win.indexed_db().map_err(berr)?.ok_or_else(|| StoreError::Backend("no indexedDB".into()))?;
    let r = f.open_with_u32(DB_NAME, DB_VERSION).map_err(berr)?;
    let rr = r.clone();
    let up = Closure::once_into_js(move |_e: Event| {
        if let Ok(v) = rr.result() {
            if let Ok(db) = v.dyn_into::<IdbDatabase>() {
                let names = db.object_store_names();
                for s in ["kv", "files"] {
                    if !names.contains(s) {
                        let _ = db.create_object_store(s);
                    }
                }
            }
        }
    });
    r.set_onupgradeneeded(Some(up.unchecked_ref()));
    let v = JsFuture::from(req_promise(&r)).await.map_err(berr)?;
    let db = v.dyn_into::<IdbDatabase>().map_err(|_| StoreError::Backend("not an IDBDatabase".into()))?;
    let names = db.object_store_names();
    if !names.contains("files") || !names.contains("kv") {
        db.close();
        return Err(StoreError::Backend("ocr_fs exists without its kv/files stores (not upgraded: version stays 1)".into()));
    }
    Ok(db)
}

pub async fn idb_get(db: &IdbDatabase, store: &str, key: &str) -> Result<JsValue, StoreError> {
    let t = db.transaction_with_str(store).map_err(berr)?;
    let r = t.object_store(store).map_err(berr)?.get(&JsValue::from_str(key)).map_err(berr)?;
    JsFuture::from(req_promise(&r)).await.map_err(berr)
}

pub async fn idb_keys(db: &IdbDatabase, store: &str) -> Result<Vec<String>, StoreError> {
    let t = db.transaction_with_str(store).map_err(berr)?;
    let r = t.object_store(store).map_err(berr)?.get_all_keys().map_err(berr)?;
    let v = JsFuture::from(req_promise(&r)).await.map_err(berr)?;
    Ok(js_sys::Array::from(&v).iter().filter_map(|k| k.as_string()).collect())
}

pub async fn idb_put(db: &IdbDatabase, store: &str, key: &str, val: &JsValue) -> Result<(), StoreError> {
    let t = db.transaction_with_str_and_mode(store, IdbTransactionMode::Readwrite).map_err(berr)?;
    t.object_store(store).map_err(berr)?.put_with_key(val, &JsValue::from_str(key)).map_err(berr)?;
    JsFuture::from(tx_done(&t)).await.map_err(berr)?;
    Ok(())
}

pub async fn idb_delete_many(db: &IdbDatabase, store: &str, keys: &[String]) -> Result<(), StoreError> {
    if keys.is_empty() {
        return Ok(());
    }
    let t = db.transaction_with_str_and_mode(store, IdbTransactionMode::Readwrite).map_err(berr)?;
    let s = t.object_store(store).map_err(berr)?;
    for k in keys {
        s.delete(&JsValue::from_str(k)).map_err(berr)?;
    }
    JsFuture::from(tx_done(&t)).await.map_err(berr)?;
    Ok(())
}
