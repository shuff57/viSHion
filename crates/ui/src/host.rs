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
