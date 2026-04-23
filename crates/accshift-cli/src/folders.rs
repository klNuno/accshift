//! Client-side folder store.
//!
//! Folders live in `<app_config_dir>/user/folders.json`, written by the GUI.
//! The CLI reads the file read-only to let `list --folder <name>` filter
//! accounts. Schema mirrors `src/lib/features/folders/store.ts`.

use accshift_core::storage::{client_store_path, STORE_FOLDERS};
use accshift_core::AppContext;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;

#[derive(Debug, Deserialize, Default)]
pub struct FolderStore {
    #[serde(default)]
    pub folders: Vec<FolderInfo>,
    #[serde(default, rename = "itemOrder")]
    pub item_order: std::collections::HashMap<String, Vec<ItemRef>>,
}

#[derive(Debug, Deserialize)]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
    pub platform: String,
}

#[derive(Debug, Deserialize)]
pub struct ItemRef {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

pub fn load(ctx: &dyn AppContext) -> Result<Option<FolderStore>, String> {
    let path = client_store_path(ctx, STORE_FOLDERS)?;
    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str::<FolderStore>(&data)
            .map(Some)
            .map_err(|e| format!("Could not parse folders.json: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Could not read folders.json: {e}")),
    }
}

/// Resolve folder name to the set of account IDs it contains (recursive into
/// subfolders). Name match is case-insensitive and scoped to the platform.
///
/// Returns `Err` if no folder matches or more than one does.
pub fn accounts_in_folder(
    store: &FolderStore,
    platform: &str,
    folder_name: &str,
) -> Result<HashSet<String>, String> {
    let matches: Vec<&FolderInfo> = store
        .folders
        .iter()
        .filter(|f| f.platform == platform && f.name.eq_ignore_ascii_case(folder_name))
        .collect();

    match matches.len() {
        0 => {
            let available: Vec<&str> = store
                .folders
                .iter()
                .filter(|f| f.platform == platform)
                .map(|f| f.name.as_str())
                .collect();
            if available.is_empty() {
                Err(format!("No folders configured for platform {platform}."))
            } else {
                Err(format!(
                    "Folder '{folder_name}' not found on {platform}. Available: {}",
                    available.join(", ")
                ))
            }
        }
        1 => {
            let mut out = HashSet::new();
            collect(store, &matches[0].id, &mut out, &mut HashSet::new());
            Ok(out)
        }
        _ => Err(format!(
            "Folder name '{folder_name}' is ambiguous on {platform} ({} matches). Rename in the GUI or use the JSON output to disambiguate.",
            matches.len()
        )),
    }
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
            "folder" => {
                collect(store, &item.id, out, visited);
            }
            _ => {}
        }
    }
}
