use crate::steam::accounts::{CopyableGame, SteamAccount};
use crate::steam::bans::BanInfo;
use crate::steam::profile::ProfileInfo;
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;

pub mod riot;
pub mod steam;

pub type PlatformFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct SwitchAccountRequest {
    pub username: String,
    pub run_as_admin: bool,
    pub launch_options: String,
}

#[derive(Debug, Clone)]
pub struct SwitchAccountModeRequest {
    pub username: String,
    pub steam_id: String,
    pub mode: String,
    pub run_as_admin: bool,
    pub launch_options: String,
}

#[derive(Debug, Clone)]
pub struct AddAccountRequest {
    pub run_as_admin: bool,
    pub launch_options: String,
}

#[derive(Debug, Clone)]
pub struct CopyGameSettingsRequest {
    pub from_steam_id: String,
    pub to_steam_id: String,
    pub app_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformStartupSnapshot {
    pub accounts: Vec<SteamAccount>,
    pub current_account: String,
}

pub trait PlatformService: Sync {
    fn id(&self) -> &'static str;
    fn set_api_key(&self, app_handle: tauri::AppHandle, key: String) -> Result<(), String>;
    fn has_api_key(&self, app_handle: tauri::AppHandle) -> bool;
    fn get_accounts(&self, app_handle: tauri::AppHandle) -> Result<Vec<SteamAccount>, String>;
    fn get_startup_snapshot(
        &self,
        app_handle: tauri::AppHandle,
    ) -> Result<PlatformStartupSnapshot, String>;
    fn get_current_account(&self, app_handle: tauri::AppHandle) -> Result<String, String>;
    fn switch_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        request: SwitchAccountRequest,
    ) -> PlatformFuture<'a, Result<(), String>>;
    fn switch_account_mode<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        request: SwitchAccountModeRequest,
    ) -> PlatformFuture<'a, Result<(), String>>;
    fn add_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        request: AddAccountRequest,
    ) -> PlatformFuture<'a, Result<(), String>>;
    fn forget_account<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        steam_id: String,
    ) -> PlatformFuture<'a, Result<(), String>>;
    fn open_userdata(&self, app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String>;
    fn copy_game_settings(
        &self,
        app_handle: tauri::AppHandle,
        request: CopyGameSettingsRequest,
    ) -> Result<(), String>;
    fn get_copyable_games(
        &self,
        app_handle: tauri::AppHandle,
        from_steam_id: String,
        to_steam_id: String,
    ) -> Result<Vec<CopyableGame>, String>;
    fn get_installation_path(&self, app_handle: tauri::AppHandle) -> Result<String, String>;
    fn set_installation_path(
        &self,
        app_handle: tauri::AppHandle,
        path: String,
    ) -> Result<(), String>;
    fn select_installation_path(&self) -> Result<String, String>;
    fn open_api_key_page(&self) -> Result<(), String>;
    fn get_profile_info<'a>(
        &'a self,
        steam_id: String,
        client: reqwest::Client,
    ) -> PlatformFuture<'a, Result<Option<ProfileInfo>, String>>;
    fn get_player_bans<'a>(
        &'a self,
        app_handle: tauri::AppHandle,
        steam_ids: Vec<String>,
        client: reqwest::Client,
    ) -> PlatformFuture<'a, Result<Vec<BanInfo>, String>>;
}

pub fn get_service(platform_id: &str) -> Option<&'static dyn PlatformService> {
    match platform_id {
        "steam" => {
            let service: &'static dyn PlatformService = &steam::STEAM_PLATFORM_SERVICE;
            debug_assert_eq!(service.id(), platform_id);
            Some(service)
        }
        _ => None,
    }
}

pub fn require_service(platform_id: &str) -> Result<&'static dyn PlatformService, String> {
    get_service(platform_id).ok_or_else(|| format!("Unknown platform service: {platform_id}"))
}
