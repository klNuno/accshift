use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::vdf::{parse_vdf, set_persona_state};
use crate::error::AppError;
use crate::fs_utils;
use crate::os;

const MAX_KILL_WAIT_MS: u64 = 5000;
const KILL_POLL_INTERVAL_MS: u64 = 500;
const POST_KILL_SETTLE_MS: u64 = 750;
const NON_GAME_APP_IDS: &[&str] = &[
    "7",   // Steam client internals
    "760", // Steam community / screenshots
];

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SteamAccount {
    pub steam_id: String,
    pub account_name: String,
    pub persona_name: String,
    pub last_login_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CopyableGame {
    pub app_id: String,
    pub name: String,
}

struct ParsedLoginUser {
    steam_id: String,
    account_name: String,
    persona_name: String,
    last_login_at: Option<u64>,
    is_most_recent: bool,
}

pub(crate) fn steam_id_to_account_id(steam_id64: &str) -> Option<u32> {
    let id: u64 = steam_id64.parse().ok()?;
    Some((id & 0xFFFFFFFF) as u32)
}

fn is_steam_running() -> bool {
    os::is_process_running(os::steam_process_name())
}

fn wait_for_process_exit(process_name: &str) -> Result<(), AppError> {
    if !os::is_process_running(process_name) {
        return Ok(());
    }
    let max_polls = MAX_KILL_WAIT_MS / KILL_POLL_INTERVAL_MS;
    for _ in 0..max_polls {
        std::thread::sleep(std::time::Duration::from_millis(KILL_POLL_INTERVAL_MS));
        if !os::is_process_running(process_name) {
            return Ok(());
        }
    }
    Err(AppError::KillSteamTimeout)
}

fn kill_process_tree_if_running(process_name: &str) -> Result<(), AppError> {
    if !os::is_process_running(process_name) {
        return Ok(());
    }
    if let Err(AppError::SteamElevated) = os::kill_process(process_name) {
        return Err(AppError::SteamElevated);
    }
    wait_for_process_exit(process_name)
}

fn kill_steam() -> Result<(), AppError> {
    let steam_running = is_steam_running();
    let web_helper_running = os::is_process_running(os::steam_web_helper_process_name());
    if !steam_running && !web_helper_running {
        return Ok(());
    }

    kill_steam_client_processes()?;
    std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
    Ok(())
}

fn kill_and_relaunch(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &str,
) -> Result<(), AppError> {
    let needs_kill =
        is_steam_running() || os::is_process_running(os::steam_web_helper_process_name());

    if !needs_kill {
        return launch_steam(steam_path, run_as_admin, launch_options);
    }

    match kill_steam_client_processes() {
        Ok(()) => {
            std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
            launch_steam(steam_path, run_as_admin, launch_options)
        }
        Err(AppError::SteamElevated) if run_as_admin => {
            let args = parse_launch_options(launch_options);
            os::kill_and_relaunch_steam_elevated(steam_path, &args)
        }
        Err(e) => Err(e),
    }
}

fn restore_auto_login_user(previous_username: &str) -> Result<(), AppError> {
    if previous_username.trim().is_empty() {
        os::clear_auto_login_user()
    } else {
        os::set_auto_login_user(previous_username)
    }
}

fn with_auto_login_user<T>(
    next_username: Option<&str>,
    action: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let previous_username = os::get_auto_login_user()?;
    match next_username {
        Some(username) => os::set_auto_login_user(username)?,
        None => os::clear_auto_login_user()?,
    }

    match action() {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = restore_auto_login_user(&previous_username);
            Err(error)
        }
    }
}

fn kill_steam_client_processes() -> Result<(), AppError> {
    kill_process_tree_if_running(os::steam_process_name())?;
    kill_process_tree_if_running(os::steam_web_helper_process_name())
}

fn parse_launch_options(launch_options: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes: Option<char> = None;
    let mut escaped = false;

    for ch in launch_options.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quotes != Some('\'') => {
                escaped = true;
            }
            '"' | '\'' => match in_quotes {
                Some(quote) if quote == ch => in_quotes = None,
                Some(_) => current.push(ch),
                None => in_quotes = Some(ch),
            },
            c if c.is_whitespace() && in_quotes.is_none() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        args.push(current);
    }

    args
}

fn launch_steam(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &str,
) -> Result<(), AppError> {
    let args = parse_launch_options(launch_options);
    os::launch_steam(steam_path, run_as_admin, &args)
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

fn extract_manifest_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('"').collect();
        if parts.len() >= 4 && parts[1].eq_ignore_ascii_case(key) {
            return Some(parts[3].to_string());
        }
    }
    None
}

fn unescape_vdf_path(input: &str) -> String {
    input.replace("\\\\", "\\")
}

fn load_library_paths(steam_path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![steam_path.to_path_buf()];
    let libraryfolders_path = steam_path.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = fs::read_to_string(libraryfolders_path) else {
        return paths;
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('"').collect();
        if parts.len() >= 4 && parts[1].eq_ignore_ascii_case("path") {
            let raw = parts[3].trim();
            if !raw.is_empty() {
                paths.push(PathBuf::from(unescape_vdf_path(raw)));
            }
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
            let app_id = extract_manifest_value(&content, "appid");
            let name = extract_manifest_value(&content, "name");
            if let (Some(app_id), Some(name)) = (app_id, name) {
                names.entry(app_id).or_insert(name);
            }
        }
    }
    names
}

pub fn get_current_account_name(steam_path: &Path) -> Result<String, AppError> {
    let users = parse_login_users(steam_path)?;
    let mut fallback: Option<(u64, String)> = None;

    for user in users {
        let account_name = user.account_name;
        if account_name.is_empty() {
            continue;
        }

        if user.is_most_recent {
            return Ok(account_name);
        }

        let timestamp = user.last_login_at.unwrap_or(0);

        match &fallback {
            Some((prev_ts, _)) if *prev_ts >= timestamp => {}
            _ => fallback = Some((timestamp, account_name)),
        }
    }

    Ok(fallback.map(|(_, name)| name).unwrap_or_default())
}

pub fn get_accounts(steam_path: &Path) -> Result<Vec<SteamAccount>, AppError> {
    let (accounts, _) = get_accounts_snapshot(steam_path)?;
    Ok(accounts)
}

pub fn get_accounts_snapshot(steam_path: &Path) -> Result<(Vec<SteamAccount>, String), AppError> {
    let users = parse_login_users(steam_path)?;

    let mut current_account = String::new();
    let mut fallback: Option<(u64, String)> = None;
    let mut accounts: Vec<SteamAccount> = Vec::with_capacity(users.len());

    for user in users {
        if !user.account_name.is_empty() {
            if user.is_most_recent {
                current_account = user.account_name.clone();
            } else {
                let timestamp = user.last_login_at.unwrap_or(0);
                match &fallback {
                    Some((prev_ts, _)) if *prev_ts >= timestamp => {}
                    _ => fallback = Some((timestamp, user.account_name.clone())),
                }
            }
        }

        accounts.push(SteamAccount {
            steam_id: user.steam_id,
            account_name: user.account_name,
            persona_name: user.persona_name,
            last_login_at: user.last_login_at,
        });
    }

    if current_account.is_empty() {
        current_account = fallback.map(|(_, name)| name).unwrap_or_default();
    }

    accounts.sort_by(|a, b| a.account_name.cmp(&b.account_name));
    Ok((accounts, current_account))
}

fn parse_login_users(steam_path: &Path) -> Result<Vec<ParsedLoginUser>, AppError> {
    let loginusers_path = steam_path.join("config").join("loginusers.vdf");
    if !loginusers_path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(&loginusers_path).map_err(|e| AppError::FileRead(e.to_string()))?;

    let parsed = parse_vdf(&content);

    let users: Vec<ParsedLoginUser> = parsed
        .into_iter()
        .map(|(steam_id, data)| ParsedLoginUser {
            steam_id,
            account_name: data.get("accountname").cloned().unwrap_or_default(),
            persona_name: data.get("personaname").cloned().unwrap_or_default(),
            last_login_at: data.get("timestamp").and_then(|ts| ts.parse::<u64>().ok()),
            is_most_recent: data.get("mostrecent").map(|v| v == "1").unwrap_or(false),
        })
        .collect();

    Ok(users)
}

fn remove_loginuser_entry(content: &str, steam_id: &str) -> (String, bool) {
    let mut out: Vec<&str> = Vec::new();
    let mut depth: i32 = 0;
    let mut skipping = false;
    let mut skip_depth: i32 = 0;
    let mut removed = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if skipping {
            if trimmed == "{" {
                depth += 1;
                continue;
            }
            if trimmed == "}" {
                depth -= 1;
                if depth == skip_depth {
                    skipping = false;
                }
                continue;
            }
            continue;
        }

        let parts: Vec<&str> = trimmed.split('"').collect();
        if depth == 1 && parts.len() >= 2 && parts[1] == steam_id {
            skipping = true;
            skip_depth = depth;
            removed = true;
            continue;
        }

        out.push(line);
        if trimmed == "{" {
            depth += 1;
        } else if trimmed == "}" {
            depth -= 1;
        }
    }

    let mut rebuilt = out.join("\n");
    if content.ends_with('\n') {
        rebuilt.push('\n');
    }

    (rebuilt, removed)
}

pub fn switch_account(
    steam_path: &Path,
    username: &str,
    run_as_admin: bool,
    launch_options: &str,
) -> Result<(), AppError> {
    with_auto_login_user(Some(username), || {
        kill_and_relaunch(steam_path, run_as_admin, launch_options)
    })
}

pub fn add_account(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &str,
) -> Result<(), AppError> {
    with_auto_login_user(None, || {
        kill_and_relaunch(steam_path, run_as_admin, launch_options)
    })
}

pub fn forget_account(steam_path: &Path, steam_id: &str) -> Result<(), AppError> {
    kill_steam()?;

    // Remove account entry from loginusers.vdf.
    let loginusers_path = steam_path.join("config").join("loginusers.vdf");
    if loginusers_path.exists() {
        let content =
            fs::read_to_string(&loginusers_path).map_err(|e| AppError::FileRead(e.to_string()))?;
        let (updated, removed) = remove_loginuser_entry(&content, steam_id);
        if removed {
            fs::write(&loginusers_path, updated).map_err(|e| AppError::FileRead(e.to_string()))?;
        }
    }

    Ok(())
}

pub fn switch_account_mode(
    steam_path: &Path,
    username: &str,
    steam_id: &str,
    mode: &str,
    run_as_admin: bool,
    launch_options: &str,
) -> Result<(), AppError> {
    with_auto_login_user(Some(username), || {
        if let Some(account_id) = steam_id_to_account_id(steam_id) {
            let state = match mode {
                "invisible" => "7",
                _ => "1",
            };
            set_persona_state(steam_path, account_id, state);
        }

        kill_and_relaunch(steam_path, run_as_admin, launch_options)
    })
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
    if !app_id.chars().all(|c| c.is_ascii_digit()) || app_id.is_empty() {
        return Err(AppError::FileRead("Invalid app id".into()));
    }

    let from_root = steam_user_data_path(steam_path, from_steam_id)?;
    let to_root = steam_user_data_path(steam_path, to_steam_id)?;
    let source = from_root.join(app_id);
    let target = to_root.join(app_id);

    if !source.exists() {
        return Err(AppError::UserdataNotFound(source.display().to_string()));
    }

    if target.exists() {
        fs::remove_dir_all(&target).map_err(|e| AppError::FileRead(e.to_string()))?;
    }

    fs_utils::copy_dir_recursive(&source, &target, &[]).map_err(AppError::FileRead)
}

pub fn get_copyable_games(
    steam_path: &Path,
    from_steam_id: &str,
    _to_steam_id: &str,
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

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

pub fn clear_integrated_browser_cache() -> Result<(), AppError> {
    kill_steam_client_processes()?;

    let local_app_data = std::env::var("LOCALAPPDATA")
        .map_err(|e| AppError::FileRead(format!("Could not resolve LOCALAPPDATA: {e}")))?;
    let htmlcache_path = PathBuf::from(local_app_data)
        .join("Steam")
        .join("htmlcache");

    if htmlcache_path.exists() {
        fs::remove_dir_all(&htmlcache_path).map_err(|e| AppError::FileRead(e.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_launch_options;

    #[test]
    fn parse_launch_options_keeps_quoted_groups() {
        let args = parse_launch_options("-silent -applaunch 730 \"-novid -fullscreen\"");
        assert_eq!(
            args,
            vec!["-silent", "-applaunch", "730", "-novid -fullscreen"]
        );
    }

    #[test]
    fn parse_launch_options_handles_single_quotes() {
        let args = parse_launch_options("-foo 'bar baz' -qux");
        assert_eq!(args, vec!["-foo", "bar baz", "-qux"]);
    }

    #[test]
    fn parse_launch_options_handles_escaped_spaces() {
        let args = parse_launch_options("-foo bar\\ baz");
        assert_eq!(args, vec!["-foo", "bar baz"]);
    }
}
