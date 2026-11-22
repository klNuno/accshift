//! What the unscaled overlay pass (`AppModel::overlay_ctx`: onboarding,
//! AFK cover, streamer mode, PIN lock) adds to the stage's platform output.
//! The window has one IME and one AccessKit tree; both contexts feed them.
//!
//! The overlay's AccessKit nodes join the stage's tree under the stage's
//! root. Both contexts hash widget ids the same way, so the overlay's ids
//! are moved out of the stage's id space ([`remap`]), and the screen
//! reader's actions on them are routed back to the overlay
//! ([`OverlayA11y::route`]).

use egui::accesskit::{
    ActionData, ActionRequest, Affine, Node, NodeId, Role, TextPosition, TreeInfo, TreeUpdate,
};
use egui::output::IMEOutput;
use egui::Event;
use std::collections::HashSet;

const SALT: u64 = 0x9e37_79b9_7f4a_7c15;

/// An overlay node id in the merged tree, and back: its own inverse.
pub fn remap(id: NodeId) -> NodeId {
    NodeId(id.0 ^ SALT)
}

fn remap_position(p: &mut TextPosition) {
    p.node = remap(p.node);
}

fn remap_node(node: &mut Node) {
    macro_rules! vecs {
        ($($get:ident $set:ident),*) => {$(
            if !node.$get().is_empty() {
                let ids: Vec<NodeId> = node.$get().iter().copied().map(remap).collect();
                node.$set(ids);
            }
        )*};
    }
    macro_rules! ones {
        ($($get:ident $set:ident),*) => {$(
            if let Some(id) = node.$get() {
                node.$set(remap(id));
            }
        )*};
    }
    vecs!(
        children set_children,
        controls set_controls,
        details set_details,
        described_by set_described_by,
        flow_to set_flow_to,
        labelled_by set_labelled_by,
        owns set_owns,
        radio_group set_radio_group
    );
    ones!(
        active_descendant set_active_descendant,
        error_message set_error_message,
        in_page_link_target set_in_page_link_target,
        member_of set_member_of,
        next_on_line set_next_on_line,
        previous_on_line set_previous_on_line,
        popup_for set_popup_for
    );
    if let Some(sel) = node.text_selection().copied() {
        let mut sel = sel;
        remap_position(&mut sel.anchor);
        remap_position(&mut sel.focus);
        node.set_text_selection(sel);
    }
}

/// A screen-reader action on a merged-tree node, in the overlay's ids.
pub fn to_overlay(request: &ActionRequest) -> ActionRequest {
    let mut request = request.clone();
    request.target_node = remap(request.target_node);
    if let Some(ActionData::SetTextSelection(sel)) = &mut request.data {
        remap_position(&mut sel.anchor);
        remap_position(&mut sel.focus);
    }
    request
}

/// The overlay's side of the merged AccessKit tree.
#[derive(Default)]
pub struct OverlayA11y {
    /// The merged-tree ids of the overlay's last tree.
    nodes: HashSet<NodeId>,
    /// Actions on overlay nodes, in the overlay's ids, for its next pass.
    requests: Vec<ActionRequest>,
}

impl OverlayA11y {
    /// Before the stage's pass: moves the actions aimed at the overlay's
    /// nodes out of the stage's `events`.
    pub fn route(&mut self, events: &mut Vec<Event>) {
        events.retain(|e| match e {
            Event::AccessKitActionRequest(r) if self.nodes.contains(&r.target_node) => {
                self.requests.push(to_overlay(r));
                false
            }
            _ => true,
        });
    }

    /// The overlay pass's input: the stage's `events` without the stage's
    /// own AccessKit actions, then the routed ones.
    pub fn overlay_events(&mut self, events: Vec<Event>) -> Vec<Event> {
        events
            .into_iter()
            .filter(|e| !matches!(e, Event::AccessKitActionRequest(_)))
            .chain(self.requests.drain(..).map(Event::AccessKitActionRequest))
            .collect()
    }

    /// The overlay pass's tree, ready to graft: ids remapped, its root a
    /// plain container whose transform takes the stage root's scale to the
    /// overlay's (`scale` = overlay ppp / stage ppp). Remembers its nodes.
    pub fn subtree(&mut self, update: Option<TreeUpdate>, scale: f32) -> Option<TreeUpdate> {
        self.nodes.clear();
        let mut update = update?;
        let root = update.tree.as_ref()?.root;
        for (id, node) in &mut update.nodes {
            if *id == root {
                node.set_role(Role::GenericContainer);
                node.set_transform(Affine::scale(scale.into()));
            }
            *id = remap(*id);
            remap_node(node);
            self.nodes.insert(*id);
        }
        update.focus = remap(update.focus);
        update.tree = Some(TreeInfo::new(remap(root)));
        Some(update)
    }

    /// No overlay tree this frame.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.requests.clear();
    }
}

/// Grafts the overlay's tree ([`OverlayA11y::subtree`]) under the stage's
/// root, as its last child. The overlay's focus wins when one of its nodes
/// has it, or when the overlay captures input (`captures`).
pub fn graft(main: &mut Option<TreeUpdate>, overlay: Option<TreeUpdate>, captures: bool) {
    let (Some(main), Some(overlay)) = (main.as_mut(), overlay) else {
        return;
    };
    let (Some(main_root), Some(overlay_root)) = (
        main.tree.as_ref().map(|t| t.root),
        overlay.tree.as_ref().map(|t| t.root),
    ) else {
        return;
    };
    let Some((_, root)) = main.nodes.iter_mut().find(|(id, _)| *id == main_root) else {
        return;
    };
    root.push_child(overlay_root);
    if captures || overlay.focus != overlay_root {
        main.focus = overlay.focus;
    }
    main.nodes.extend(overlay.nodes);
}

/// The window's IME: the stage's (already in stage points), unless the
/// overlay has one, or takes the input (`captures`): then the overlay's
/// wins even when it has none, so a stage field that kept focus under it
/// does not keep the IME on. `ratio` takes the overlay's points to the
/// stage's.
pub fn merge_ime(
    main: Option<IMEOutput>,
    overlay: Option<IMEOutput>,
    ratio: f32,
    captures: bool,
) -> Option<IMEOutput> {
    match overlay {
        Some(mut ime) => {
            ime.rect = ime.rect * ratio;
            ime.cursor_rect = ime.cursor_rect * ratio;
            Some(ime)
        }
        None if captures => None,
        None => main,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, TextSelection, TreeId};
    use egui::{pos2, Rect};

    fn ime(x: f32) -> IMEOutput {
        IMEOutput {
            purpose: egui::IMEPurpose::Normal,
            rect: Rect::from_min_size(pos2(x, x), egui::vec2(10.0, 10.0)),
            cursor_rect: Rect::from_min_size(pos2(x, x), egui::vec2(1.0, 10.0)),
            should_interrupt_composition: false,
        }
    }

    #[test]
    fn a_capturing_overlay_without_ime_turns_the_stage_ime_off() {
        assert_eq!(merge_ime(Some(ime(4.0)), None, 1.0, true), None);
        assert_eq!(merge_ime(Some(ime(4.0)), None, 1.0, false), Some(ime(4.0)));
        let merged = merge_ime(Some(ime(4.0)), Some(ime(10.0)), 0.5, true).unwrap();
        assert_eq!(merged.rect.min, pos2(5.0, 5.0));
        assert_eq!(merged.rect.width(), 5.0);
    }

    fn tree(root: u64, nodes: Vec<(u64, Node)>, focus: u64) -> TreeUpdate {
        TreeUpdate {
            nodes: nodes.into_iter().map(|(id, n)| (NodeId(id), n)).collect(),
            tree: Some(TreeInfo::new(NodeId(root))),
            tree_id: TreeId::ROOT,
            focus: NodeId(focus),
        }
    }

    fn with_children(role: Role, children: &[u64]) -> Node {
        let mut node = Node::new(role);
        node.set_children(children.iter().copied().map(NodeId).collect::<Vec<_>>());
        node
    }

    #[test]
    fn the_overlay_tree_joins_the_stage_tree_under_its_root() {
        // The same ids in both trees: the stage's 7 is not the overlay's 7.
        let mut main = Some(tree(
            1,
            vec![
                (1, with_children(Role::Window, &[7])),
                (7, Node::new(Role::Button)),
            ],
            7,
        ));
        let mut field = Node::new(Role::PasswordInput);
        field.set_labelled_by(vec![NodeId(8)]);
        let overlay = tree(
            1,
            vec![
                (1, with_children(Role::Window, &[7, 8])),
                (7, field),
                (8, Node::new(Role::Label)),
            ],
            7,
        );
        let mut a11y = OverlayA11y::default();
        let sub = a11y.subtree(Some(overlay), 0.5);
        graft(&mut main, sub, false);
        let main = main.unwrap();
        let get = |id: NodeId| {
            main.nodes
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| n)
                .unwrap()
        };
        let ids: HashSet<NodeId> = main.nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), 5, "no id collides: {ids:?}");
        assert_eq!(get(NodeId(1)).children(), &[NodeId(7), remap(NodeId(1))]);
        let root = get(remap(NodeId(1)));
        assert_eq!(root.role(), Role::GenericContainer);
        assert_eq!(root.transform(), Some(&Affine::scale(0.5)));
        assert_eq!(root.children(), &[remap(NodeId(7)), remap(NodeId(8))]);
        assert_eq!(get(remap(NodeId(7))).role(), Role::PasswordInput);
        assert_eq!(get(remap(NodeId(7))).labelled_by(), &[remap(NodeId(8))]);
        assert_eq!(
            main.focus,
            remap(NodeId(7)),
            "the overlay's field has focus"
        );
    }

    #[test]
    fn the_stage_keeps_focus_unless_the_overlay_has_it_or_captures() {
        let main = || Some(tree(1, vec![(1, Node::new(Role::Window))], 1));
        let overlay = || tree(1, vec![(1, Node::new(Role::Window))], 1);
        let mut a11y = OverlayA11y::default();
        let mut m = main();
        graft(&mut m, a11y.subtree(Some(overlay()), 1.0), false);
        assert_eq!(m.unwrap().focus, NodeId(1));
        let mut m = main();
        graft(&mut m, a11y.subtree(Some(overlay()), 1.0), true);
        assert_eq!(m.unwrap().focus, remap(NodeId(1)));
    }

    #[test]
    fn actions_on_overlay_nodes_go_back_to_the_overlay() {
        let mut a11y = OverlayA11y::default();
        a11y.subtree(
            Some(tree(
                1,
                vec![
                    (1, with_children(Role::Window, &[7])),
                    (7, Node::new(Role::TextInput)),
                ],
                1,
            )),
            1.0,
        );
        let request = |node: NodeId, data| ActionRequest {
            action: Action::SetTextSelection,
            target_tree: TreeId::ROOT,
            target_node: node,
            data,
        };
        let position = |node| TextPosition {
            node,
            character_index: 2,
        };
        let selection = |node| {
            Some(ActionData::SetTextSelection(TextSelection {
                anchor: position(node),
                focus: position(node),
            }))
        };
        let mut events = vec![
            Event::AccessKitActionRequest(request(NodeId(7), None)),
            Event::AccessKitActionRequest(request(remap(NodeId(7)), selection(remap(NodeId(9))))),
        ];
        a11y.route(&mut events);
        assert_eq!(
            events,
            vec![Event::AccessKitActionRequest(request(NodeId(7), None))],
            "the stage keeps its own"
        );
        let overlay = a11y.overlay_events(events);
        assert_eq!(
            overlay,
            vec![Event::AccessKitActionRequest(request(
                NodeId(7),
                selection(NodeId(9))
            ))],
            "the overlay gets its own, in its ids, and not the stage's"
        );
        assert!(a11y.overlay_events(Vec::new()).is_empty(), "taken once");
    }
}
