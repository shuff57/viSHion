//! In-memory `Store` (tests, previews). Same key rules as the web backends.
use crate::{check_key, Result, Store};
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct MemStore {
    map: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.map.borrow().len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.borrow().is_empty()
    }
}

impl Store for MemStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        check_key(key)?;
        Ok(self.map.borrow().get(key).cloned())
    }
    async fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        check_key(key)?;
        self.map.borrow_mut().insert(key.into(), value.to_vec());
        Ok(())
    }
    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self.map.borrow().keys().filter(|k| k.starts_with(prefix)).cloned().collect())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        check_key(key)?;
        self.map.borrow_mut().remove(key);
        Ok(())
    }
}
