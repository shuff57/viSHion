//! The hidden-DOM twin as data (plan rule 3). Every frame the screen describes itself as an ordered list of
//! `Node`s: headings, a live status, paragraphs of transcript text with the flagged words inline as
//! controls, and plain controls. The web layer reconciles that list into `#controls`; native uses it for
//! tests (`App::controls`) and as the source for a future UI Automation tree.
//!
//! Order is region first (header, page, transcript, tray), then on-screen position (16px row bands, then
//! x), so Tab and reading order follow what a sighted user sees.

/// Screen regions in reading order.
pub const HEADER: u8 = 0;
pub const SCAN: u8 = 1;
pub const TEXT: u8 = 2;
pub const TRAY: u8 = 3;
/// A modal dialog: while one is open it is the only thing in the twin besides the screen's h1.
pub const DIALOG: u8 = 4;

#[derive(Clone, PartialEq, Debug)]
pub struct Reg {
    pub id: String,
    /// "button", "textbox", "checkbox" or "switch" (state in `pressed`, exposed as aria-checked) or
    /// "listitem" (a button inside a list item of its group, see `a11y::group_of`).
    pub role: &'static str,
    pub label: String,
    pub pressed: Option<bool>,
    /// Current value of a textbox.
    pub value: Option<String>,
    pub disabled: bool,
    pub region: u8,
    pub pos: (i32, i32),
    /// Top-left corner in points, rounded (tests and layout dumps; `pos` is the coarse sort key).
    pub at: (i32, i32),
    /// Hit-target size in points. Everything interactive must be at least 44 x 44.
    pub size: (u16, u16),
    /// What a transcript word shows when it is drawn as DOM text (the label is its spoken name).
    pub shown: Option<String>,
}

/// How a part of a transcript line looks when the web draws the line as DOM text (`Node::Para::flags`).
pub mod look {
    pub const BOLD: u16 = 1;
    pub const ITALIC: u16 = 2;
    pub const CODE: u16 = 4;
    /// Display math (a block of its own).
    pub const DISPLAY: u16 = 8;
    /// The "Margin note" lead-in of a margin line.
    pub const LEAD: u16 = 16;
    /// A flagged word: spelling to check / could not be read / already checked, and the one open in the tray.
    pub const WARN: u16 = 32;
    pub const BAD: u16 = 64;
    pub const CHECKED: u16 = 128;
    pub const SEL: u16 = 256;
    /// The transcript as plain text (`white-space: pre-wrap`, monospace).
    pub const PLAIN: u16 = 512;
}

#[derive(Clone, PartialEq, Debug)]
pub enum Part {
    Text(String),
    Math(String),
    /// Struck-through text (`~~probably~~`): a deletion, so it is not read as if it were still there.
    Del(String),
    Control(Reg),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Node {
    /// `at`/`size`: the rectangle in points where the text is drawn (touch exploration, screen magnifiers and
    /// the highlight of a screen reader follow it); `size` 0 x 0 when unknown.
    Heading { id: String, level: u8, text: String, region: u8, pos: (i32, i32), at: (i32, i32), size: (u16, u16) },
    /// A short status line (counts, save state). Only a `-error` one is a live region (`role=alert`); the
    /// rest is spoken once through `announce`, so one action is one utterance.
    Status { id: String, text: String, region: u8, pos: (i32, i32), at: (i32, i32), size: (u16, u16) },
    Text { id: String, text: String, region: u8, pos: (i32, i32), at: (i32, i32), size: (u16, u16) },
    /// A transcript line. `keys`: arrow keys inside it move the reading underline; `reading`: it is the
    /// line being read (web: `data-reading`, so the underline is observable without pixels).
    ///
    /// `flags` (web, the transcript drawn as DOM text): one `look` word per part, empty when the line is
    /// drawn by egui. `margin`: a `Margin:` line. `vis`: the part of the screen the line may be seen in (the
    /// scroll area it is in, less what covers it); `size` is then the height the line took, which the page
    /// measures from the DOM, not a rectangle egui drew.
    Para { id: String, parts: Vec<Part>, region: u8, pos: (i32, i32), keys: bool, reading: bool, at: (i32, i32), size: (u16, u16), flags: Vec<u16>, margin: bool, vis: (i32, i32, u16, u16) },
    /// A focusable region that takes keys of its own (`keys` names the set: stage, transcript, strip).
    /// `at`/`size`: its rectangle in points; the web twin is laid over the canvas there.
    Group { id: String, label: String, keys: &'static str, region: u8, pos: (i32, i32), at: (i32, i32), size: (u16, u16) },
    Control(Reg),
    /// A picture with a text alternative (`role=img`), laid over the canvas where it is drawn.
    Image { id: String, label: String, region: u8, pos: (i32, i32), at: (i32, i32), size: (u16, u16) },
}

impl Node {
    pub fn key(&self) -> (u8, i32, i32) {
        let (r, p) = match self {
            Node::Heading { region, pos, .. }
            | Node::Status { region, pos, .. }
            | Node::Text { region, pos, .. }
            | Node::Para { region, pos, .. }
            | Node::Group { region, pos, .. }
            | Node::Image { region, pos, .. } => (*region, *pos),
            Node::Control(c) => (c.region, c.pos),
        };
        (r, p.0, p.1)
    }

    pub fn id(&self) -> &str {
        match self {
            Node::Heading { id, .. } | Node::Status { id, .. } | Node::Text { id, .. } | Node::Para { id, .. } | Node::Group { id, .. } | Node::Image { id, .. } => id,
            Node::Control(c) => &c.id,
        }
    }
}

/// Every control in the list, including those inline in paragraphs, in reading order.
pub fn controls(nodes: &[Node]) -> Vec<&Reg> {
    let mut out = vec![];
    for n in nodes {
        match n {
            Node::Control(c) => out.push(c),
            Node::Para { parts, .. } => {
                for p in parts {
                    if let Part::Control(c) = p {
                        out.push(c);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Top-left corner and size of a rectangle in points, rounded: what the twin lays over the canvas.
pub fn geo(r: egui::Rect) -> ((i32, i32), (u16, u16)) {
    let w = r.width().round().clamp(0.0, u16::MAX as f32) as u16;
    let h = r.height().round().clamp(0.0, u16::MAX as f32) as u16;
    ((r.min.x.round() as i32, r.min.y.round() as i32), (w, h))
}

/// `pos` of a rect min (row band of 16px, then x).
pub fn pos_of(min: egui::Pos2) -> (i32, i32) {
    (min.y as i32 / 16, min.x as i32)
}

/// What Tab visits, in reading order: every enabled control (those inline in a paragraph included) and the
/// focusable key regions. The web gets this from the DOM; the native window has to be told, because egui
/// would walk widgets in the order they were drawn (a panel docked at the bottom is drawn before the page
/// that surrounds it) and stop on its own scroll areas and splitters, which have no name.
pub fn tab_stops(nodes: &[Node]) -> Vec<&str> {
    let mut out = vec![];
    for n in nodes {
        match n {
            Node::Control(c) if !c.disabled => out.push(c.id.as_str()),
            Node::Group { id, .. } => out.push(id.as_str()),
            Node::Para { parts, .. } => {
                for p in parts {
                    if let Part::Control(c) = p {
                        if !c.disabled {
                            out.push(c.id.as_str());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The stop after (or, `back`, before) `current`, wrapping round at the ends. With no current stop, or one
/// the screen no longer has, forward goes to the first and back to the last.
pub fn next_stop(nodes: &[Node], current: Option<&str>, back: bool) -> Option<String> {
    let stops = tab_stops(nodes);
    if stops.is_empty() {
        return None;
    }
    let at = current.and_then(|c| stops.iter().position(|s| *s == c));
    let i = match (at, back) {
        (Some(i), false) => (i + 1) % stops.len(),
        (Some(i), true) => (i + stops.len() - 1) % stops.len(),
        (None, false) => 0,
        (None, true) => stops.len() - 1,
    };
    Some(stops[i].to_string())
}
