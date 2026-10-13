//! The accessibility tree of a frame as plain data: the same structure the web twin builds as ARIA (banner,
//! `main` with the page image and the transcript, the word panel, a dialog; related controls in groups and
//! lists), for platform trees that are not a DOM (UI Automation, AT-SPI, VoiceOver through AccessKit) and
//! for tests that need no browser. `native` writes it into AccessKit.
use crate::a11y::{group_of, in_main, landmark, tex_speech};
use crate::twin::{Node, Part, Reg};

/// What a node of the tree is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Banner,
    Main,
    Region,
    Complementary,
    Dialog,
    Group,
    List,
    ListItem,
    Heading,
    Paragraph,
    Text,
    Math,
    Deleted,
    Button,
    ToggleButton,
    CheckBox,
    Switch,
    TextInput,
    Image,
    Status,
    Alert,
}

/// One node. `key` is the twin id (stable across frames); containers invented for structure have a key of
/// their own (`region-1`, `grp-zoom`, `li:page-p1`).
#[derive(Clone, PartialEq, Debug)]
pub struct Tree {
    pub key: String,
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    pub level: Option<u8>,
    pub checked: Option<bool>,
    pub disabled: bool,
    /// Why it is unavailable, or what a group is for (`a11y::disabled_reason`).
    pub description: Option<String>,
    /// Rectangle in points (x0, y0, x1, y1), where known; a container's is the union of its children's.
    pub at: Option<(i32, i32, i32, i32)>,
    pub children: Vec<Tree>,
}

impl Tree {
    fn new(key: impl Into<String>, role: Role, name: impl Into<String>) -> Tree {
        Tree { key: key.into(), role, name: name.into(), value: None, level: None, checked: None, disabled: false, description: None, at: None, children: vec![] }
    }

    /// First node (depth first) with this key.
    pub fn find(&self, key: &str) -> Option<&Tree> {
        if self.key == key {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(key))
    }

    /// Every node of the subtree, parents before children.
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Tree>) {
        out.push(self);
        for c in &self.children {
            c.walk(out);
        }
    }
}

type Rect = (i32, i32, i32, i32);

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
        (a, None) => a,
        (None, b) => b,
    }
}

fn rect_of(at: (i32, i32), size: (u16, u16)) -> Option<Rect> {
    (size.0 > 0 && size.1 > 0).then(|| (at.0, at.1, at.0 + size.0 as i32, at.1 + size.1 as i32))
}

fn control_node(r: &Reg) -> Tree {
    let (role, checked) = match (r.role, r.pressed) {
        ("textbox", _) => (Role::TextInput, None),
        ("checkbox", p) => (Role::CheckBox, p),
        ("switch", p) => (Role::Switch, p),
        (_, Some(p)) => (Role::ToggleButton, Some(p)),
        _ => (Role::Button, None),
    };
    let mut t = Tree::new(&r.id, role, &r.label);
    t.checked = checked;
    t.disabled = r.disabled;
    if r.disabled {
        t.description = crate::a11y::disabled_reason(&r.id).map(String::from);
    }
    if r.role == "textbox" {
        t.value = Some(r.value.clone().unwrap_or_default());
    }
    t.at = rect_of(r.at, r.size);
    t
}

fn node_of(n: &Node) -> Tree {
    match n {
        Node::Heading { id, level, text, at, size, .. } => {
            let mut t = Tree::new(id, Role::Heading, text);
            t.level = Some(*level);
            t.at = rect_of(*at, *size);
            t
        }
        Node::Status { id, text, at, size, .. } => {
            let mut t = Tree::new(id, if id.ends_with("-error") { Role::Alert } else { Role::Text }, text);
            t.at = rect_of(*at, *size);
            t
        }
        Node::Text { id, text, at, size, .. } => {
            let mut t = Tree::new(id, Role::Text, text);
            t.at = rect_of(*at, *size);
            t
        }
        Node::Group { id, label, at, size, .. } => {
            let mut t = Tree::new(id, Role::Group, label);
            t.at = rect_of(*at, *size);
            t
        }
        Node::Image { id, label, at, size, .. } => {
            let mut t = Tree::new(id, Role::Image, label);
            t.at = rect_of(*at, *size);
            t
        }
        Node::Control(r) => control_node(r),
        Node::Para { id, parts, at, size, .. } => {
            let mut t = Tree::new(id, Role::Paragraph, "");
            t.at = rect_of(*at, *size);
            for (i, p) in parts.iter().enumerate() {
                t.children.push(match p {
                    Part::Text(s) => Tree::new(format!("{id}#t{i}"), Role::Text, s),
                    Part::Math(tex) => {
                        let mut m = Tree::new(format!("{id}#m{i}"), Role::Math, tex_speech(tex));
                        m.value = Some(tex.clone());
                        m
                    }
                    Part::Del(s) => Tree::new(format!("{id}#d{i}"), Role::Deleted, s),
                    Part::Control(r) => control_node(r),
                });
            }
            // the runs of text and the formulas are somewhere in their line: the line's rectangle is closer
            // than none (a platform tree reads an empty rectangle as "not on screen")
            for c in t.children.iter_mut() {
                if c.at.is_none() {
                    c.at = t.at;
                }
            }
            t
        }
    }
}

/// Containers take the rectangle of what they hold.
fn bounds(mut t: Tree) -> Tree {
    t.children = std::mem::take(&mut t.children).into_iter().map(bounds).collect();
    for c in &t.children {
        t.at = union(t.at, c.at);
    }
    t
}

/// The tree of a frame's twin nodes, which must be in reading order (`App::step` sorts them).
pub fn of(nodes: &[Node]) -> Vec<Tree> {
    let mut regions: Vec<(u8, Vec<&Node>)> = vec![];
    for n in nodes {
        // the toast is spoken through the live region, not read as a part of the page
        if matches!(n, Node::Status { id, .. } if id == "toast") {
            continue;
        }
        let r = n.key().0;
        match regions.last_mut() {
            Some((k, v)) if *k == r => v.push(n),
            _ => regions.push((r, vec![n])),
        }
    }
    let mut top: Vec<Tree> = vec![];
    let mut main: Vec<Tree> = vec![];
    let main_node = |kids: Vec<Tree>| {
        let mut m = Tree::new("main", Role::Main, "");
        m.children = kids;
        m
    };
    for (region, list) in &regions {
        let mut kids: Vec<Tree> = vec![];
        let mut open: Option<(&'static str, Tree)> = None;
        for n in list {
            let grp = match n {
                Node::Control(r) => group_of(&r.id),
                _ => None,
            };
            let Some((key, role, label)) = grp else {
                if let Some((_, g)) = open.take() {
                    kids.push(g);
                }
                kids.push(node_of(n));
                continue;
            };
            if open.as_ref().map(|(k, _)| *k) != Some(key) {
                if let Some((_, g)) = open.take() {
                    kids.push(g);
                }
                let r = if role == "list" { Role::List } else { Role::Group };
                let mut g = Tree::new(format!("grp-{key}"), r, label);
                g.description = crate::a11y::group_description(key, list).map(String::from);
                open = Some((key, g));
            }
            let item = node_of(n);
            if let Some((_, g)) = open.as_mut() {
                if role == "list" {
                    let mut li = Tree::new(format!("li:{}", item.key), Role::ListItem, "");
                    li.children.push(item);
                    g.children.push(li);
                } else {
                    g.children.push(item);
                }
            }
        }
        if let Some((_, g)) = open.take() {
            kids.push(g);
        }
        let mut s = match landmark(*region, list) {
            Some((role, label)) => {
                let r = match role {
                    "banner" => Role::Banner,
                    "complementary" => Role::Complementary,
                    "dialog" => Role::Dialog,
                    _ => Role::Region,
                };
                Tree::new(format!("region-{region}"), r, label)
            }
            None => Tree::new(format!("region-{region}"), Role::Group, ""),
        };
        s.children = kids;
        if in_main(*region) {
            main.push(s);
        } else {
            if !main.is_empty() {
                top.push(main_node(std::mem::take(&mut main)));
            }
            top.push(s);
        }
    }
    if !main.is_empty() {
        top.push(main_node(main));
    }
    top.into_iter().map(bounds).collect()
}
