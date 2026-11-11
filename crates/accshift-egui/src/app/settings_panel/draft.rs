//! The draft's values: the number fields, the flags the toggles flip and
//! the normalisation every save runs first.

use accshift_app::settings::{AppSettings, SteamShutdownMode, StreamerMode};

use super::super::platforms::{self, PlatformDef, RuntimeOs};
use super::SettingFlag;

/// Rounded and clamped, `fallback` for a missing or non-finite value.
pub fn clamp_int(value: Option<f64>, min: i64, max: i64, fallback: i64) -> i64 {
    match value {
        Some(v) if v.is_finite() => (v.round() as i64).clamp(min, max),
        _ => fallback,
    }
}

/// The number a field's text holds: trimmed, empty reads as 0, and a text
/// with letters other than an exponent's `e` is not a number.
pub fn js_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    if t.chars()
        .any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    {
        return None;
    }
    t.parse::<f64>().ok()
}

/// The text of a number field, committed on demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumericInput {
    pub input: String,
    pub min: i64,
    pub max: i64,
}

impl NumericInput {
    pub fn new(value: i64, min: i64, max: i64) -> Self {
        Self {
            input: value.to_string(),
            min,
            max,
        }
    }

    /// The clamped value to store; `current` when the text is not a number.
    /// The text shows the stored value afterwards.
    pub fn commit(&mut self, current: i64) -> i64 {
        let clamped = clamp_int(js_number(&self.input), self.min, self.max, current);
        self.input = clamped.to_string();
        clamped
    }

    /// Shows `value`, after the setting changed under the field.
    pub fn refresh(&mut self, value: i64) {
        self.input = value.to_string();
    }
}

/// The five number fields of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericField {
    UiScale,
    BgOpacity,
    AvatarCacheDays,
    BanCheckDays,
    InactivityBlur,
}

/// Flips the setting a toggle stands for.
pub(super) fn toggle_flag(d: &mut AppSettings, flag: SettingFlag) {
    let on = match flag {
        SettingFlag::ShowCardNotesInline => &mut d.account_display.show_card_notes_inline,
        SettingFlag::ExpandedFolders => &mut d.account_display.expanded_folders,
        SettingFlag::SuspendGraphicsWhenMinimized => &mut d.suspend_graphics_when_minimized,
        SettingFlag::MinimizeOnAccountSwitch => &mut d.minimize_on_account_switch,
        SettingFlag::StartWithSystem => &mut d.start_with_system,
        SettingFlag::CloseToTray => &mut d.close_to_tray,
        SettingFlag::DeepLinks => &mut d.deep_links_enabled,
        SettingFlag::Personas => &mut d.personas_enabled,
        SettingFlag::Cli => &mut d.cli_enabled,
        SettingFlag::ShowUsernames => &mut d.account_display.show_usernames,
        SettingFlag::SteamRunAsAdmin => &mut d.platform_settings.steam.run_as_admin,
        SettingFlag::SteamForceShutdown => {
            let steam = &mut d.platform_settings.steam;
            steam.shutdown_mode = match steam.shutdown_mode {
                SteamShutdownMode::Force => SteamShutdownMode::Graceful,
                SteamShutdownMode::Graceful => SteamShutdownMode::Force,
            };
            return;
        }
        SettingFlag::StreamerMode => {
            d.streamer_mode = match d.streamer_mode {
                StreamerMode::Auto => StreamerMode::Off,
                StreamerMode::Off => StreamerMode::Auto,
            };
            return;
        }
    };
    *on ^= true;
}

/// Brings the settings back in range: the numbers clamped, the launch
/// options trimmed, a PIN hash only while the PIN is on, at least one
/// enabled platform, and a startup platform that exists, is enabled and,
/// when an enabled one is usable here, is usable.
pub fn normalize(s: &mut AppSettings, platforms: &[PlatformDef], os: RuntimeOs) {
    let clamp = |v: i64, min, max, fallback| clamp_int(Some(v as f64), min, max, fallback);
    s.background_opacity = clamp(s.background_opacity, 0, 100, 100);
    s.ui_scale_percent = clamp(s.ui_scale_percent, 75, 150, 100);
    s.data_refresh.avatar_cache_days = clamp(s.data_refresh.avatar_cache_days, 0, 90, 7);
    s.data_refresh.ban_check_days = clamp(s.data_refresh.ban_check_days, 0, 90, 7);
    s.inactivity_blur_seconds = clamp(s.inactivity_blur_seconds, 0, 3600, 60);
    s.platform_settings.steam.launch_options =
        s.platform_settings.steam.launch_options.trim().to_string();
    if !s.pin_enabled {
        s.pin_hash.clear();
    }
    // Every platform of the registry is implemented, user ones included.
    if !platforms.iter().any(|p| p.id == s.default_platform_id) {
        s.default_platform_id = "steam".to_string();
    }
    if s.enabled_platforms.is_empty() {
        s.enabled_platforms = vec!["steam".to_string()];
    }
    if !s.enabled_platforms.contains(&s.default_platform_id) {
        s.default_platform_id = s.enabled_platforms[0].clone();
    }
    let selectable: Vec<&String> = s
        .enabled_platforms
        .iter()
        .filter(|id| platforms::usable(platforms, id, os))
        .collect();
    if let Some(first) = selectable.first() {
        if !selectable.contains(&&s.default_platform_id) {
            s.default_platform_id = (*first).clone();
        }
    }
}

/// The enabled platforms and the startup one, as one string: a save that
/// changes it checks the active tab is still enabled.
pub(super) fn platform_snapshot(s: &AppSettings) -> String {
    let mut enabled = s.enabled_platforms.clone();
    enabled.sort();
    serde_json::json!({ "enabledPlatforms": enabled, "defaultPlatformId": s.default_platform_id })
        .to_string()
}
