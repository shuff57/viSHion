//! The seam between the screen and the platform (plan: "everything platform-specific stays in crates/ui and
//! crates/app"). The UI never touches storage. It sends `Cmd`s and receives `Event`s; `crates/app` owns the
//! store (native `FsStore` on a worker thread, web `IdbStore` on the local executor) and the project layer.
use std::sync::{Arc, Mutex};
use vishion_core::model::{Assignment, Page, Project, Settings};

/// UI to host.
#[derive(Debug)]
pub enum Cmd {
    /// Read `<slug>/images/<id>.jpg`; answered by `Event::Image`.
    LoadImage { slug: String, id: String },
    /// `savePage`: written compact under the key the page was loaded from.
    SavePage { slug: String, page: Page },
    /// `saveMeta` with `meta.updated` touched (backup first, as the JS app does).
    SaveMeta { slug: String, meta: Project },
    /// `settings.json` (secrets stripped by the project layer).
    SaveSettings { settings: Settings },
    /// `deletePage`: the page file, its image and its originals. The UI already dropped it from the order.
    RemovePage { slug: String, id: String },
    /// Turn the stored image a quarter turn clockwise; answered by `Event::Image`.
    RotateImage { slug: String, id: String },
    /// Ask for a picture, store it as the page's image and answer `Event::PageReplaced`.
    ReplaceImage { slug: String, id: String },
    /// Ask for pictures and add one page each for `student`; answered by `Event::PagesAdded`.
    AddPages { slug: String, student: String },
    /// Straighten the stored image and redo its line boxes (needs the imaging port).
    Straighten { slug: String, id: String },
    /// Read these pages (again), in order: `Event::Transcribed` per page. Demo mode answers with sample text;
    /// otherwise the host needs the OCR connection (phase 3) and says so.
    Transcribe { slug: String, ids: Vec<String> },
    /// Ask for likely words for the unreadable words of a page (`W.guess`): `Event::Guessed`.
    Guess { slug: String, id: String, raw: String },
    /// Write `exports/<name>` (the strip's "Export N selected").
    Export { slug: String, name: String, bytes: Vec<u8> },
    /// Web: fetch the STIX Two Math font once (it is not part of the wasm, plan risk "wasm size").
    LoadMathFont,
    /// Web: fetch the CJK, Hangul and emoji fallback font (4 MB, not part of the wasm) when a page has text
    /// the bundled faces cannot draw. The native app links it in.
    LoadFallbackFont,
}

/// Host to UI.
#[derive(Debug)]
pub enum Event {
    /// An assignment is open. `storage` is the label for the save indicator ("Saved to your folder").
    Opened {
        slug: String,
        assignment: Box<Assignment>,
        settings: Box<Settings>,
        storage: String,
        /// Pages whose file could not be parsed (kept on disk, never rewritten).
        unreadable: Vec<String>,
        /// The assignment came from the backup copy of a damaged `project.json`.
        notice: Option<String>,
    },
    /// The store holds no assignment.
    Empty { storage: String },
    /// Opening failed (the message is shown on screen; nothing is written).
    Failed(String),
    Image { id: String, bytes: Option<Vec<u8>> },
    Saved,
    SaveFailed(String),
    /// A line for the toast (`OCR.toast`); `error` tints it as a problem.
    Notice { text: String, error: bool },
    /// A page's picture was replaced: the page is reset to "pending" and shows `bytes`.
    PageReplaced { id: String, name: String, bytes: Vec<u8> },
    /// New pages with their pictures, in order.
    PagesAdded { pages: Vec<(Page, Vec<u8>)> },
    /// The answer to `Transcribe` for one page.
    Transcribed { id: String, outcome: ReadOutcome },
    /// The answer to `Guess`: `Err` is a message for the toast, the page is unchanged.
    Guessed { id: String, result: Result<serde_json::Map<String, serde_json::Value>, String> },
    /// Several events from one command, applied in order.
    Many(Vec<Event>),
    /// The math font bytes (web).
    MathFont(Vec<u8>),
    /// The fallback font bytes (web).
    FallbackFont(Vec<u8>),
}

/// What reading a page gave.
#[derive(Debug)]
pub enum ReadOutcome {
    /// A transcript. `guesses` are present when the host also guessed the unreadable words (demo mode).
    Done { raw: String, tokens: u32, ms: u32, guesses: Option<serde_json::Map<String, serde_json::Value>> },
    /// The server could not be reached: the page keeps what it had (`queue.js` "unreachable").
    Unreachable(String),
    /// The page could not be read: its status becomes "error" with this message.
    Failed(String),
}

/// Thread-safe mailbox the host pushes into. Pushing wakes egui.
#[derive(Clone)]
pub struct Inbox {
    q: Arc<Mutex<Vec<Event>>>,
    ctx: egui::Context,
}

impl Inbox {
    pub fn new(ctx: egui::Context) -> Self {
        Inbox { q: Arc::default(), ctx }
    }

    pub fn push(&self, e: Event) {
        if let Ok(mut q) = self.q.lock() {
            q.push(e);
        }
        self.ctx.request_repaint();
    }

    pub(crate) fn drain(&self) -> Vec<Event> {
        self.q.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }
}

/// How the UI hands work to the host.
pub type Dispatch = Box<dyn Fn(Cmd)>;
