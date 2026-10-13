//! The host side of the UI seam (`vishion_ui::host`): turns `Cmd`s into project-layer calls over a `Store`
//! and answers with `Event`s. Native runs it on a worker thread (`main.rs`), web on the local executor
//! (`lib.rs`). Nothing here draws; nothing in `ui` touches storage.
use serde_json::Value;
use std::future::Future;
use vishion_core::model::Settings;
use vishion_store::mem::MemStore;
use vishion_store::projects::Projects;
use vishion_store::{Result, Store};
use vishion_ui::{Cmd, Event};

/// The stores the app can run on. One concrete type keeps `Backend` and the executors non-generic.
pub enum AnyStore {
    Mem(MemStore),
    #[cfg(not(target_arch = "wasm32"))]
    Fs(vishion_store::fs::FsStore),
    #[cfg(target_arch = "wasm32")]
    Idb(vishion_store::idb::IdbStore),
}

impl Store for AnyStore {
    fn get(&self, key: &str) -> impl Future<Output = Result<Option<Vec<u8>>>> {
        async move {
            match self {
                AnyStore::Mem(s) => s.get(key).await,
                #[cfg(not(target_arch = "wasm32"))]
                AnyStore::Fs(s) => s.get(key).await,
                #[cfg(target_arch = "wasm32")]
                AnyStore::Idb(s) => s.get(key).await,
            }
        }
    }
    fn put(&self, key: &str, value: &[u8]) -> impl Future<Output = Result<()>> {
        async move {
            match self {
                AnyStore::Mem(s) => s.put(key, value).await,
                #[cfg(not(target_arch = "wasm32"))]
                AnyStore::Fs(s) => s.put(key, value).await,
                #[cfg(target_arch = "wasm32")]
                AnyStore::Idb(s) => s.put(key, value).await,
            }
        }
    }
    fn list(&self, prefix: &str) -> impl Future<Output = Result<Vec<String>>> {
        async move {
            match self {
                AnyStore::Mem(s) => s.list(prefix).await,
                #[cfg(not(target_arch = "wasm32"))]
                AnyStore::Fs(s) => s.list(prefix).await,
                #[cfg(target_arch = "wasm32")]
                AnyStore::Idb(s) => s.list(prefix).await,
            }
        }
    }
    fn delete(&self, key: &str) -> impl Future<Output = Result<()>> {
        async move {
            match self {
                AnyStore::Mem(s) => s.delete(key).await,
                #[cfg(not(target_arch = "wasm32"))]
                AnyStore::Fs(s) => s.delete(key).await,
                #[cfg(target_arch = "wasm32")]
                AnyStore::Idb(s) => s.delete(key).await,
            }
        }
    }
}

pub struct Backend {
    pub projects: Projects<AnyStore>,
    /// Shown in the save indicator.
    pub storage: String,
}

impl Backend {
    pub fn new(store: AnyStore, storage: &str) -> Self {
        Backend { projects: Projects::new(store), storage: storage.into() }
    }

    /// Open `want`, or the most recently updated assignment. Answers `Empty` / `Failed` / `Opened`.
    pub async fn open(&self, want: Option<&str>, now: i64) -> Event {
        let slug = match want {
            Some(s) => s.to_string(),
            None => match self.projects.list_projects(now).await {
                Ok(list) => match list.into_iter().next() {
                    Some(s) => s.slug,
                    None => return Event::Empty { storage: self.storage.clone() },
                },
                Err(e) => return Event::Failed(e.to_string()),
            },
        };
        let settings = match self.projects.load_settings().await {
            Ok(Some(m)) => Settings::load(&Value::Object(m)),
            // a damaged settings.json is left alone; the app runs on defaults for this session
            _ => Settings::defaults(),
        };
        match self.projects.open_project(&slug, true, now).await {
            Ok(Some(o)) => Event::Opened {
                slug: o.slug,
                assignment: Box::new(o.assignment),
                settings: Box::new(settings),
                storage: self.storage.clone(),
                unreadable: o.unreadable.into_iter().map(|u| u.id).collect(),
                notice: o.from_backup.then(|| {
                    if o.restored {
                        "project.json was damaged, so it was restored from the last backup.".to_string()
                    } else {
                        "project.json was damaged, so this is the last backup.".to_string()
                    }
                }),
            },
            Ok(None) => Event::Failed(format!("There is no assignment called \u{201C}{slug}\u{201D}.")),
            Err(e) => Event::Failed(e.to_string()),
        }
    }

    pub async fn handle(&self, cmd: Cmd, now: i64) -> Event {
        match cmd {
            Cmd::LoadImage { slug, id } => match self.projects.load_image(&slug, &id).await {
                Ok(bytes) => Event::Image { id, bytes },
                Err(_) => Event::Image { id, bytes: None },
            },
            Cmd::SavePage { slug, page } => done(self.projects.save_page(&slug, &page).await),
            Cmd::SaveMeta { slug, mut meta } => done(self.projects.save_meta(&slug, &mut meta, Some(now)).await),
            Cmd::SaveSettings { settings } => done(self.projects.save_settings(&settings).await),
        }
    }
}

fn done(r: Result<()>) -> Event {
    match r {
        Ok(()) => Event::Saved,
        Err(e) => Event::SaveFailed(e.to_string()),
    }
}

/// Run a future to completion on the calling thread. Native only: `FsStore` does blocking I/O inside
/// `async fn`, so every future completes on its first polls and this never has to wait.
#[cfg(not(target_arch = "wasm32"))]
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}
