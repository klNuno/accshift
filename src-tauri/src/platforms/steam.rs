use crate::config;
use crate::os;
use crate::platforms::{
    AddAccountRequest, CopyGameSettingsRequest, PlatformFuture, PlatformService,
    PlatformStartupSnapshot, SwitchAccountModeRequest, SwitchAccountRequest,
};
use crate::steam::accounts::{self, CopyableGame, SteamAccount};
use crate::steam::bans::{self, BanInfo};
use crate::steam::profile::{self, ProfileInfo};
use std::collections::HashSet;
use std::path::PathBuf;

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
    accounts::get_accounts(&steam_path).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<PlatformStartupSnapshot, String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    let (accounts, current_from_file) =
        accounts::get_accounts_snapshot(&steam_path).map_err(|e| {
            eprintln!("Error: {:?}", e);
            e.to_string()
        })?;
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
    accounts::get_current_account_name(&steam_path).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

pub async fn switch_account(
    app_handle: tauri::AppHandle,
    username: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    validate_username(&username)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        accounts::switch_account(&steam_path, &username, run_as_admin, &launch_options)
    })
    .await
    .map_err(|e| format!("Switch account task failed: {e}"))?
    .map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
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
    tauri::async_runtime::spawn_blocking(move || {
        accounts::switch_account_mode(
            &steam_path,
            &username,
            &steam_id,
            &mode,
            run_as_admin,
            &launch_options,
        )
    })
    .await
    .map_err(|e| format!("Switch account mode task failed: {e}"))?
    .map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

pub async fn add_account(
    app_handle: tauri::AppHandle,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        accounts::add_account(&steam_path, run_as_admin, &launch_options)
    })
    .await
    .map_err(|e| format!("Add account task failed: {e}"))?
    .map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

pub async fn forget_account(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || accounts::forget_account(&steam_path, &steam_id))
        .await
        .map_err(|e| format!("Forget account task failed: {e}"))?
        .map_err(|e| {
            eprintln!("Error: {:?}", e);
            e.to_string()
        })
}

pub fn open_userdata(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::open_userdata_with_path(&steam_path, &steam_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
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
    accounts::copy_game_settings(&steam_path, &from_steam_id, &to_steam_id, &app_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

pub fn get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<CopyableGame>, String> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_copyable_games(&steam_path, &from_steam_id, &to_steam_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
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
            eprintln!("Error: failed to read Steam API key: {e}");
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

    fn add_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        request: AddAccountRequest,
    ) -> PlatformFuture<'a, Result<(), String>> {
        Box::pin(async move {
            add_account(app_handle, request.run_as_admin, request.launch_options).await
        })
    }

    fn forget_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        steam_id: String,
    ) -> PlatformFuture<'a, Result<(), String>> {
        Box::pin(async move { forget_account(app_handle, steam_id).await })
    }

    fn open_userdata(
        &self,
        app_handle: tauri::AppHandle,
        steam_id: String,
    ) -> Result<(), String> {
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
