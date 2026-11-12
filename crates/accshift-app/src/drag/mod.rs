//! Card drag and drop.
//!
//! The UI measures what the layout answers (the card under a press, the
//! slot rects at drag start, the card under the pointer while dragging) and
//! hands it over as values; the manager keeps the state machine of the
//! pointer and decides what a release commits.
//!
//! - `geometry`: the slot rects and the index a drop would take.
//! - `host`: the app state a drag reads.
//! - `target`: what the pointer is over, once per frame.
//! - `commit`: what a release changes in the folder store.

mod commit;
mod geometry;
mod host;
mod target;

pub use commit::{DropCommit, DropOutcome};
pub use geometry::{preview_index_at, SlotRect};
pub use host::{DragHost, StoreHost};
pub use target::PointHit;

use crate::folders::{ItemKind, ItemRef};

/// Manhattan distance in pixels before a press becomes a drag.
pub const DRAG_THRESHOLD: f32 = 5.0;

/// The card a press landed on.
#[derive(Debug, Clone, PartialEq)]
pub struct PressedCard {
    pub item: ItemRef,
    /// A section wrapper rather than a card inside it, sections mode only.
    pub section_card: bool,
    /// Sections mode: the folder of the section holding the card, `None`
    /// for the root section (the source folder of an account drag).
    pub section_folder: Option<String>,
}

/// The slots captured when a drag starts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slots {
    pub rects: Vec<SlotRect>,
    /// The cards stack as a list, or a folder moves among the sections.
    pub list_mode: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Pending {
    card: PressedCard,
    start: (f32, f32),
}

/// The drag state machine: a press, then a drag past [`DRAG_THRESHOLD`],
/// then a release or a cancel.
#[derive(Debug, Default)]
pub struct DragManager {
    drag_item: Option<ItemRef>,
    drag_over_folder_id: Option<String>,
    drag_over_back: bool,
    is_dragging: bool,
    preview_index: Option<usize>,
    pending: Option<Pending>,
    /// The click right after a drag release is swallowed.
    eat_next_click: bool,
    last: (f32, f32),
    has_pointer: bool,
    /// Escape cancelled a drag: its release and click are swallowed.
    drag_cancelled: bool,
    sections_mode: bool,
    /// A scroll made the slots stale: the next frame measures again.
    rects_stale: bool,
    slots: Slots,
    old_index: Option<usize>,
}

impl DragManager {
    pub fn drag_item(&self) -> Option<&ItemRef> {
        self.drag_item.as_ref()
    }
    pub fn drag_over_folder_id(&self) -> Option<&str> {
        self.drag_over_folder_id.as_deref()
    }
    pub fn drag_over_back(&self) -> bool {
        self.drag_over_back
    }
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }
    pub fn preview_index(&self) -> Option<usize> {
        self.preview_index
    }
    /// A card is pressed or moving.
    pub fn in_flight(&self) -> bool {
        self.pending.is_some() || self.is_dragging
    }
    /// The press that started the drag, in client px.
    pub fn press_start(&self) -> Option<(f32, f32)> {
        self.pending.as_ref().map(|p| p.start)
    }
    /// The pressed card.
    pub fn pressed(&self) -> Option<&PressedCard> {
        self.pending.as_ref().map(|p| &p.card)
    }
    /// The slots need measuring again (after a scroll).
    pub fn rects_stale(&self) -> bool {
        self.is_dragging && self.rects_stale
    }

    /// A primary press on the workspace. `card` is the card under it;
    /// `sections_mode` whether the grid shows sections.
    pub fn mouse_down(
        &mut self,
        primary: bool,
        card: Option<PressedCard>,
        sections_mode: bool,
        at: (f32, f32),
    ) {
        if !primary {
            return;
        }
        let Some(card) = card else {
            return;
        };
        // In sections mode only the section wrappers drag as folders;
        // subfolder cards inside sections stay click-to-navigate.
        if sections_mode && card.item.kind == ItemKind::Folder && !card.section_card {
            return;
        }
        self.last = at;
        self.has_pointer = true;
        self.drag_cancelled = false;
        self.sections_mode = sections_mode;
        self.pending = Some(Pending { card, start: at });
    }

    /// A pointer move. `buttons_down` is false when no button is held: the
    /// release was missed outside the window. Returns true when this move
    /// started the drag: the caller then measures the slots
    /// ([`Self::set_slots`]) and the ghost.
    pub fn mouse_move(&mut self, at: (f32, f32), buttons_down: bool) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        if !buttons_down {
            self.cancel();
            return false;
        }
        self.last = at;
        self.has_pointer = true;
        if self.is_dragging {
            return false;
        }
        let dx = at.0 - pending.start.0;
        let dy = at.1 - pending.start.1;
        if dx.abs() + dy.abs() < DRAG_THRESHOLD {
            return false;
        }
        self.is_dragging = true;
        self.drag_item = Some(pending.card.item.clone());
        true
    }

    /// Takes the slots of the dragged card's kind and finds its index among
    /// them. In sections mode an account has no slot: it only moves into a
    /// folder.
    pub fn set_slots(&mut self, slots: Slots, host: &dyn DragHost) {
        self.rects_stale = false;
        let Some(item) = &self.drag_item else {
            return;
        };
        if self.sections_mode && item.kind == ItemKind::Account {
            self.slots = Slots::default();
            self.old_index = None;
            return;
        }
        let items = match item.kind {
            ItemKind::Folder => host.folder_items(),
            ItemKind::Account => host.account_items(),
        };
        self.old_index = items.iter().position(|i| i.id == item.id);
        self.slots = slots;
    }

    /// The sections layout is what `set_slots` measures for a folder.
    pub fn sections_mode(&self) -> bool {
        self.sections_mode
    }

    /// A scroll during the drag: the slots are stale.
    pub fn scrolled(&mut self) {
        if self.is_dragging && self.has_pointer {
            self.rects_stale = true;
        }
    }

    /// The pointer, as the last move left it.
    pub fn pointer(&self) -> (f32, f32) {
        self.last
    }

    fn cancel(&mut self) {
        let eat = self.eat_next_click;
        let cancelled = self.drag_cancelled;
        *self = Self::default();
        self.eat_next_click = eat;
        self.drag_cancelled = cancelled;
    }

    /// Escape: false when no card is pressed or moving.
    pub fn cancel_from_escape(&mut self) -> bool {
        if !self.in_flight() {
            return false;
        }
        if self.is_dragging {
            self.drag_cancelled = true;
        }
        self.cancel();
        true
    }

    /// The button release: commits the drop when a drag was under way.
    pub fn mouse_up(&mut self, host: &dyn DragHost) -> DropOutcome {
        if self.drag_cancelled {
            // The release after an Escape: swallow it and its click.
            self.drag_cancelled = false;
            self.eat_next_click = true;
            return DropOutcome::default();
        }
        let Some(pending) = self.pending.clone() else {
            return DropOutcome::default();
        };
        if !self.is_dragging {
            self.cancel();
            return DropOutcome::default();
        }
        let out = match self.drag_item.clone() {
            Some(item) => DropOutcome {
                commit: self.drop_commit(item, &pending.card, host),
                refresh: true,
            },
            None => DropOutcome::default(),
        };
        self.cancel();
        self.eat_next_click = true;
        out
    }

    /// Whether the click of this release is eaten. The guard only lasts the
    /// release's own frame: call [`Self::end_frame`] after the frame's
    /// clicks.
    pub fn eats_click(&self) -> bool {
        self.eat_next_click
    }

    /// Clears the click guard once the release's frame is over.
    pub fn end_frame(&mut self) {
        self.eat_next_click = false;
    }
}

#[cfg(test)]
mod tests {
    use super::geometry::tests::row;
    use super::*;

    struct Host {
        current: Option<String>,
        folders: Vec<ItemRef>,
        accounts: Vec<ItemRef>,
        /// (folder, parent).
        tree: Vec<(String, Option<String>)>,
    }

    impl DragHost for Host {
        fn current_folder_id(&self) -> Option<String> {
            self.current.clone()
        }
        fn folder_items(&self) -> Vec<ItemRef> {
            self.folders.clone()
        }
        fn account_items(&self) -> Vec<ItemRef> {
            self.accounts.clone()
        }
        fn is_folder_descendant(&self, folder_id: &str, ancestor_id: &str) -> bool {
            let mut at = Some(folder_id.to_string());
            while let Some(id) = at {
                if id == ancestor_id {
                    return true;
                }
                at = self.folder_parent(&id);
            }
            false
        }
        fn folder_parent(&self, folder_id: &str) -> Option<String> {
            self.tree
                .iter()
                .find(|(id, _)| id == folder_id)
                .and_then(|(_, parent)| parent.clone())
        }
    }

    fn host() -> Host {
        Host {
            current: None,
            folders: vec![ItemRef::folder("f1"), ItemRef::folder("f2")],
            accounts: ["a", "b", "c", "d"].map(ItemRef::account).to_vec(),
            tree: vec![("f1".into(), None), ("f2".into(), Some("f1".into()))],
        }
    }

    fn pressed(item: ItemRef) -> Option<PressedCard> {
        Some(PressedCard {
            item,
            section_card: false,
            section_folder: None,
        })
    }

    #[test]
    fn a_press_drags_only_past_the_threshold() {
        let h = host();
        let mut d = DragManager::default();
        d.mouse_down(true, pressed(ItemRef::account("a")), false, (50.0, 50.0));
        assert!(!d.mouse_move((52.0, 52.0), true));
        assert!(!d.is_dragging());
        assert!(d.mouse_move((53.0, 52.0), true));
        assert!(d.is_dragging());
        d.set_slots(
            Slots {
                rects: row(),
                list_mode: false,
            },
            &h,
        );
        assert_eq!(d.mouse_up(&h).commit, None);
        assert!(d.eats_click());
        d.end_frame();
        assert!(!d.eats_click());
    }

    #[test]
    fn a_reorder_commits_the_previewed_order() {
        let h = host();
        let mut d = DragManager::default();
        d.mouse_down(true, pressed(ItemRef::account("a")), false, (50.0, 50.0));
        d.mouse_move((260.0, 50.0), true);
        d.set_slots(
            Slots {
                rects: row(),
                list_mode: false,
            },
            &h,
        );
        d.update(&PointHit::Account, &h);
        assert_eq!(d.preview_index(), Some(2));
        let out = d.mouse_up(&h);
        assert!(out.refresh);
        let order: Vec<&str> = match &out.commit {
            Some(DropCommit::Reorder {
                folder: None,
                items,
            }) => items.iter().map(|i| i.id.as_str()).collect(),
            other => panic!("{other:?}"),
        };
        assert_eq!(order, ["f1", "f2", "b", "c", "a", "d"]);
        assert!(!d.in_flight());
    }

    #[test]
    fn a_drop_on_a_folder_moves_into_it_and_the_back_card_moves_up() {
        let mut h = host();
        let mut d = DragManager::default();
        d.mouse_down(true, pressed(ItemRef::account("b")), false, (0.0, 0.0));
        d.mouse_move((10.0, 0.0), true);
        d.update(
            &PointHit::Folder {
                id: "f1".into(),
                section_card: false,
            },
            &h,
        );
        assert_eq!(d.drag_over_folder_id(), Some("f1"));
        assert_eq!(
            d.mouse_up(&h).commit,
            Some(DropCommit::Move {
                item: ItemRef::account("b"),
                from: None,
                to: Some("f1".into()),
            })
        );

        h.current = Some("f2".into());
        d.mouse_down(true, pressed(ItemRef::account("c")), false, (0.0, 0.0));
        d.mouse_move((0.0, 10.0), true);
        d.update(&PointHit::Back, &h);
        assert!(d.drag_over_back());
        assert_eq!(
            d.mouse_up(&h).commit,
            Some(DropCommit::Move {
                item: ItemRef::account("c"),
                from: Some("f2".into()),
                to: Some("f1".into()),
            })
        );
    }

    #[test]
    fn a_folder_never_drops_into_its_own_subtree() {
        let h = host();
        let mut d = DragManager::default();
        d.mouse_down(true, pressed(ItemRef::folder("f1")), false, (0.0, 0.0));
        d.mouse_move((10.0, 0.0), true);
        d.update(
            &PointHit::Folder {
                id: "f2".into(),
                section_card: false,
            },
            &h,
        );
        assert_eq!(d.drag_over_folder_id(), None);
    }

    #[test]
    fn escape_cancels_and_eats_the_release() {
        let h = host();
        let mut d = DragManager::default();
        assert!(!d.cancel_from_escape());
        d.mouse_down(true, pressed(ItemRef::account("a")), false, (0.0, 0.0));
        d.mouse_move((10.0, 0.0), true);
        assert!(d.cancel_from_escape());
        assert!(!d.is_dragging());
        assert_eq!(d.mouse_up(&h), DropOutcome::default());
        assert!(d.eats_click());
    }

    #[test]
    fn a_move_without_a_button_aborts() {
        let mut d = DragManager::default();
        d.mouse_down(true, pressed(ItemRef::account("a")), false, (0.0, 0.0));
        assert!(!d.mouse_move((20.0, 0.0), false));
        assert!(!d.in_flight());
    }

    #[test]
    fn sections_mode_drags_section_wrappers_and_moves_accounts_from_their_section() {
        let h = host();
        let mut d = DragManager::default();
        d.mouse_down(true, pressed(ItemRef::folder("f2")), true, (0.0, 0.0));
        assert!(!d.in_flight());

        d.mouse_down(
            true,
            Some(PressedCard {
                item: ItemRef::account("a"),
                section_card: false,
                section_folder: Some("f2".into()),
            }),
            true,
            (0.0, 0.0),
        );
        d.mouse_move((10.0, 0.0), true);
        d.set_slots(
            Slots {
                rects: row(),
                list_mode: false,
            },
            &h,
        );
        d.update(&PointHit::Grid, &h);
        assert_eq!(d.preview_index(), None);
        d.update(
            &PointHit::Folder {
                id: "f1".into(),
                section_card: true,
            },
            &h,
        );
        assert_eq!(
            d.mouse_up(&h).commit,
            Some(DropCommit::Move {
                item: ItemRef::account("a"),
                from: Some("f2".into()),
                to: Some("f1".into()),
            })
        );
    }
}
