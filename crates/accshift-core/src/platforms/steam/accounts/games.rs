//! An account's userdata folder and the games it holds: listing them with
//! their names, and copying one game's settings to another account.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::super::vdf::vdf_tokenize_line;
use super::{is_steam_running, steam_id_to_account_id};
use crate::error::AppError;
use crate::fs_utils;
use crate::os;

pub(crate) const NON_GAME_APP_IDS: &[&str] = &[
    "7",   // Steam client internals
    "760", // Steam community / screenshots
];

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CopyableGame {
    pub app_id: String,
    pub name: String,
}

pub(crate) fn steam_user_data_path(steam_path: &Path, steam_id: &str) -> Result<PathBuf, AppError> {
    let account_id = steam_id_to_account_id(steam_id).ok_or(AppError::InvalidSteamId)?;
    Ok(steam_path.join("userdata").join(account_id.to_string()))
}

pub(crate) fn list_account_games(userdata_root: &Path) -> Result<HashSet<String>, AppError> {
    let mut ids = HashSet::new();
    if !userdata_root.exists() {
        return Ok(ids);
    }
    for entry in fs::read_dir(userdata_root).map_err(|e| AppError::FileRead(e.to_string()))? {
        let entry = entry.map_err(|e| AppError::FileRead(e.to_string()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
                ids.insert(name.to_string());
            }
        }
    }
    Ok(ids)
}

/// The `"key" "value"` pair a VDF line holds, if it starts with one.
fn vdf_pair(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('"') {
        return None;
    }
    let mut tokens = vdf_tokenize_line(trimmed).into_iter();
    Some((tokens.next()?, tokens.next()?))
}

// Pulls "appid" and "name" out of an appmanifest in a single pass. Both keys
// sit in the first handful of lines while the file goes on with hundreds of
// InstalledDepots entries, so stop as soon as both are found.
pub(super) fn extract_appid_and_name(content: &str) -> (Option<String>, Option<String>) {
    let mut app_id = None;
    let mut name = None;

    for (key, value) in content.lines().filter_map(vdf_pair) {
        if app_id.is_none() && key.eq_ignore_ascii_case("appid") {
            app_id = Some(value);
        } else if name.is_none() && key.eq_ignore_ascii_case("name") {
            name = Some(value);
        }
        if app_id.is_some() && name.is_some() {
            break;
        }
    }

    (app_id, name)
}

pub(super) fn load_library_paths(steam_path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![steam_path.to_path_buf()];
    let libraryfolders_path = steam_path.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = fs::read_to_string(libraryfolders_path) else {
        return paths;
    };

    for (key, value) in content.lines().filter_map(vdf_pair) {
        let raw = value.trim();
        if key.eq_ignore_ascii_case("path") && !raw.is_empty() {
            paths.push(PathBuf::from(raw));
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn load_app_names(steam_path: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for library_root in load_library_paths(steam_path) {
        let steamapps = if library_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("steamapps"))
            .unwrap_or(false)
        {
            library_root
        } else {
            library_root.join("steamapps")
        };

        let Ok(entries) = fs::read_dir(steamapps) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let (app_id, name) = extract_appid_and_name(&content);
            if let (Some(app_id), Some(name)) = (app_id, name) {
                names.entry(app_id).or_insert(name);
            }
        }
    }
    names
}

pub fn open_userdata_with_path(steam_path: &Path, steam_id: &str) -> Result<(), AppError> {
    let userdata_path = steam_user_data_path(steam_path, steam_id)?;

    if !userdata_path.exists() {
        return Err(AppError::UserdataNotFound(
            userdata_path.display().to_string(),
        ));
    }

    let canonical = userdata_path
        .canonicalize()
        .map_err(|e| AppError::PathResolve(e.to_string()))?;

    os::open_folder(&canonical)
}

pub fn copy_game_settings(
    steam_path: &Path,
    from_steam_id: &str,
    to_steam_id: &str,
    app_id: &str,
) -> Result<(), AppError> {
    copy_game_settings_with(
        steam_path,
        from_steam_id,
        to_steam_id,
        app_id,
        is_steam_running,
    )
}

/// `copy_game_settings` with the Steam probe injected, so the refusal can be
/// tested without a real client.
pub(super) fn copy_game_settings_with(
    steam_path: &Path,
    from_steam_id: &str,
    to_steam_id: &str,
    app_id: &str,
    steam_running: impl FnOnce() -> bool,
) -> Result<(), AppError> {
    if !app_id.chars().all(|c| c.is_ascii_digit()) || app_id.is_empty() {
        return Err(AppError::FileRead("Invalid app id".into()));
    }
    // A running Steam rewrites userdata from memory and Steam Cloud on exit,
    // so a copy made now would report success and then be undone.
    if steam_running() {
        return Err(AppError::FileRead(
            "Steam is running and would overwrite the copied settings. Close Steam, then copy \
             again."
                .into(),
        ));
    }

    let from_root = steam_user_data_path(steam_path, from_steam_id)?;
    let to_root = steam_user_data_path(steam_path, to_steam_id)?;
    let source = from_root.join(app_id);
    let target = to_root.join(app_id);

    if !source.exists() {
        return Err(AppError::UserdataNotFound(source.display().to_string()));
    }

    // Stage the copy next to the target, then swap. A failure mid-copy must
    // not leave the destination account half-deleted.
    let staging = to_root.join(format!(".{app_id}.copy-staging"));
    remove_leftover_dir(&staging)?;
    fs_utils::copy_dir_recursive(&source, &staging, &[]).map_err(AppError::FileRead)?;

    let backup = to_root.join(format!(".{app_id}.copy-backup"));
    remove_leftover_dir(&backup)?;
    swap_in_staged_dir(&staging, &target, &backup)
}

/// Removes what an interrupted copy left behind at `path`.
fn remove_leftover_dir(path: &Path) -> Result<(), AppError> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|e| AppError::FileRead(e.to_string()))?;
    }
    Ok(())
}

/// Moves `staging` to `target`, keeping the previous target as `backup` until
/// the move succeeds and putting it back when it does not.
fn swap_in_staged_dir(staging: &Path, target: &Path, backup: &Path) -> Result<(), AppError> {
    let had_target = target.exists();
    if had_target {
        fs::rename(target, backup).map_err(|e| AppError::FileRead(e.to_string()))?;
    }
    match fs::rename(staging, target) {
        Ok(()) => {
            if had_target {
                let _ = fs::remove_dir_all(backup);
            }
            Ok(())
        }
        Err(e) => {
            if had_target {
                let _ = fs::rename(backup, target);
            }
            let _ = fs::remove_dir_all(staging);
            Err(AppError::FileRead(e.to_string()))
        }
    }
}

pub fn get_copyable_games(
    steam_path: &Path,
    from_steam_id: &str,
) -> Result<Vec<CopyableGame>, AppError> {
    let from_root = steam_user_data_path(steam_path, from_steam_id)?;
    let from_games = list_account_games(&from_root)?;
    let names = load_app_names(steam_path);

    let mut games: Vec<CopyableGame> = from_games
        .iter()
        .filter(|app_id| !NON_GAME_APP_IDS.contains(&app_id.as_str()))
        .filter_map(|app_id| {
            names.get(app_id).map(|name| CopyableGame {
                app_id: app_id.clone(),
                name: name.clone(),
            })
        })
        .collect();

    games.sort_by_key(|g| g.name.to_lowercase());
    Ok(games)
}
