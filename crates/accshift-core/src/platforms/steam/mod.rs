use crate::config;
use crate::error::{PlatformError, PlatformErrorKind};
use crate::os;
use crate::platforms::{
    log_platform_error, log_platform_failure, log_platform_info, PlatformService, SetupStatus,
};
use crate::{AppContext, AppCtx};
pub mod accounts;
pub mod bans;
pub mod bulk_edit;
pub mod cs2_bridge;
pub mod profile;
pub mod switch_params;
pub mod vdf;

mod api_key;
mod install;
mod setup;

#[allow(unused_imports)]
use self::api_key::*;
#[allow(unused_imports)]
use self::install::*;
#[cfg(test)]
use self::setup::*;

pub use self::api_key::{has_api_key, set_api_key};
pub use self::install::{
    classify_steam_folder, get_steam_path, select_steam_path, set_steam_path, SteamFolder,
    STEAM_PATH_NEVER_SIGNED_IN, STEAM_PATH_NOT_A_DIRECTORY, STEAM_PATH_NOT_STEAM,
};
pub use self::setup::{begin_account_setup, cancel_account_setup, get_account_setup_status};

use accounts::{CopyableGame, SteamAccount};
use bans::BanInfo;
use profile::ProfileInfo;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

fn validate_steam_id(id: &str) -> Result<(), String> {
    if id.len() != 17 || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid SteamID64".into());
    }
    Ok(())
}

fn validate_username(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name.len() > 128
        || name.chars().any(|c| c == '\0' || c.is_control())
    {
        return Err("Invalid username".into());
    }
    Ok(())
}

fn is_force_kill(params: &Value) -> bool {
    params
        .get("shutdownMode")
        .and_then(Value::as_str)
        .map(|m| m == "force")
        .unwrap_or(false)
}

pub struct SteamService;

pub static STEAM_SERVICE: SteamService = SteamService;

/// Callers pass the account state already read for their own use (registry
/// autologin value and loginusers.vdf MostRecent, both formatted
/// `<error:...>` on failure) so a switch doesn't re-read them once per log
/// line.
fn build_switch_state_details(
    auto_login_user: &str,
    current_from_file: &str,
    requested_username: Option<&str>,
    steam_id: Option<&str>,
    mode: Option<&str>,
    run_as_admin: bool,
    launch_options: &str,
) -> String {
    // Both booleans come from one process-table scan instead of one per name.
    let client_processes = [
        os::steam_process_name(),
        os::steam_web_helper_process_name(),
    ];
    let running = os::running_process_names(&client_processes);

    use super::redact_id;
    use super::redact_opt;
    serde_json::json!({
        "requestedUsername": redact_opt(requested_username),
        "steamId": redact_opt(steam_id),
        "mode": mode,
        "runAsAdmin": run_as_admin,
        "launchOptionsConfigured": !launch_options.trim().is_empty(),
        "autoLoginUser": redact_id(auto_login_user),
        "currentAccountFromLoginusers": redact_id(current_from_file),
        "steamRunning": running.contains(&os::steam_process_name()),
        "steamWebHelperRunning": running.contains(&os::steam_web_helper_process_name()),
    })
    .to_string()
}

pub fn get_accounts(app_handle: AppCtx) -> Result<Vec<SteamAccount>, PlatformError> {
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_accounts(&steam_path)
        .map_err(|e| log_platform_failure(&app_handle, "steam.get_accounts", e.into()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SteamStartupSnapshot {
    accounts: Vec<SteamAccount>,
    current_account: String,
}

fn get_startup_snapshot_inner(
    app_handle: &dyn AppContext,
) -> Result<SteamStartupSnapshot, PlatformError> {
    let steam_path = resolve_steam_path(app_handle)?;
    let (accounts, current_from_file) = accounts::get_accounts_snapshot(&steam_path)
        .map_err(|e| log_platform_failure(app_handle, "steam.get_startup_snapshot", e.into()))?;
    let current_account = {
        let from_registry = os::get_auto_login_user().unwrap_or_default();
        if from_registry.trim().is_empty() {
            current_from_file
        } else {
            from_registry
        }
    };

    Ok(SteamStartupSnapshot {
        accounts,
        current_account,
    })
}

pub fn get_current_account(app_handle: AppCtx) -> Result<String, PlatformError> {
    let from_registry = os::get_auto_login_user().unwrap_or_default();
    if !from_registry.trim().is_empty() {
        return Ok(from_registry);
    }

    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_current_account_name(&steam_path)
        .map_err(|e| log_platform_failure(&app_handle, "steam.get_current_account", e.into()))
}

pub fn switch_account_and_launch_game(
    app_handle: AppCtx,
    username: String,
    app_id: String,
    run_as_admin: bool,
    launch_options: String,
    shutdown_mode: String,
) -> Result<(), PlatformError> {
    validate_username(&username)?;
    if app_id.is_empty() || !app_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid app id".into());
    }
    let steam_path = resolve_steam_path(&app_handle)?;
    // The pre-switch log line and the already_on_target check below both want
    // loginusers.vdf's MostRecent entry, so it is read once and shared.
    let current_result = accounts::get_current_account_name(&steam_path);
    let auto_login_user = os::get_auto_login_user().unwrap_or_else(|e| format!("<error:{e}>"));
    let current_from_file = match &current_result {
        Ok(current) => current.clone(),
        Err(e) => format!("<error:{e}>"),
    };
    log_platform_info(
        &app_handle,
        "steam.switch_account_and_launch_game",
        "Steam switch+launch requested",
        build_switch_state_details(
            &auto_login_user,
            &current_from_file,
            Some(&username),
            None,
            None,
            run_as_admin,
            &launch_options,
        ),
    );

    // The frontend routes every "run game as X" through this command; whether
    // a switch is actually needed is decided here, against live state, not
    // from cached UI state. loginusers.vdf's MostRecent is written by Steam
    // itself at login, so it tracks the real session more reliably than the
    // autologin registry value (which is just our own last write). When Steam
    // is already running with the target account logged in, a restart would
    // only cost the user their session. Hand the launch to the running
    // client instead.
    let already_on_target = accounts::is_steam_running()
        && matches!(&current_result, Ok(current) if current.eq_ignore_ascii_case(&username));
    if already_on_target {
        return match os::open_url(&format!("steam://rungameid/{app_id}")) {
            Ok(()) => {
                log_platform_info(
                    &app_handle,
                    "steam.switch_account_and_launch_game",
                    "Target account already active; launched game without switching",
                    format!("app_id={app_id}"),
                );
                Ok(())
            }
            Err(e) => Err(log_platform_failure(
                &app_handle,
                "steam.switch_account_and_launch_game",
                e.into(),
            )),
        };
    }

    let force_kill = shutdown_mode == "force";
    let result = accounts::switch_account_and_launch_game(
        &steam_path,
        &username,
        &app_id,
        run_as_admin,
        &launch_options,
        force_kill,
    );

    // The switch just changed exactly this state, so both values are re-read.
    let post_auto_login_user = os::get_auto_login_user().unwrap_or_else(|e| format!("<error:{e}>"));
    let post_current_from_file =
        accounts::get_current_account_name(&steam_path).unwrap_or_else(|e| format!("<error:{e}>"));
    let post_state = build_switch_state_details(
        &post_auto_login_user,
        &post_current_from_file,
        Some(&username),
        None,
        None,
        run_as_admin,
        &launch_options,
    );

    match &result {
        Ok(()) => log_platform_info(
            &app_handle,
            "steam.switch_account_and_launch_game",
            "Steam switch+launch completed",
            &post_state,
        ),
        Err(error) => log_platform_error(
            &app_handle,
            "steam.switch_account_and_launch_game",
            "Steam switch+launch failed",
            format!("error={error}; state={post_state}"),
        ),
    }

    result.map_err(|e| {
        log_platform_failure(
            &app_handle,
            "steam.switch_account_and_launch_game",
            e.into(),
        )
    })
}

pub fn open_userdata(app_handle: AppCtx, steam_id: String) -> Result<(), PlatformError> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::open_userdata_with_path(&steam_path, &steam_id)
        .map_err(|e| log_platform_failure(&app_handle, "steam.open_userdata", e.into()))
}

pub fn clear_integrated_browser_cache(app_handle: AppCtx) -> Result<(), PlatformError> {
    accounts::clear_integrated_browser_cache().map_err(|e| {
        log_platform_failure(
            &app_handle,
            "steam.clear_integrated_browser_cache",
            e.into(),
        )
    })
}

pub fn copy_game_settings(
    app_handle: AppCtx,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), PlatformError> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::copy_game_settings(&steam_path, &from_steam_id, &to_steam_id, &app_id)
        .map_err(|e| log_platform_failure(&app_handle, "steam.copy_game_settings", e.into()))
}

pub fn get_copyable_games(
    app_handle: AppCtx,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<CopyableGame>, PlatformError> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_copyable_games(&steam_path, &from_steam_id)
        .map_err(|e| log_platform_failure(&app_handle, "steam.get_copyable_games", e.into()))
}

pub fn bulk_edit(
    app_handle: AppCtx,
    request: bulk_edit::BulkEditRequest,
) -> Result<bulk_edit::BulkEditResult, PlatformError> {
    for steam_id in &request.steam_ids {
        validate_steam_id(steam_id)?;
    }
    let steam_path = resolve_steam_path(&app_handle)?;
    log_platform_info(
        &app_handle,
        "steam.bulk_edit",
        "Bulk edit requested",
        format!(
            "accounts={} news_popup={:?} dnd={:?} launch_options={}",
            request.steam_ids.len(),
            request.news_popup,
            request.do_not_disturb,
            request.launch_options.len()
        ),
    );
    // Steam keeps localconfig.vdf in memory and rewrites it on exit. Edits
    // made while it runs are silently lost. Stop it first; it stays closed.
    match accounts::stop_steam(&steam_path, false)? {
        accounts::StopOutcome::NeedsElevation => {
            // Maps to ClientRunning: retry works once the elevated Steam exits.
            return Err(crate::error::AppError::SteamElevated.into());
        }
        accounts::StopOutcome::NotRunning | accounts::StopOutcome::Stopped => {}
    }
    let result = bulk_edit::apply_bulk_edit(&steam_path, &request);
    log_platform_info(
        &app_handle,
        "steam.bulk_edit",
        "Bulk edit completed",
        format!(
            "succeeded={} failed={}",
            result.succeeded,
            result.failed.len()
        ),
    );
    Ok(result)
}

pub fn get_account_games(
    app_handle: AppCtx,
    steam_id: String,
) -> Result<Vec<CopyableGame>, PlatformError> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    bulk_edit::get_account_games(&steam_path, &steam_id)
        .map_err(|e| log_platform_failure(&app_handle, "steam.get_account_games", e.into()))
}

pub fn open_steam_api_key_page() -> Result<(), PlatformError> {
    os::open_url("https://steamcommunity.com/dev/apikey").map_err(Into::into)
}

pub async fn get_profile_info(
    steam_id: String,
    client: reqwest::Client,
) -> Result<Option<ProfileInfo>, PlatformError> {
    validate_steam_id(&steam_id)?;
    Ok(profile::fetch_profile_info(&client, &steam_id).await)
}

/// Variante batch de [`get_profile_info`] : une map id -> profil pour tous
/// les comptes en un appel. Avec cle API : GetPlayerSummaries (100 ids par
/// requete) ; sans cle : fallback XML parallele cote Rust.
pub async fn get_profile_infos(
    app_handle: AppCtx,
    steam_ids: Vec<String>,
    client: reqwest::Client,
) -> Result<HashMap<String, ProfileInfo>, PlatformError> {
    let mut seen = HashSet::new();
    let mut unique_steam_ids: Vec<String> = Vec::new();

    for id in steam_ids {
        validate_steam_id(&id)?;
        if seen.insert(id.clone()) {
            unique_steam_ids.push(id);
        }
    }

    if unique_steam_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Une cle illisible n'est pas bloquante : le fallback XML public couvre.
    let api_key = match read_api_key(&app_handle) {
        Ok(value) => value.trim().to_string(),
        Err(e) => {
            log_platform_error(
                &app_handle,
                "steam.get_profile_infos",
                "Failed to read Steam API key",
                e,
            );
            String::new()
        }
    };

    Ok(profile::fetch_profile_infos(&client, &api_key, &unique_steam_ids).await)
}

pub async fn get_player_bans(
    app_handle: AppCtx,
    steam_ids: Vec<String>,
    client: reqwest::Client,
) -> Result<Vec<BanInfo>, PlatformError> {
    let mut seen = HashSet::new();
    let mut unique_steam_ids: Vec<String> = Vec::new();

    for id in steam_ids {
        validate_steam_id(&id)?;
        if seen.insert(id.clone()) {
            unique_steam_ids.push(id);
        }
    }

    let api_key = match read_api_key(&app_handle) {
        Ok(value) => value.trim().to_string(),
        Err(e) => {
            log_platform_error(
                &app_handle,
                "steam.get_player_bans",
                "Failed to read Steam API key",
                e,
            );
            return Ok(vec![]);
        }
    };
    if api_key.is_empty() {
        return Ok(vec![]);
    }
    bans::fetch_player_bans(&client, &api_key, unique_steam_ids)
        .await
        .map_err(Into::into)
}

impl PlatformService for SteamService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let accounts = get_accounts(app.clone())?;
        serde_json::to_value(accounts).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let snapshot = get_startup_snapshot_inner(&app)?;
        serde_json::to_value(snapshot).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_current_account(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_current_account(app.clone())
    }

    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        params: Value,
    ) -> Result<(), PlatformError> {
        validate_username(account_id)?;
        let run_as_admin = params
            .get("runAsAdmin")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let launch_options = params
            .get("launchOptions")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let force_kill = is_force_kill(&params);
        let steam_path = resolve_steam_path(&app)?;

        let auto_login_user = os::get_auto_login_user().unwrap_or_else(|e| format!("<error:{e}>"));
        let current_from_file = accounts::get_current_account_name(&steam_path)
            .unwrap_or_else(|e| format!("<error:{e}>"));
        log_platform_info(
            &app,
            "steam.switch_account",
            "Steam switch requested",
            build_switch_state_details(
                &auto_login_user,
                &current_from_file,
                Some(account_id),
                None,
                None,
                run_as_admin,
                &launch_options,
            ),
        );

        // Optional persona mode (CLI --invisible/--online, GUI context menu).
        // When absent the switch leaves the account's persona state untouched.
        let mode = params.get("mode").and_then(Value::as_str).unwrap_or("");
        if !mode.is_empty() && !["online", "invisible"].contains(&mode) {
            return Err("Invalid mode".into());
        }

        let result = if mode.is_empty() {
            accounts::switch_account(
                &steam_path,
                account_id,
                run_as_admin,
                &launch_options,
                force_kill,
            )
        } else {
            // Persona state lives in userdata/<account_id>/, keyed by steam
            // id. Callers that already know it pass it in params (the GUI
            // context menu does); otherwise resolve it from loginusers.vdf.
            // Unknown id → plain switch. Two entries can share the same
            // account_name (deleted/recreated account, hand-edited VDF); the
            // accounts Vec's relative order for ties comes from HashMap
            // iteration, not something we control, so pick the lowest
            // steam_id deterministically instead of the first match (which
            // would otherwise vary run to run).
            let steam_id = params
                .get("steamId")
                .and_then(Value::as_str)
                .filter(|id| validate_steam_id(id).is_ok())
                .map(str::to_string)
                .or_else(|| {
                    accounts::get_accounts_snapshot(&steam_path)
                        .ok()
                        .and_then(|(accounts, _)| {
                            accounts
                                .into_iter()
                                .filter(|a| a.account_name == account_id)
                                .min_by(|a, b| a.steam_id.cmp(&b.steam_id))
                                .map(|a| a.steam_id)
                        })
                })
                .unwrap_or_default();
            accounts::switch_account_mode(
                &steam_path,
                account_id,
                &steam_id,
                mode,
                run_as_admin,
                &launch_options,
                force_kill,
            )
        }
        .map_err(|e| log_platform_failure(&app, "steam.switch_account", e.into()));

        // The switch just changed exactly this state, so both values are
        // re-read.
        let post_auto_login_user =
            os::get_auto_login_user().unwrap_or_else(|e| format!("<error:{e}>"));
        let post_current_from_file = accounts::get_current_account_name(&steam_path)
            .unwrap_or_else(|e| format!("<error:{e}>"));
        let post_state = build_switch_state_details(
            &post_auto_login_user,
            &post_current_from_file,
            Some(account_id),
            None,
            None,
            run_as_admin,
            &launch_options,
        );

        match &result {
            Ok(()) => log_platform_info(
                &app,
                "steam.switch_account",
                "Steam switch completed",
                &post_state,
            ),
            Err(error) => log_platform_error(
                &app,
                "steam.switch_account",
                "Steam switch failed",
                format!("error={error}; state={post_state}"),
            ),
        }

        result
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError> {
        validate_steam_id(account_id)?;
        let steam_path = resolve_steam_path(&app)?;
        accounts::forget_account(&steam_path, account_id)
            .map_err(|e| log_platform_failure(&app, "steam.forget_account", e.into()))
    }

    fn begin_setup(&self, app: AppCtx, params: Value) -> Result<SetupStatus, PlatformError> {
        let run_as_admin = params
            .get("runAsAdmin")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let launch_options = params
            .get("launchOptions")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let force_kill = is_force_kill(&params);
        begin_account_setup(app.clone(), run_as_admin, launch_options, force_kill)
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError> {
        get_account_setup_status(app.clone(), setup_id.to_string())
    }

    fn cancel_setup(&self, app: AppCtx, setup_id: &str) -> Result<(), PlatformError> {
        cancel_account_setup(app.clone(), setup_id.to_string())
    }

    fn get_path(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_steam_path(app.clone())
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), PlatformError> {
        set_steam_path(app.clone(), path.to_string())
    }

    fn select_path(&self) -> Result<String, PlatformError> {
        select_steam_path()
    }
}

#[cfg(test)]
mod tests;
