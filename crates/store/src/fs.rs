//! Native directory-backed store. Keys are '/'-separated relative paths exactly like the JS app's
//! `slug/project.json` and `slug/pages/<id>.json`. Segments may not be empty, `.`, `..`, or contain
//! `\`, `:` or NUL, so a key can never leave the root. Writes are atomic: temp file in the same
//! directory, then rename over the target.
use crate::{Result, Store, StoreError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const TMP_PREFIX: &str = ".vishion-tmp-";
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub struct FsStore {
    root: PathBuf,
    /// Directories already swept for stale temp files left by killed writers.
    swept: std::sync::Mutex<std::collections::HashSet<PathBuf>>,
}

use crate::is_device_name;

impl FsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), swept: Default::default() }
    }

    /// Once per directory and process: delete `.vishion-tmp-*` files older than an hour (a killed writer).
    fn sweep_stale(&self, dir: &Path) {
        if !self.swept.lock().map(|mut s| s.insert(dir.to_path_buf())).unwrap_or(false) {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let hour = std::time::Duration::from_secs(3600);
        for e in rd.flatten() {
            if !e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
                continue;
            }
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d > hour);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    fn path(&self, key: &str) -> Result<PathBuf> {
        let mut p = self.root.clone();
        for seg in key.split('/') {
            let bad = seg.is_empty()
                || seg == "."
                || seg == ".."
                || seg.starts_with(TMP_PREFIX)
                // Windows drops a trailing dot / space, so `a.` would alias `a`; device names open devices
                || seg.ends_with('.')
                || seg.ends_with(' ')
                || is_device_name(seg)
                || seg.chars().any(|c| matches!(c, '\\' | '\0' | ':'));
            if bad {
                return Err(StoreError::InvalidKey(key.into()));
            }
            p.push(seg);
        }
        Ok(p)
    }
}

/// fsync the directory so a rename survives power loss (Unix; Windows cannot open directories this way,
/// and NTFS journals the rename). Best effort: a failure here never fails the write.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// A path that is not there: absent, or beneath a regular file (JS folder backend: TypeMismatchError is
/// "missing"). 267 = ERROR_DIRECTORY, 20 = ENOTDIR.
fn is_missing(e: &std::io::Error) -> bool {
    matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) || e.raw_os_error() == Some(if cfg!(windows) { 267 } else { 20 })
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if is_missing(&e) => return Ok(()),
        Err(e) => return Err(e),
    };
    for e in rd {
        let e = e?;
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with(TMP_PREFIX) {
            continue;
        }
        let key = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if e.file_type()?.is_dir() {
            walk(&e.path(), &key, out)?;
        } else {
            out.push(key);
        }
    }
    Ok(())
}

impl Store for FsStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(key)?) {
            Ok(b) => Ok(Some(b)),
            Err(e) if is_missing(&e) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        let p = self.path(key)?;
        let dir = p.parent().unwrap_or(&self.root);
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(
            "{TMP_PREFIX}{}-{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let res = (|| {
            let mut f = std::fs::File::create(&tmp)?;
            std::io::Write::write_all(&mut f, value)?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &p)?;
            sync_dir(dir);
            Ok::<(), std::io::Error>(())
        })();
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res?;
        self.sweep_stale(dir);
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        // Start the walk at the deepest whole directory of the prefix instead of the root.
        let (dir_rel, _) = prefix.rsplit_once('/').unwrap_or(("", prefix));
        let start = if dir_rel.is_empty() { self.root.clone() } else { self.path(dir_rel)? };
        let mut out = vec![];
        walk(&start, dir_rel, &mut out)?;
        out.retain(|k| k.starts_with(prefix));
        out.sort();
        Ok(out)
    }

    async fn children(&self, dir: &str) -> Result<Vec<(String, bool)>> {
        // read_dir directly so empty directories are visible (unique_slug sees them, like JS)
        let d = dir.trim_matches('/');
        let p = if d.is_empty() { self.root.clone() } else { self.path(d)? };
        let rd = match std::fs::read_dir(&p) {
            Ok(r) => r,
            Err(e) if is_missing(&e) => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut out = vec![];
        for e in rd {
            let e = e?;
            let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
            if name.starts_with(TMP_PREFIX) {
                continue;
            }
            out.push((name, e.file_type()?.is_dir()));
        }
        out.sort();
        Ok(out)
    }

    async fn delete_tree(&self, path: &str) -> Result<()> {
        let p = self.path(path.trim_end_matches('/'))?;
        let r = match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&p),
            Ok(_) => std::fs::remove_file(&p),
            Err(e) => Err(e),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) if is_missing(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.path(key)?) {
            Ok(()) => Ok(()),
            Err(e) if is_missing(&e) => Ok(()),
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

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vishion-store-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn legacy_slash_keys_nest_list_and_delete() {
        let dir = tmp_dir("nest");
        let s = FsStore::new(&dir);
        block_on(s.put("quiz-1/project.json", b"{}")).unwrap();
        block_on(s.put("quiz-1/pages/p1.json", b"{\"a\":1}")).unwrap();
        block_on(s.put("quiz-2/project.json", b"{}")).unwrap();
        assert!(dir.join("quiz-1").join("pages").join("p1.json").is_file());
        assert_eq!(
            block_on(s.get("quiz-1/pages/p1.json")).unwrap().as_deref(),
            Some(&b"{\"a\":1}"[..])
        );
        assert_eq!(
            block_on(s.list("quiz-1/")).unwrap(),
            vec!["quiz-1/pages/p1.json", "quiz-1/project.json"]
        );
        assert_eq!(block_on(s.list("")).unwrap().len(), 3);
        block_on(s.delete("quiz-1/pages/p1.json")).unwrap();
        assert_eq!(block_on(s.get("quiz-1/pages/p1.json")).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn traversal_and_odd_keys_are_rejected() {
        let dir = tmp_dir("bad");
        let s = FsStore::new(&dir);
        for k in [
            "",
            "/",
            "/a",
            "a/",
            "a//b",
            "../x",
            "a/../b",
            "./a",
            "a\\b",
            "c:x",
            "a/\0",
            ".vishion-tmp-1-1",
            "p/a.",
            "p/a ",
            "p/NUL",
            "p/nul.txt",
            "COM1",
        ] {
            assert!(
                block_on(s.put(k, b"x")).is_err(),
                "key {k:?} should be rejected"
            );
            assert!(block_on(s.get(k)).is_err(), "key {k:?} should be rejected");
        }
        assert!(!dir.exists(), "rejected keys must not create anything");
    }

    #[test]
    fn put_overwrites_atomically_and_leaves_no_temp_files() {
        let dir = tmp_dir("atomic");
        let s = FsStore::new(&dir);
        block_on(s.put("p/project.json", b"one")).unwrap();
        block_on(s.put("p/project.json", b"two-longer")).unwrap();
        assert_eq!(
            block_on(s.get("p/project.json")).unwrap().as_deref(),
            Some(&b"two-longer"[..])
        );
        let names: Vec<_> = std::fs::read_dir(dir.join("p"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["project.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
