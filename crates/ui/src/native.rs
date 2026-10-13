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
        Role::Image => AkRole::Image,
        Role::Status => AkRole::Status,
        Role::Alert => AkRole::Alert,
    }
}

fn id_of(key: &str, widgets: &HashMap<String, Id>) -> Id {
    widgets.get(key).copied().unwrap_or_else(|| Id::new(("twin", key)))
}

fn write(ctx: &egui::Context, t: &Tree, widgets: &HashMap<String, Id>, used: &mut HashSet<NodeId>) -> NodeId {
    let kids: Vec<NodeId> = t.children.iter().map(|c| write(ctx, c, widgets, used)).collect();
    let id = id_of(&t.key, widgets);
    ctx.accesskit_node_builder(id, |b: &mut NodeBuilder| {
        b.set_role(role(t.role));
        if !t.name.is_empty() {
            b.set_name(t.name.clone());
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
