//! `Store` is the only way the app touches persistence.
//!
//! * `fs`   native directory store (not on wasm32).
//! * `idb`  IndexedDB `ocr_fs` / `files` (wasm32 only; opens at version 1 exactly, S3 rule 4).
//! * `fsa`  File System Access folder handle (wasm32 only).
//! * `legacy_web` (wasm32) reads the old app's localStorage sessions and `ocr_images` IndexedDB.
//! * `mem`  in-memory store for tests and previews.
//! * `projects` the project layer (js/fs.js + js/fs-projects.js semantics) over any `Store`.
//! * `suite` the project-layer test suite every backend must pass (native fs, memory, idb, fsa).
use std::future::Future;

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    InvalidKey(String),
    /// A web backend failed (IndexedDB / File System Access / DOM exception text).
    Backend(String),
    /// A document exists but cannot be parsed. The file is left untouched.
    Parse { path: String, message: String },
    /// A user-facing refusal (same wording as the JS app where there is one).
    Invalid(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
            StoreError::InvalidKey(k) => write!(f, "invalid key: {k:?}"),
            StoreError::Backend(m) => write!(f, "storage: {m}"),
            StoreError::Parse { path, message } => write!(f, "{path} is damaged: {message}"),
            StoreError::Invalid(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for StoreError {}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Key/value blob store. Async because the web backends are (IndexedDB, File System Access).
/// Keys are `/`-separated relative paths exactly like the JS app's `slug/project.json`.
pub trait Store {
    fn get(&self, key: &str) -> impl Future<Output = Result<Option<Vec<u8>>>>;
    fn put(&self, key: &str, value: &[u8]) -> impl Future<Output = Result<()>>;
    /// Every key starting with `prefix`, sorted.
    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>>>;
    /// Delete one key; a missing key is not an error.
    fn delete(&self, key: &str) -> impl Future<Output = Result<()>>;

    /// Direct children of directory `dir` (`""` is the root): `(name, is_dir)`, sorted by name.
    fn children(&self, dir: &str) -> impl Future<Output = Result<Vec<(String, bool)>>> {
        async move {
            let pre = dir_prefix(dir);
            let mut out: Vec<(String, bool)> = vec![];
            for k in self.list(&pre).await? {
                let rest = &k[pre.len()..];
                let (name, is_dir) = match rest.split_once('/') {
                    Some((n, _)) => (n, true),
                    None => (rest, false),
                };
                if !name.is_empty() && !out.iter().any(|(n, _)| n == name) {
                    out.push((name.to_string(), is_dir));
                }
            }
            out.sort();
            Ok(out)
        }
    }

    /// `fs.remove(path)`: delete `path` itself and everything under `path/`.
    fn delete_tree(&self, path: &str) -> impl Future<Output = Result<()>> {
        async move {
            let p = path.trim_end_matches('/');
            check_key(p)?; // "" / "/" would match every key
            for k in self.list(&dir_prefix(p)).await? {
                self.delete(&k).await?;
            }
            self.delete(p).await
        }
    }
}

/// `"a/b"` and `"a/b/"` become `"a/b/"`; `""` stays `""`.
pub fn dir_prefix(dir: &str) -> String {
    let d = dir.trim_matches('/');
    if d.is_empty() {
        String::new()
    } else {
        format!("{d}/")
    }
}

/// Key rule shared by the in-memory and web backends: no empty segment, `.`, `..`, backslash or NUL.
/// (The native store is stricter, see `fs`.)
pub fn check_key(key: &str) -> Result<()> {
    for seg in key.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.chars().any(|c| matches!(c, '\\' | '\0')) {
            return Err(StoreError::InvalidKey(key.into()));
        }
    }
    Ok(())
}

/// `stored_text`: bytes as the JS `TextDecoder` / `Blob.text()` would decode them (BOM stripped, invalid
/// UTF-8 replaced).
pub fn decode_text(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    match s.strip_prefix('\u{feff}') {
        Some(r) => r.to_string(),
        None => s.into_owned(),
    }
}

pub mod mem;
pub mod projects;
pub mod suite;

#[cfg(not(target_arch = "wasm32"))]
pub mod fs;

#[cfg(target_arch = "wasm32")]
pub mod fsa;
#[cfg(target_arch = "wasm32")]
pub mod idb;
#[cfg(target_arch = "wasm32")]
pub mod legacy_web;
#[cfg(target_arch = "wasm32")]
mod js;
