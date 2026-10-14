//! File System Access backend: a `FileSystemDirectoryHandle` (the folder chosen with
//! `showDirectoryPicker`, persisted by the JS app as `kv/root` in `ocr_fs`, or an OPFS directory).
//! Layout is the project tree itself. Writes go through `createWritable()`, which swaps the file in on
//! `close()`, so a failed write never leaves a half-written file. Types are handwritten externs so the
//! crate does not depend on `--cfg=web_sys_unstable_apis`.
use crate::idb::IdbStore;
use crate::js::{berr, is_missing, obj};
use crate::{check_key, dir_prefix, Result, Store, StoreError};
use js_sys::{Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = FileSystemDirectoryHandle)]
    type DirHandle;
    #[wasm_bindgen(method, js_name = queryPermission)]
    fn query_permission(this: &DirHandle, d: &JsValue) -> Promise;
    #[wasm_bindgen(method, js_name = requestPermission)]
    fn request_permission(this: &DirHandle, d: &JsValue) -> Promise;
    #[wasm_bindgen(method, js_name = getDirectoryHandle)]
    fn get_directory_handle(this: &DirHandle, name: &str, o: &JsValue) -> Promise;
    #[wasm_bindgen(method, js_name = getFileHandle)]
    fn get_file_handle(this: &DirHandle, name: &str, o: &JsValue) -> Promise;
    #[wasm_bindgen(method, js_name = removeEntry)]
    fn remove_entry(this: &DirHandle, name: &str, o: &JsValue) -> Promise;
    #[wasm_bindgen(method)]
    fn values(this: &DirHandle) -> JsValue;
    #[wasm_bindgen(method, getter)]
    fn name(this: &DirHandle) -> String;

    #[wasm_bindgen(js_name = FileSystemFileHandle)]
    type FileHandle;
    #[wasm_bindgen(method, js_name = getFile)]
    fn get_file(this: &FileHandle) -> Promise;
    #[wasm_bindgen(method, js_name = createWritable)]
    fn create_writable(this: &FileHandle) -> Promise;

    #[wasm_bindgen(js_name = FileSystemWritableFileStream)]
    type Writable;
    #[wasm_bindgen(method)]
    fn write(this: &Writable, d: &JsValue) -> Promise;
    #[wasm_bindgen(method)]
    fn close(this: &Writable) -> Promise;
    #[wasm_bindgen(method)]
    fn abort(this: &Writable) -> Promise;
}

pub struct FsaStore {
    root: DirHandle,
}

impl FsaStore {
    /// Wrap a directory handle. Errors when `handle` is not a `FileSystemDirectoryHandle`.
    pub fn new(handle: JsValue) -> Result<FsaStore> {
        if !handle.is_instance_of::<DirHandle>() {
            return Err(StoreError::Backend("not a FileSystemDirectoryHandle".into()));
        }
        Ok(FsaStore { root: handle.unchecked_into() })
    }

    /// The handle the JS app stored as `kv/root` in `ocr_fs` (`None` when none was chosen).
    pub async fn from_ocr_fs(idb: &IdbStore) -> Result<Option<FsaStore>> {
        let v = idb.kv_get("root").await?;
        if v.is_undefined() || v.is_null() {
            return Ok(None);
        }
        Ok(Some(FsaStore::new(v)?))
    }

    /// Display name of the folder.
    pub fn name(&self) -> String {
        self.root.name()
    }

    /// `queryPermission({mode:'readwrite'})`: `"granted"`, `"prompt"` or `"denied"`. Needs no gesture.
    pub async fn permission(&self) -> Result<String> {
        let o = obj(&[("mode", "readwrite".into())]);
        let v = JsFuture::from(self.root.query_permission(&o)).await.map_err(berr)?;
        Ok(v.as_string().unwrap_or_default())
    }

    /// `requestPermission({mode:'readwrite'})`. The browser needs transient user activation, so call this
    /// synchronously from the click handler (before any `.await`) and await the returned future after.
    pub fn request_permission(&self) -> impl std::future::Future<Output = Result<String>> {
        let o = obj(&[("mode", "readwrite".into())]);
        let p = JsFuture::from(self.root.request_permission(&o));
        async move { Ok(p.await.map_err(berr)?.as_string().unwrap_or_default()) }
    }

    async fn dir_at(&self, parts: &[&str], create: bool) -> std::result::Result<DirHandle, JsValue> {
        let rv: &JsValue = self.root.as_ref();
        let mut cur: DirHandle = rv.clone().unchecked_into();
        for p in parts {
            let o = obj(&[("create", create.into())]);
            let h = JsFuture::from(cur.get_directory_handle(p, &o)).await?;
            cur = h.unchecked_into();
        }
        Ok(cur)
    }

    async fn entries(d: &DirHandle) -> std::result::Result<Vec<(String, bool, JsValue)>, JsValue> {
        let it = d.values();
        let next: Function = Reflect::get(&it, &"next".into())?.dyn_into()?;
        let mut out = vec![];
        loop {
            let r = JsFuture::from(next.call0(&it)?.dyn_into::<Promise>()?).await?;
            if Reflect::get(&r, &"done".into())?.as_bool().unwrap_or(true) {
                break;
            }
            let h = Reflect::get(&r, &"value".into())?;
            let name = Reflect::get(&h, &"name".into())?.as_string().unwrap_or_default();
            let kind = Reflect::get(&h, &"kind".into())?.as_string().unwrap_or_default();
            out.push((name, kind == "directory", h));
        }
        Ok(out)
    }

    fn walk<'a>(
        d: &'a DirHandle,
        prefix: String,
        out: &'a mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<(), JsValue>> + 'a>> {
        Box::pin(async move {
            for (name, is_dir, h) in Self::entries(d).await? {
                let p = format!("{prefix}{name}");
                if is_dir {
                    let sub: DirHandle = h.unchecked_into();
                    Self::walk(&sub, format!("{p}/"), out).await?;
                } else {
                    out.push(p);
                }
            }
            Ok(())
        })
    }

    async fn remove(&self, dirs: &[&str], name: &str, recursive: bool) -> Result<()> {
        let d = match self.dir_at(dirs, false).await {
            Ok(d) => d,
            Err(e) if is_missing(&e) => return Ok(()),
            Err(e) => return Err(berr(e)),
        };
        match JsFuture::from(d.remove_entry(name, &obj(&[("recursive", recursive.into())]))).await {
            Ok(_) => Ok(()),
            Err(e) if is_missing(&e) => Ok(()),
            Err(e) => Err(berr(e)),
        }
    }
}

fn split(key: &str) -> Result<(Vec<&str>, &str)> {
    check_key(key)?;
    let mut parts: Vec<&str> = key.split('/').collect();
    let name = parts.pop().ok_or_else(|| StoreError::InvalidKey(key.into()))?;
    Ok((parts, name))
}

impl Store for FsaStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let (dirs, name) = split(key)?;
        let r: std::result::Result<Vec<u8>, JsValue> = async {
            let d = self.dir_at(&dirs, false).await?;
            let fh = JsFuture::from(d.get_file_handle(name, &obj(&[]))).await?;
            let fh: FileHandle = fh.unchecked_into();
            let file = JsFuture::from(fh.get_file()).await?;
            let ab = JsFuture::from(file.unchecked_into::<web_sys::Blob>().array_buffer()).await?;
            Ok(Uint8Array::new(&ab).to_vec())
        }
        .await;
        match r {
            Ok(b) => Ok(Some(b)),
            Err(e) if is_missing(&e) => Ok(None),
            Err(e) => Err(berr(e)),
        }
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        let (dirs, name) = split(key)?;
        let d = self.dir_at(&dirs, true).await.map_err(berr)?;
        let fh = JsFuture::from(d.get_file_handle(name, &obj(&[("create", true.into())]))).await.map_err(berr)?;
        let fh: FileHandle = fh.unchecked_into();
        let w: Writable = JsFuture::from(fh.create_writable()).await.map_err(berr)?.unchecked_into();
        let data = Uint8Array::from(value);
        let res: std::result::Result<(), JsValue> = async {
            JsFuture::from(w.write(&data)).await?;
            JsFuture::from(w.close()).await?;
            Ok(())
        }
        .await;
        if let Err(e) = res {
            let _ = JsFuture::from(w.abort()).await;
            return Err(berr(e));
        }
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let (dir_rel, _) = prefix.rsplit_once('/').unwrap_or(("", prefix));
        let parts: Vec<&str> = dir_rel.split('/').filter(|s| !s.is_empty()).collect();
        let d = match self.dir_at(&parts, false).await {
            Ok(d) => d,
            Err(e) if is_missing(&e) => return Ok(vec![]),
            Err(e) => return Err(berr(e)),
        };
        let mut out = vec![];
        let start = if dir_rel.is_empty() { String::new() } else { format!("{dir_rel}/") };
        Self::walk(&d, start, &mut out).await.map_err(berr)?;
        out.retain(|k| k.starts_with(prefix));
        out.sort();
        Ok(out)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let (dirs, name) = split(key)?;
        self.remove(&dirs, name, false).await
    }

    async fn delete_tree(&self, path: &str) -> Result<()> {
        let (dirs, name) = split(path.trim_end_matches('/'))?;
        self.remove(&dirs, name, true).await
    }

    async fn children(&self, dir: &str) -> Result<Vec<(String, bool)>> {
        let p = dir_prefix(dir);
        let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
        let d = match self.dir_at(&parts, false).await {
            Ok(d) => d,
            Err(e) if is_missing(&e) => return Ok(vec![]),
            Err(e) => return Err(berr(e)),
        };
        let mut out: Vec<(String, bool)> = Self::entries(&d).await.map_err(berr)?.into_iter().map(|(n, d, _)| (n, d)).collect();
        out.sort();
        Ok(out)
    }
}
