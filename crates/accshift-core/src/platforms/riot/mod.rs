use crate::config::{self, RiotProfileConfig};
use crate::error::PlatformError;
use crate::platforms::{log_platform_error, log_platform_info, PlatformService, SetupStatus};
use crate::{AppContext, AppCtx};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

use crate::os;
use crate::snapshot_crypto::{self, decrypted_copy_file, encrypted_copy_file, DirCopyOptions};

mod identity;
mod local_api;
mod paths;
mod process;
mod profiles;
mod rollback;
mod setup;
mod snapshot;
mod switch;
mod yaml;

#[allow(unused_imports)]
use self::identity::*;
#[allow(unused_imports)]
use self::local_api::*;
#[allow(unused_imports)]
use self::paths::*;
#[allow(unused_imports)]
use self::process::*;
#[allow(unused_imports)]
use self::profiles::*;
#[allow(unused_imports)]
use self::rollback::*;
#[allow(unused_imports)]
use self::setup::*;
#[allow(unused_imports)]
use self::snapshot::*;
#[allow(unused_imports)]
use self::yaml::*;

pub use self::rollback::{sweep_rollback_dirs, RollbackSweepStats};
pub use self::setup::{
    begin_profile_setup, cancel_profile_setup, capture_profile, get_profile_setup_status,
};
pub use self::switch::switch_profile;

const RIOT_CLIENT_PROCESS_NAMES: &[&str] = &[
    "RiotClientServices.exe",
    "RiotClientUx.exe",
    "RiotClientUxRender.exe",
    "LeagueClient.exe",
    "LeagueClientUx.exe",
    "LeagueClientUxRender.exe",
];

const RIOT_GAME_PROCESS_NAMES: &[&str] = &["LeagueofLegends.exe", "VALORANT-Win64-Shipping.exe"];

const KILL_RETRY_COUNT: usize = 4;
const KILL_RETRY_DELAY_MS: u64 = 450;
const POST_KILL_SETTLE_MS: u64 = 250;

#[derive(Clone, Copy)]
enum RiotPathBase {
    LocalAppData,
    ProgramData,
    InstallDir,
}

#[derive(Clone, Copy)]
enum RiotSnapshotKind {
    File,
    Directory,
}

struct RiotSnapshotItem {
    snapshot_name: &'static str,
    base: RiotPathBase,
    relative_path: &'static str,
    kind: RiotSnapshotKind,
    optional: bool,
    ignored_names: &'static [&'static str],
}

const RIOT_SNAPSHOT_ITEMS: &[RiotSnapshotItem] = &[
    RiotSnapshotItem {
        snapshot_name: "RiotGamesPrivateSettings.yaml",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml",
        kind: RiotSnapshotKind::File,
        optional: false,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "LeagueRiotGamesPrivateSettings.yaml",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/League of Legends/Data/RiotGamesPrivateSettings.yaml",
        kind: RiotSnapshotKind::File,
        optional: true,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "Sessions",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Data/Sessions",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "RiotClientConfig",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Config",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &["lockfile"],
    },
    RiotSnapshotItem {
        snapshot_name: "InstallConfig",
        base: RiotPathBase::InstallDir,
        relative_path: "Config",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "RiotMetadata",
        base: RiotPathBase::ProgramData,
        relative_path: "Riot Games/Metadata/Riot Client",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &[],
    },
];

const RIOT_SETUP_RESET_ITEMS: &[&str] = &[
    "RiotGamesPrivateSettings.yaml",
    "LeagueRiotGamesPrivateSettings.yaml",
    "Sessions",
    "RiotClientConfig",
];
const RIOT_SETUP_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotStartupSnapshot {
    pub profiles: Vec<RiotProfileConfig>,
    pub current_profile: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotProfileSetupStatus {
    pub profile_id: String,
    pub state: String,
    pub account_id: String,
    pub account_display_name: String,
    pub error_message: String,
}

#[derive(Debug, Deserialize)]
struct RiotAliasResponse {
    #[serde(default)]
    game_name: String,
    #[serde(default)]
    tag_line: String,
}

struct RiotLocalApiAccess {
    protocol: String,
    port: u16,
    password: String,
}

#[derive(Clone)]
struct RiotDetectedIdentity {
    account_name: String,
    account_tag_line: String,
    account_puuid: String,
}

fn find_profile<'a>(
    cfg: &'a config::AppConfig,
    profile_id: &str,
) -> Option<&'a config::RiotProfileConfig> {
    cfg.riot.profiles.iter().find(|p| p.id == profile_id)
}

fn find_profile_mut<'a>(
    cfg: &'a mut config::AppConfig,
    profile_id: &str,
) -> Option<&'a mut config::RiotProfileConfig> {
    cfg.riot.profiles.iter_mut().find(|p| p.id == profile_id)
}

// Read paths are side-effect free on purpose: they used to call
// cleanup_expired_pending_profiles, which does save_config + remove_dir_all on
// every read. The UI polls these, so the disk churn (and lock contention) added
// up. Expiry cleanup now runs only on write entry points: begin_profile_setup,
// cancel_profile_setup, switch_profile, and the setup-status poll
// (get_profile_setup_status). Expired pending profiles are hidden from reads
// anyway because visible_profiles filters out setup_pending state.
pub fn get_profiles(app_handle: AppCtx) -> Result<Vec<RiotProfileConfig>, String> {
    let cfg = config::load_config(&app_handle);
    Ok(visible_profiles(&cfg))
}

pub fn get_startup_snapshot(app_handle: AppCtx) -> Result<RiotStartupSnapshot, String> {
    let cfg = config::load_config(&app_handle);
    let current_profile = visible_current_profile_id(&cfg);
    Ok(RiotStartupSnapshot {
        profiles: visible_profiles(&cfg),
        current_profile,
    })
}

pub fn get_current_profile(app_handle: AppCtx) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    Ok(visible_current_profile_id(&cfg))
}

pub fn forget_profile(app_handle: AppCtx, profile_id: String) -> Result<(), String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    config::update_config(&app_handle, |cfg| {
        cfg.riot
            .profiles
            .retain(|profile| profile.id != profile_id.as_str());
        release_current_profile(cfg, std::slice::from_ref(&profile_id));
    })?;
    forget_setup_launch(&profile_id);

    let snapshot_dir = profile_snapshot_path(&app_handle, &profile_id)?;
    if snapshot_dir.exists() {
        // Free the keyring entries the encrypted files point at before deleting
        // them, otherwise the secrets are orphaned in the OS keyring forever.
        free_snapshot_secrets(&app_handle, &snapshot_dir);
        fs::remove_dir_all(&snapshot_dir).map_err(|e| {
            format!(
                "Could not remove Riot profile snapshot {}: {e}",
                snapshot_dir.display()
            )
        })?;
    }

    Ok(())
}

pub fn get_riot_path(app_handle: AppCtx) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    if !cfg.riot.path_override.trim().is_empty() {
        return Ok(cfg.riot.path_override);
    }
    resolve_riot_client_path(&app_handle).map(|path| path.to_string_lossy().to_string())
}

pub fn set_riot_path(app_handle: AppCtx, path: String) -> Result<(), String> {
    config::update_config(&app_handle, |cfg| {
        cfg.riot.path_override = path.trim().to_string();
    })
}

pub fn select_riot_path() -> Result<String, String> {
    os::select_file(
        "Select Riot Client executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

pub struct RiotService;

pub static RIOT_SERVICE: RiotService = RiotService;

impl PlatformService for RiotService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let profiles = get_profiles(app.clone())?;
        serde_json::to_value(profiles).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let snapshot = get_startup_snapshot(app.clone())?;
        serde_json::to_value(snapshot).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_current_account(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_current_profile(app.clone()).map_err(Into::into)
    }

    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        _params: Value,
    ) -> Result<(), PlatformError> {
        switch_profile(app.clone(), account_id.to_string()).map_err(Into::into)
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError> {
        forget_profile(app.clone(), account_id.to_string()).map_err(Into::into)
    }

    fn begin_setup(&self, app: AppCtx, _params: Value) -> Result<SetupStatus, PlatformError> {
        let status = begin_profile_setup(app.clone())?;
        Ok(SetupStatus {
            setup_id: status.profile_id,
            state: status.state,
            account_id: status.account_id,
            account_display_name: status.account_display_name,
            error_message: status.error_message,
        })
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError> {
        let status = get_profile_setup_status(app.clone(), setup_id.to_string())?;
        Ok(SetupStatus {
            setup_id: status.profile_id,
            state: status.state,
            account_id: status.account_id,
            account_display_name: status.account_display_name,
            error_message: status.error_message,
        })
    }

    fn cancel_setup(&self, app: AppCtx, setup_id: &str) -> Result<(), PlatformError> {
        cancel_profile_setup(app.clone(), setup_id.to_string()).map_err(Into::into)
    }

    fn get_path(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_riot_path(app.clone()).map_err(Into::into)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), PlatformError> {
        set_riot_path(app.clone(), path.to_string()).map_err(Into::into)
    }

    fn select_path(&self) -> Result<String, PlatformError> {
        select_riot_path().map_err(Into::into)
    }
}

#[cfg(test)]
mod rollback_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tests;
