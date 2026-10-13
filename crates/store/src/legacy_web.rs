//! Browser side of the legacy import (js/fs-legacy.js): where the old single-file app kept its data.
//! `localStorage["ocr_sessions_v2"]` holds the session array; each page picture is a data URL in the
//! IndexedDB `ocr_images` / `images` store keyed by session id. Both are only readable from the same
//! origin. The conversion into a project is the pure `vishion_core::import::plan`.
use crate::js::{berr, req_promise};
use js_sys::Array;
use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, IdbDatabase};

/// Raw `ocr_sessions_v2` text, `None` when absent or storage is unavailable.
pub fn sessions_json() -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(vishion_core::import::SESSIONS_KEY).ok()?
}

/// `legacyImages()`: session id to data URL. Never fails; no database means no images. The database is
/// never created (an upgrade is aborted) and never modified.
pub async fn images() -> BTreeMap<String, String> {
    read_images().await.unwrap_or_default()
}

async fn read_images() -> Result<BTreeMap<String, String>, crate::StoreError> {
    let f = web_sys::window()
        .and_then(|w| w.indexed_db().ok().flatten())
        .ok_or_else(|| crate::StoreError::Backend("no indexedDB".into()))?;
    let r = f.open("ocr_images").map_err(berr)?;
    let rr = r.clone();
    let abort = Closure::once_into_js(move |_e: Event| {
        if let Some(t) = rr.transaction() {
            let _ = t.abort();
        }
    });
    r.set_onupgradeneeded(Some(abort.unchecked_ref()));
    let db = JsFuture::from(req_promise(&r)).await.map_err(berr)?.dyn_into::<IdbDatabase>().map_err(|_| crate::StoreError::Backend("not a database".into()))?;
    let mut out = BTreeMap::new();
    if db.object_store_names().contains("images") {
        let t = db.transaction_with_str("images").map_err(berr)?;
        let s = t.object_store("images").map_err(berr)?;
        let keys = JsFuture::from(req_promise(&s.get_all_keys().map_err(berr)?)).await.map_err(berr)?;
        let vals = JsFuture::from(req_promise(&s.get_all().map_err(berr)?)).await.map_err(berr)?;
        for (k, v) in Array::from(&keys).iter().zip(Array::from(&vals).iter()) {
            let key = k.as_string().or_else(|| k.as_f64().map(|n| if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }));
            if let (Some(key), Some(url)) = (key, v.as_string()) {
                out.insert(key, url);
            }
        }
    }
    db.close();
    Ok(out)
}
