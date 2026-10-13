//! The native accessibility tree (UI Automation on Windows, AT-SPI, VoiceOver): the twin of the web build.
//! egui hands AccessKit a node for every widget it laid out, but those are unnamed. Each frame the screen's
//! twin nodes (`tree::of`) are written into AccessKit as well: roles, names, values, headings, math and the
//! reading order. Controls keep the egui widget's own node id, so activating one from a screen reader
//! (`Action::Default`) is a click on that widget.
use crate::tree::{Role, Tree};
use accesskit::{Checked, Live, NodeBuilder, NodeId, Rect, Role as AkRole};
use egui::Id;
use std::collections::{HashMap, HashSet};

fn role(r: Role) -> AkRole {
    match r {
        Role::Banner => AkRole::Banner,
        Role::Main => AkRole::Main,
        Role::Region => AkRole::Region,
        Role::Complementary => AkRole::Complementary,
        Role::Dialog => AkRole::Dialog,
        Role::Group => AkRole::Group,
        Role::List => AkRole::List,
        Role::ListItem => AkRole::ListItem,
        Role::Heading => AkRole::Heading,
        Role::Paragraph => AkRole::Paragraph,
        Role::Text => AkRole::StaticText,
        Role::Math => AkRole::Math,
        Role::Deleted => AkRole::ContentDeletion,
        Role::Button => AkRole::Button,
        Role::ToggleButton => AkRole::ToggleButton,
        Role::CheckBox => AkRole::CheckBox,
        Role::Switch => AkRole::Switch,
        Role::TextInput => AkRole::TextInput,
        Role::PasswordInput => AkRole::PasswordInput,
        Role::Image => AkRole::Image,
        Role::Status => AkRole::Status,
        Role::Alert => AkRole::Alert,
    }
}

/// The name a node is given. UI Automation as AccessKit implements it (accesskit_windows 0.15) carries a
/// control type and a name but not the landmark type or the heading level, so there Narrator and NVDA read a
/// banner, a main and a heading as a bare group or text. The words are put in the name instead, as a web page
/// does with `aria-label`: "Maya R., heading level 1", "Word panel, complementary landmark". Other platforms
/// get the role and level natively and the name stays what it is.
pub fn platform_name(t: &Tree, uia: bool) -> String {
    if !uia {
        return t.name.clone();
    }
    let with = |name: &str, kind: String| if name.is_empty() { kind } else { format!("{name}, {kind}") };
    // no description property either: the reason a control is unavailable is said with its name
    if let (Some(d), false) = (&t.description, matches!(t.role, Role::Group | Role::List)) {
        return format!("{}, unavailable: {d}", t.name);
    }
    match t.role {
        Role::Heading => with(&t.name, format!("heading level {}", t.level.unwrap_or(2))),
        Role::Banner => with(&t.name, "banner landmark".into()),
        Role::Main => with(&t.name, "main landmark".into()),
        Role::Complementary => with(&t.name, "complementary landmark".into()),
        Role::Region => with(&t.name, "region landmark".into()),
        _ => t.name.clone(),
    }
}

/// A Windows contrast theme (Aquatic, Night sky, High Contrast Black / White): the window is drawn in the
/// system's own colours, as the web build does with `forced-colors` (read once at start, like it). Elsewhere,
/// and in tests, nothing is forced.
#[cfg(all(windows, not(test)))]
pub fn forced_tokens(base: &crate::theme::Tokens) -> Option<crate::theme::Tokens> {
    #[repr(C)]
    struct HighContrast {
        size: u32,
        flags: u32,
        scheme: *mut u16,
    }
    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, data: *mut HighContrast, winini: u32) -> i32;
        fn GetSysColor(index: i32) -> u32;
    }
    const SPI_GETHIGHCONTRAST: u32 = 0x0042;
    const HCF_HIGHCONTRASTON: u32 = 1;
    let mut hc = HighContrast { size: std::mem::size_of::<HighContrast>() as u32, flags: 0, scheme: std::ptr::null_mut() };
    // SAFETY: both calls only read system settings; `hc` is the struct the first one fills in.
    let on = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.size, &mut hc, 0) } != 0 && hc.flags & HCF_HIGHCONTRASTON != 0;
    if !on {
        return None;
    }
    // COLORREF is 0x00BBGGRR; COLOR_WINDOW 5, COLOR_WINDOWTEXT 8, COLOR_GRAYTEXT 17, COLOR_HOTLIGHT 26
    let sys = |i: i32| {
        let c = unsafe { GetSysColor(i) };
        egui::Color32::from_rgb(c as u8, (c >> 8) as u8, (c >> 16) as u8)
    };
    Some(crate::theme::forced(base, sys(5), sys(8), sys(17), sys(26)))
}

#[cfg(not(all(windows, not(test))))]
pub fn forced_tokens(_base: &crate::theme::Tokens) -> Option<crate::theme::Tokens> {
    None
}

fn id_of(key: &str, widgets: &HashMap<String, Id>) -> Id {
    widgets.get(key).copied().unwrap_or_else(|| Id::new(("twin", key)))
}

fn write(ctx: &egui::Context, t: &Tree, widgets: &HashMap<String, Id>, used: &mut HashSet<NodeId>) -> NodeId {
    let kids: Vec<NodeId> = t.children.iter().map(|c| write(ctx, c, widgets, used)).collect();
    let id = id_of(&t.key, widgets);
    ctx.accesskit_node_builder(id, |b: &mut NodeBuilder| {
        b.set_role(role(t.role));
        let name = platform_name(t, cfg!(windows) && !cfg!(test));
        if !name.is_empty() {
            b.set_name(name);
        }
        if let Some(d) = &t.description {
            b.set_description(d.clone());
        }
        if let Some(v) = &t.value {
            b.set_value(v.clone());
        }
        if let Some(l) = t.level {
            b.set_hierarchical_level(l as usize);
        }
        if let Some(c) = t.checked {
            b.set_checked(if c { Checked::True } else { Checked::False });
        }
        if t.disabled {
            b.set_disabled();
        }
        match t.role {
            Role::Dialog => b.set_modal(),
            Role::Status => b.set_live(Live::Polite),
            Role::Alert => b.set_live(Live::Assertive),
            _ => {}
        }
        if let Some((x0, y0, x1, y1)) = t.at {
            b.set_bounds(Rect { x0: x0 as f64, y0: y0 as f64, x1: x1 as f64, y1: y1 as f64 });
        }
        // a widget's node may already have children of egui's own (the text runs of a text box): they stay
        let mut all: Vec<NodeId> = b.children().to_vec();
        for k in kids {
            if !all.contains(&k) {
                all.push(k);
            }
        }
        b.set_children(all);
    });
    used.insert(NodeId(id.value()));
    NodeId(id.value())
}

/// Write this frame's tree. `widgets` maps a twin id to the egui widget that draws it; `said` is the last
/// announcement (a polite live region, so a screen reader speaks it when it changes) and `n` counts them, so
/// the same sentence twice in a row is still a change. Does nothing while no screen reader is attached.
pub fn publish(ctx: &egui::Context, tree: &[Tree], widgets: &HashMap<String, Id>, said: &str, n: u32, title: &str) {
    let root = egui::accesskit_root_id();
    if ctx.accesskit_node_builder(root, |_| ()).is_none() {
        return;
    }
    // Every node egui made this frame hangs off the root (an unnamed leaf for each widget it laid out). They
    // all have to stay reachable, or AccessKit refuses the update; those the twin does not use go under a
    // hidden container, which platform trees leave out.
    let made: Vec<NodeId> = ctx.accesskit_node_builder(root, |b| b.children().to_vec()).unwrap_or_default();
    let mut used = HashSet::new();
    let mut top: Vec<NodeId> = tree.iter().map(|t| write(ctx, t, widgets, &mut used)).collect();
    // egui keeps the focus it was given for one frame even when nothing it drew has that id (a choice of the
    // word the panel just left): its update then names a focus node the tree does not contain, and
    // accesskit_consumer asserts in the window thread. Focus nobody draws is let go of.
    if let Some(f) = ctx.memory(|m| m.focused()) {
        let n = NodeId(f.value());
        let known = made.contains(&n) || used.contains(&n) || widgets.values().any(|w| NodeId(w.value()) == n);
        if !known {
            ctx.memory_mut(|m| m.surrender_focus(f));
        }
    }
    let left: Vec<NodeId> = made.into_iter().filter(|id| !used.contains(id)).collect();
    if !left.is_empty() {
        let spare = Id::new(("twin", "spare"));
        ctx.accesskit_node_builder(spare, |b| {
            b.set_role(AkRole::GenericContainer);
            b.set_hidden();
            b.set_children(left);
        });
        top.push(NodeId(spare.value()));
    }
    let live = Id::new(("twin", "announcement"));
    let text = if n % 2 == 1 { said.to_string() } else { format!("{said}\u{a0}") };
    ctx.accesskit_node_builder(live, |b| {
        b.set_role(AkRole::Status);
        b.set_live(Live::Polite);
        b.set_name(text);
    });
    top.push(NodeId(live.value()));
    ctx.accesskit_node_builder(root, |b| {
        b.set_name(title.to_string());
        b.set_children(top);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twin::Node;

    /// Narrator and NVDA get the landmark and the heading level in the name on Windows, and nothing is changed
    /// elsewhere or for ordinary controls.
    #[test]
    fn uia_names_carry_landmarks_and_heading_levels() {
        let nodes = vec![Node::Heading { id: "student-name".into(), level: 1, text: "Maya R.".into(), region: 2, pos: (0, 0), at: (0, 0), size: (0, 0) }];
        let t = crate::tree::of(&nodes);
        let h = t[0].find("student-name").unwrap();
        assert_eq!(platform_name(h, true), "Maya R., heading level 1");
        assert_eq!(platform_name(h, false), "Maya R.");
        let banner = crate::tree::Tree::find(&t[0], "student-name").map(|_| ()).is_some();
        assert!(banner);
        for (role, want) in [(Role::Banner, "banner landmark"), (Role::Main, "main landmark")] {
            let mut tr = h.clone();
            tr.role = role;
            tr.name = String::new();
            assert_eq!(platform_name(&tr, true), want);
        }
        let mut tray = h.clone();
        tray.role = Role::Complementary;
        tray.name = "Word panel".into();
        assert_eq!(platform_name(&tray, true), "Word panel, complementary landmark");
        let mut b = h.clone();
        b.role = Role::Button;
        b.name = "Zoom in".into();
        assert_eq!(platform_name(&b, true), "Zoom in");
    }
}
