use crate::config::{self, RobloxAccountConfig};
use crate::error::PlatformError;
use crate::os::registry;
use crate::platforms::setup_jobs::{SetupJobs, DEFAULT_SETUP_TTL_MS};
use crate::platforms::{log_platform_error, log_platform_info, PlatformService, SetupStatus};
use crate::{AppContext, AppCtx};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;
use uuid::Uuid;
use zeroize::Zeroizing;

mod accounts;
mod api;
mod client;
mod cookie;
mod setup;

#[allow(unused_imports)]
pub use self::accounts::*;
#[allow(unused_imports)]
pub use self::api::*;
#[allow(unused_imports)]
pub use self::client::*;
#[allow(unused_imports)]
pub use self::cookie::*;
#[allow(unused_imports)]
pub use self::setup::*;

const ROBLOX_PROCESS_NAMES: &[&str] = &["RobloxPlayerBeta.exe", "RobloxStudioBeta.exe"];
const ROBLOX_AUTH_RESPONSE_MAX_BYTES: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobloxAccount {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub last_login_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RobloxStartupSnapshot {
    pub accounts: Vec<RobloxAccount>,
    pub current_account: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RobloxProfileInfo {
    pub avatar_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Quick Login job tracking
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct QuickLoginJob {
    code: String,
    private_key: Zeroizing<String>,
}

static SETUP_JOBS: SetupJobs<QuickLoginJob> = SetupJobs::new("Roblox", DEFAULT_SETUP_TTL_MS);

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &dyn AppContext) -> Result<Vec<RobloxAccount>, String> {
    Ok(read_accounts(app_handle))
}

pub fn get_startup_snapshot(app_handle: &dyn AppContext) -> Result<RobloxStartupSnapshot, String> {
    let accounts = read_accounts(app_handle);
    Ok(RobloxStartupSnapshot {
        current_account: String::new(),
        accounts,
    })
}

pub fn switch_account(app_handle: &dyn AppContext, user_id: &str) -> Result<(), String> {
    let accounts = load_account_configs(app_handle);

    // Before overwriting HKCU with the target cookie, capture any cookie that
    // Roblox Studio rotated in place for the currently active account (the one
    // we last switched to, which owns the live HKCU cookie). Otherwise the
    // rotation is lost and that account is silently logged out next time it is
    // loaded. This also covers re-loading the already-active account: its own
    // rotation is persisted first, then decrypted below. Windows-only; the
    // helper is a no-op elsewhere.
    let rotated = accounts
        .iter()
        .max_by_key(|a| a.last_used_at.unwrap_or(0))
        .map(|active| persist_rotated_cookie(app_handle, active))
        .unwrap_or(false);

    // Re-read after the possible rotation persist so we decrypt the freshest
    // stored cookie for the account we are switching to. Nothing was rewritten
    // when the rotation was a no-op, so the vector we already have is current.
    let accounts = if rotated {
        load_account_configs(app_handle)
    } else {
        accounts
    };
    let account = accounts
        .iter()
        .find(|a| a.user_id == user_id)
        .ok_or_else(|| "Roblox account not found".to_string())?;

    let cookie = crate::os::decrypt_secret(&account.cookie_encrypted)
        .map_err(|e| format!("Could not decrypt Roblox cookie: {e}"))?;

    if cookie.trim().is_empty() {
        return Err("Stored Roblox cookie is empty".to_string());
    }

    log_platform_info(
        app_handle,
        "roblox.switch_account",
        "Roblox switch requested",
        format!("userId={}", super::redact_id(user_id)),
    );

    // Get an auth ticket BEFORE killing: the API needs the cookie, not a running process
    let ticket = request_auth_ticket(&cookie)?;

    kill_roblox();
    // Studio reads the session from the registry. The game launch below does
    // not need it, so a failure is reported and the switch goes on.
    if let Err(e) = write_cookie_to_registry(&cookie) {
        log_platform_error(
            app_handle,
            "roblox.switch_account",
            "Could not write the Roblox Studio cookie; Studio keeps the previous account",
            e,
        );
    }

    // Update last_used_at
    let mut accounts = load_account_configs(app_handle);
    if let Some(a) = accounts.iter_mut().find(|a| a.user_id == user_id) {
        a.last_used_at = Some(super::now_unix_ms());
    }
    let _ = save_account_configs(app_handle, &accounts);

    let launch_result = launch_roblox_with_ticket(&ticket);

    log_platform_info(
        app_handle,
        "roblox.switch_account",
        "Roblox switch completed",
        format!(
            "userId={}; launch={}",
            super::redact_id(user_id),
            launch_result.is_ok()
        ),
    );

    launch_result
}

pub fn forget_account(app_handle: &dyn AppContext, user_id: &str) -> Result<(), String> {
    let mut accounts = load_account_configs(app_handle);

    // Drop the keyring entry the encrypted cookie points at before we drop the
    // account record. On Linux / macOS the "ciphertext" is a UUID into the OS
    // keyring, so retaining-and-saving alone leaves the secret orphaned there.
    // Log and continue on failure: a dangling keyring entry must not block the
    // user from removing the account from the app.
    if let Some(account) = accounts.iter().find(|a| a.user_id == user_id) {
        if !account.cookie_encrypted.is_empty() {
            if let Err(e) = crate::os::delete_secret(&account.cookie_encrypted) {
                log_platform_error(
                    app_handle,
                    "roblox.forget_account",
                    "Could not delete stored Roblox cookie secret",
                    format!("{e}"),
                );
            }
        }
    }

    accounts.retain(|a| a.user_id != user_id);
    save_account_configs(app_handle, &accounts)
}

// ---------------------------------------------------------------------------
// PlatformService implementation
// ---------------------------------------------------------------------------

pub struct RobloxService;

pub static ROBLOX_SERVICE: RobloxService = RobloxService;

impl PlatformService for RobloxService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let accounts = get_accounts(&app)?;
        serde_json::to_value(accounts).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let snapshot = get_startup_snapshot(&app)?;
        serde_json::to_value(snapshot).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_current_account(&self, _app: AppCtx) -> Result<String, PlatformError> {
        Ok(String::new())
    }

    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        _params: Value,
    ) -> Result<(), PlatformError> {
        switch_account(&app, account_id).map_err(Into::into)
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError> {
        forget_account(&app, account_id).map_err(Into::into)
    }

    fn begin_setup(&self, app: AppCtx, _params: Value) -> Result<SetupStatus, PlatformError> {
        begin_account_setup(&app).map_err(Into::into)
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError> {
        get_account_setup_status(&app, setup_id).map_err(Into::into)
    }

    fn cancel_setup(&self, _app: AppCtx, setup_id: &str) -> Result<(), PlatformError> {
        cancel_account_setup(setup_id).map_err(Into::into)
    }

    /// Roblox has no launcher path to resolve: the player installs itself
    /// under LOCALAPPDATA and the session lives in HKCU. Either one means the
    /// machine has seen Roblox, which is what the caller asks about.
    fn is_installed(&self, _app: AppCtx) -> bool {
        let player_installed = env::var("LOCALAPPDATA")
            .map(|dir| PathBuf::from(dir).join("Roblox").is_dir())
            .unwrap_or(false);
        player_installed || matches!(read_cookie_from_registry(), Ok(Some(_)))
    }
}

#[cfg(test)]
mod tests;
