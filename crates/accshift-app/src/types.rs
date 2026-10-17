//! Payloads the backend answers with that `accshift-core` does not define,
//! because the Tauri app built them itself. Each one serializes to the JSON
//! its webview received from the matching command.
//!
//! Payloads `accshift-core` already defines are re-exported here so callers
//! have one place to import from.

use serde::{Deserialize, Serialize};

pub use accshift_core::platforms::descriptor::library::DescriptorPreview;
pub use accshift_core::platforms::descriptor::plan::DryRunPlan;
pub use accshift_core::platforms::steam::accounts::{CopyableGame, SteamAccount};
pub use accshift_core::platforms::steam::bans::BanInfo;
pub use accshift_core::platforms::steam::bulk_edit::{
    BulkEditFailure, BulkEditRequest, BulkEditResult, LaunchOptionEdit,
};
pub use accshift_core::platforms::steam::cs2_bridge::{
    Cs2BridgeAccount, Cs2BridgeSettings, Cs2BridgeTestResult,
};
pub use accshift_core::platforms::steam::profile::ProfileInfo;
pub use accshift_core::platforms::{
    RejectedDescriptor, SetupStatus, SkippedPlatform, UserPlatformReport,
};
pub use accshift_core::storage::{ClientStorageSnapshot, StorageManifest};
pub use accshift_core::themes::CustomTheme;

/// `get_boot_payload`: everything the UI needs before the window shows.
/// Mirrors `BootPayload` in `src-tauri/src/commands/app.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPayload {
    /// `migrated`, `none`, `error:<message>` (and `skipped` from the mock).
    pub migration: String,
    pub runtime_os: String,
    pub storage_snapshot: ClientStorageSnapshot,
    pub custom_themes: Vec<CustomTheme>,
    pub user_platforms: UserPlatformReport,
}

/// `get_desktop_wallpaper`: the desktop picture behind the window, for the
/// glass themes. Mirrors `WallpaperSnapshot` in
/// `src-tauri/src/commands/backdrop.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperSnapshot {
    pub data_url: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// `pin_unlock`. Mirrors `PinUnlockResult` in `src-tauri/src/commands/pin.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinUnlockResult {
    /// `unlocked`, `invalid`, `retry_later` or `not_configured`.
    pub status: String,
    /// The stored hash is the legacy unsalted form: the caller rewrites it.
    pub legacy: bool,
    /// For `invalid` and `retry_later`: how long the next attempt is refused.
    pub retry_after_ms: u64,
}

/// `telemetry_get_state`. Mirrors `TelemetryUiState` in
/// `src-tauri/src/commands/telemetry.rs`, snake_case on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelemetryUiState {
    pub mode_a_enabled: bool,
    pub mode_b_enabled: bool,
    pub install_id_set: bool,
    pub forget_pending: bool,
    pub onboarding_completed: bool,
}

/// Arguments of `telemetry_track_settings_snapshot`, the `SettingsSnapshot`
/// of `src/lib/app/telemetryClient.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub ui_language: String,
    pub enabled_platforms: Vec<String>,
    pub personas_enabled: bool,
    pub pin_enabled: bool,
    pub cli_enabled: bool,
    pub deep_links_enabled: bool,
    pub streamer_mode: String,
    pub animations: String,
}

/// A Roblox account as `platform_get_accounts` and
/// `roblox_add_account_by_cookie` send it. Mirrors `RobloxAccount` in
/// `accshift_core::platforms::roblox`, which only exists on Windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobloxAccount {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub last_login_at: Option<u64>,
}

/// `roblox_get_profile_info`. Mirrors `RobloxProfileInfo` in
/// `accshift_core::platforms::roblox` (Windows only there).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RobloxProfileInfo {
    pub avatar_url: Option<String>,
}
