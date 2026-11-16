//! Client-side folder store.
//!
//! Folders live in `<app_config_dir>/user/folders.json`, written by the GUI.
//! The CLI reads the file read-only for `list --folder` and `folders`.
//! Schema mirrors `accshift_app::folders::FolderStore`, and so do its
//! sanitizing rules (trimmed fields, folders missing an id, name or platform
//! dropped), held to the app's by the parity test in `tests.rs`.

use accshift_core::storage::{client_store_path, read_json_if_exists, STORE_FOLDERS};
use accshift_core::AppContext;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// The `itemOrder` key prefix of a platform's root, as the app writes it.
const ROOT_KEY_PREFIX: &str = "root:";

#[derive(Debug, Default)]
pub struct FolderStore {
    pub folders: Vec<FolderInfo>,
    pub item_order: HashMap<String, Vec<ItemRef>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRef {
    pub kind: String,
    pub id: String,
}

pub fn load(ctx: &dyn AppContext) -> Result<Option<FolderStore>, String> {
    let path = client_store_path(ctx, STORE_FOLDERS)?;
    Ok(read_json_if_exists::<Value>(&path)?.map(|raw| FolderStore::from_value(&raw)))
}

fn trimmed(record: &serde_json::Map<String, Value>, key: &str) -> String {
    record
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

impl FolderStore {
    /// The app's `sanitize_folder_store`, for the fields the CLI reads.
    pub fn from_value(raw: &Value) -> Self {
        let root = raw.as_object();
        let folders: Vec<FolderInfo> = root
            .and_then(|map| map.get("folders"))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let record = item.as_object()?;
                        let id = trimmed(record, "id");
                        let name = trimmed(record, "name");
                        let platform = trimmed(record, "platform");
                        if id.is_empty() || name.is_empty() || platform.is_empty() {
                            return None;
                        }
                        let parent_id = Some(trimmed(record, "parentId")).filter(|p| !p.is_empty());
                        Some(FolderInfo {
                            id,
                            name,
                            parent_id,
                            platform,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let valid: HashSet<&str> = folders.iter().map(|f| f.id.as_str()).collect();
        let mut item_order = HashMap::new();
        if let Some(order) = root
            .and_then(|map| map.get("itemOrder"))
            .and_then(Value::as_object)
        {
            for (key, entry) in order {
                let Some(items) = entry.as_array() else {
                    continue;
                };
                let is_root = key.starts_with(ROOT_KEY_PREFIX) && key.len() > ROOT_KEY_PREFIX.len();
                if !is_root && !valid.contains(key.as_str()) {
                    continue;
                }
                let refs = items
                    .iter()
                    .filter_map(|item| {
                        let record = item.as_object()?;
                        let id = trimmed(record, "id");
                        let kind = record.get("type").and_then(Value::as_str)?;
                        let keep = match kind {
                            "account" => true,
                            "folder" => valid.contains(id.as_str()),
                            _ => false,
                        };
                        (keep && !id.is_empty()).then(|| ItemRef {
                            kind: kind.to_string(),
                            id,
                        })
                    })
                    .collect();
                item_order.insert(key.clone(), refs);
            }
        }
        Self {
            folders,
            item_order,
        }
    }

    fn folder(&self, id: &str) -> Option<&FolderInfo> {
        self.folders.iter().find(|f| f.id == id)
    }

    /// "Parent / Child" from the platform root down to `folder`.
    pub fn path_of(&self, folder: &FolderInfo) -> String {
        let mut names = vec![folder.name.as_str()];
        let mut seen = HashSet::from([folder.id.as_str()]);
        let mut parent = folder.parent_id.as_deref();
        while let Some(id) = parent {
            if !seen.insert(id) {
                break;
            }
            let Some(next) = self.folder(id) else {
                break;
            };
            names.push(next.name.as_str());
            parent = next.parent_id.as_deref();
        }
        names.reverse();
        names.join(" / ")
    }
}

/// Why `--folder` matched no single folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderLookupError {
    /// The platform has no folders at all.
    NoneConfigured,
    /// Nothing matched; the available folder paths.
    NotFound(Vec<String>),
    /// Several folders share the name: `(id, path)` of each.
    Ambiguous(Vec<(String, String)>),
}

/// The folder a `--folder` value names on `platform`: an exact folder id
/// first, then a name without case sensitivity.
pub fn resolve<'a>(
    store: &'a FolderStore,
    platform: &str,
    query: &str,
) -> Result<&'a FolderInfo, FolderLookupError> {
    let on_platform: Vec<&FolderInfo> = store
        .folders
        .iter()
        .filter(|f| f.platform == platform)
        .collect();
    if on_platform.is_empty() {
        return Err(FolderLookupError::NoneConfigured);
    }
    let query = query.trim();
    if let Some(by_id) = on_platform.iter().find(|f| f.id == query) {
        return Ok(by_id);
    }
    let wanted = query.to_lowercase();
    let by_name: Vec<&FolderInfo> = on_platform
        .iter()
        .copied()
        .filter(|f| f.name.to_lowercase() == wanted)
        .collect();
    match by_name.as_slice() {
        [one] => Ok(one),
        [] => Err(FolderLookupError::NotFound(
            on_platform.iter().map(|f| store.path_of(f)).collect(),
        )),
        many => Err(FolderLookupError::Ambiguous(
            many.iter()
                .map(|f| (f.id.clone(), store.path_of(f)))
                .collect(),
        )),
    }
}

/// The account ids in `folder_id` and every folder nested in it.
pub fn accounts_in_folder(store: &FolderStore, folder_id: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    collect(store, folder_id, &mut out, &mut HashSet::new());
    out
}

fn collect(
    store: &FolderStore,
    folder_id: &str,
    out: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) {
    if !visited.insert(folder_id.to_string()) {
        return;
    }
    let Some(items) = store.item_order.get(folder_id) else {
        return;
    };
    for item in items {
        match item.kind.as_str() {
            "account" => {
                out.insert(item.id.clone());
            }
            "folder" => collect(store, &item.id, out, visited),
            _ => {}
        }
    }
}

/// One folder of `accshift folders`, in tree order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub path: String,
    /// 0 for a folder at the platform root.
    pub depth: usize,
    /// Accounts placed directly in this folder.
    pub accounts: usize,
    /// Accounts in this folder and every folder nested in it.
    pub total_accounts: usize,
}

/// The platform's folders depth first, siblings in the order the app shows
/// them (`itemOrder`), then any the order does not mention, in store order.
pub fn tree(store: &FolderStore, platform: &str) -> Vec<FolderNode> {
    let on_platform: Vec<&FolderInfo> = store
        .folders
        .iter()
        .filter(|f| f.platform == platform)
        .collect();
    let ids: HashSet<&str> = on_platform.iter().map(|f| f.id.as_str()).collect();
    let mut out = Vec::new();
    let mut visited = HashSet::new();
    // A parent that is gone (or on another platform) puts its child at the
    // root, as the app does.
    let is_top = |f: &FolderInfo| f.parent_id.as_deref().is_none_or(|p| !ids.contains(p));
    let root_key = format!("{ROOT_KEY_PREFIX}{platform}");
    let top: Vec<&FolderInfo> = ordered_children(store, &root_key, &on_platform, |f| is_top(f));
    for folder in top {
        walk(store, &on_platform, folder, 0, &mut visited, &mut out);
    }
    // A cycle in parent ids leaves folders unreached: list them at the root
    // rather than hide them.
    for folder in &on_platform {
        if !visited.contains(folder.id.as_str()) {
            walk(store, &on_platform, folder, 0, &mut visited, &mut out);
        }
    }
    out
}

fn ordered_children<'a>(
    store: &FolderStore,
    bucket: &str,
    on_platform: &[&'a FolderInfo],
    belongs: impl Fn(&FolderInfo) -> bool,
) -> Vec<&'a FolderInfo> {
    let mut out: Vec<&'a FolderInfo> = Vec::new();
    if let Some(items) = store.item_order.get(bucket) {
        for item in items.iter().filter(|i| i.kind == "folder") {
            if let Some(f) = on_platform.iter().find(|f| f.id == item.id) {
                if belongs(f) && !out.iter().any(|o| o.id == f.id) {
                    out.push(f);
                }
            }
        }
    }
    for f in on_platform {
        if belongs(f) && !out.iter().any(|o| o.id == f.id) {
            out.push(f);
        }
    }
    out
}

fn walk<'a>(
    store: &FolderStore,
    on_platform: &[&'a FolderInfo],
    folder: &'a FolderInfo,
    depth: usize,
    visited: &mut HashSet<&'a str>,
    out: &mut Vec<FolderNode>,
) {
    if !visited.insert(folder.id.as_str()) {
        return;
    }
    let direct = store
        .item_order
        .get(&folder.id)
        .map(|items| items.iter().filter(|i| i.kind == "account").count())
        .unwrap_or(0);
    out.push(FolderNode {
        id: folder.id.clone(),
        name: folder.name.clone(),
        parent_id: folder.parent_id.clone(),
        path: store.path_of(folder),
        depth,
        accounts: direct,
        total_accounts: accounts_in_folder(store, &folder.id).len(),
    });
    let children = ordered_children(store, &folder.id, on_platform, |f| {
        f.parent_id.as_deref() == Some(folder.id.as_str())
    });
    for child in children {
        walk(store, on_platform, child, depth + 1, visited, out);
    }
}
