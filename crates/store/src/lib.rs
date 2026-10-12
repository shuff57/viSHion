//! `Store` is the only way the app touches persistence. Implementations: `fs` (native, here);
//! `idb` and `fsa` for web arrive in Phase 1+ (IDB database `ocr_fs` must open at version 1, S3 rule 2).
use std::future::Future;

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    InvalidKey(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
            StoreError::InvalidKey(k) => write!(f, "invalid key: {k:?}"),
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
pub trait Store {
    fn get(&self, key: &str) -> impl Future<Output = Result<Option<Vec<u8>>>>;
    fn put(&self, key: &str, value: &[u8]) -> impl Future<Output = Result<()>>;
    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>>>;
    fn delete(&self, key: &str) -> impl Future<Output = Result<()>>;
}

#[cfg(not(target_arch = "wasm32"))]
pub mod fs;
