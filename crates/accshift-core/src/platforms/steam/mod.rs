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
pub mod vdf;

use accounts::{CopyableGame, SteamAccount};
use bans::BanInfo;
use profile::ProfileInfo;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

const STEAM_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Clone)]
struct SteamAccountSetupJob {
    steam_path: PathBuf,
    known_account_ids: HashSet<String>,
    launch_started: bool,
    error_message: Option<String>,
    last_touched_at: u64,
}

fn steam_setup_jobs() -> &'static Mutex<HashMap<String, SteamAccountSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, SteamAccountSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn encrypt_api_key(api_key: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Ok(String::new());
    }
    os::encrypt_secret(api_key).map_err(|e| e.to_string())
}

fn decrypt_api_key(encrypted_api_key: &str) -> Result<String, String> {
    if encrypted_api_key.trim().is_empty() {
        return Ok(String::new());
    }
    os::decrypt_secret(encrypted_api_key).map_err(|e| e.to_string())
}

enum SecretPersistence {
    Replaced(String),
    Unused,
}

fn rotate_secret_with<Encrypt, Persist, Delete, Warn>(
    plaintext: &str,
    encrypt: Encrypt,
    persist: Persist,
    mut delete: Delete,
    mut warn: Warn,
) -> Result<bool, String>
where
    Encrypt: FnOnce(&str) -> Result<String, String>,
    Persist: FnOnce(String) -> Result<SecretPersistence, String>,
    Delete: FnMut(&str) -> Result<(), String>,
    Warn: FnMut(&'static str, String),
{
    let plaintext = plaintext.trim();
    let replacement = if plaintext.is_empty() {
        String::new()
    } else {
        encrypt(plaintext)?
    };

    let previous = match persist(replacement.clone()) {
        Ok(SecretPersistence::Replaced(previous)) => previous,
        Ok(SecretPersistence::Unused) => {
            // A concurrent writer replaced the legacy value while this token
            // was being prepared. It never became active and has no owner.
            if !replacement.is_empty() {
                if let Err(cleanup_error) = delete(&replacement) {
                    warn("unused", cleanup_error);
                }
            }
            return Ok(false);
        }
        Err(error) => {
            // Encryption may already have created a keyring entry. If the
            // config write fails, the new token has no owner and must be
            // removed while the previous token stays untouched.
            if !replacement.is_empty() {
                if let Err(cleanup_error) = delete(&replacement) {
                    warn("replacement", cleanup_error);
                }
            }
            return Err(error);
        }
    };

    // The config now points at the replacement (or at no token when clearing),
    // so the superseded keyring entry can be released. Cleanup is best-effort:
    // failing after the config commit must not make the UI retry the rotation.
    if !previous.is_empty() && previous != replacement {
        if let Err(cleanup_error) = delete(&previous) {
            warn("previous", cleanup_error);
        }
    }
    Ok(true)
}

fn warn_secret_cleanup(
    app_handle: &dyn AppContext,
    log_target: &str,
    phase: &'static str,
    error: String,
) {
    log_platform_error(
        app_handle,
        log_target,
        "Could not remove superseded secret from OS storage",
        format!("phase={phase}; error={error}"),
    );
}

pub(super) fn replace_config_secret(
    app_handle: &dyn AppContext,
    plaintext: &str,
    log_target: &str,
    update: impl FnOnce(&mut config::AppConfig, String) -> String,
) -> Result<(), String> {
    rotate_secret_with(
        plaintext,
        |value| os::encrypt_secret(value).map_err(|e| e.to_string()),
        |replacement| {
            let mut previous = String::new();
            config::update_config(app_handle, |cfg| {
                previous = update(cfg, replacement);
            })?;
            Ok(SecretPersistence::Replaced(previous))
        },
        |token| os::delete_secret(token).map_err(|e| e.to_string()),
        |phase, error| warn_secret_cleanup(app_handle, log_target, phase, error),
    )
    .map(|_| ())
}

fn read_api_key(app_handle: &dyn AppContext) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    let encrypted = cfg.steam.api_key_encrypted.trim();
    if !encrypted.is_empty() {
        return decrypt_api_key(encrypted).map(|v| v.trim().to_string());
    }

    let legacy = cfg.steam.api_key.trim().to_string();
    if legacy.is_empty() {
        return Ok(String::new());
    }

    let migrated = rotate_secret_with(
        &legacy,
        encrypt_api_key,
        |replacement| {
            let mut persistence = SecretPersistence::Unused;
            config::update_config(app_handle, |latest| {
                if latest.steam.api_key_encrypted.trim().is_empty()
                    && latest.steam.api_key.trim() == legacy
                {
                    let previous =
                        std::mem::replace(&mut latest.steam.api_key_encrypted, replacement);
                    latest.steam.api_key.clear();
                    persistence = SecretPersistence::Replaced(previous);
                }
            })?;
            Ok(persistence)
        },
        |token| os::delete_secret(token).map_err(|e| e.to_string()),
        |phase, error| warn_secret_cleanup(app_handle, "steam.migrate_api_key", phase, error),
    )?;
    if migrated {
        return Ok(legacy);
    }

    // A concurrent setter won the race. Return its value instead of the stale
    // plaintext observed before taking the config write lock.
    let latest = config::load_config(app_handle);
    if !latest.steam.api_key_encrypted.trim().is_empty() {
        decrypt_api_key(&latest.steam.api_key_encrypted).map(|value| value.trim().to_string())
    } else {
        Ok(latest.steam.api_key.trim().to_string())
    }
}

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

fn resolve_steam_path(app_handle: &dyn AppContext) -> Result<PathBuf, PlatformError> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.steam.path_override.trim();
    let steam_path = if !override_path.is_empty() {
        PathBuf::from(override_path)
    } else {
        // AppError::RegistryOpen maps to ClientNotInstalled.
        os::steam_installation_path()?
    };

    if !steam_path.exists() || !steam_path.join("config").join("loginusers.vdf").exists() {
        return Err(PlatformError::new(
            PlatformErrorKind::ClientNotInstalled,
            "Could not locate Steam installation",
        ));
    }

    Ok(steam_path)
}

pub struct SteamService;

pub static STEAM_SERVICE: SteamService = SteamService;

fn build_switch_state_details(
    steam_path: &std::path::Path,
    requested_username: Option<&str>,
    steam_id: Option<&str>,
    mode: Option<&str>,
    run_as_admin: bool,
    launch_options: &str,
) -> String {
    let auto_login_user = os::get_auto_login_user().unwrap_or_else(|e| format!("<error:{e}>"));
    let current_from_file =
        accounts::get_current_account_name(steam_path).unwrap_or_else(|e| format!("<error:{e}>"));

    use super::redact_id;
    use super::redact_opt;
    serde_json::json!({
        "requestedUsername": redact_opt(requested_username),
        "steamId": redact_opt(steam_id),
        "mode": mode,
        "runAsAdmin": run_as_admin,
        "launchOptionsConfigured": !launch_options.trim().is_empty(),
        "autoLoginUser": redact_id(&auto_login_user),
        "currentAccountFromLoginusers": redact_id(&current_from_file),
        "steamRunning": os::is_process_running(os::steam_process_name()),
        "steamWebHelperRunning": os::is_process_running(os::steam_web_helper_process_name()),
    })
    .to_string()
}

pub fn set_api_key(app_handle: AppCtx, key: String) -> Result<(), PlatformError> {
    replace_config_secret(
        &app_handle,
        &key,
        "steam.set_api_key",
        |cfg, replacement| {
            let previous = std::mem::replace(&mut cfg.steam.api_key_encrypted, replacement);
            cfg.steam.api_key = String::new();
            previous
        },
    )
    .map_err(Into::into)
}

pub fn has_api_key(app_handle: AppCtx) -> bool {
    read_api_key(&app_handle)
        .map(|api_key| !api_key.trim().is_empty())
        .unwrap_or(false)
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
    log_platform_info(
        &app_handle,
        "steam.switch_account_and_launch_game",
        "Steam switch+launch requested",
        build_switch_state_details(
            &steam_path,
            Some(&username),
            None,
            None,
            run_as_admin,
            &launch_options,
        ),
    );

    let force_kill = shutdown_mode == "force";
    let result = accounts::switch_account_and_launch_game(
        &steam_path,
        &username,
        &app_id,
        run_as_admin,
        &launch_options,
        force_kill,
    );

    let post_state = build_switch_state_details(
        &steam_path,
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

pub fn begin_account_setup(
    app_handle: AppCtx,
    run_as_admin: bool,
    launch_options: String,
    force_kill: bool,
) -> Result<SetupStatus, PlatformError> {
    let steam_path = resolve_steam_path(&app_handle)?;
    let known_accounts = accounts::get_accounts(&steam_path)
        .map_err(|e| log_platform_failure(&app_handle, "steam.begin_account_setup", e.into()))?;
    let known_account_ids = known_accounts
        .into_iter()
        .map(|account| account.steam_id)
        .collect::<HashSet<_>>();
    let setup_id = format!("steam-setup-{}", Uuid::new_v4());
    let created_at = super::now_unix_ms();

    {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| PlatformError::other("Steam setup storage is unavailable"))?;
        jobs.retain(|_, job| !super::setup_expired(job.last_touched_at, STEAM_SETUP_TTL_MS));
        jobs.insert(
            setup_id.clone(),
            SteamAccountSetupJob {
                steam_path: steam_path.clone(),
                known_account_ids,
                launch_started: false,
                error_message: None,
                last_touched_at: created_at,
            },
        );
    }

    let setup_id_for_job = setup_id.clone();
    let app_handle_for_job = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let launch_result =
            accounts::add_account(&steam_path, run_as_admin, &launch_options, force_kill)
                .map_err(|e| e.to_string());
        if let Ok(mut jobs) = steam_setup_jobs().lock() {
            if let Some(job) = jobs.get_mut(&setup_id_for_job) {
                job.launch_started = true;
                if let Err(error) = launch_result {
                    log_platform_error(
                        &app_handle_for_job,
                        "steam.begin_account_setup.launch",
                        "Steam account setup launch failed",
                        &error,
                    );
                    job.error_message = Some(error);
                }
            }
        }
    });

    Ok(super::make_setup_status(
        &setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

pub fn get_account_setup_status(
    app_handle: AppCtx,
    setup_id: String,
) -> Result<SetupStatus, PlatformError> {
    let setup_id = setup_id.trim().to_string();
    if setup_id.is_empty() {
        return Err("Invalid Steam setup id".into());
    }

    let job = {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| PlatformError::other("Steam setup storage is unavailable"))?;
        jobs.retain(|_, job| !super::setup_expired(job.last_touched_at, STEAM_SETUP_TTL_MS));
        let Some(job) = jobs.get_mut(&setup_id) else {
            // Unknown id here almost always means the TTL purge dropped it.
            return Err(PlatformError::new(
                PlatformErrorKind::SetupExpired,
                "Steam setup not found",
            ));
        };
        job.last_touched_at = super::now_unix_ms();
        job.clone()
    };

    if let Some(error) = job.error_message {
        return Ok(super::make_setup_status(&setup_id, "failed", "", "", error));
    }

    if !job.launch_started {
        return Ok(super::make_setup_status(
            &setup_id,
            "waiting_for_client",
            "",
            "",
            "",
        ));
    }

    let accounts = accounts::get_accounts(&job.steam_path).map_err(|e| {
        log_platform_failure(&app_handle, "steam.get_account_setup_status", e.into())
    })?;
    let maybe_added = accounts
        .into_iter()
        .filter(|account| !job.known_account_ids.contains(&account.steam_id))
        .max_by_key(|account| account.last_login_at.unwrap_or(0));

    if let Some(account) = maybe_added {
        if let Ok(mut jobs) = steam_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
        return Ok(super::make_setup_status(
            &setup_id,
            "ready",
            account.steam_id,
            account.persona_name,
            "",
        ));
    }

    Ok(super::make_setup_status(
        &setup_id,
        "waiting_for_login",
        "",
        "",
        "",
    ))
}

pub fn cancel_account_setup(_app_handle: AppCtx, setup_id: String) -> Result<(), PlatformError> {
    let setup_id = setup_id.trim();
    if setup_id.is_empty() {
        return Ok(());
    }
    let mut jobs = steam_setup_jobs()
        .lock()
        .map_err(|_| PlatformError::other("Steam setup storage is unavailable"))?;
    jobs.retain(|_, job| !super::setup_expired(job.last_touched_at, STEAM_SETUP_TTL_MS));
    jobs.remove(setup_id);
    Ok(())
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

pub fn get_steam_path(app_handle: AppCtx) -> Result<String, PlatformError> {
    let cfg = config::load_config(&app_handle);
    if !cfg.steam.path_override.trim().is_empty() {
        return Ok(cfg.steam.path_override);
    }
    resolve_steam_path(&app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_steam_path(app_handle: AppCtx, path: String) -> Result<(), PlatformError> {
    let trimmed = path.trim().to_string();
    // The override is later joined with steam.exe and launched — only accept
    // an existing directory that actually looks like a Steam install.
    if !trimmed.is_empty() {
        let candidate = PathBuf::from(&trimmed);
        if !candidate.is_dir() {
            return Err("Steam path override must be an existing directory".into());
        }
        if !candidate.join(os::steam_executable_name()).exists()
            && !candidate.join("config").join("loginusers.vdf").exists()
        {
            return Err("This folder does not look like a Steam installation".into());
        }
    }
    config::update_config(&app_handle, |cfg| {
        if trimmed.is_empty() {
            cfg.steam.path_override = String::new();
        } else {
            cfg.steam.path_override = trimmed;
        }
    })
    .map_err(Into::into)
}

pub fn select_steam_path() -> Result<String, PlatformError> {
    os::select_folder("Select Steam folder").map_err(Into::into)
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
    // Steam keeps localconfig.vdf in memory and rewrites it on exit — edits
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

        log_platform_info(
            &app,
            "steam.switch_account",
            "Steam switch requested",
            build_switch_state_details(
                &steam_path,
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

        let post_state = build_switch_state_details(
            &steam_path,
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
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn validate_steam_id_accepts_17_digit_numeric() {
        assert!(validate_steam_id("76561198000000000").is_ok());
    }

    #[test]
    fn validate_steam_id_rejects_too_short() {
        assert!(validate_steam_id("7656119800000000").is_err());
    }

    #[test]
    fn validate_steam_id_rejects_too_long() {
        assert!(validate_steam_id("765611980000000001").is_err());
    }

    #[test]
    fn validate_steam_id_rejects_non_numeric() {
        assert!(validate_steam_id("7656119800000000a").is_err());
    }

    #[test]
    fn validate_steam_id_rejects_empty() {
        assert!(validate_steam_id("").is_err());
    }

    #[test]
    fn validate_username_rejects_empty() {
        assert!(validate_username("").is_err());
        assert!(validate_username("   ").is_err());
    }

    #[test]
    fn validate_username_rejects_too_long() {
        let long_name = "a".repeat(129);
        assert!(validate_username(&long_name).is_err());
    }

    #[test]
    fn validate_username_accepts_128_chars() {
        let max_name = "a".repeat(128);
        assert!(validate_username(&max_name).is_ok());
    }

    #[test]
    fn validate_username_rejects_control_char() {
        assert!(validate_username("bad\u{0000}name").is_err());
        assert!(validate_username("bad\nname").is_err());
    }

    #[test]
    fn validate_username_accepts_normal_name() {
        assert!(validate_username("some_user123").is_ok());
    }

    #[test]
    fn secret_rotation_deletes_previous_token_after_persisting_replacement() {
        let deleted = RefCell::new(Vec::new());
        let result = rotate_secret_with(
            "new secret",
            |value| {
                assert_eq!(value, "new secret");
                Ok("new-token".to_string())
            },
            |replacement| {
                assert_eq!(replacement, "new-token");
                Ok(SecretPersistence::Replaced("old-token".to_string()))
            },
            |token| {
                deleted.borrow_mut().push(token.to_string());
                Ok(())
            },
            |_, _| panic!("cleanup should succeed"),
        );

        assert!(result.unwrap());
        assert_eq!(&*deleted.borrow(), &["old-token"]);
    }

    #[test]
    fn secret_rotation_deletes_replacement_when_persist_fails() {
        let deleted = RefCell::new(Vec::new());
        let result = rotate_secret_with(
            "new secret",
            |_| Ok("new-token".to_string()),
            |_| Err("config write failed".to_string()),
            |token| {
                deleted.borrow_mut().push(token.to_string());
                Ok(())
            },
            |_, _| panic!("cleanup should succeed"),
        );

        assert_eq!(result.unwrap_err(), "config write failed");
        assert_eq!(&*deleted.borrow(), &["new-token"]);
    }

    #[test]
    fn clearing_secret_skips_encryption_and_deletes_previous_token() {
        let deleted = RefCell::new(Vec::new());
        let result = rotate_secret_with(
            "   ",
            |_| panic!("empty secrets must not be encrypted"),
            |replacement| {
                assert!(replacement.is_empty());
                Ok(SecretPersistence::Replaced("old-token".to_string()))
            },
            |token| {
                deleted.borrow_mut().push(token.to_string());
                Ok(())
            },
            |_, _| panic!("cleanup should succeed"),
        );

        assert!(result.unwrap());
        assert_eq!(&*deleted.borrow(), &["old-token"]);
    }

    #[test]
    fn unused_secret_replacement_is_deleted_without_touching_current_token() {
        let deleted = RefCell::new(Vec::new());
        let result = rotate_secret_with(
            "legacy secret",
            |_| Ok("unused-token".to_string()),
            |_| Ok(SecretPersistence::Unused),
            |token| {
                deleted.borrow_mut().push(token.to_string());
                Ok(())
            },
            |_, _| panic!("cleanup should succeed"),
        );

        assert!(!result.unwrap());
        assert_eq!(&*deleted.borrow(), &["unused-token"]);
    }
}
