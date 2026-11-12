//! What the pointer is over while a card drags: the hit the UI reports and
//! the drop target the manager makes of it.

use super::{preview_index_at, DragHost, DragManager};
use crate::folders::ItemKind;

/// The nearest folder card, back card or account card under the pointer,
/// as the UI hit-tests it.
#[derive(Debug, Clone, PartialEq)]
pub enum PointHit {
    /// Outside the window.
    Nothing,
    /// Inside the window, but over no card: the grid itself.
    Grid,
    Back,
    Folder {
        id: String,
        section_card: bool,
    },
    Account,
}

/// What a release over the current hit would do.
#[derive(Debug, Clone, PartialEq)]
enum DropTarget {
    None,
    Back,
    Folder(String),
    Reorder,
}

impl DragManager {
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

    /// Once per frame while dragging: `hit` is what lies under
    /// [`Self::pointer`]. Highlights the folder or back card under it, or
    /// moves the reorder preview.
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
}
