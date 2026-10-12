//! The folder store, ported from `src/lib/features/folders/{types,store}.ts`.
//!
//! [`FolderStore`] is the store's JSON as a struct. Its methods are the TS
//! functions; the mutating ones return whether the store has to be written,
//! and [`crate::stores::ClientStores`] writes it.

use crate::js;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CURRENT_VERSION: u32 = 1;
/// Every `itemOrder` key is this prefix plus a platform id, or a folder id.
pub const ROOT_KEY_PREFIX: &str = "root:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub platform: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Account,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemRef {
    #[serde(rename = "type")]
    pub kind: ItemKind,
    pub id: String,
}

impl ItemRef {
    pub fn account(id: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Account,
            id: id.into(),
        }
    }
    pub fn folder(id: impl Into<String>) -> Self {
        Self {
            kind: ItemKind::Folder,
            id: id.into(),
        }
    }
    fn same(&self, other: &ItemRef) -> bool {
        self.kind == other.kind && self.id == other.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStore {
    pub version: u32,
    pub folders: Vec<FolderInfo>,
    pub item_order: BTreeMap<String, Vec<ItemRef>>,
}

impl Default for FolderStore {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            folders: Vec::new(),
            item_order: BTreeMap::new(),
        }
    }
}

/// One folder of [`FolderStore::root_sections`] (or the root, `folder: None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSection {
    pub folder: Option<FolderInfo>,
    pub folder_items: Vec<ItemRef>,
    pub account_items: Vec<ItemRef>,
}

/// `getRootKey`.
pub fn root_key(platform: &str) -> String {
    format!("{ROOT_KEY_PREFIX}{platform}")
}

/// `folderId || getRootKey(platform)`: an empty id is the root too.
fn bucket_key(folder_id: Option<&str>, platform: &str) -> String {
    match folder_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => root_key(platform),
    }
}

fn non_empty(folder_id: Option<&str>) -> Option<&str> {
    folder_id.filter(|id| !id.is_empty())
}

fn trimmed_string(record: &serde_json::Map<String, Value>, key: &str) -> String {
    js::as_str(record.get(key))
        .map(|text| js::trim(text).to_string())
        .unwrap_or_default()
}

fn sanitize_folder(value: &Value) -> Option<FolderInfo> {
    let empty = serde_json::Map::new();
    let raw = js::as_record(Some(value)).unwrap_or(&empty);
    let id = trimmed_string(raw, "id");
    let name = trimmed_string(raw, "name");
    let platform = trimmed_string(raw, "platform");
    let parent_id = match raw.get("parentId") {
        Some(Value::String(parent)) if !js::trim(parent).is_empty() => {
            Some(js::trim(parent).to_string())
        }
        _ => None,
    };
    if id.is_empty() || name.is_empty() || platform.is_empty() {
        return None;
    }
    Some(FolderInfo {
        id,
        name,
        parent_id,
        platform,
    })
}

fn sanitize_item_ref(value: &Value) -> Option<ItemRef> {
    let raw = js::as_record(Some(value))?;
    let id = trimmed_string(raw, "id");
    if id.is_empty() {
        return None;
    }
    let kind = match js::as_str(raw.get("type")) {
        Some("account") => ItemKind::Account,
        Some("folder") => ItemKind::Folder,
        _ => return None,
    };
    Some(ItemRef { kind, id })
}

/// `sanitizeStore` then `migrateStore`. Drops folders missing an id, name or
/// platform, and every `itemOrder` bucket that is neither a platform root nor
/// a live folder (a dead folder's bucket would keep its accounts forever).
pub fn sanitize_folder_store(value: &Value) -> FolderStore {
    let raw = js::as_record(Some(value));
    let folders: Vec<FolderInfo> = match raw.and_then(|map| map.get("folders")) {
        Some(Value::Array(items)) => items.iter().filter_map(sanitize_folder).collect(),
        _ => Vec::new(),
    };
    let valid: BTreeSet<&str> = folders.iter().map(|folder| folder.id.as_str()).collect();
    let mut item_order = BTreeMap::new();
    if let Some(order) = js::as_record(raw.and_then(|map| map.get("itemOrder"))) {
        for (key, entry) in order {
            if js::trim(key).is_empty() {
                continue;
            }
            let Value::Array(items) = entry else {
                continue;
            };
            let is_root = key.starts_with(ROOT_KEY_PREFIX) && key.len() > ROOT_KEY_PREFIX.len();
            if !is_root && !valid.contains(key.as_str()) {
                continue;
            }
            let refs: Vec<ItemRef> = items
                .iter()
                .filter_map(sanitize_item_ref)
                .filter(|item| item.kind != ItemKind::Folder || valid.contains(item.id.as_str()))
                .collect();
            item_order.insert(key.clone(), refs);
        }
    }
    FolderStore {
        version: CURRENT_VERSION,
        folders,
        item_order,
    }
}

/// `getStore` on a raw store value: `None` (missing) is the empty store.
pub fn load_folder_store(value: Option<&Value>) -> FolderStore {
    value.map(sanitize_folder_store).unwrap_or_default()
}

/// `generateId`: base-36 milliseconds plus four random base-36 characters.
pub fn generate_folder_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    let random = js::to_base36(js::random_u64());
    let suffix: String = random.chars().rev().take(4).collect();
    format!("{}{suffix}", js::to_base36(millis))
}

impl FolderStore {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// `getItemsInFolder`.
    pub fn items_in_folder(&self, folder_id: Option<&str>, platform: &str) -> &[ItemRef] {
        self.item_order
            .get(&bucket_key(folder_id, platform))
            .map_or(&[], Vec::as_slice)
    }

    /// `getRootSections`: each root folder with its items, then the root's own
    /// accounts.
    pub fn root_sections(&self, platform: &str) -> Vec<FolderSection> {
        let root_items = self.items_in_folder(None, platform);
        let mut sections = Vec::new();
        for folder_ref in root_items
            .iter()
            .filter(|item| item.kind == ItemKind::Folder)
        {
            let Some(folder) = self.folder(&folder_ref.id) else {
                continue;
            };
            let items = self
                .item_order
                .get(&folder.id)
                .map_or(&[][..], Vec::as_slice);
            sections.push(FolderSection {
                folder: Some(folder.clone()),
                folder_items: items
                    .iter()
                    .filter(|item| item.kind == ItemKind::Folder)
                    .cloned()
                    .collect(),
                account_items: items
                    .iter()
                    .filter(|item| item.kind == ItemKind::Account)
                    .cloned()
                    .collect(),
            });
        }
        sections.push(FolderSection {
            folder: None,
            folder_items: Vec::new(),
            account_items: root_items
                .iter()
                .filter(|item| item.kind == ItemKind::Account)
                .cloned()
                .collect(),
        });
        sections
    }

    /// `createFolder` with a given id (see [`generate_folder_id`]).
    pub fn create_folder(
        &mut self,
        id: String,
        name: &str,
        parent_id: Option<&str>,
        platform: &str,
    ) -> FolderInfo {
        let folder = FolderInfo {
            id,
            name: name.to_string(),
            parent_id: parent_id.map(str::to_string),
            platform: platform.to_string(),
        };
        self.folders.push(folder.clone());
        self.item_order
            .entry(bucket_key(parent_id, platform))
            .or_default()
            .push(ItemRef::folder(folder.id.clone()));
        self.item_order.insert(folder.id.clone(), Vec::new());
        folder
    }

    /// `getFolder`.
    pub fn folder(&self, id: &str) -> Option<&FolderInfo> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    /// `listFolders`: every folder of one platform, in creation order.
    pub fn list_folders(&self, platform: &str) -> Vec<&FolderInfo> {
        self.folders
            .iter()
            .filter(|folder| folder.platform == platform)
            .collect()
    }

    /// `findItemFolderId`: the folder holding this item, `None` at the root.
    pub fn find_item_folder_id(&self, item: &ItemRef, platform: &str) -> Option<&str> {
        self.folders
            .iter()
            .filter(|folder| folder.platform == platform)
            .find(|folder| {
                self.item_order
                    .get(&folder.id)
                    .is_some_and(|items| items.iter().any(|entry| entry.same(item)))
            })
            .map(|folder| folder.id.as_str())
    }

    /// `getFolderPath`: from the outermost folder down to `folder_id`.
    pub fn folder_path(&self, folder_id: Option<&str>) -> Vec<&FolderInfo> {
        let mut path = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = non_empty(folder_id);
        while let Some(id) = current {
            // TS loops forever on a parent cycle; stop at the first repeat.
            if !seen.insert(id) {
                break;
            }
            let Some(folder) = self.folder(id) else {
                break;
            };
            path.insert(0, folder);
            current = non_empty(folder.parent_id.as_deref());
        }
        path
    }

    /// `isFolderDescendant`: `folder_id` is `ancestor_id` or sits under it.
    pub fn is_folder_descendant(&self, folder_id: Option<&str>, ancestor_id: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut current = non_empty(folder_id);
        while let Some(id) = current {
            if id == ancestor_id {
                return true;
            }
            if !seen.insert(id) {
                break;
            }
            current = self
                .folder(id)
                .and_then(|folder| non_empty(folder.parent_id.as_deref()));
        }
        false
    }

    /// `syncAccounts`: new accounts join the root, gone ones leave every
    /// bucket of the platform. Returns whether anything changed.
    pub fn sync_accounts(&mut self, account_ids: &[String], platform: &str) -> bool {
        let root = root_key(platform);
        let mut changed = false;
        if !self.item_order.contains_key(&root) {
            self.item_order.insert(root.clone(), Vec::new());
            changed = true;
        }
        let mut keys = vec![root.clone()];
        keys.extend(
            self.folders
                .iter()
                .filter(|folder| folder.platform == platform)
                .map(|folder| folder.id.clone()),
        );
        let placed: BTreeSet<String> = keys
            .iter()
            .filter_map(|key| self.item_order.get(key))
            .flatten()
            .filter(|item| item.kind == ItemKind::Account)
            .map(|item| item.id.clone())
            .collect();
        for id in account_ids {
            if !placed.contains(id) {
                if let Some(bucket) = self.item_order.get_mut(&root) {
                    bucket.push(ItemRef::account(id.clone()));
                }
                changed = true;
            }
        }
        let valid: BTreeSet<&str> = account_ids.iter().map(String::as_str).collect();
        for key in &keys {
            if let Some(items) = self.item_order.get_mut(key) {
                let before = items.len();
                items.retain(|item| {
                    item.kind == ItemKind::Folder || valid.contains(item.id.as_str())
                });
                if items.len() != before {
                    changed = true;
                }
            }
        }
        changed
    }

    /// `moveItem`. A folder never moves into its own subtree. `insert_index`
    /// past the end appends, like `splice`. Returns whether the store changed
    /// (always, once the move is allowed: TS saves even a no-op move).
    pub fn move_item(
        &mut self,
        item: &ItemRef,
        from_folder_id: Option<&str>,
        to_folder_id: Option<&str>,
        platform: &str,
        insert_index: Option<usize>,
    ) -> bool {
        if item.kind == ItemKind::Folder && self.is_folder_descendant(to_folder_id, &item.id) {
            return false;
        }
        let from_key = bucket_key(from_folder_id, platform);
        let to_key = bucket_key(to_folder_id, platform);
        if let Some(items) = self.item_order.get_mut(&from_key) {
            items.retain(|entry| !entry.same(item));
        }
        let target = self.item_order.entry(to_key).or_default();
        match insert_index {
            Some(index) => target.insert(index.min(target.len()), item.clone()),
            None => target.push(item.clone()),
        }
        if item.kind == ItemKind::Folder {
            if let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == item.id) {
                folder.parent_id = to_folder_id.map(str::to_string);
            }
        }
        true
    }

    /// `reorderItems`: replaces one bucket.
    pub fn reorder_items(&mut self, folder_id: Option<&str>, platform: &str, items: Vec<ItemRef>) {
        self.item_order
            .insert(bucket_key(folder_id, platform), items);
    }

    fn folder_tree_ids(&self, root_folder_id: &str) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        let mut stack = vec![root_folder_id.to_string()];
        while let Some(current) = stack.pop() {
            if current.is_empty() || ids.contains(&current) {
                continue;
            }
            for folder in &self.folders {
                if folder.parent_id.as_deref() == Some(current.as_str()) {
                    stack.push(folder.id.clone());
                }
            }
            ids.insert(current);
        }
        ids
    }

    fn nested_account_items(
        &self,
        folder_id: &str,
        to_delete: &BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        out: &mut Vec<ItemRef>,
    ) {
        if !visited.insert(folder_id.to_string()) {
            return;
        }
        for item in self.item_order.get(folder_id).into_iter().flatten() {
            match item.kind {
                ItemKind::Account => out.push(item.clone()),
                ItemKind::Folder if to_delete.contains(&item.id) => {
                    self.nested_account_items(&item.id, to_delete, visited, out)
                }
                ItemKind::Folder => {}
            }
        }
    }

    /// `deleteFolder`: the folder and its subtree go; their accounts take the
    /// folder's place in its parent. Returns whether the folder existed.
    pub fn delete_folder(&mut self, folder_id: &str) -> bool {
        let Some(folder) = self.folder(folder_id).cloned() else {
            return false;
        };
        let parent_key = bucket_key(folder.parent_id.as_deref(), &folder.platform);
        self.item_order.entry(parent_key.clone()).or_default();
        let to_delete = self.folder_tree_ids(folder_id);
        let mut moved = Vec::new();
        self.nested_account_items(folder_id, &to_delete, &mut BTreeSet::new(), &mut moved);
        if let Some(parent_items) = self.item_order.get_mut(&parent_key) {
            match parent_items
                .iter()
                .position(|item| item.kind == ItemKind::Folder && item.id == folder_id)
            {
                Some(index) => {
                    parent_items.splice(index..=index, moved);
                }
                None => parent_items.extend(moved),
            }
        }
        for items in self.item_order.values_mut() {
            items.retain(|item| item.kind != ItemKind::Folder || !to_delete.contains(&item.id));
        }
        for id in &to_delete {
            self.item_order.remove(id);
        }
        self.folders
            .retain(|folder| !to_delete.contains(&folder.id));
        true
    }

    /// `renameFolder`. Returns whether the folder existed.
    pub fn rename_folder(&mut self, folder_id: &str, new_name: &str) -> bool {
        match self
            .folders
            .iter_mut()
            .find(|folder| folder.id == folder_id)
        {
            Some(folder) => {
                folder.name = new_name.to_string();
                true
            }
            None => false,
        }
    }
}
