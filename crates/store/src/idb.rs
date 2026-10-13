//! IndexedDB backend: database `ocr_fs` (version 1), object store `files`, key = the project-relative
//! path, exactly what js/fs.js `idbBackend` uses. JS value types are preserved on write-back (S3 rule 5):
//! a key that holds a string stays a string, a Blob stays a Blob of the same type, a Uint8Array /
//! ArrayBuffer keeps its class. New keys follow the JS app: JSON documents are strings, everything else a
//! Blob with a type from the extension.
use crate::js::{berr, idb_delete_many, idb_get, idb_keys, idb_put, open_ocr_fs};
use crate::{check_key, dir_prefix, Result, Store, StoreError};
use js_sys::{Array, ArrayBuffer, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, IdbDatabase};

const FILES: &str = "files";

pub struct IdbStore {
    db: IdbDatabase,
}

enum Kind {
    Str,
    Blob(String),
    U8,
    Ab,
}

fn mime_for(key: &str) -> &'static str {
    match key.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("md") => "text/markdown",
        Some("txt") => "text/plain",
        Some("csv") => "text/csv",
        _ => "",
    }
}

impl IdbStore {
    /// Open (or create, at version 1) `ocr_fs`.
    pub async fn open() -> Result<IdbStore> {
        Ok(IdbStore { db: open_ocr_fs().await? })
    }

    pub fn from_db(db: IdbDatabase) -> IdbStore {
        IdbStore { db }
    }

    /// The underlying database.
    pub fn db(&self) -> &IdbDatabase {
        &self.db
    }

    /// Raw value of `kv/<key>` (`undefined` when absent). The folder backend reads `root` from here.
    pub async fn kv_get(&self, key: &str) -> Result<JsValue> {
        idb_get(&self.db, "kv", key).await
    }

    pub fn close(&self) {
        self.db.close();
    }

    fn kind_of(v: &JsValue) -> Option<Kind> {
        if v.is_undefined() || v.is_null() {
            None
        } else if v.is_string() {
            Some(Kind::Str)
        } else if let Some(b) = v.dyn_ref::<Blob>() {
            Some(Kind::Blob(b.type_()))
        } else if v.dyn_ref::<ArrayBuffer>().is_some() {
            Some(Kind::Ab)
        } else if v.dyn_ref::<Uint8Array>().is_some() {
            Some(Kind::U8)
        } else {
            None
        }
    }

    fn make_value(kind: Kind, bytes: &[u8]) -> Result<JsValue> {
        let u8a = Uint8Array::from(bytes);
        Ok(match kind {
            Kind::Str => match std::str::from_utf8(bytes) {
                Ok(s) => JsValue::from_str(s),
                Err(_) => u8a.into(), // not text: keep the bytes rather than corrupt them
            },
            Kind::U8 => u8a.into(),
            Kind::Ab => u8a.buffer().into(),
            Kind::Blob(mime) => {
                let bag = BlobPropertyBag::new();
                bag.set_type(&mime);
                Blob::new_with_u8_array_sequence_and_options(&Array::of1(&u8a), &bag).map_err(berr)?.into()
            }
        })
    }
}

impl Store for IdbStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        check_key(key)?;
        let v = idb_get(&self.db, FILES, key).await?;
        if v.is_undefined() || v.is_null() {
            return Ok(None);
        }
        if let Some(s) = v.as_string() {
            return Ok(Some(s.into_bytes()));
        }
        if let Some(b) = v.dyn_ref::<Blob>() {
            let ab = JsFuture::from(b.array_buffer()).await.map_err(berr)?;
            return Ok(Some(Uint8Array::new(&ab).to_vec()));
        }
        if let Some(a) = v.dyn_ref::<ArrayBuffer>() {
            return Ok(Some(Uint8Array::new(a).to_vec()));
        }
        if let Some(a) = v.dyn_ref::<Uint8Array>() {
            return Ok(Some(a.to_vec()));
        }
        Err(StoreError::Backend(format!("{key}: unsupported stored value type")))
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        check_key(key)?;
        let existing = idb_get(&self.db, FILES, key).await?;
        let kind = Self::kind_of(&existing).unwrap_or_else(|| {
            if matches!(key.rsplit('.').next(), Some("json") | Some("bak") | Some("damaged")) {
                Kind::Str
            } else {
                Kind::Blob(mime_for(key).to_string())
            }
        });
        let v = Self::make_value(kind, value)?;
        idb_put(&self.db, FILES, key, &v).await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut keys: Vec<String> = idb_keys(&self.db, FILES).await?.into_iter().filter(|k| k.starts_with(prefix)).collect();
        keys.sort();
        Ok(keys)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        check_key(key)?;
        idb_delete_many(&self.db, FILES, &[key.to_string()]).await
    }

    async fn delete_tree(&self, path: &str) -> Result<()> {
        let p = path.trim_end_matches('/');
        crate::check_key(p)?; // "" / "/" would match every key
        let pre = dir_prefix(p);
        let del: Vec<String> = idb_keys(&self.db, FILES).await?.into_iter().filter(|k| k == p || k.starts_with(&pre)).collect();
        idb_delete_many(&self.db, FILES, &del).await
    }
}
