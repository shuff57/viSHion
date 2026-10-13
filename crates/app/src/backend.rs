//! The host side of the UI seam (`vishion_ui::host`): turns `Cmd`s into project-layer calls over a `Store`
//! and answers with `Event`s. Native runs it on a worker thread (`main.rs`), web on the local executor
//! (`lib.rs`). Nothing here draws; nothing in `ui` touches storage.
use serde_json::Value;
use std::future::Future;
use vishion_core::model::Settings;
use vishion_store::mem::MemStore;
use vishion_store::projects::Projects;
use vishion_store::{Result, Store};
use vishion_core::model::{Assignment, Page};
use vishion_ui::{Cmd, Event};

use crate::pictures;

/// A file the user picked: its name and its bytes.
#[derive(Clone, Debug)]
pub struct Picked {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// `OCR.uid()`: base36 time plus five more base36 characters.
pub fn uid(now: i64, salt: u64) -> String {
    fn b36(mut n: u64) -> String {
        const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        if n == 0 {
            return "0".into();
        }
        let mut v = vec![];
        while n > 0 {
            v.push(D[(n % 36) as usize]);
            n /= 36;
        }
        v.reverse();
        String::from_utf8(v).unwrap()
    }
    // splitmix64 over the time and a per-call salt gives the random tail
    let mut z = (now as u64).wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let tail = b36(z % 36u64.pow(5));
    format!("{}{:0>5}", b36(now as u64), tail)
}

/// The file dialog (native). The worker thread blocks on it, which is what it is for.
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_pictures(multiple: bool) -> Vec<Picked> {
    let d = rfd::FileDialog::new().add_filter("Pictures", &["jpg", "jpeg", "png", "gif", "bmp", "webp"]);
    let files = if multiple { d.pick_files().unwrap_or_default() } else { d.pick_file().into_iter().collect() };
    files
        .into_iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().to_string();
            Some(Picked { name, bytes: std::fs::read(&p).ok()? })
        })
        .collect()
}

/// `<input type=file>` (web). The click happens right after the user's own click, so the browser allows it.
#[cfg(target_arch = "wasm32")]
pub async fn pick_pictures(multiple: bool) -> Vec<Picked> {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return vec![] };
    let Ok(el) = doc.create_element("input") else { return vec![] };
    let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() else { return vec![] };
    input.set_type("file");
    input.set_accept("image/*");
    input.set_multiple(multiple);
    let inp = input.clone();
    let p = js_sys::Promise::new(&mut |res, _| {
        let r1 = res.clone();
        let on_change = Closure::once_into_js(move || {
            let _ = r1.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = input.add_event_listener_with_callback("change", on_change.unchecked_ref());
        let r2 = res;
        let on_cancel = Closure::once_into_js(move || {
            let _ = r2.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = input.add_event_listener_with_callback("cancel", on_cancel.unchecked_ref());
        input.click();
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
    let mut out = vec![];
    if let Some(list) = inp.files() {
        for i in 0..list.length() {
            let Some(f) = list.item(i) else { continue };
            let name = f.name();
            if let Ok(buf) = wasm_bindgen_futures::JsFuture::from(f.array_buffer()).await {
                out.push(Picked { name, bytes: js_sys::Uint8Array::new(&buf).to_vec() });
            }
        }
    }
    out
}

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
    /// The sample assignment: reading a page answers with sample text (js/ocr.js demo mode) instead of
    /// needing a server.
    pub demo: bool,
}

impl Backend {
    pub fn new(store: AnyStore, storage: &str) -> Self {
        Backend { projects: Projects::new(store), storage: storage.into(), demo: false }
    }

    /// A backend over the in-memory sample assignment (`--demo`, `?demo=1`).
    pub fn new_demo(store: AnyStore, storage: &str) -> Self {
        Backend { demo: true, ..Self::new(store, storage) }
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

    /// `doReplace` of js/ui/upload.js: the picture is checked, stored, and the page told to reset itself.
    pub async fn replace_with(&self, slug: &str, id: &str, f: Picked) -> Event {
        match pictures::prepare(&f.bytes) {
            Ok(jpg) => match self.projects.save_image(slug, id, &jpg).await {
                Ok(()) => Event::PageReplaced { id: id.to_string(), name: f.name, bytes: jpg },
                Err(e) => Event::Notice { text: format!("Could not save that picture: {e}"), error: true },
            },
            Err(why) => Event::Notice { text: format!("{}: {why}", f.name), error: true },
        }
    }

    /// New pages for `student`, one per readable picture, pictures stored. The page files are written by
    /// the UI's own save path (which also records the order in the project).
    pub async fn add_with(&self, slug: &str, student: &str, files: Vec<Picked>, now: i64) -> Event {
        let mut pages: Vec<(Page, Vec<u8>)> = vec![];
        let mut skipped: Vec<String> = vec![];
        for (n, f) in files.into_iter().enumerate() {
            match pictures::prepare(&f.bytes) {
                Ok(jpg) => {
                    let id = uid(now, n as u64);
                    if self.projects.save_image(slug, &id, &jpg).await.is_err() {
                        skipped.push(f.name);
                        continue;
                    }
                    let mut p = Assignment::new_page(&id, &f.name);
                    p.set("student", serde_json::json!({"name": student, "source": "chosen", "confirmed": true}));
                    pages.push((p, jpg));
                }
                Err(_) => skipped.push(f.name),
            }
        }
        if pages.is_empty() {
            let text = if skipped.is_empty() {
                "No picture was added.".to_string()
            } else {
                format!("Could not read {}.", skipped.join(", "))
            };
            return Event::Notice { text, error: !skipped.is_empty() };
        }
        Event::PagesAdded { pages }
    }

    pub async fn handle(&self, cmd: Cmd, now: i64) -> Event {
        match cmd {
            Cmd::RemovePage { slug, id } => done(self.projects.delete_page(&slug, &id).await),
            Cmd::RotateImage { slug, id } => match self.projects.load_image(&slug, &id).await {
                Ok(Some(b)) => match pictures::rotate90(&b) {
                    Ok(r) => match self.projects.save_image(&slug, &id, &r).await {
                        Ok(()) => Event::Image { id, bytes: Some(r) },
                        Err(e) => Event::SaveFailed(e.to_string()),
                    },
                    Err(e) => Event::Notice { text: format!("Could not turn this page: {e}"), error: true },
                },
                _ => Event::Notice { text: "The image of this page is missing.".into(), error: true },
            },
            Cmd::ReplaceImage { slug, id } => {
                let mut files = pick_pictures(false).await;
                match files.pop() {
                    Some(f) => self.replace_with(&slug, &id, f).await,
                    None => Event::Notice { text: "No picture chosen. The page is unchanged.".into(), error: false },
                }
            }
            Cmd::AddPages { slug, student } => {
                let files = pick_pictures(true).await;
                self.add_with(&slug, &student, files, now).await
            }
            Cmd::Straighten { .. } => Event::Notice {
                text: "Straightening needs the imaging port, which arrives with phase 5. The page is unchanged.".into(),
                error: false,
            },
            Cmd::Transcribe { .. } => Event::Notice {
                text: "Reading pages needs the OCR connection, which arrives with phase 3. Nothing was changed.".into(),
                error: true,
            },
            Cmd::Export { slug, name, bytes } => match self.projects.write_export(&slug, &name, &bytes).await {
                Ok(()) => Event::Notice { text: format!("Exported to exports/{name}."), error: false },
                Err(e) => Event::Notice { text: format!("Could not export: {e}"), error: true },
            },
            // answered by the web executor, which owns `fetch`; the native app never asks
            Cmd::LoadMathFont | Cmd::LoadFallbackFont => Event::Notice { text: String::new(), error: false },
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
