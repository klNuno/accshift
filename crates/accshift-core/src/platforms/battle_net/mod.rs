use crate::config::{self, AppConfig, BattleNetAccountConfig};
use crate::error::PlatformError;
use crate::platforms::{log_platform_error, log_platform_info, PlatformService, SetupStatus};
use crate::{AppContext, AppCtx};
#[cfg(windows)]
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;
#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
#[cfg(windows)]
use winreg::{RegKey, HKEY};

mod accounts;
mod launcher;
mod saved_accounts;
mod setup;
mod tag_cache;

#[allow(unused_imports)]
use self::accounts::*;
#[allow(unused_imports)]
use self::launcher::*;
#[allow(unused_imports)]
use self::saved_accounts::*;
#[allow(unused_imports)]
use self::tag_cache::*;

pub use self::setup::{begin_account_setup, cancel_account_setup, get_account_setup_status};

// The Windows launcher runs as two processes; the macOS client is a single
// Mach-O named `Battle.net`.
#[cfg(windows)]
const BATTLE_NET_PROCESS_NAMES: &[&str] = &["Battle.net.exe", "Battle.net Launcher.exe"];
#[cfg(target_os = "macos")]
const BATTLE_NET_PROCESS_NAMES: &[&str] = &["Battle.net"];
#[cfg(windows)]
const BATTLE_NET_EXECUTABLE_CANDIDATES: &[&str] = &[
    "Battle.net\\Battle.net Launcher.exe",
    "Battle.net\\Battle.net.exe",
];
#[cfg(windows)]
const BATTLE_NET_EXECUTABLE_NAMES: &[&str] = &["Battle.net Launcher.exe", "Battle.net.exe"];
const BATTLE_NET_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BattleNetAccount {
    pub email: String,
    pub battle_tag: String,
    pub last_login_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleNetStartupSnapshot {
    pub accounts: Vec<BattleNetAccount>,
    pub current_account: String,
}

pub fn get_accounts(app_handle: AppCtx) -> Result<Vec<BattleNetAccount>, String> {
    read_accounts(&app_handle)
}

pub fn get_startup_snapshot(app_handle: AppCtx) -> Result<BattleNetStartupSnapshot, String> {
    let accounts = read_accounts(&app_handle)?;
    Ok(BattleNetStartupSnapshot {
        current_account: current_account(&accounts),
        accounts,
    })
}

pub fn get_current_account() -> Result<String, String> {
    Ok(read_saved_accounts()?
        .into_iter()
        .next()
        .unwrap_or_default())
}

pub fn switch_account(app_handle: AppCtx, email: String) -> Result<(), String> {
    let target_email = validate_account_email(&email)?;
    log_platform_info(
        &app_handle,
        "battle_net.switch_account",
        "Battle.net switch requested",
        build_battle_net_switch_details(Some(&target_email)),
    );
    let accounts = known_account_emails(&app_handle)?;

    let Some(target) = accounts
        .iter()
        .find(|account| normalize_account_key(account) == normalize_account_key(&target_email))
        .cloned()
    else {
        return Err("Battle.net account not found".into());
    };

    // Refuse an unknown target before quitting. Once the launcher is down it
    // rewrites SavedAccountNames, so the list we write is that flush, not the
    // one read above. A failed step after the quit still brings it back.
    kill_battle_net()?;
    let fresh = relaunch_if_err(&app_handle, read_saved_accounts())?;
    let reordered = order_saved_accounts_for_switch(&fresh, &target);
    relaunch_if_err(&app_handle, write_saved_accounts(&app_handle, &reordered))?;
    relaunch_if_err(
        &app_handle,
        remember_account_usage(&app_handle, &target, false),
    )?;
    let result = launch_battle_net(&app_handle);

    let post_switch_details = build_battle_net_switch_details(Some(&target));
    match &result {
        Ok(()) => log_platform_info(
            &app_handle,
            "battle_net.switch_account",
            "Battle.net switch completed",
            post_switch_details,
        ),
        Err(error) => log_platform_error(
            &app_handle,
            "battle_net.switch_account",
            "Battle.net switch failed",
            format!("error={error}; state={post_switch_details}"),
        ),
    }

    result
}

pub fn forget_account(app_handle: AppCtx, email: String) -> Result<(), String> {
    let target_email = validate_account_email(&email)?;

    // Same exit flush as a switch: filter the list the launcher wrote on the
    // way down. A failed write or metadata update relaunches it. Success
    // leaves it closed, which is what a forget did before.
    kill_battle_net()?;
    let fresh = relaunch_if_err(&app_handle, read_saved_accounts())?;
    let filtered = saved_accounts_without(&fresh, &target_email);
    relaunch_if_err(&app_handle, write_saved_accounts(&app_handle, &filtered))?;
    relaunch_if_err(
        &app_handle,
        forget_account_metadata(&app_handle, &target_email),
    )
}

/// After `kill_battle_net` succeeded, a later error must not leave the client
/// closed. The error itself is returned unchanged.
fn relaunch_if_err<T>(app_handle: &dyn AppContext, result: Result<T, String>) -> Result<T, String> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = launch_battle_net(app_handle);
            Err(error)
        }
    }
}

pub fn get_battle_net_path(app_handle: AppCtx) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    if !cfg.battle_net.path_override.trim().is_empty() {
        return Ok(cfg.battle_net.path_override);
    }
    resolve_battle_net_executable(&app_handle).map(|path| path.to_string_lossy().to_string())
}

pub fn set_battle_net_path(app_handle: AppCtx, path: String) -> Result<(), String> {
    config::update_config(&app_handle, |cfg| {
        cfg.battle_net.path_override = path.trim().to_string();
    })
}

pub fn select_battle_net_path() -> Result<String, String> {
    crate::os::select_file(
        "Select Battle.net executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

pub struct BattleNetService;

pub static BATTLE_NET_SERVICE: BattleNetService = BattleNetService;

impl PlatformService for BattleNetService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let accounts = get_accounts(app.clone())?;
        serde_json::to_value(accounts).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let snapshot = get_startup_snapshot(app.clone())?;
        serde_json::to_value(snapshot).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_current_account(&self, _app: AppCtx) -> Result<String, PlatformError> {
        get_current_account().map_err(Into::into)
    }

    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        _params: Value,
    ) -> Result<(), PlatformError> {
        switch_account(app.clone(), account_id.to_string()).map_err(Into::into)
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError> {
        forget_account(app.clone(), account_id.to_string()).map_err(Into::into)
    }

    fn begin_setup(&self, app: AppCtx, _params: Value) -> Result<SetupStatus, PlatformError> {
        begin_account_setup(app.clone()).map_err(Into::into)
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError> {
        get_account_setup_status(app.clone(), setup_id.to_string()).map_err(Into::into)
    }

    fn cancel_setup(&self, app: AppCtx, setup_id: &str) -> Result<(), PlatformError> {
        cancel_account_setup(app.clone(), setup_id.to_string()).map_err(Into::into)
    }

    fn get_path(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_battle_net_path(app.clone()).map_err(Into::into)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), PlatformError> {
        set_battle_net_path(app.clone(), path.to_string()).map_err(Into::into)
    }

    fn select_path(&self) -> Result<String, PlatformError> {
        select_battle_net_path().map_err(Into::into)
    }
}

#[cfg(test)]
mod listing_tests;
#[cfg(test)]
mod tests;
