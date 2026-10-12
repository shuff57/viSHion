//! Native directory-backed store. Keys are flat file names (no separators, no `..`).
use crate::{Result, Store, StoreError};
use std::path::PathBuf;

pub struct FsStore {
    root: PathBuf,
}

impl FsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, key: &str) -> Result<PathBuf> {
        let bad = key.is_empty()
            || key == "."
            || key == ".."
            || key.chars().any(|c| matches!(c, '/' | '\\' | '\0' | ':'));
        if bad {
            return Err(StoreError::InvalidKey(key.into()));
        }
        Ok(self.root.join(key))
    }
}

impl Store for FsStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(key)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        let p = self.path(key)?;
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(p, value)?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut out = vec![];
        let rd = match std::fs::read_dir(&self.root) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for e in rd {
            if let Some(n) = e?.file_name().to_str() {
                if n.starts_with(prefix) {
                    out.push(n.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.path(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    // FsStore futures never suspend, so a single poll completes them.
    fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("fs future suspended"),
        }
    }

    #[test]
    fn put_get_list_delete() {
        let dir = std::env::temp_dir().join(format!("vishion-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = FsStore::new(&dir);
        assert_eq!(block_on(s.get("a.json")).unwrap(), None);
        block_on(s.put("a.json", b"{}")).unwrap();
        block_on(s.put("b.json", b"[]")).unwrap();
        assert_eq!(
            block_on(s.get("a.json")).unwrap().as_deref(),
            Some(&b"{}"[..])
        );
        assert_eq!(block_on(s.list("")).unwrap(), vec!["a.json", "b.json"]);
        block_on(s.delete("a.json")).unwrap();
        assert_eq!(block_on(s.list("")).unwrap(), vec!["b.json"]);
        assert!(block_on(s.get("../x")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
