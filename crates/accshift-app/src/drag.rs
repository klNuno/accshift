//! Card drag and drop: `createDragManager` (`dragAndDrop.svelte.ts`) and
//! `previewIndexAt` (`dropGeometry.ts`).
//!
//! What the DOM answers in TS (the card under a press, the slot rects at
//! drag start, `elementFromPoint` while dragging) comes from the UI as
//! values; the manager keeps the state machine and decides the drop.

use crate::display::FolderNavigation;
use crate::folders::{ItemKind, ItemRef};
use crate::stores::ClientStores;

/// `DRAG_THRESHOLD`: Manhattan distance before a press becomes a drag.
pub const DRAG_THRESHOLD: f32 = 5.0;

/// The part of a `DOMRect` the drop calculation reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// `previewIndexAt`: the index the dragged card would take if dropped at
/// `(x, y)`, `None` when the layout offers nowhere to drop (no slots, or
/// the card's own index was never resolved).
///
/// The pointer picks the nearest slot centre, then the side it fell on
/// decides before or after that slot. Dropping past the old position
/// shifts the target back by one: removing the card first closes its gap.
pub fn preview_index_at(
    x: f32,
    y: f32,
    slots: &[SlotRect],
    old_index: Option<usize>,
    list_mode: bool,
) -> Option<usize> {
    let old_index = old_index?;
    if slots.is_empty() {
        return None;
    }
    let mut nearest = 0;
    let mut nearest_distance = f32::INFINITY;
    for (i, slot) in slots.iter().enumerate() {
        let dx = x - (slot.left + slot.width / 2.0);
        let dy = y - (slot.top + slot.height / 2.0);
        let distance = dx * dx + dy * dy;
        if distance < nearest_distance {
            nearest_distance = distance;
            nearest = i;
        }
    }
    let slot = slots[nearest];
    // A list stacks vertically, a grid flows horizontally: the axis the
    // cards advance along decides before or after.
    let past = if list_mode {
        y > slot.top + slot.height / 2.0
    } else {
        x > slot.left + slot.width / 2.0
    };
    let drop_index = if past { nearest + 1 } else { nearest };
    let insert_at = if drop_index > old_index {
        drop_index - 1
    } else {
        drop_index
    };
    Some(insert_at.min(slots.len() - 1))
}

/// The card a press landed on (`closest("[data-account-id],
/// [data-folder-id]")`).
#[derive(Debug, Clone, PartialEq)]
pub struct PressedCard {
    pub item: ItemRef,
    /// A section wrapper (`data-section-card="true"`), sections mode only.
    pub section_card: bool,
    /// Sections mode: the folder of the section holding the card, `None`
    /// for the root section (the source folder of an account drag).
    pub section_folder: Option<String>,
}

/// What `elementFromPoint` found under the pointer, read through
/// `closest("[data-folder-id], [data-back-card], [data-account-id]")`.
#[derive(Debug, Clone, PartialEq)]
pub enum PointHit {
    /// Outside the document.
    Nothing,
    /// The document, but no card: the grid itself.
    Grid,
    Back,
    Folder {
        id: String,
        section_card: bool,
    },
    Account,
}

/// The slots captured when a drag starts (`refreshSlotRects`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slots {
    pub rects: Vec<SlotRect>,
    /// `.list-container`, or the sections when a folder moves among them.
    pub list_mode: bool,
}

/// What the pointer is over.
#[derive(Debug, Clone, PartialEq)]
enum DropTarget {
    None,
    Back,
    Folder(String),
    Reorder,
}

/// The app state the drag reads (the `DragManagerOptions` getters and the
/// folder store).
pub trait DragHost {
    fn current_folder_id(&self) -> Option<String>;
    /// `navigation.folderItems`.
    fn folder_items(&self) -> Vec<ItemRef>;
    /// `navigation.accountItems`.
    fn account_items(&self) -> Vec<ItemRef>;
    /// `isFolderDescendant(folderId, ancestorId)`.
    fn is_folder_descendant(&self, folder_id: &str, ancestor_id: &str) -> bool;
    /// `getFolder(id)?.parentId`.
    fn folder_parent(&self, folder_id: &str) -> Option<String>;
}

/// The folder navigation and the folder store, as the app wires them.
pub struct StoreHost<'a> {
    pub navigation: &'a FolderNavigation,
    pub stores: &'a ClientStores,
}

impl DragHost for StoreHost<'_> {
    fn current_folder_id(&self) -> Option<String> {
        self.navigation.current_folder_id().map(str::to_owned)
    }
    fn folder_items(&self) -> Vec<ItemRef> {
        self.navigation.folder_items()
    }
    fn account_items(&self) -> Vec<ItemRef> {
        self.navigation.account_items()
    }
    fn is_folder_descendant(&self, folder_id: &str, ancestor_id: &str) -> bool {
        self.stores
            .folders()
            .is_folder_descendant(Some(folder_id), ancestor_id)
    }
    fn folder_parent(&self, folder_id: &str) -> Option<String> {
        self.stores.folders().folder(folder_id)?.parent_id.clone()
    }
}

/// The store change a drop asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum DropCommit {
    /// `moveItem(item, from, to, activeTab)`.
    Move {
        item: ItemRef,
        from: Option<String>,
        to: Option<String>,
    },
    /// `reorderItems(folder, activeTab, items)`.
    Reorder {
        folder: Option<String>,
        items: Vec<ItemRef>,
    },
}

impl DropCommit {
    /// `moveItem` or `reorderItems` on the active tab's folders.
    pub fn apply(self, stores: &mut ClientStores, active_tab: &str) {
        match self {
            Self::Move { item, from, to } => {
                stores.move_item(&item, from.as_deref(), to.as_deref(), active_tab, None)
            }
            Self::Reorder { folder, items } => {
                stores.reorder_items(folder.as_deref(), active_tab, items)
            }
        }
    }
}

/// What a release did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DropOutcome {
    pub commit: Option<DropCommit>,
    /// `options.onRefresh()`.
    pub refresh: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Pending {
    card: PressedCard,
    start: (f32, f32),
}

/// `createDragManager`.
#[derive(Debug, Default)]
pub struct DragManager {
    drag_item: Option<ItemRef>,
    drag_over_folder_id: Option<String>,
    drag_over_back: bool,
    is_dragging: bool,
    preview_index: Option<usize>,
    pending: Option<Pending>,
    /// `eatNextClick`: the click right after a drag release.
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

    /// `handleGridMouseDown`: a primary press on the workspace. `card` is
    /// the card under it; `sections_mode` whether the grid shows sections.
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

    /// `handleDocMouseMove`. `buttons_down` is `e.buttons !== 0`: a move
    /// with no button held means the release was missed outside the
    /// window. Returns true when this move started the drag: the caller
    /// then measures the slots ([`Self::set_slots`]) and the ghost.
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

    /// `refreshSlotRects`: the slots of the dragged card's kind and its
    /// index among them. In sections mode an account has no slot: it only
    /// moves into a folder.
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

    /// `handleDocScroll`: the slots are stale.
    pub fn scrolled(&mut self) {
        if self.is_dragging && self.has_pointer {
            self.rects_stale = true;
        }
    }

    /// The pointer, as the last move left it.
    pub fn pointer(&self) -> (f32, f32) {
        self.last
    }

    fn drop_target(&self, hit: &PointHit, host: &dyn DragHost) -> DropTarget {
        match hit {
            PointHit::Nothing => DropTarget::None,
            PointHit::Grid | PointHit::Account => DropTarget::Reorder,
            PointHit::Back => DropTarget::Back,
            PointHit::Folder { id, section_card } => {
                if let Some(item) = self
                    .drag_item
                    .as_ref()
                    .filter(|i| i.kind == ItemKind::Folder)
                {
                    // A folder never swallows itself or its own subtree.
                    if host.is_folder_descendant(id, &item.id) {
                        return DropTarget::Reorder;
                    }
                    // A section takes accounts, never another folder:
                    // folders reorder among the sections.
                    if self.sections_mode && *section_card {
                        return DropTarget::Reorder;
                    }
                }
                DropTarget::Folder(id.clone())
            }
        }
    }

    /// `updateDragAt`, once per frame while dragging: `hit` is what lies
    /// under [`Self::pointer`].
    pub fn update(&mut self, hit: &PointHit, host: &dyn DragHost) {
        if !self.is_dragging {
            return;
        }
        let target = self.drop_target(hit, host);
        self.drag_over_back = target == DropTarget::Back;
        self.drag_over_folder_id = match &target {
            DropTarget::Folder(id) => Some(id.clone()),
            _ => None,
        };
        if target != DropTarget::Reorder {
            self.preview_index = None;
            return;
        }
        // Left alone when the layout offers no slot.
        let (x, y) = self.last;
        if let Some(at) = preview_index_at(
            x,
            y,
            &self.slots.rects,
            self.old_index,
            self.slots.list_mode,
        ) {
            self.preview_index = Some(at);
        }
    }

    fn cancel(&mut self) {
        let eat = self.eat_next_click;
        let cancelled = self.drag_cancelled;
        *self = Self::default();
        self.eat_next_click = eat;
        self.drag_cancelled = cancelled;
    }

    /// `cancelFromEscape`: false when no card is pressed or moving.
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

    /// `handleDocMouseUp`.
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
        let current = host.current_folder_id();
        // In sections mode an account leaves the folder of its section.
        let source = if self.sections_mode
            && self
                .drag_item
                .as_ref()
                .is_some_and(|i| i.kind == ItemKind::Account)
        {
            pending.card.section_folder.clone()
        } else {
            current.clone()
        };
        let mut out = DropOutcome::default();
        if let Some(item) = self.drag_item.clone() {
            if self.drag_over_back && current.is_some() {
                let parent = current.as_deref().and_then(|c| host.folder_parent(c));
                out.commit = Some(DropCommit::Move {
                    item,
                    from: current,
                    to: parent,
                });
            } else if let Some(over) = self.drag_over_folder_id.clone() {
                let into_itself = item.kind == ItemKind::Folder && item.id == over;
                if !into_itself && source.as_deref() != Some(over.as_str()) {
                    out.commit = Some(DropCommit::Move {
                        item,
                        from: source,
                        to: Some(over),
                    });
                }
            } else if let Some(preview) = self.preview_index {
                // Commit the order the preview shows.
                let folders = host.folder_items();
                let accounts = host.account_items();
                let items = match item.kind {
                    ItemKind::Folder => {
                        let mut moved: Vec<ItemRef> =
                            folders.into_iter().filter(|i| i.id != item.id).collect();
                        moved.insert(preview.min(moved.len()), item);
                        moved.into_iter().chain(accounts).collect()
                    }
                    ItemKind::Account => {
                        let mut moved: Vec<ItemRef> =
                            accounts.into_iter().filter(|i| i.id != item.id).collect();
                        moved.insert(preview.min(moved.len()), item);
                        folders.into_iter().chain(moved).collect()
                    }
                };
                out.commit = Some(DropCommit::Reorder {
                    folder: current,
                    items,
                });
            }
            out.refresh = true;
        }
        self.cancel();
        self.eat_next_click = true;
        out
    }

    /// `handleCaptureClick`: whether the click of this release is eaten.
    /// The guard only lasts the release's own turn (`setTimeout(0)`):
    /// call [`Self::end_frame`] after the frame's clicks.
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
    use super::*;

    fn row() -> Vec<SlotRect> {
        (0..4)
            .map(|i| SlotRect {
                left: i as f32 * 100.0,
                top: 0.0,
                width: 100.0,
                height: 100.0,
            })
            .collect()
    }

    fn column() -> Vec<SlotRect> {
        (0..4)
            .map(|i| SlotRect {
                left: 0.0,
                top: i as f32 * 100.0,
                width: 100.0,
                height: 100.0,
            })
            .collect()
    }

    #[test]
    fn has_nowhere_to_drop_when_the_layout_was_never_measured() {
        assert_eq!(preview_index_at(50.0, 50.0, &[], Some(0), false), None);
        assert_eq!(preview_index_at(50.0, 50.0, &row(), None, false), None);
    }

    #[test]
    fn drops_before_or_after_the_nearest_card() {
        assert_eq!(
            preview_index_at(210.0, 50.0, &row(), Some(0), false),
            Some(1)
        );
        assert_eq!(
            preview_index_at(260.0, 50.0, &row(), Some(0), false),
            Some(2)
        );
    }

    #[test]
    fn reads_the_vertical_axis_in_list_mode() {
        assert_eq!(
            preview_index_at(60.0, 240.0, &column(), Some(3), true),
            Some(2)
        );
        assert_eq!(
            preview_index_at(60.0, 260.0, &column(), Some(3), true),
            Some(3)
        );
    }

    #[test]
    fn closes_the_gap_the_card_left_behind() {
        assert_eq!(
            preview_index_at(260.0, 50.0, &row(), Some(0), false),
            Some(2)
        );
        assert_eq!(
            preview_index_at(30.0, 50.0, &row(), Some(3), false),
            Some(0)
        );
    }

    #[test]
    fn never_points_outside_the_list() {
        assert_eq!(
            preview_index_at(-5000.0, 50.0, &row(), Some(2), false),
            Some(0)
        );
        assert_eq!(
            preview_index_at(5000.0, 50.0, &row(), Some(0), false),
            Some(3)
        );
    }

    #[test]
    fn resolves_a_pointer_inside_no_card() {
        let gapped: Vec<SlotRect> = (0..4)
            .map(|i| SlotRect {
                left: i as f32 * 120.0,
                top: 0.0,
                width: 100.0,
                height: 100.0,
            })
            .collect();
        assert_eq!(
            preview_index_at(115.0, 50.0, &gapped, Some(3), false),
            Some(1)
        );
    }

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
