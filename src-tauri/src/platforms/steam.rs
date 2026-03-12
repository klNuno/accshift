use crate::config;
use crate::os;
use crate::platforms::{
    log_platform_error, log_platform_info, to_logged_error, CopyGameSettingsRequest,
    PlatformFuture, PlatformService, PlatformStartupSnapshot, SwitchAccountModeRequest,
    SwitchAccountRequest,
};
use crate::steam::accounts::{self, CopyableGame, SteamAccount};
use crate::steam::bans::{self, BanInfo};
use crate::steam::profile::{self, ProfileInfo};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const STEAM_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamAccountSetupStatus {
    pub setup_id: String,
    pub state: String,
    pub account_id: String,
    pub account_display_name: String,
    pub error_message: String,
}

#[derive(Clone)]
struct SteamAccountSetupJob {
    steam_path: PathBuf,
    known_account_ids: HashSet<String>,
    launch_started: bool,
    error_message: Option<String>,
    last_touched_at: u64,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn steam_setup_jobs() -> &'static Mutex<HashMap<String, SteamAccountSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, SteamAccountSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn steam_setup_expired(last_touched_at: u64) -> bool {
    now_unix_ms().saturating_sub(last_touched_at) > STEAM_SETUP_TTL_MS
}

fn purge_expired_steam_setup_jobs(jobs: &mut HashMap<String, SteamAccountSetupJob>) {
    jobs.retain(|_, job| !steam_setup_expired(job.last_touched_at));
}

fn setup_status(
    setup_id: &str,
    state: &str,
    account_id: impl Into<String>,
    account_display_name: impl Into<String>,
    error_message: impl Into<String>,
) -> SteamAccountSetupStatus {
    SteamAccountSetupStatus {
        setup_id: setup_id.to_string(),
        state: state.to_string(),
        account_id: account_id.into(),
        account_display_name: account_display_name.into(),
        error_message: error_message.into(),
    }
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

fn read_api_key(app_handle: &tauri::AppHandle) -> Result<String, String> {
    let mut cfg = config::load_config(app_handle);
    let encrypted = cfg.steam.api_key_encrypted.trim();
    if !encrypted.is_empty() {
        return decrypt_api_key(encrypted).map(|v| v.trim().to_string());
    }

    let legacy = cfg.steam.api_key.trim().to_string();
    if legacy.is_empty() {
        return Ok(String::new());
    }

    cfg.steam.api_key_encrypted = encrypt_api_key(&legacy)?;
    cfg.steam.api_key = String::new();
    config::save_config(app_handle, &cfg)?;
    Ok(legacy)
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

fn resolve_steam_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.steam.path_override.trim();
    let steam_path = if !override_path.is_empty() {
        PathBuf::from(override_path)
    } else {
        os::steam_installation_path().map_err(|e| e.to_string())?
    };

    if !steam_path.exists() || !steam_path.join(os::steam_executable_name()).exists() {
        return Err("Could not locate Steam installation".into());
    }

    Ok(steam_path)
}

pub struct SteamPlatformService;

pub static STEAM_PLATFORM_SERVICE: SteamPlatformService = SteamPlatformService;

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

    serde_json::json!({
        "requestedUsername": requested_username,
        "steamId": steam_id,
        "mode": mode,
        "runAsAdmin": run_as_admin,
        "launchOptionsConfigured": !launch_options.trim().is_empty(),
        "autoLoginUser": auto_login_user,
        "currentAccountFromLoginusers": current_from_file,
        "steamRunning": os::is_process_running(os::steam_process_name()),
        "steamWebHelperRunning": os::is_process_running(os::steam_web_helper_process_name()),
    })
    .to_string()
}

pub fn set_api_key(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    let trimmed = key.trim();
    let mut cfg = config::load_config(&app_handle);
    cfg.steam.api_key = String::new();
    cfg.steam.api_key_encrypted = if trimmed.is_empty() {
        String::new()
    } else {
        encrypt_api_key(trimmed)?
    };
    config::save_config(&app_handle, &cfg)
}

pub fn has_api_key(app_handle: tauri::AppHandle) -> bool {
    read_api_key(&app_handle)
        .map(|api_key| !api_key.trim().is_empty())
        .unwrap_or(false)
}

pub fn get_accounts(app_handle: tauri::AppHandle) -> Result<Vec<SteamAccount>, String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_accounts(&steam_path)
        .map_err(|e| to_logged_error(&app_handle, "steam.get_accounts", e))
}

pub fn get_startup_snapshot(
    app_handle: tauri::AppHandle,
) -> Result<PlatformStartupSnapshot, String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    let (accounts, current_from_file) = accounts::get_accounts_snapshot(&steam_path)
        .map_err(|e| to_logged_error(&app_handle, "steam.get_startup_snapshot", e))?;
    let current_account = {
        let from_registry = os::get_auto_login_user().unwrap_or_default();
        if from_registry.trim().is_empty() {
            current_from_file
        } else {
            from_registry
        }
    };

    Ok(PlatformStartupSnapshot {
        accounts,
        current_account,
    })
}

pub fn get_current_account(app_handle: tauri::AppHandle) -> Result<String, String> {
    let from_registry = os::get_auto_login_user().unwrap_or_default();
    if !from_registry.trim().is_empty() {
        return Ok(from_registry);
    }

    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_current_account_name(&steam_path)
        .map_err(|e| to_logged_error(&app_handle, "steam.get_current_account", e))
}

pub async fn switch_account(
    app_handle: tauri::AppHandle,
    username: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    validate_username(&username)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    log_platform_info(
        &app_handle,
        "steam.switch_account",
        "Steam switch requested",
        build_switch_state_details(
            &steam_path,
            Some(&username),
            None,
            None,
            run_as_admin,
            &launch_options,
        ),
    );

    let app_handle_for_task = app_handle.clone();
    let username_for_task = username.clone();
    let launch_options_for_task = launch_options.clone();
    tauri::async_runtime::spawn_blocking(move || {
        log_platform_info(
            &app_handle_for_task,
            "steam.switch_account",
            "Steam switch started",
            build_switch_state_details(
                &steam_path,
                Some(&username_for_task),
                None,
                None,
                run_as_admin,
                &launch_options_for_task,
            ),
        );

        let result = accounts::switch_account(
            &steam_path,
            &username_for_task,
            run_as_admin,
            &launch_options_for_task,
        );

        match &result {
            Ok(()) => log_platform_info(
                &app_handle_for_task,
                "steam.switch_account",
                "Steam switch completed",
                build_switch_state_details(
                    &steam_path,
                    Some(&username_for_task),
                    None,
                    None,
                    run_as_admin,
                    &launch_options_for_task,
                ),
            ),
            Err(error) => log_platform_error(
                &app_handle_for_task,
                "steam.switch_account",
                "Steam switch failed",
                format!(
                    "error={error}; state={}",
                    build_switch_state_details(
                        &steam_path,
                        Some(&username_for_task),
                        None,
                        None,
                        run_as_admin,
                        &launch_options_for_task,
                    )
                ),
            ),
        }

        result
    })
    .await
    .map_err(|e| format!("Switch account task failed: {e}"))?
    .map_err(|e| to_logged_error(&app_handle, "steam.switch_account", e))
}

pub async fn switch_account_mode(
    app_handle: tauri::AppHandle,
    username: String,
    steam_id: String,
    mode: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    validate_username(&username)?;
    validate_steam_id(&steam_id)?;
    if !["online", "invisible"].contains(&mode.as_str()) {
        return Err("Invalid mode".into());
    }
    let steam_path = resolve_steam_path(&app_handle)?;
    log_platform_info(
        &app_handle,
        "steam.switch_account_mode",
        "Steam switch mode requested",
        build_switch_state_details(
            &steam_path,
            Some(&username),
            Some(&steam_id),
            Some(&mode),
            run_as_admin,
            &launch_options,
        ),
    );

    let app_handle_for_task = app_handle.clone();
    let username_for_task = username.clone();
    let steam_id_for_task = steam_id.clone();
    let mode_for_task = mode.clone();
    let launch_options_for_task = launch_options.clone();
    tauri::async_runtime::spawn_blocking(move || {
        log_platform_info(
            &app_handle_for_task,
            "steam.switch_account_mode",
            "Steam switch mode started",
            build_switch_state_details(
                &steam_path,
                Some(&username_for_task),
                Some(&steam_id_for_task),
                Some(&mode_for_task),
                run_as_admin,
                &launch_options_for_task,
            ),
        );

        let result = accounts::switch_account_mode(
            &steam_path,
            &username_for_task,
            &steam_id_for_task,
            &mode_for_task,
            run_as_admin,
            &launch_options_for_task,
        );

        match &result {
            Ok(()) => log_platform_info(
                &app_handle_for_task,
                "steam.switch_account_mode",
                "Steam switch mode completed",
                build_switch_state_details(
                    &steam_path,
                    Some(&username_for_task),
                    Some(&steam_id_for_task),
                    Some(&mode_for_task),
                    run_as_admin,
                    &launch_options_for_task,
                ),
            ),
            Err(error) => log_platform_error(
                &app_handle_for_task,
                "steam.switch_account_mode",
                "Steam switch mode failed",
                format!(
                    "error={error}; state={}",
                    build_switch_state_details(
                        &steam_path,
                        Some(&username_for_task),
                        Some(&steam_id_for_task),
                        Some(&mode_for_task),
                        run_as_admin,
                        &launch_options_for_task,
                    )
                ),
            ),
        }

        result
    })
    .await
    .map_err(|e| format!("Switch account mode task failed: {e}"))?
    .map_err(|e| to_logged_error(&app_handle, "steam.switch_account_mode", e))
}

pub fn begin_account_setup(
    app_handle: tauri::AppHandle,
    run_as_admin: bool,
    launch_options: String,
) -> Result<SteamAccountSetupStatus, String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    let known_accounts = accounts::get_accounts(&steam_path)
        .map_err(|e| to_logged_error(&app_handle, "steam.begin_account_setup", e))?;
    let known_account_ids = known_accounts
        .into_iter()
        .map(|account| account.steam_id)
        .collect::<HashSet<_>>();
    let setup_id = format!("steam-setup-{}", Uuid::new_v4());
    let created_at = now_unix_ms();

    {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| "Steam setup storage is unavailable".to_string())?;
        purge_expired_steam_setup_jobs(&mut jobs);
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
    thread::spawn(move || {
        let launch_result = accounts::add_account(&steam_path, run_as_admin, &launch_options)
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

    Ok(setup_status(&setup_id, "waiting_for_client", "", "", ""))
}

pub fn get_account_setup_status(
    app_handle: tauri::AppHandle,
    setup_id: String,
) -> Result<SteamAccountSetupStatus, String> {
    let setup_id = setup_id.trim().to_string();
    if setup_id.is_empty() {
        return Err("Invalid Steam setup id".into());
    }

    let job = {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| "Steam setup storage is unavailable".to_string())?;
        purge_expired_steam_setup_jobs(&mut jobs);
        let Some(job) = jobs.get_mut(&setup_id) else {
            return Err("Steam setup not found".into());
        };
        job.last_touched_at = now_unix_ms();
        job.clone()
    };

    if let Some(error) = job.error_message {
        return Ok(setup_status(&setup_id, "failed", "", "", error));
    }

    if !job.launch_started {
        return Ok(setup_status(&setup_id, "waiting_for_client", "", "", ""));
    }

    let accounts = accounts::get_accounts(&job.steam_path)
        .map_err(|e| to_logged_error(&app_handle, "steam.get_account_setup_status", e))?;
    let maybe_added = accounts
        .into_iter()
        .filter(|account| !job.known_account_ids.contains(&account.steam_id))
        .max_by_key(|account| account.last_login_at.unwrap_or(0));

    if let Some(account) = maybe_added {
        if let Ok(mut jobs) = steam_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
        return Ok(setup_status(
            &setup_id,
            "ready",
            account.steam_id,
            account.persona_name,
            "",
        ));
    }

    Ok(setup_status(&setup_id, "waiting_for_login", "", "", ""))
}

pub fn cancel_account_setup(_app_handle: tauri::AppHandle, setup_id: String) -> Result<(), String> {
    let setup_id = setup_id.trim();
    if setup_id.is_empty() {
        return Ok(());
    }
    let mut jobs = steam_setup_jobs()
        .lock()
        .map_err(|_| "Steam setup storage is unavailable".to_string())?;
    purge_expired_steam_setup_jobs(&mut jobs);
    jobs.remove(setup_id);
    Ok(())
}

pub async fn forget_account(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || accounts::forget_account(&steam_path, &steam_id))
        .await
        .map_err(|e| format!("Forget account task failed: {e}"))?
        .map_err(|e| to_logged_error(&app_handle, "steam.forget_account", e))
}

pub fn open_userdata(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::open_userdata_with_path(&steam_path, &steam_id)
        .map_err(|e| to_logged_error(&app_handle, "steam.open_userdata", e))
}

pub fn clear_integrated_browser_cache(app_handle: tauri::AppHandle) -> Result<(), String> {
    accounts::clear_integrated_browser_cache()
        .map_err(|e| to_logged_error(&app_handle, "steam.clear_integrated_browser_cache", e))
}

pub fn copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), String> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::copy_game_settings(&steam_path, &from_steam_id, &to_steam_id, &app_id)
        .map_err(|e| to_logged_error(&app_handle, "steam.copy_game_settings", e))
}

pub fn get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<CopyableGame>, String> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_copyable_games(&steam_path, &from_steam_id, &to_steam_id)
        .map_err(|e| to_logged_error(&app_handle, "steam.get_copyable_games", e))
}

pub fn get_steam_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    if !cfg.steam.path_override.trim().is_empty() {
        return Ok(cfg.steam.path_override);
    }
    resolve_steam_path(&app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_steam_path(app_handle: tauri::AppHandle, path: String) -> Result<(), String> {
    let trimmed = path.trim();
    let mut cfg = config::load_config(&app_handle);
    if trimmed.is_empty() {
        cfg.steam.path_override = String::new();
    } else {
        cfg.steam.path_override = trimmed.to_string();
    }
    config::save_config(&app_handle, &cfg)
}

pub fn select_steam_path() -> Result<String, String> {
    os::select_folder("Select Steam folder").map_err(|e| e.to_string())
}

pub fn open_steam_api_key_page() -> Result<(), String> {
    os::open_url("https://steamcommunity.com/dev/apikey").map_err(|e| e.to_string())
}

pub async fn get_profile_info(
    steam_id: String,
    client: reqwest::Client,
) -> Result<Option<ProfileInfo>, String> {
    validate_steam_id(&steam_id)?;
    Ok(profile::fetch_profile_info(&client, &steam_id).await)
}

pub async fn get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: reqwest::Client,
) -> Result<Vec<BanInfo>, String> {
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
    bans::fetch_player_bans(&client, &api_key, unique_steam_ids).await
}

impl PlatformService for SteamPlatformService {
    fn id(&self) -> &'static str {
        "steam"
    }

    fn set_api_key(&self, app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
        set_api_key(app_handle, key)
    }

    fn has_api_key(&self, app_handle: tauri::AppHandle) -> bool {
        has_api_key(app_handle)
    }

    fn get_accounts(&self, app_handle: tauri::AppHandle) -> Result<Vec<SteamAccount>, String> {
        get_accounts(app_handle)
    }

    fn get_startup_snapshot(
        &self,
        app_handle: tauri::AppHandle,
    ) -> Result<PlatformStartupSnapshot, String> {
        get_startup_snapshot(app_handle)
    }

    fn get_current_account(&self, app_handle: tauri::AppHandle) -> Result<String, String> {
        get_current_account(app_handle)
    }

    fn switch_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        request: SwitchAccountRequest,
    ) -> PlatformFuture<'a, Result<(), String>> {
        Box::pin(async move {
            switch_account(
                app_handle,
                request.username,
                request.run_as_admin,
                request.launch_options,
            )
            .await
        })
    }

    fn switch_account_mode<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        request: SwitchAccountModeRequest,
    ) -> PlatformFuture<'a, Result<(), String>> {
        Box::pin(async move {
            switch_account_mode(
                app_handle,
                request.username,
                request.steam_id,
                request.mode,
                request.run_as_admin,
                request.launch_options,
            )
            .await
        })
    }

    fn forget_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        steam_id: String,
    ) -> PlatformFuture<'a, Result<(), String>> {
        Box::pin(async move { forget_account(app_handle, steam_id).await })
    }

    fn open_userdata(&self, app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
        open_userdata(app_handle, steam_id)
    }

    fn copy_game_settings(
        &self,
        app_handle: tauri::AppHandle,
        request: CopyGameSettingsRequest,
    ) -> Result<(), String> {
        copy_game_settings(
            app_handle,
            request.from_steam_id,
            request.to_steam_id,
            request.app_id,
        )
    }

    fn get_copyable_games(
        &self,
        app_handle: tauri::AppHandle,
        from_steam_id: String,
        to_steam_id: String,
    ) -> Result<Vec<CopyableGame>, String> {
        get_copyable_games(app_handle, from_steam_id, to_steam_id)
    }

    fn get_installation_path(&self, app_handle: tauri::AppHandle) -> Result<String, String> {
        get_steam_path(app_handle)
    }

    fn set_installation_path(
        &self,
        app_handle: tauri::AppHandle,
        path: String,
    ) -> Result<(), String> {
        set_steam_path(app_handle, path)
    }

    fn select_installation_path(&self) -> Result<String, String> {
        select_steam_path()
    }

    fn open_api_key_page(&self) -> Result<(), String> {
        open_steam_api_key_page()
    }

    fn get_profile_info<'a>(
        &'a self,
        steam_id: String,
        client: reqwest::Client,
    ) -> PlatformFuture<'a, Result<Option<ProfileInfo>, String>> {
        Box::pin(async move { get_profile_info(steam_id, client).await })
    }

    fn get_player_bans<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        steam_ids: Vec<String>,
        client: reqwest::Client,
    ) -> PlatformFuture<'a, Result<Vec<BanInfo>, String>> {
        Box::pin(async move { get_player_bans(app_handle, steam_ids, client).await })
    }
}
