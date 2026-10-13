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
}

#[derive(Clone, PartialEq, Debug)]
pub enum Part {
    Text(String),
    Math(String),
    Control(Reg),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Node {
    Heading { id: String, level: u8, text: String, region: u8, pos: (i32, i32) },
    /// `role=status`, announced politely when its text changes.
    Status { id: String, text: String, region: u8, pos: (i32, i32) },
    Text { id: String, text: String, region: u8, pos: (i32, i32) },
    /// A transcript line. `keys`: arrow keys inside it move the reading underline; `reading`: it is the
    /// line being read (web: `data-reading`, so the underline is observable without pixels).
    Para { id: String, parts: Vec<Part>, region: u8, pos: (i32, i32), keys: bool, reading: bool },
    /// A focusable region that takes keys of its own (`keys` names the set: stage, transcript, strip).
    /// `at`/`size`: its rectangle in points; the web twin is laid over the canvas there.
    Group { id: String, label: String, keys: &'static str, region: u8, pos: (i32, i32), at: (i32, i32), size: (u16, u16) },
    Control(Reg),
}

impl Node {
    pub fn key(&self) -> (u8, i32, i32) {
        let (r, p) = match self {
            Node::Heading { region, pos, .. }
            | Node::Status { region, pos, .. }
            | Node::Text { region, pos, .. }
            | Node::Para { region, pos, .. }
            | Node::Group { region, pos, .. } => (*region, *pos),
            Node::Control(c) => (c.region, c.pos),
        };
        (r, p.0, p.1)
    }

    pub fn id(&self) -> &str {
        match self {
            Node::Heading { id, .. } | Node::Status { id, .. } | Node::Text { id, .. } | Node::Para { id, .. } | Node::Group { id, .. } => id,
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

/// `pos` of a rect min (row band of 16px, then x).
pub fn pos_of(min: egui::Pos2) -> (i32, i32) {
    (min.y as i32 / 16, min.x as i32)
}
