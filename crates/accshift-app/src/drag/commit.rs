//! What a release commits: a move into a folder or up a level, or the order
//! the reorder preview shows.

use super::{DragHost, DragManager, PressedCard};
use crate::folders::{ItemKind, ItemRef};
use crate::stores::ClientStores;

/// The store change a drop asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum DropCommit {
    /// The item leaves folder `from` for folder `to` (`None`: the root).
    Move {
        item: ItemRef,
        from: Option<String>,
        to: Option<String>,
    },
    /// The new order of every card of `folder`, folders first.
    Reorder {
        folder: Option<String>,
        items: Vec<ItemRef>,
    },
}

impl DropCommit {
    /// Applies the change to the active tab's folders.
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
    /// The workspace must reload its cards.
    pub refresh: bool,
}

impl DragManager {
    /// The change dropping `item` here asks for, `None` when the drop
    /// changes nothing. The back card wins, then the folder under the
    /// pointer, then the reorder preview.
    pub(super) fn drop_commit(
        &self,
        item: ItemRef,
        card: &PressedCard,
        host: &dyn DragHost,
    ) -> Option<DropCommit> {
        let current = host.current_folder_id();
        if self.drag_over_back && current.is_some() {
            let parent = current.as_deref().and_then(|c| host.folder_parent(c));
            return Some(DropCommit::Move {
                item,
                from: current,
                to: parent,
            });
        }
        if let Some(over) = self.drag_over_folder_id.clone() {
            // In sections mode an account leaves the folder of its section.
            let source = if self.sections_mode && item.kind == ItemKind::Account {
                card.section_folder.clone()
            } else {
                current
            };
            let into_itself = item.kind == ItemKind::Folder && item.id == over;
            if into_itself || source.as_deref() == Some(over.as_str()) {
                return None;
            }
            return Some(DropCommit::Move {
                item,
                from: source,
                to: Some(over),
            });
        }
        let preview = self.preview_index?;
        let items = reordered(item, preview, host.folder_items(), host.account_items());
        Some(DropCommit::Reorder {
            folder: current,
            items,
        })
    }
}

/// The order the preview shows: `item` at index `preview` among the cards
/// of its kind, folders before accounts.
fn reordered(
    item: ItemRef,
    preview: usize,
    folders: Vec<ItemRef>,
    accounts: Vec<ItemRef>,
) -> Vec<ItemRef> {
    match item.kind {
        ItemKind::Folder => move_to(folders, item, preview)
            .into_iter()
            .chain(accounts)
            .collect(),
        ItemKind::Account => folders
            .into_iter()
            .chain(move_to(accounts, item, preview))
            .collect(),
    }
}

/// `list` without `item`, then `item` put back at `index` (clamped).
fn move_to(list: Vec<ItemRef>, item: ItemRef, index: usize) -> Vec<ItemRef> {
    let mut moved: Vec<ItemRef> = list.into_iter().filter(|i| i.id != item.id).collect();
    moved.insert(index.min(moved.len()), item);
    moved
}
