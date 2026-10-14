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
use vishion_ui::{Cmd, Conn, Event, ReadOutcome};

use crate::pictures;

/// Said when words are guessed without demo mode (the guess runs on the judge model, which is not ported).
const NEEDS_OCR: &str = "Guessing unreadable words is not available with this server yet. Nothing was changed.";

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
    pick_with("Pictures", &["jpg", "jpeg", "png", "gif", "bmp", "webp"], "image/*", multiple).await
}

/// A project folder (`Import folder`): every file under it, named by its path from the folder's parent, so the
/// folder's own name is the first part (as `webkitRelativePath` is in a browser).
#[cfg(not(target_arch = "wasm32"))]
pub async fn pick_folder() -> Vec<Picked> {
    match rfd::FileDialog::new().pick_folder() {
        Some(dir) => read_folder(&dir),
        None => vec![],
    }
}

/// Every file under `dir`, named `<folder name>/<path>`.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_folder(dir: &std::path::Path) -> Vec<Picked> {
    fn walk(dir: &std::path::Path, rel: &str, out: &mut Vec<Picked>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            let rel = format!("{rel}/{name}");
            if path.is_dir() {
                walk(&path, &rel, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push(Picked { name: rel, bytes });
            }
        }
    }
    let root = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "folder".into());
    let mut out = vec![];
    walk(dir, &root, &mut out);
    out
}

/// A project zip (`Import zip`).
pub async fn pick_zip() -> Vec<Picked> {
    pick_with("Zip", &["zip"], ".zip,application/zip", false).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn pick_with(label: &str, exts: &[&str], _accept: &str, multiple: bool) -> Vec<Picked> {
    let d = rfd::FileDialog::new().add_filter(label, exts);
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
    pick_with("Pictures", &[], "image/*", multiple).await
}

/// A project folder (`Import folder`): `<input type=file webkitdirectory>`; names are `webkitRelativePath`.
#[cfg(target_arch = "wasm32")]
pub async fn pick_folder() -> Vec<Picked> {
    pick_inner("", true, true).await
}

#[cfg(target_arch = "wasm32")]
async fn pick_with(_label: &str, _exts: &[&str], accept: &str, multiple: bool) -> Vec<Picked> {
    pick_inner(accept, multiple, false).await
}

#[cfg(target_arch = "wasm32")]
async fn pick_inner(accept: &str, multiple: bool, folder: bool) -> Vec<Picked> {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return vec![] };
    let Ok(el) = doc.create_element("input") else { return vec![] };
    let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() else { return vec![] };
    input.set_type("file");
    input.set_accept(accept);
    if folder {
        let _ = input.set_attribute("webkitdirectory", "");
    }
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
            let rel = js_sys::Reflect::get(&f, &"webkitRelativePath".into()).ok().and_then(|v| v.as_string()).filter(|s| !s.is_empty());
            let name = rel.unwrap_or_else(|| f.name());
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
    /// A folder the person chose with the browser's folder picker.
    #[cfg(target_arch = "wasm32")]
    Fsa(vishion_store::fsa::FsaStore),
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
                #[cfg(target_arch = "wasm32")]
                AnyStore::Fsa(s) => s.get(key).await,
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
                #[cfg(target_arch = "wasm32")]
                AnyStore::Fsa(s) => s.put(key, value).await,
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
                #[cfg(target_arch = "wasm32")]
                AnyStore::Fsa(s) => s.list(prefix).await,
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
                #[cfg(target_arch = "wasm32")]
                AnyStore::Fsa(s) => s.delete(key).await,
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
    /// `Q.stop`: set from the UI thread while a run is reading a page.
    pub cancel: vishion_net::Cancel,
}

impl Backend {
    pub fn new(store: AnyStore, storage: &str) -> Self {
        Backend { projects: Projects::new(store), storage: storage.into(), demo: false, cancel: Default::default() }
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

    /// `settings.straighten !== false`: new pictures are levelled when they are picked.
    async fn straighten_on(&self) -> bool {
        match self.projects.load_settings().await {
            Ok(Some(m)) => Settings::load(&Value::Object(m)).get("straighten").and_then(Value::as_bool) != Some(false),
            _ => true,
        }
    }

    /// `prepare`, then `deskew` when the setting asks for it (upload.js `doReplace`, add.js).
    async fn prepare_picture(&self, bytes: &[u8]) -> std::result::Result<Vec<u8>, String> {
        let jpg = pictures::prepare(bytes)?;
        if !self.straighten_on().await {
            return Ok(jpg);
        }
        Ok(match pictures::deskew(&jpg) {
            Ok((Some(level), _)) => level,
            _ => jpg,
        })
    }

    /// `doReplace` of js/ui/upload.js: the picture is checked, stored, and the page told to reset itself.
    pub async fn replace_with(&self, slug: &str, id: &str, f: Picked) -> Event {
        match self.prepare_picture(&f.bytes).await {
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
            match self.prepare_picture(&f.bytes).await {
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

    /// The name at the top of a prepared page (js/student.js `read`): a separate short request to the same
    /// server. `Ok(None)` is "no name found"; `Err` is a server that did not answer.
    async fn read_name(&self, jpg: &[u8], conn: &Conn) -> std::result::Result<Option<String>, ()> {
        let strip = pictures::crop_top(jpg, vishion_core::student::STRIP).map_err(|_| ())?;
        let req = vishion_net::Request {
            endpoint: vishion_net::Endpoint::new(conn.url.clone()),
            api_key: &conn.api_key,
            model: &conn.model,
            prompt: vishion_core::student::ASK,
            image: &strip,
            timeout_secs: 45,
            poll_slots: false,
        };
        match vishion_net::Provider::for_setting(&conn.provider).complete(req, 0.1, 60).await {
            Ok(t) => Ok(vishion_core::student::parse_reply(&t)),
            Err(vishion_net::Error::Http(_)) => Err(()),
            Err(_) => Err(()),
        }
    }

    /// The staging step of js/ui/add.js `addFiles` + `process`: each file is checked, shrunk (and levelled when
    /// the setting asks), and given a student name. Results go to `emit` one by one so the screen fills up.
    pub async fn stage_files(&self, mut files: Vec<Picked>, conn: &Conn, read_names: bool, preset: Option<&str>, emit: &dyn Fn(Event)) {
        let demo = self.demo || conn.demo || self.demo_mode().await;
        let mut skipped: Vec<(String, String)> = vec![];
        // `page2` before `page10`
        files.sort_by(|a, b| vishion_core::jsx::nat_compare(&a.name, &b.name));
        let mut down = false;
        for f in files {
            if let Some(why) = vishion_core::student::check_file(&f.name, f.bytes.len()) {
                skipped.push((f.name, why.to_string()));
                continue;
            }
            let jpg = match self.prepare_picture(&f.bytes).await {
                Ok(j) => j,
                Err(_) => {
                    skipped.push((f.name, "Could not read this image.".into()));
                    continue;
                }
            };
            let (name, source) = if let Some(p) = preset {
                (Some(p.to_string()), "chosen")
            } else if !read_names {
                (None, "file")
            } else if demo {
                (vishion_core::student::demo_name(&f.name).map(String::from), "read")
            } else if down {
                (None, "file")
            } else {
                match self.read_name(&jpg, conn).await {
                    Ok(n) => (n, "read"),
                    Err(()) => {
                        down = true;
                        (None, "file")
                    }
                }
            };
            emit(Event::Staged { item: vishion_ui::StagedItem { file_name: f.name, bytes: jpg, name, source: source.into() }, names_down: down });
        }
        // always sent: it ends the staging (the screen stops saying it is working)
        emit(Event::Skipped(skipped));
    }

    /// Ask for pictures, then stage them one by one (`emit` per picture).
    pub async fn stage_pictures(&self, conn: &Conn, read_names: bool, preset: Option<&str>, emit: &dyn Fn(Event)) {
        let files = pick_pictures(true).await;
        self.stage_files(files, conn, read_names, preset, emit).await;
    }

    /// Save staged pictures as pages (js/ui/add.js `commit`): the student comes from the staging step, or is the
    /// file name when none was found.
    pub async fn commit_staged(&self, slug: &str, items: Vec<vishion_ui::StagedItem>, now: i64) -> Event {
        let mut pages: Vec<(Page, Vec<u8>)> = vec![];
        for (n, it) in items.into_iter().enumerate() {
            let id = uid(now, n as u64);
            if self.projects.save_image(slug, &id, &it.bytes).await.is_err() {
                return Event::Notice { text: format!("Could not save {}.", it.file_name), error: true };
            }
            let mut p = Assignment::new_page(&id, &it.file_name);
            let student = match &it.name {
                Some(n) => serde_json::json!({"name": n, "source": it.source, "confirmed": it.source != "read"}),
                None => {
                    let stem = match it.file_name.rfind('.') {
                        Some(d) if d > 0 => &it.file_name[..d],
                        _ => &it.file_name,
                    };
                    serde_json::json!({"name": stem, "source": "file", "confirmed": false})
                }
            };
            p.set("student", student);
            pages.push((p, it.bytes));
        }
        if pages.is_empty() {
            return Event::Notice { text: "No picture was added.".into(), error: false };
        }
        Event::PagesAdded { pages }
    }

    /// Sessions of the old single-file app and their pictures (`data:` URLs by session id). On the web they are
    /// read in place from this browser's storage; natively the person picks an exported JSON file
    /// (`[sessions]` or `{ sessions, images }`).
    async fn legacy_input(&self) -> Option<(Vec<Value>, std::collections::BTreeMap<String, String>)> {
        #[cfg(target_arch = "wasm32")]
        {
            let sessions = vishion_core::import::parse_sessions(vishion_store::legacy_web::sessions_json().as_deref());
            if sessions.is_empty() {
                return None;
            }
            Some((sessions, vishion_store::legacy_web::images().await))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let f = pick_with("Old sessions", &["json"], ".json,application/json", false).await.pop()?;
            Some(vishion_core::import::parse_export(&String::from_utf8_lossy(&f.bytes)))
        }
    }

    /// Whether Home offers "Import old sessions": sessions are waiting in this browser, or (native) a file can
    /// be picked.
    fn legacy_available(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            vishion_core::import::available(vishion_store::legacy_web::sessions_json().as_deref())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            true
        }
    }

    async fn import_legacy(&self, now: i64) -> Event {
        let Some((sessions, images)) = self.legacy_input().await else {
            return Event::Notice { text: "No sessions from the old app were found.".into(), error: false };
        };
        self.import_legacy_from(&sessions, &images, now).await
    }

    /// The conversion and the write of [`Backend::import_legacy`], with the sessions already in hand.
    pub async fn import_legacy_from(&self, sessions: &[Value], images: &std::collections::BTreeMap<String, String>, now: i64) -> Event {
        let settings = match self.projects.load_settings().await {
            Ok(Some(m)) => Settings::load(&Value::Object(m)),
            _ => Settings::defaults(),
        };
        let topics = settings.get("topics").and_then(Value::as_object).cloned().unwrap_or_default();
        let opt = vishion_core::import::Options { name: None, topics: &topics, now };
        let mut n = 0u64;
        let mut new_id = || {
            n += 1;
            uid(now, 1000 + n)
        };
        let plan = vishion_core::import::plan(
            sessions,
            images,
            &opt,
            &mut new_id,
            &mut |url, _name| {
                let (_, bytes) = vishion_core::import::decode_data_url(url)?;
                pictures::prepare(&bytes).ok()
            },
            &mut |_img, _raw| vec![],
        );
        match plan {
            Err(m) => Event::Notice { text: m, error: true },
            Ok(plan) => match self.projects.apply_legacy(&plan, now).await {
                Ok(done) => {
                    let skipped = done.pages - done.with_image;
                    let text = format!(
                        "Imported {}{}.",
                        vishion_ui::state::plural(done.pages, "page", "pages"),
                        if skipped > 0 { format!(" ({skipped} without an image)") } else { String::new() }
                    );
                    Event::Many(vec![Event::Notice { text, error: false }, self.open(Some(&done.slug), now).await])
                }
                Err(e) => Event::Notice { text: format!("Import failed: {e}"), error: true },
            },
        }
    }

    /// Demo mode of js/ocr.js: the sample assignment, or the `demo` setting.
    async fn demo_mode(&self) -> bool {
        if self.demo {
            return true;
        }
        match self.projects.load_settings().await {
            Ok(Some(m)) => Settings::load(&Value::Object(m)).get("demo").and_then(Value::as_bool) == Some(true),
            _ => false,
        }
    }

    /// `Q.start`: read these pages one after the other, sending `emit` the progress of each (`Reading`) and
    /// its outcome (`Transcribed`). Stops at the first Stop or unreachable server like `queue.js`.
    pub async fn read_pages(&self, slug: &str, ids: Vec<String>, conn: Conn, emit: &dyn Fn(Event)) {
        let demo = self.demo || conn.demo || self.demo_mode().await;
        self.cancel.reset();
        for (index, id) in ids.into_iter().enumerate() {
            if self.cancel.is_set() {
                break;
            }
            let out = if demo { self.read_demo(index) } else { self.read_live(slug, &id, &conn, emit).await };
            let stop = matches!(out, ReadOutcome::Stopped | ReadOutcome::Unreachable(_));
            emit(Event::Transcribed { id, outcome: out });
            if stop {
                break;
            }
        }
    }

    /// Demo mode of js/ocr.js: sample text and sample guesses.
    fn read_demo(&self, index: usize) -> ReadOutcome {
        let r = vishion_core::demo::read(index);
        let (_, n) = vishion_core::guess::numbered(&r.text);
        let guesses = Some(vishion_core::demo::guesses(n));
        ReadOutcome::Done { raw: r.text, tokens: r.tokens, ms: r.ms, guesses }
    }

    /// `Q.runOne` against the server.
    async fn read_live(&self, slug: &str, id: &str, conn: &Conn, emit: &dyn Fn(Event)) -> ReadOutcome {
        use vishion_net::{Error, Progress, Request};
        let image = match self.projects.load_image(slug, id).await {
            Ok(Some(b)) => b,
            _ => return ReadOutcome::Failed("The image for this page is missing from the project.".into()),
        };
        emit(Event::Reading { id: id.into(), text: String::new() });
        let req = Request {
            endpoint: vishion_net::Endpoint::new(conn.url.clone()),
            api_key: &conn.api_key,
            model: &conn.model,
            prompt: &conn.prompt,
            image: &image,
            timeout_secs: conn.timeout_secs,
            poll_slots: vishion_net::Provider::for_setting(&conn.provider).polls_slots(),
        };
        let mut on = |p: Progress| {
            if let Progress::Text { full, .. } = p {
                emit(Event::Reading { id: id.into(), text: full.to_string() });
            }
        };
        match vishion_net::Provider::for_setting(&conn.provider).transcribe(req, &self.cancel, &mut on).await {
            Ok(d) => {
                let raw = vishion_core::clean::clean(&d.text);
                // `autoGuess`: when a page finishes with unreadable words, look again (a failure leaves none)
                let (numbered, n) = vishion_core::guess::numbered(&raw);
                let guesses = if conn.guess && n > 0 {
                    self.guess_live(slug, id, &numbered, n, conn, &conn.examples).await.ok()
                } else {
                    None
                };
                ReadOutcome::Done { raw, tokens: d.tokens, ms: d.ms, guesses }
            }
            Err(Error::Aborted) => ReadOutcome::Stopped,
            Err(Error::TimedOut) => ReadOutcome::Failed(format!(
                "Timed out after {} seconds. Retry, or raise the timeout in Settings.",
                conn.timeout_secs
            )),
            Err(Error::Network(m)) | Err(Error::Unavailable(m)) => ReadOutcome::Unreachable(m),
            Err(Error::Http(m)) => ReadOutcome::Failed(m),
            Err(Error::Stream { message, .. }) => ReadOutcome::Failed(message),
        }
    }

    /// `W.guess` against the server: the page picture and the numbered transcript, one answer back.
    async fn guess_live(&self, slug: &str, id: &str, numbered: &str, count: usize, conn: &Conn, examples: &[String]) -> std::result::Result<serde_json::Map<String, Value>, String> {
        let image = match self.projects.load_image(slug, id).await {
            Ok(Some(b)) => b,
            _ => return Err("The image is missing.".into()),
        };
        let prompt = vishion_core::guess::prompt(numbered, examples);
        let req = vishion_net::Request {
            endpoint: vishion_net::Endpoint::new(conn.url.clone()),
            api_key: &conn.api_key,
            model: &conn.model,
            prompt: &prompt,
            image: &image,
            timeout_secs: conn.timeout_secs,
            poll_slots: false,
        };
        match vishion_net::Provider::for_setting(&conn.provider).complete(req, 0.2, 500).await {
            Ok(t) => Ok(vishion_core::guess::parse_guesses(&t, count)),
            Err(vishion_net::Error::Http(m)) | Err(vishion_net::Error::Network(m)) | Err(vishion_net::Error::Unavailable(m)) => Err(m),
            Err(_) => Err(NEEDS_OCR.into()),
        }
    }

    /// `fs.listProjects` as the home screen's cards.
    async fn list_event(&self, now: i64) -> Event {
        match self.projects.list_projects(now).await {
            Ok(list) => Event::Projects(
                list.into_iter()
                    .map(|s| vishion_ui::Card { slug: s.slug, name: s.name, pages: s.pages.max(0) as usize, done: s.done.max(0) as usize, updated: s.updated })
                    .collect(),
            ),
            Err(e) => Event::Notice { text: format!("Could not list the assignments: {e}"), error: true },
        }
    }

    /// The list again after a change, or the toast saying why not.
    async fn changed(&self, r: std::result::Result<(), impl std::fmt::Display>, what: &str, now: i64) -> Event {
        match r {
            Ok(()) => self.list_event(now).await,
            Err(e) => Event::Notice { text: format!("{what}: {e}"), error: true },
        }
    }

    pub async fn handle(&self, cmd: Cmd, now: i64) -> Event {
        match cmd {
            Cmd::RemovePage { slug, id } => done(self.projects.delete_page(&slug, &id).await),
            Cmd::RotateImage { slug, id } => match self.projects.load_image(&slug, &id).await {
                Ok(Some(b)) => match pictures::rotate90(&b) {
                    Ok(r) => match self.projects.save_image(&slug, &id, &r).await {
                        Ok(()) => Event::Many(vec![Event::Image { id, bytes: Some(r) }, Event::Saved]),
                        Err(e) => Event::SaveFailed(e.to_string()),
                    },
                    Err(e) => Event::SaveFailed(format!("Could not turn this page: {e}")),
                },
                _ => Event::SaveFailed("The image of this page is missing.".into()),
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
            Cmd::Straighten { slug, id } => match self.projects.load_image(&slug, &id).await {
                Ok(Some(b)) => match pictures::deskew(&b) {
                    Ok((Some(jpg), angle)) => match self.projects.save_image(&slug, &id, &jpg).await {
                        Ok(()) => Event::Straightened { id, angle, bytes: Some(jpg) },
                        Err(e) => Event::SaveFailed(e.to_string()),
                    },
                    Ok((None, _)) => Event::Straightened { id, angle: 0.0, bytes: None },
                    Err(_) => Event::Notice { text: "Could not straighten this page.".into(), error: true },
                },
                _ => Event::Notice { text: "The image of this page is missing.".into(), error: true },
            },
            // the executors stream this one with `read_pages`; here it is collected (tests, simple hosts)
            Cmd::Transcribe { slug, ids, conn } => {
                let out = std::cell::RefCell::new(vec![]);
                self.read_pages(&slug, ids, conn, &|e| out.borrow_mut().push(e)).await;
                Event::Many(out.into_inner())
            }
            Cmd::StagePictures { conn, read_names, preset } => {
                let out = std::cell::RefCell::new(vec![]);
                self.stage_pictures(&conn, read_names, preset.as_deref(), &|e| out.borrow_mut().push(e)).await;
                Event::Many(out.into_inner())
            }
            Cmd::StageFiles { files, conn, read_names, preset } => {
                let out = std::cell::RefCell::new(vec![]);
                let files = files.into_iter().map(|(name, bytes)| Picked { name, bytes }).collect();
                self.stage_files(files, &conn, read_names, preset.as_deref(), &|e| out.borrow_mut().push(e)).await;
                Event::Many(out.into_inner())
            }
            Cmd::CommitStaged { slug, items } => self.commit_staged(&slug, items, now).await,
            Cmd::ImportZip => {
                let Some(f) = pick_zip().await.pop() else {
                    return Event::Notice { text: "No file chosen.".into(), error: false };
                };
                match vishion_core::export::read_zip(&f.bytes) {
                    Err(m) => Event::Notice { text: m, error: true },
                    Ok(entries) => match self.projects.import_files(&entries).await {
                        Ok(slug) => Event::Many(vec![Event::Notice { text: "Assignment imported.".into(), error: false }, self.open(Some(&slug), now).await]),
                        Err(e) => Event::Notice { text: format!("Import failed: {e}"), error: true },
                    },
                }
            }
            Cmd::ImportFolder => {
                let files = pick_folder().await;
                if files.is_empty() {
                    return Event::Notice { text: "No folder chosen.".into(), error: false };
                }
                let entries: Vec<(String, Vec<u8>)> = files.into_iter().map(|f| (f.name, f.bytes)).collect();
                match self.projects.import_files(&entries).await {
                    Ok(slug) => Event::Many(vec![Event::Notice { text: "Assignment imported.".into(), error: false }, self.open(Some(&slug), now).await]),
                    Err(e) => Event::Notice { text: format!("Import failed: {e}"), error: true },
                }
            }
            Cmd::DownloadZip { slug } => match self.projects.project_files(&slug).await {
                Ok(files) => {
                    let name = format!("{slug}.zip");
                    let bytes = vishion_core::export::store_zip(&files);
                    match self.projects.write_export(&slug, &name, &bytes).await {
                        Ok(()) => Event::Notice { text: format!("Saved exports/{name}."), error: false },
                        Err(e) => Event::Notice { text: format!("Could not build the zip: {e}"), error: true },
                    }
                }
                Err(e) => Event::Notice { text: format!("Could not build the zip: {e}"), error: true },
            },
            // the folder card is a browser thing: the web executor answers it, a native host has no such card
            Cmd::Folder(_) => Event::Many(vec![]),
            Cmd::ImportLegacy => self.import_legacy(now).await,
            Cmd::ListProjects => Event::Many(vec![Event::LegacyAvailable(self.legacy_available()), self.list_event(now).await]),
            Cmd::OpenProject { slug } => self.open(Some(&slug), now).await,
            Cmd::CreateProject { name } => match self.projects.create_project(&name, now).await {
                Ok(slug) => self.open(Some(&slug), now).await,
                Err(e) => Event::Notice { text: format!("Could not create the assignment: {e}"), error: true },
            },
            Cmd::RenameProject { slug, name } => {
                let r = self.projects.rename_project(&slug, &name, now).await;
                self.changed(r.map(|_| ()), "Could not rename the assignment", now).await
            }
            Cmd::DuplicateProject { slug } => match self.projects.duplicate_project(&slug, now).await {
                Ok(_) => Event::Many(vec![Event::Notice { text: "Assignment duplicated.".into(), error: false }, self.list_event(now).await]),
                Err(e) => Event::Notice { text: format!("Could not duplicate: {e}"), error: true },
            },
            Cmd::DeleteProject { slug } => {
                let r = self.projects.delete_project(&slug).await;
                self.changed(r, "Could not delete the assignment", now).await
            }
            Cmd::Stop => {
                self.cancel.stop();
                Event::Many(vec![])
            }
            Cmd::Guess { slug, id, raw, conn, examples } => {
                let (numbered, n) = vishion_core::guess::numbered(&raw);
                if self.demo_mode().await {
                    Event::Guessed { id, result: Ok(vishion_core::demo::guesses(n)) }
                } else if n == 0 {
                    Event::Guessed { id, result: Ok(Default::default()) }
                } else {
                    Event::Guessed { id: id.clone(), result: self.guess_live(&slug, &id, &numbered, n, &conn, &examples).await }
                }
            }
            Cmd::Export { slug, name, bytes } => match self.projects.write_export(&slug, &name, &bytes).await {
                Ok(()) => Event::Notice { text: format!("Exported to exports/{name}."), error: false },
                Err(e) => Event::Notice { text: format!("Could not export: {e}"), error: true },
            },
            // answered by the web executor, which owns `fetch`; the native app never asks
            Cmd::LoadMathFont | Cmd::LoadFallbackFont | Cmd::LoadEmojiFont => Event::Notice { text: String::new(), error: false },
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
