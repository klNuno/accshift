//! The app state a drag reads: the current folder, the cards it shows and
//! the folder tree.

use crate::display::FolderNavigation;
use crate::folders::ItemRef;
use crate::stores::ClientStores;

/// The app state the drag reads: the folder navigation and the folder store.
pub trait DragHost {
    fn current_folder_id(&self) -> Option<String>;
    /// The folder cards of the current folder, in display order.
    fn folder_items(&self) -> Vec<ItemRef>;
    /// The account cards of the current folder, in display order.
    fn account_items(&self) -> Vec<ItemRef>;
    /// True when `folder_id` is `ancestor_id` or lies inside it.
    fn is_folder_descendant(&self, folder_id: &str, ancestor_id: &str) -> bool;
    /// The parent of a folder, `None` for a root folder or an unknown id.
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
