use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "windows"))]
use super::vdf::vdf_set_nested_value;
use super::vdf::{parse_vdf, set_persona_state};
use crate::error::AppError;
use crate::fs_utils;
use crate::os;

const KILL_WAIT_MS: u32 = 5000;
// Long enough to cover Snap on Linux: the `steam` wrapper takes 3-4s to boot
// before it even forwards -shutdown, then teardown itself takes several more.
// Windows and macOS exit in 2-5s; the wait stops as soon as they do.
const GRACEFUL_SHUTDOWN_WAIT_MS: u32 = 12_000;
pub(crate) const NON_GAME_APP_IDS: &[&str] = &[
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
    if os::wait_for_process_exit(process_name, KILL_WAIT_MS) {
        Ok(())
    } else {
        Err(AppError::KillSteamTimeout)
    }
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

fn try_graceful_shutdown(steam_path: &Path) -> bool {
    // Only wait when the shutdown request was actually delivered — otherwise
    // we would burn the full timeout before falling back to a kill.
    if !os::request_steam_shutdown(steam_path) {
        return false;
    }
    os::wait_for_process_exit(os::steam_process_name(), GRACEFUL_SHUTDOWN_WAIT_MS)
        && os::wait_for_process_exit(os::steam_web_helper_process_name(), 2000)
}

pub(super) enum StopOutcome {
    NotRunning,
    Stopped,
    NeedsElevation,
}

pub(super) fn stop_steam(steam_path: &Path, force_kill: bool) -> Result<StopOutcome, AppError> {
    let needs_kill =
        is_steam_running() || os::is_process_running(os::steam_web_helper_process_name());
    if !needs_kill {
        return Ok(StopOutcome::NotRunning);
    }

    if force_kill {
        return match kill_steam_client_processes() {
            Ok(()) => Ok(StopOutcome::Stopped),
            Err(AppError::SteamElevated) => Ok(StopOutcome::NeedsElevation),
            Err(e) => Err(e),
        };
    }

    if try_graceful_shutdown(steam_path) {
        return Ok(StopOutcome::Stopped);
    }

    match kill_steam_client_processes() {
        Ok(()) => Ok(StopOutcome::Stopped),
        Err(AppError::SteamElevated) => Ok(StopOutcome::NeedsElevation),
        Err(e) => Err(e),
    }
}

fn write_auto_login(next_username: Option<&str>) -> Result<(), AppError> {
    match next_username {
        Some(username) => os::set_auto_login_user(username),
        None => os::clear_auto_login_user(),
    }
}

// Flip the per-user AllowAutoLogin / MostRecent flags inside loginusers.vdf.
//
// On Linux and macOS, Steam treats `AutoLoginUser=""` in registry.vdf as a
// hint, not a hard rule — if any user in loginusers.vdf still has
// `AllowAutoLogin=1` and `MostRecent=1`, Steam silently logs them in at
// launch. Windows respects the registry value strictly so this step is a
// no-op there.
//
// `target` = Some(account_name) → only that user gets the flags set to 1.
// `target` = None               → all users get the flags cleared.
#[cfg(not(target_os = "windows"))]
fn set_login_user_flags(steam_path: &Path, target: Option<&str>) -> Result<(), AppError> {
    let path = steam_path.join("config").join("loginusers.vdf");
    if !path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::FileRead(e.to_string()))?;
    let users = parse_vdf(&content);
    if users.is_empty() {
        return Ok(());
    }

    let mut updated = content;
    for (steam_id, fields) in &users {
        let account_name = fields.get("accountname").cloned().unwrap_or_default();
        let is_target = target
            .map(|t| account_name == t && !account_name.is_empty())
            .unwrap_or(false);
        let flag = if is_target { "1" } else { "0" };
        updated = vdf_set_nested_value(&updated, &[steam_id.as_str(), "AllowAutoLogin"], flag);
        updated = vdf_set_nested_value(&updated, &[steam_id.as_str(), "MostRecent"], flag);
    }

    crate::storage::write_bytes_atomic(&path, updated.as_bytes()).map_err(AppError::FileRead)
}

#[cfg(target_os = "windows")]
fn set_login_user_flags(_steam_path: &Path, _target: Option<&str>) -> Result<(), AppError> {
    Ok(())
}

fn restore_auto_login_user(previous_username: &str) -> Result<(), AppError> {
    if previous_username.trim().is_empty() {
        os::clear_auto_login_user()
    } else {
        os::set_auto_login_user(previous_username)
    }
}

// Switch the Steam autologin and relaunch Steam.
//
// Steam on Linux/macOS owns `registry.vdf` in memory and rewrites it at
// shutdown, so the autologin write MUST happen *after* Steam stops. On
// Windows the autologin lives in HKCU (OS-scope) so the order does not
// matter, but the same flow works.
//
// `pre_launch` runs after Steam is stopped and after the autologin is
// written, so it can safely touch other Steam-owned files (e.g. persona
// state).
fn switch_autologin_and_relaunch(
    steam_path: &Path,
    next_username: Option<&str>,
    run_as_admin: bool,
    launch_options: &str,
    extra_args: &[&str],
    force_kill: bool,
    pre_launch: impl FnOnce(),
) -> Result<(), AppError> {
    let previous = os::get_auto_login_user()?;

    match stop_steam(steam_path, force_kill)? {
        StopOutcome::NeedsElevation if run_as_admin => {
            // Windows-only path: elevated combined kill+relaunch via UAC.
            // The HKCU registry write is user-scope, so writing now (before
            // the elevated kill) is safe — Steam does not own it.
            write_auto_login(next_username)?;
            let _ = set_login_user_flags(steam_path, next_username);
            pre_launch();
            let mut args = parse_launch_options(launch_options);
            args.extend(extra_args.iter().map(|s| s.to_string()));
            return os::kill_and_relaunch_steam_elevated(steam_path, &args);
        }
        StopOutcome::NeedsElevation => return Err(AppError::SteamElevated),
        StopOutcome::NotRunning | StopOutcome::Stopped => {}
    }

    if let Err(e) = write_auto_login(next_username) {
        let _ = launch_steam(steam_path, run_as_admin, launch_options, extra_args);
        return Err(e);
    }

    // Linux/macOS: also strip AllowAutoLogin/MostRecent in loginusers.vdf,
    // otherwise Steam re-logs the most-recent user even with an empty
    // AutoLoginUser. No-op on Windows.
    let _ = set_login_user_flags(steam_path, next_username);

    pre_launch();

    if let Err(e) = launch_steam(steam_path, run_as_admin, launch_options, extra_args) {
        let _ = restore_auto_login_user(&previous);
        // Also revert the loginusers.vdf flags flipped above, so registry and
        // VDF stay consistent on the failure path.
        let previous_target = (!previous.trim().is_empty()).then_some(previous.as_str());
        let _ = set_login_user_flags(steam_path, previous_target);
        return Err(e);
    }

    Ok(())
}

fn kill_steam_client_processes() -> Result<(), AppError> {
    let steam_name = os::steam_process_name();
    let helper_name = os::steam_web_helper_process_name();

    std::thread::scope(|s| {
        let steam_handle = s.spawn(|| kill_process_tree_if_running(steam_name));
        let helper_result = kill_process_tree_if_running(helper_name);

        let steam_result = steam_handle
            .join()
            .map_err(|_| AppError::KillSteamTimeout)?;
        steam_result?;
        helper_result
    })
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
    extra_args: &[&str],
) -> Result<(), AppError> {
    let mut args = parse_launch_options(launch_options);
    args.extend(extra_args.iter().map(|s| s.to_string()));
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
    force_kill: bool,
) -> Result<(), AppError> {
    switch_autologin_and_relaunch(
        steam_path,
        Some(username),
        run_as_admin,
        launch_options,
        &[],
        force_kill,
        || {},
    )
}

pub fn switch_account_and_launch_game(
    steam_path: &Path,
    username: &str,
    app_id: &str,
    run_as_admin: bool,
    launch_options: &str,
    force_kill: bool,
) -> Result<(), AppError> {
    if app_id.is_empty() || !app_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::FileRead("Invalid app id".into()));
    }
    switch_autologin_and_relaunch(
        steam_path,
        Some(username),
        run_as_admin,
        launch_options,
        &["-applaunch", app_id],
        force_kill,
        || {},
    )
}

pub fn add_account(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &str,
    force_kill: bool,
) -> Result<(), AppError> {
    switch_autologin_and_relaunch(
        steam_path,
        None,
        run_as_admin,
        launch_options,
        &[],
        force_kill,
        || {},
    )
}

pub fn forget_account(steam_path: &Path, steam_id: &str) -> Result<(), AppError> {
    // Remove account entry from loginusers.vdf.
    let loginusers_path = steam_path.join("config").join("loginusers.vdf");
    if loginusers_path.exists() {
        let content =
            fs::read_to_string(&loginusers_path).map_err(|e| AppError::FileRead(e.to_string()))?;
        let (updated, removed) = remove_loginuser_entry(&content, steam_id);
        if removed {
            // Steam keeps loginusers.vdf in memory and rewrites it on exit —
            // editing it while Steam runs silently resurrects the entry. Stop
            // Steam first (graceful, then kill); it stays closed afterwards.
            match stop_steam(steam_path, false)? {
                StopOutcome::NeedsElevation => return Err(AppError::SteamElevated),
                StopOutcome::NotRunning | StopOutcome::Stopped => {}
            }
            crate::storage::write_bytes_atomic(&loginusers_path, updated.as_bytes())
                .map_err(AppError::FileRead)?;
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
    force_kill: bool,
) -> Result<(), AppError> {
    let account_id = steam_id_to_account_id(steam_id);
    let state = match mode {
        "invisible" => "7",
        _ => "1",
    };

    let mut persona_result: Result<(), AppError> = Ok(());
    switch_autologin_and_relaunch(
        steam_path,
        Some(username),
        run_as_admin,
        launch_options,
        &[],
        force_kill,
        || {
            if let Some(account_id) = account_id {
                persona_result = set_persona_state(steam_path, account_id, state);
            }
        },
    )?;
    // The switch itself succeeded; still surface a persona write failure so
    // the user knows the requested mode did not apply.
    persona_result
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

    // Stage the copy next to the target, then swap. A failure mid-copy must
    // not leave the destination account half-deleted.
    let staging = to_root.join(format!(".{app_id}.copy-staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| AppError::FileRead(e.to_string()))?;
    }
    fs_utils::copy_dir_recursive(&source, &staging, &[]).map_err(AppError::FileRead)?;

    let backup = to_root.join(format!(".{app_id}.copy-backup"));
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|e| AppError::FileRead(e.to_string()))?;
    }
    let had_target = target.exists();
    if had_target {
        fs::rename(&target, &backup).map_err(|e| AppError::FileRead(e.to_string()))?;
    }
    match fs::rename(&staging, &target) {
        Ok(()) => {
            if had_target {
                let _ = fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(e) => {
            if had_target {
                let _ = fs::rename(&backup, &target);
            }
            let _ = fs::remove_dir_all(&staging);
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

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

pub fn clear_integrated_browser_cache() -> Result<(), AppError> {
    kill_steam_client_processes()?;

    let htmlcache_path = os::steam_htmlcache_path()?;
    if htmlcache_path.exists() {
        fs::remove_dir_all(&htmlcache_path).map_err(|e| AppError::FileRead(e.to_string()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_launch_options, steam_id_to_account_id};

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

    // -----------------------------------------------------------------------
    // steam_id_to_account_id
    // -----------------------------------------------------------------------

    #[test]
    fn steam_id_to_account_id_known_value() {
        // SteamID64 76561197960265729 -> low 32 bits = 1 (Gabe Newell's account)
        assert_eq!(steam_id_to_account_id("76561197960265729"), Some(1));
    }

    #[test]
    fn steam_id_to_account_id_another_known_value() {
        // 76561197960265728 is the base (0x0110000100000000), low 32 bits = 0
        assert_eq!(steam_id_to_account_id("76561197960265728"), Some(0));
    }

    #[test]
    fn steam_id_to_account_id_large_account_id() {
        // 76561198000000000 -> low 32 bits: 0x0110000100000000 subtracted from base
        // 76561198000000000 = 0x01100001_025317C0, low 32 = 0x025317C0 = 39_734_272
        assert_eq!(
            steam_id_to_account_id("76561198000000000"),
            Some(39_734_272)
        );
    }

    #[test]
    fn steam_id_to_account_id_empty_string() {
        assert_eq!(steam_id_to_account_id(""), None);
    }

    #[test]
    fn steam_id_to_account_id_non_numeric() {
        assert_eq!(steam_id_to_account_id("not_a_number"), None);
    }

    #[test]
    fn steam_id_to_account_id_alphabetic_mixed() {
        assert_eq!(steam_id_to_account_id("7656abc"), None);
    }

    #[test]
    fn steam_id_to_account_id_zero() {
        assert_eq!(steam_id_to_account_id("0"), Some(0));
    }

    #[test]
    fn steam_id_to_account_id_max_u32_low_bits() {
        // 4294967295 = 0xFFFFFFFF, low 32 bits = u32::MAX
        assert_eq!(steam_id_to_account_id("4294967295"), Some(u32::MAX));
    }
}
