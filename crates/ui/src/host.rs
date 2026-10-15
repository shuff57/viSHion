//! The seam between the screen and the platform (plan: "everything platform-specific stays in crates/ui and
//! crates/app"). The UI never touches storage. It sends `Cmd`s and receives `Event`s; `crates/app` owns the
//! store (native `FsStore` on a worker thread, web `IdbStore` on the local executor) and the project layer.
use std::sync::{Arc, Mutex};
use vishion_core::model::{Assignment, Page, Project, Settings};

/// How to reach the OCR server for a run (`S.d.settings` + `O.buildPrompt`; the API key lives in the UI
/// because `settings.json` is stored without secrets).
#[derive(Debug, Clone, Default)]
pub struct Conn {
    pub url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
    pub prompt: String,
    pub demo: bool,
    /// `settings.provider`: which provider answers (`net::Provider::for_setting`).
    pub provider: String,
    /// `settings.guessIllegible` (default on): ask the model for likely words after a page is read.
    pub guess: bool,
    /// The teacher's earlier resolutions for the guess prompt (`W.referenceExamples`).
    pub examples: Vec<String>,
}

/// A picture that is ready to be added: prepared (JPEG, at most 1600 px) and, perhaps, a name for its page.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedItem {
    pub file_name: String,
    pub bytes: Vec<u8>,
    /// The student's name (read from the page, or chosen) and where it came from: `read` | `chosen` | `file`.
    pub name: Option<String>,
    pub source: String,
}

/// The storage card of the home screen (`S.d.folder`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FolderInfo {
    pub mode: String,
    pub name: String,
    pub permission: String,
    /// The browser can pick folders.
    pub supported: bool,
}

/// One assignment on the home screen (`fs.listProjects`).
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub slug: String,
    pub name: String,
    pub pages: usize,
    pub done: usize,
    /// Last change, ms since the epoch.
    pub updated: f64,
}

/// UI to host.
#[derive(Debug)]
pub enum Cmd {
    /// The home screen's list: `Event::Projects`.
    ListProjects,
    /// Settings, Find models: the models the server lists that can read images (`working` narrows a cloud
    /// picker to the models a check found usable; `all` ignores it).
    FindModels { conn: Conn, working: Option<Vec<String>>, all: bool },
    /// The banner's Retry: is the transcription server answering? `Event::Pinged`.
    Ping { conn: Conn },
    /// Settings, Check which models work: one tiny image to every model that takes images, `Event::ModelsProgress`
    /// as they answer and `Event::ModelsChecked` at the end.
    CheckModels { conn: Conn },
    /// Settings, Test connection: ask the server (the address and key as typed) what it serves.
    TestConnection { conn: Conn },
    /// Web: `choose` a folder to save in, `browser` storage instead, or `reconnect` the remembered folder.
    Folder(String),
    /// Ask for pictures and get each one ready for staging (`Event::Staged` one by one, `Event::Skipped` for
    /// files that cannot be added). `preset` is a student chosen beforehand; otherwise, with `read_names`,
    /// the model reads the name at the top of each page.
    StagePictures { conn: Conn, read_names: bool, preset: Option<String> },
    /// Stage pictures that were dropped on the window (name, bytes), as `StagePictures` stages the picked ones.
    StageFiles { files: Vec<(String, Vec<u8>)>, conn: Conn, read_names: bool, preset: Option<String> },
    /// Web: open the camera and send `Event::CameraFrame`s until `CameraStop`.
    CameraStart,
    CameraStop,
    /// The shutter: keep this frame (RGBA, `w` x `h`) as a staged picture named `name`; answered by
    /// `Event::Staged` like a picked file.
    CaptureShot { name: String, w: u32, h: u32, rgba: Vec<u8>, conn: Conn, read_names: bool, preset: Option<String> },
    /// Save the staged pictures as new pages: `Event::PagesAdded`.
    CommitStaged { slug: String, items: Vec<StagedItem> },
    /// Ask for a project zip and import it as a new assignment: `Event::Opened`.
    ImportZip,
    /// Bring sessions of the old single-file app into a new assignment: `Event::Opened`.
    ImportLegacy,
    /// Ask for a project folder and import it as a new assignment: `Event::Opened`.
    ImportFolder,
    /// Write `exports/<slug>.zip` with every file of the assignment (the home card's Download zip).
    DownloadZip { slug: String },
    /// Open another assignment: `Event::Opened`.
    OpenProject { slug: String },
    /// `fs.createProject` then open it: `Event::Opened`.
    CreateProject { name: String },
    /// `fs.renameProject`, then the list again.
    RenameProject { slug: String, name: String },
    /// `fs.duplicateProject`, then the list again.
    DuplicateProject { slug: String },
    /// `fs.deleteProject`, then the list again.
    DeleteProject { slug: String },
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
    Transcribe { slug: String, ids: Vec<String>, conn: Conn },
    /// `Q.stop`: the page being read goes back to pending and the run ends. Handled where the command is
    /// sent (the worker may be busy reading), so it never needs an answer.
    Stop,
    /// Ask for likely words for the unreadable words of a page (`W.guess`): `Event::Guessed`.
    Guess { slug: String, id: String, raw: String, conn: Conn, examples: Vec<String> },
    /// Write `exports/<name>` (the strip's "Export N selected").
    Export { slug: String, name: String, bytes: Vec<u8> },
    /// Web: fetch the STIX Two Math font once (it is not part of the wasm, plan risk "wasm size").
    LoadMathFont,
    /// Web: fetch the colour emoji face (1.4 MB, not part of the wasm) when a page has an emoji. The native
    /// app links it in.
    LoadEmojiFont,
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
    /// Whether Home offers "Import old sessions".
    LegacyAvailable(bool),
    /// The browser allowed the remembered folder again (the banner's Reconnect): save everything once more.
    Reconnected,
    /// The answer to `Ping`.
    Pinged { ok: bool, text: String },
    /// How far Check which models work has got.
    ModelsProgress { done: usize, total: usize },
    /// The end of Check which models work: the ids that answered (what to remember as working), the picker
    /// narrowed to them (when any did) and the sentence for Settings.
    ModelsChecked { ok: Vec<String>, list: Vec<(String, String, Option<bool>)>, note: String },
    /// The answer to `FindModels`: the models, or the sentence that says why not.
    ModelsFound(Result<Vec<(String, String, Option<bool>)>, String>),
    /// The answer to `TestConnection`: a line for the Settings dialog, and whether it is good news.
    ConnectionTested { ok: bool, text: String },
    /// Where assignments are saved (web): `mode` is `none` | `folder` | `idb`, `permission` `granted` | `prompt`.
    Folder(FolderInfo),
    /// A frame of the camera (RGBA).
    CameraFrame { w: u32, h: u32, rgba: Vec<u8> },
    /// The camera could not be opened (the person said no, or there is none).
    CameraError(String),
    /// One picture is ready to stage. `names_down`: the name reader could not reach the server.
    Staged { item: StagedItem, names_down: bool },
    /// Files that cannot be added, with the reason each.
    Skipped(Vec<(String, String)>),
    /// The assignments the home screen lists.
    Projects(Vec<Card>),
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
    /// A page is being read: the text so far (`page:stream`), shown where the transcript will be.
    Reading { id: String, text: String },
    /// The answer to `Transcribe` for one page.
    Transcribed { id: String, outcome: ReadOutcome },
    /// The answer to `Straighten`: the angle that was removed (0 = the page was already level) and the new
    /// picture when it changed. The line boxes are redone from the picture now on screen.
    Straightened { id: String, angle: f64, bytes: Option<Vec<u8>> },
    /// The answer to `Guess`: `Err` is a message for the toast, the page is unchanged.
    Guessed { id: String, result: Result<serde_json::Map<String, serde_json::Value>, String> },
    /// Several events from one command, applied in order.
    Many(Vec<Event>),
    /// The math font bytes (web).
    MathFont(Vec<u8>),
    /// The fallback font bytes (web).
    FallbackFont(Vec<u8>),
    /// The colour emoji face (web).
    EmojiFont(Vec<u8>),
}

/// What reading a page gave.
#[derive(Debug)]
pub enum ReadOutcome {
    /// A transcript. `guesses` are present when the host also guessed the unreadable words (demo mode).
    Done { raw: String, tokens: u32, ms: u32, guesses: Option<serde_json::Map<String, serde_json::Value>> },
    /// The server could not be reached: the page keeps what it had (`queue.js` "unreachable").
    Unreachable(String),
    /// Stop was pressed: the page goes back to pending (`queue.js` abort).
    Stopped,
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
