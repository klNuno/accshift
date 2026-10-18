//! The `Backend` trait: one blocking method per Tauri command the frontend
//! calls, minus the window controls the egui shell does itself.
//!
//! Method names are the command names. Arguments are the Rust parameters of
//! the command in `src-tauri/src/commands/*.rs` (the webview sends them in
//! camelCase, Tauri renames them). Return values serialize to the JSON the
//! webview receives. Every method blocks: call it from [`crate::jobs`], never
//! from the UI thread.

use crate::error::BackendResult;
use crate::types::*;
use serde_json::Value;
use std::collections::BTreeMap;

/// Every command of [`Backend`], in trait order. The window commands
/// (`minimize_window`, `toggle_maximize_window`, `close_window`,
/// `set_maximize_button_rect`, `set_keep_backdrop_active`) are not in it,
/// nor is `diagnostics`, which no frontend code calls.
pub const COMMANDS: &[&str] = &[
    "get_boot_payload",
    "get_runtime_os",
    "detect_streaming_software",
    "migrate_legacy_config",
    "load_client_storage_snapshot",
    "save_client_storage_store",
    "get_storage_manifest",
    "log_app_event",
    "finish_boot",
    "pin_unlock",
    "pin_lock",
    "platform_get_accounts",
    "platform_get_startup_snapshot",
    "platform_get_current_account",
    "platform_switch_account",
    "platform_forget_account",
    "platform_begin_setup",
    "platform_get_setup_status",
    "platform_cancel_setup",
    "platform_get_path",
    "platform_set_path",
    "platform_select_path",
    "platform_detect_installed",
    "platform_set_account_label",
    "platform_dry_run",
    "reload_user_platforms",
    "descriptor_select_file",
    "descriptor_preview_file",
    "descriptor_install_file",
    "descriptor_remove",
    "open_descriptors_folder",
    "open_url",
    "open_logs_folder",
    "get_desktop_wallpaper",
    "steam_set_api_key",
    "steam_has_api_key",
    "steam_open_api_key_page",
    "steam_switch_account_and_launch_game",
    "steam_get_profile_info",
    "steam_get_profile_infos",
    "steam_get_player_bans",
    "steam_copy_game_settings",
    "steam_get_copyable_games",
    "steam_open_userdata",
    "steam_clear_browser_cache",
    "steam_bulk_edit",
    "steam_get_account_games",
    "cs2_bridge_get_settings",
    "cs2_bridge_set_settings",
    "cs2_bridge_fetch",
    "cs2_bridge_check",
    "cs2_bridge_test",
    "riot_capture_profile",
    "roblox_add_account_by_cookie",
    "roblox_get_profile_info",
    "roblox_check_sessions",
    "list_custom_themes",
    "save_custom_theme",
    "delete_custom_theme",
    "telemetry_get_state",
    "telemetry_set_mode_a",
    "telemetry_set_mode_b",
    "telemetry_retry_forget",
    "telemetry_complete_onboarding",
    "telemetry_track_persona_switch",
    "telemetry_track_account_added",
    "telemetry_track_account_add_started",
    "telemetry_track_account_add_cancelled",
    "telemetry_track_operation_failed",
    "telemetry_track_update",
    "telemetry_track_settings_snapshot",
    "telemetry_track_streamer_mode",
    "telemetry_export",
];

/// The logic layer the UI talks to. `Send + Sync` so one instance serves the
/// whole job pool.
pub trait Backend: Send + Sync {
    // ---------------------------------------------------------------- app

    /// Legacy migration, client storage snapshot, custom themes, runtime OS and
    /// the user descriptor folder, in one call.
    fn get_boot_payload(&self) -> BackendResult<BootPayload>;
    /// `windows`, `macos` or `linux`.
    fn get_runtime_os(&self) -> BackendResult<String>;
    /// True when OBS, Streamlabs, XSplit and the like are running.
    fn detect_streaming_software(&self) -> BackendResult<bool>;
    /// `migrated`, `none` or `error:<message>`.
    fn migrate_legacy_config(&self) -> BackendResult<String>;
    fn load_client_storage_snapshot(&self) -> BackendResult<ClientStorageSnapshot>;
    /// The store's new fingerprint. `None` from the mock, which saves nothing.
    fn save_client_storage_store(
        &self,
        store_id: &str,
        value: &Value,
    ) -> BackendResult<Option<String>>;
    fn get_storage_manifest(&self) -> BackendResult<StorageManifest>;
    fn log_app_event(
        &self,
        level: &str,
        source: &str,
        message: &str,
        details: Option<&str>,
    ) -> BackendResult<()>;
    /// Boot milestones and first-run telemetry. The Tauri command also shows
    /// the window; the egui shell does that part itself.
    fn finish_boot(&self, source: &str, marks: Option<&Value>) -> BackendResult<()>;
    /// Where downloaded avatars are kept between launches
    /// ([`crate::accounts::avatars::AvatarDiskCache`]). `None` keeps them in
    /// memory only: the mock, whose scenes must not depend on a past run.
    fn avatar_cache_dir(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Launch at system startup, minimized to the tray
    /// ([`crate::lifecycle::autostart`]). Windows only; the mock and other
    /// platforms do nothing.
    fn set_autostart(&self, enabled: bool) -> BackendResult<()> {
        let _ = enabled;
        Ok(())
    }

    // ---------------------------------------------------------------- PIN

    fn pin_unlock(&self, code: &str) -> BackendResult<PinUnlockResult>;
    fn pin_lock(&self) -> BackendResult<()>;

    // ---------------------------------------------------------------- platforms

    /// The platform's own account list (`SteamAccount[]`, Riot profiles,
    /// `RobloxAccount[]`, descriptor accounts), as the command's `Value`.
    fn platform_get_accounts(&self, platform_id: &str) -> BackendResult<Value>;
    /// Accounts and current account in one call; the shape is the platform's.
    fn platform_get_startup_snapshot(&self, platform_id: &str) -> BackendResult<Value>;
    fn platform_get_current_account(&self, platform_id: &str) -> BackendResult<String>;
    /// Refused with the `pin_locked:` message while the PIN session is locked.
    fn platform_switch_account(
        &self,
        platform_id: &str,
        account_id: &str,
        params: &Value,
    ) -> BackendResult<()>;
    fn platform_forget_account(&self, platform_id: &str, account_id: &str) -> BackendResult<()>;
    fn platform_begin_setup(&self, platform_id: &str, params: &Value)
        -> BackendResult<SetupStatus>;
    fn platform_get_setup_status(
        &self,
        platform_id: &str,
        setup_id: &str,
    ) -> BackendResult<SetupStatus>;
    fn platform_cancel_setup(&self, platform_id: &str, setup_id: &str) -> BackendResult<()>;
    fn platform_get_path(&self, platform_id: &str) -> BackendResult<String>;
    fn platform_set_path(&self, platform_id: &str, path: &str) -> BackendResult<()>;
    /// Opens a folder picker. Cancelling is an error.
    fn platform_select_path(&self, platform_id: &str) -> BackendResult<String>;
    fn platform_detect_installed(&self) -> BackendResult<Vec<String>>;
    fn platform_set_account_label(
        &self,
        platform_id: &str,
        account_id: &str,
        label: &str,
    ) -> BackendResult<()>;
    fn platform_dry_run(&self, platform_id: &str, account_id: &str) -> BackendResult<DryRunPlan>;

    // ---------------------------------------------------------------- descriptors

    fn reload_user_platforms(&self) -> BackendResult<UserPlatformReport>;
    /// Opens a file picker. Cancelling is an error.
    fn descriptor_select_file(&self) -> BackendResult<String>;
    fn descriptor_preview_file(&self, path: &str) -> BackendResult<DescriptorPreview>;
    fn descriptor_install_file(&self, path: &str) -> BackendResult<UserPlatformReport>;
    fn descriptor_remove(&self, platform_id: &str) -> BackendResult<UserPlatformReport>;
    fn open_descriptors_folder(&self) -> BackendResult<()>;

    // ---------------------------------------------------------------- utility

    fn open_url(&self, url: &str) -> BackendResult<()>;
    fn open_logs_folder(&self) -> BackendResult<()>;
    /// `None` when there is nothing to capture (and always from the mock).
    fn get_desktop_wallpaper(&self) -> BackendResult<Option<WallpaperSnapshot>>;

    // ---------------------------------------------------------------- Steam

    fn steam_set_api_key(&self, key: &str) -> BackendResult<()>;
    fn steam_has_api_key(&self) -> BackendResult<bool>;
    fn steam_open_api_key_page(&self) -> BackendResult<()>;
    fn steam_switch_account_and_launch_game(
        &self,
        username: &str,
        app_id: &str,
        run_as_admin: bool,
        launch_options: &str,
        shutdown_mode: &str,
    ) -> BackendResult<()>;
    fn steam_get_profile_info(&self, steam_id: &str) -> BackendResult<Option<ProfileInfo>>;
    /// Ids without a result are absent from the map.
    fn steam_get_profile_infos(
        &self,
        steam_ids: &[String],
    ) -> BackendResult<BTreeMap<String, ProfileInfo>>;
    fn steam_get_player_bans(&self, steam_ids: &[String]) -> BackendResult<Vec<BanInfo>>;
    fn steam_copy_game_settings(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
        app_id: &str,
    ) -> BackendResult<()>;
    fn steam_get_copyable_games(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
    ) -> BackendResult<Vec<CopyableGame>>;
    fn steam_open_userdata(&self, steam_id: &str) -> BackendResult<()>;
    fn steam_clear_browser_cache(&self) -> BackendResult<()>;
    fn steam_bulk_edit(&self, request: BulkEditRequest) -> BackendResult<BulkEditResult>;
    fn steam_get_account_games(&self, steam_id: &str) -> BackendResult<Vec<CopyableGame>>;

    // ---------------------------------------------------------------- CS2 bridge

    fn cs2_bridge_get_settings(&self) -> BackendResult<Cs2BridgeSettings>;
    /// `token` `None` keeps the stored token, an empty one clears it.
    fn cs2_bridge_set_settings(
        &self,
        enabled: bool,
        url: &str,
        token: Option<&str>,
    ) -> BackendResult<()>;
    fn cs2_bridge_fetch(&self) -> BackendResult<Vec<Cs2BridgeAccount>>;
    /// `None` when the bridge is off.
    fn cs2_bridge_check(&self, steam_id: &str) -> BackendResult<Option<Cs2BridgeAccount>>;
    fn cs2_bridge_test(&self) -> BackendResult<Cs2BridgeTestResult>;

    // ---------------------------------------------------------------- Riot, Roblox

    fn riot_capture_profile(&self, profile_id: &str) -> BackendResult<()>;
    fn roblox_add_account_by_cookie(&self, cookie: &str) -> BackendResult<RobloxAccount>;
    fn roblox_get_profile_info(&self, user_id: &str) -> BackendResult<RobloxProfileInfo>;
    /// User ids whose stored session is dead.
    fn roblox_check_sessions(&self) -> BackendResult<Vec<String>>;

    // ---------------------------------------------------------------- themes

    fn list_custom_themes(&self) -> BackendResult<Vec<CustomTheme>>;
    fn save_custom_theme(&self, theme: &CustomTheme) -> BackendResult<()>;
    fn delete_custom_theme(&self, theme_id: &str) -> BackendResult<()>;

    // ---------------------------------------------------------------- telemetry

    fn telemetry_get_state(&self) -> BackendResult<TelemetryUiState>;
    fn telemetry_set_mode_a(&self, enabled: bool) -> BackendResult<()>;
    fn telemetry_set_mode_b(&self, enabled: bool) -> BackendResult<()>;
    fn telemetry_retry_forget(&self) -> BackendResult<()>;
    fn telemetry_complete_onboarding(
        &self,
        mode_a_enabled: bool,
        mode_b_enabled: bool,
    ) -> BackendResult<()>;
    fn telemetry_track_persona_switch(&self, platforms: u64, succeeded: u64) -> BackendResult<()>;
    fn telemetry_track_account_added(&self, platform_id: &str) -> BackendResult<()>;
    fn telemetry_track_account_add_started(&self, platform_id: &str) -> BackendResult<()>;
    fn telemetry_track_account_add_cancelled(&self, platform_id: &str) -> BackendResult<()>;
    fn telemetry_track_operation_failed(
        &self,
        operation: &str,
        error_code: &str,
        platform_id: Option<&str>,
    ) -> BackendResult<()>;
    /// `stage` is `available`, `downloaded`, `applied` or `failed`.
    fn telemetry_track_update(
        &self,
        stage: &str,
        target_version: Option<&str>,
        error_code: Option<&str>,
    ) -> BackendResult<()>;
    fn telemetry_track_settings_snapshot(&self, snapshot: &SettingsSnapshot) -> BackendResult<()>;
    fn telemetry_track_streamer_mode(&self) -> BackendResult<()>;
    /// What the telemetry backend holds for this install (Mode B).
    fn telemetry_export(&self) -> BackendResult<Value>;
}
