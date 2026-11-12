//! The settings store: its typed shape, the Steam platform settings, the
//! draft merge of the settings panel, the API key field and the onboarding
//! consent.
//!
//! [`sanitize_settings`] takes any JSON and returns a full [`AppSettings`],
//! with the defaults, clamps and legacy keys older installs wrote.

use crate::i18n::{self, Locale};
use crate::js;
use crate::stores::StoreContext;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRefreshSettings {
    pub avatar_cache_days: i64,
    pub ban_check_days: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDisplaySettings {
    pub show_usernames: bool,
    pub show_last_login_per_platform: BTreeMap<String, bool>,
    pub show_card_notes_inline: bool,
    pub expanded_folders: bool,
}

/// `"system"` follows the OS reduced-motion preference; `"on"`/`"off"` force it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AnimationsMode {
    #[default]
    System,
    On,
    Off,
}

/// `"auto"` blurs the UI when streaming software is detected; `"off"` never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StreamerMode {
    #[default]
    Auto,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SteamShutdownMode {
    #[default]
    Graceful,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SteamPlatformSettings {
    pub run_as_admin: bool,
    pub launch_options: String,
    pub shutdown_mode: SteamShutdownMode,
}

/// The per-platform settings. Only Steam has a schema today; a platform with
/// none has no entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlatformSettings {
    pub steam: SteamPlatformSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: Locale,
    pub theme_id: String,
    pub background_opacity: i64,
    pub ui_scale_percent: i64,
    pub animations: AnimationsMode,
    pub streamer_mode: StreamerMode,
    pub suspend_graphics_when_minimized: bool,
    pub minimize_on_account_switch: bool,
    /// Windows only: launch at sign-in, minimized to the tray
    /// ([`crate::lifecycle::autostart`]).
    pub start_with_system: bool,
    /// Windows only: the close button hides to the tray instead of quitting.
    pub close_to_tray: bool,
    pub data_refresh: DataRefreshSettings,
    pub enabled_platforms: Vec<String>,
    /// Per-platform opt-out for network account health checks.
    pub health_check_per_platform: BTreeMap<String, bool>,
    pub personas_enabled: bool,
    pub default_platform_id: String,
    pub inactivity_blur_seconds: i64,
    pub deep_links_enabled: bool,
    /// Mirrored by the CLI binary, which refuses every subcommand when false.
    pub cli_enabled: bool,
    pub platform_settings: PlatformSettings,
    pub account_display: AccountDisplaySettings,
    pub pin_enabled: bool,
    pub pin_hash: String,
}

impl AppSettings {
    /// The settings as the JSON the store holds.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

pub const DEFAULT_THEME_ID: &str = "dark";
const DEFAULT_ENABLED_PLATFORM: &str = "steam";
const DEFAULT_PLATFORM_ID: &str = "steam";
const LAUNCH_OPTIONS_MAX: usize = 256;

/// The platforms with a network health check, and whether it is on by
/// default.
pub const HEALTH_CHECK_DEFAULTS: [(&str, bool); 1] = [("roblox", true)];

/// Whether each platform shows the last login when storage has no entry
/// for it. Any other platform shows `false`.
pub const SHOW_LAST_LOGIN_DEFAULTS: [(&str, bool); 9] = [
    ("steam", false),
    ("riot", false),
    ("battle-net", false),
    ("roblox", true),
    ("epic", true),
    ("ubisoft", true),
    ("gog", true),
    ("jagex", true),
    ("discord", true),
];

/// Flat keys that predate `showLastLoginPerPlatform`.
const LEGACY_LAST_LOGIN_KEYS: [(&str, &str); 3] = [
    ("steam", "showLastLogin"),
    ("riot", "showRiotLastLogin"),
    ("battle-net", "showBattleNetLastLogin"),
];

/// True for a stored PIN hash: `salt:hash` in hex (16 and 32 bytes) or the
/// legacy bare 32-byte hash.
pub fn is_valid_pin_hash(value: &str) -> bool {
    let hex = |part: &str, len: usize| {
        part.len() == len && part.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    match value.split_once(':') {
        Some((salt, hash)) => hex(salt, 32) && hex(hash, 64),
        None => hex(value, 64),
    }
}

fn sanitize_pin_hash(value: Option<&Value>) -> String {
    let Some(text) = js::as_str(value) else {
        return String::new();
    };
    let normalized = js::trim(text).to_lowercase();
    if is_valid_pin_hash(&normalized) {
        normalized
    } else {
        String::new()
    }
}

fn field<'a>(record: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    record.and_then(|map| map.get(key))
}

fn has_field(record: Option<&Map<String, Value>>, key: &str) -> bool {
    record.is_some_and(|map| map.contains_key(key))
}

/// The Steam settings, every field present. Legacy flat keys
/// (`steamRunAsAdmin`, `steamLaunchOptions`) still resolve.
pub fn sanitize_steam_settings(
    raw: Option<&Value>,
    legacy_root: Option<&Map<String, Value>>,
) -> SteamPlatformSettings {
    let record = js::as_record(raw);
    let launch_options = js::nullish_or(
        field(record, "launchOptions"),
        field(legacy_root, "steamLaunchOptions"),
    );
    SteamPlatformSettings {
        run_as_admin: js::truthy(js::nullish_or(
            field(record, "runAsAdmin"),
            field(legacy_root, "steamRunAsAdmin"),
        )),
        launch_options: js::as_str(launch_options)
            .map(|text| js::utf16_prefix(js::trim(text), LAUNCH_OPTIONS_MAX))
            .unwrap_or_default(),
        shutdown_mode: if js::as_str(field(record, "shutdownMode")) == Some("force") {
            SteamShutdownMode::Force
        } else {
            SteamShutdownMode::Graceful
        },
    }
}

fn sanitize_show_last_login(
    raw_account_display: Option<&Map<String, Value>>,
    ctx: &StoreContext,
) -> BTreeMap<String, bool> {
    let raw_map = js::as_record(field(raw_account_display, "showLastLoginPerPlatform"));
    let mut result = BTreeMap::new();
    for id in ctx.platform_ids() {
        let legacy_key = LEGACY_LAST_LOGIN_KEYS
            .iter()
            .find(|(platform, _)| *platform == id)
            .map(|(_, key)| *key);
        let value = if has_field(raw_map, id) {
            js::truthy(field(raw_map, id))
        } else if let Some(key) = legacy_key.filter(|key| has_field(raw_account_display, key)) {
            js::truthy(field(raw_account_display, key))
        } else {
            SHOW_LAST_LOGIN_DEFAULTS
                .iter()
                .find(|(platform, _)| *platform == id)
                .is_some_and(|(_, shown)| *shown)
        };
        result.insert(id.to_string(), value);
    }
    result
}

fn sanitize_health_check(raw: Option<&Map<String, Value>>) -> BTreeMap<String, bool> {
    let raw_map = js::as_record(field(raw, "healthCheckPerPlatform"));
    HEALTH_CHECK_DEFAULTS
        .iter()
        .map(|(id, default)| {
            let value = if has_field(raw_map, id) {
                js::not_false(field(raw_map, id))
            } else {
                *default
            };
            (id.to_string(), value)
        })
        .collect()
}

/// A nested setting, or the flat key older installs kept at the root.
fn nested_or_flat<'a>(
    nested: Option<&'a Map<String, Value>>,
    root: Option<&'a Map<String, Value>>,
    key: &str,
) -> Option<&'a Value> {
    js::nullish_or(field(nested, key), field(root, key))
}

/// The known platforms of `enabledPlatforms`, deduplicated, Steam when none
/// is left.
fn sanitize_enabled_platforms(raw: Option<&Map<String, Value>>, ctx: &StoreContext) -> Vec<String> {
    let mut enabled: Vec<String> = Vec::new();
    if let Some(Value::Array(items)) = field(raw, "enabledPlatforms") {
        for item in items {
            let Value::String(id) = item else {
                continue;
            };
            if ctx.knows_platform(id) && !enabled.contains(id) {
                enabled.push(id.clone());
            }
        }
    }
    if enabled.is_empty() {
        enabled.push(DEFAULT_ENABLED_PLATFORM.to_string());
    }
    enabled
}

/// The stored default platform when it is enabled, else the first enabled
/// one.
fn sanitize_default_platform(raw: Option<&Map<String, Value>>, enabled: &[String]) -> String {
    let requested = js::as_str(field(raw, "defaultPlatformId")).unwrap_or(DEFAULT_PLATFORM_ID);
    if enabled.iter().any(|id| id == requested) {
        requested.to_string()
    } else {
        enabled[0].clone()
    }
}

/// `(pin_enabled, pin_hash)`. An enabled flag without a usable hash protects
/// nothing: the PIN is off until it is set up again.
fn sanitize_pin(raw: Option<&Map<String, Value>>) -> (bool, String) {
    if !js::truthy(field(raw, "pinEnabled")) {
        return (false, String::new());
    }
    let pin_hash = sanitize_pin_hash(field(raw, "pinHash"));
    (!pin_hash.is_empty(), pin_hash)
}

/// The theme the settings ask for: `themeId`, else the light theme of the
/// older `theme` key, else the default.
fn requested_theme(raw: Option<&Map<String, Value>>) -> &str {
    match field(raw, "themeId") {
        Some(Value::String(id)) => id.as_str(),
        _ if js::as_str(field(raw, "theme")) == Some("light") => "light",
        _ => DEFAULT_THEME_ID,
    }
}

fn sanitize_data_refresh(raw: Option<&Map<String, Value>>) -> DataRefreshSettings {
    let nested = js::as_record(field(raw, "dataRefresh"));
    let days = |key| js::clamp_int(nested_or_flat(nested, raw, key), 0, 90, 7);
    DataRefreshSettings {
        avatar_cache_days: days("avatarCacheDays"),
        ban_check_days: days("banCheckDays"),
    }
}

fn sanitize_account_display(
    raw: Option<&Map<String, Value>>,
    ctx: &StoreContext,
) -> AccountDisplaySettings {
    let nested = js::as_record(field(raw, "accountDisplay"));
    AccountDisplaySettings {
        show_usernames: js::not_false(field(nested, "showUsernames"))
            && js::not_false(field(raw, "showUsernames")),
        show_last_login_per_platform: sanitize_show_last_login(nested, ctx),
        show_card_notes_inline: js::truthy(nested_or_flat(nested, raw, "showCardNotesInline")),
        expanded_folders: js::truthy(nested_or_flat(nested, raw, "expandedFolders")),
    }
}

/// Every field present, every value in range.
pub fn sanitize_settings(value: &Value, ctx: &StoreContext) -> AppSettings {
    let raw = js::as_record(Some(value));
    let raw_platform_settings = js::as_record(field(raw, "platformSettings"));
    let enabled_platforms = sanitize_enabled_platforms(raw, ctx);
    let default_platform_id = sanitize_default_platform(raw, &enabled_platforms);
    let (pin_enabled, pin_hash) = sanitize_pin(raw);

    AppSettings {
        language: if has_field(raw, "language") {
            i18n::normalize_locale(field(raw, "language"))
        } else {
            i18n::detect_preferred_locale(&ctx.preferred_languages)
        },
        theme_id: ctx.resolve_theme_id(requested_theme(raw)).to_string(),
        background_opacity: js::clamp_int(field(raw, "backgroundOpacity"), 0, 100, 100),
        ui_scale_percent: js::clamp_int(field(raw, "uiScalePercent"), 75, 150, 100),
        animations: match js::as_str(field(raw, "animations")) {
            Some("on") => AnimationsMode::On,
            Some("off") => AnimationsMode::Off,
            _ => AnimationsMode::System,
        },
        streamer_mode: if js::as_str(field(raw, "streamerMode")) == Some("off") {
            StreamerMode::Off
        } else {
            StreamerMode::Auto
        },
        suspend_graphics_when_minimized: js::not_false(field(raw, "suspendGraphicsWhenMinimized")),
        minimize_on_account_switch: js::truthy(field(raw, "minimizeOnAccountSwitch")),
        start_with_system: js::truthy(field(raw, "startWithSystem")),
        close_to_tray: js::truthy(field(raw, "closeToTray")),
        data_refresh: sanitize_data_refresh(raw),
        enabled_platforms,
        health_check_per_platform: sanitize_health_check(raw),
        personas_enabled: js::not_false(field(raw, "personasEnabled")),
        default_platform_id,
        inactivity_blur_seconds: js::clamp_int(field(raw, "inactivityBlurSeconds"), 0, 3600, 60),
        deep_links_enabled: js::not_false(field(raw, "deepLinksEnabled")),
        cli_enabled: js::not_false(field(raw, "cliEnabled")),
        platform_settings: PlatformSettings {
            steam: sanitize_steam_settings(field(raw_platform_settings, "steam"), raw),
        },
        account_display: sanitize_account_display(raw, ctx),
        pin_enabled,
        pin_hash,
    }
}

/// How long a cached avatar stays fresh, in milliseconds (0 means never
/// cache).
pub fn cache_duration_ms(settings: &AppSettings) -> u64 {
    let days = settings.data_refresh.avatar_cache_days.max(0) as u64;
    days * 24 * 60 * 60 * 1000
}

// ------------------------------------------------------------ settings drafts

/// One setting that changed: where it sits and its new value (`None` when the
/// key is gone).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsChange {
    pub path: Vec<String>,
    pub value: Option<Value>,
}

/// Every leaf that differs between `before` and `after`. Objects are walked
/// key by key, in sorted order; lists and scalars are compared and replaced
/// whole.
pub fn diff_settings(before: Option<&Value>, after: Option<&Value>) -> Vec<SettingsChange> {
    let mut out = Vec::new();
    diff_into(before, after, &mut Vec::new(), &mut out);
    out
}

fn diff_into(
    before: Option<&Value>,
    after: Option<&Value>,
    path: &mut Vec<String>,
    out: &mut Vec<SettingsChange>,
) {
    if let (Some(Value::Object(left)), Some(Value::Object(right))) = (before, after) {
        let mut keys: Vec<&String> = left.keys().chain(right.keys()).collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            path.push(key.clone());
            diff_into(left.get(key), right.get(key), path, out);
            path.pop();
        }
        return;
    }
    if before != after {
        out.push(SettingsChange {
            path: path.clone(),
            value: after.cloned(),
        });
    }
}

fn apply_change(target: &mut Map<String, Value>, change: &SettingsChange) {
    let Some((leaf, parents)) = change.path.split_last() else {
        return;
    };
    let mut node = target;
    for key in parents {
        let slot = node
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        let Value::Object(next) = slot else {
            return;
        };
        node = next;
    }
    match &change.value {
        None => {
            node.remove(leaf);
        }
        Some(value) => {
            node.insert(leaf.clone(), value.clone());
        }
    }
}

/// [`merge_settings_draft`] on JSON: `current` with only the leaves the
/// user changed between `baseline` and `draft`.
pub fn merge_settings_draft_value(current: &Value, baseline: &Value, draft: &Value) -> Value {
    let mut merged = match current {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    for change in diff_settings(Some(baseline), Some(draft)) {
        if !change.path.is_empty() {
            apply_change(&mut merged, &change);
        }
    }
    Value::Object(merged)
}

/// The settings to save from an open panel, so a field
/// something else wrote meanwhile (zoom shortcut, streamer banner, a PIN hash
/// upgraded on unlock) is not put back.
pub fn merge_settings_draft(
    current: &AppSettings,
    baseline: &AppSettings,
    draft: &AppSettings,
) -> Result<AppSettings, serde_json::Error> {
    let merged =
        merge_settings_draft_value(&current.to_value(), &baseline.to_value(), &draft.to_value());
    serde_json::from_value(merged)
}

// ------------------------------------------------------------ API key field

/// The API key field after a successful save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKeyDraft {
    pub api_key: String,
    pub touched: bool,
}

/// The API key field after a save: cleared only when it still holds the key
/// just written; a replacement typed meanwhile stays dirty.
pub fn api_key_draft_after_successful_save(current: &str, saved: &str) -> ApiKeyDraft {
    if js::trim(current) == saved {
        ApiKeyDraft {
            api_key: String::new(),
            touched: false,
        }
    } else {
        ApiKeyDraft {
            api_key: current.to_string(),
            touched: true,
        }
    }
}

// ------------------------------------------------------------ onboarding consent

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingConsentOutcome {
    Saved,
    Failed,
}

/// Saves the onboarding consent. A failure stays in the dialog, so the
/// caller shows it inline instead of a toast.
pub fn complete_onboarding_consent<T, E: std::fmt::Display>(
    persist: impl FnOnce() -> Result<T, E>,
) -> OnboardingConsentOutcome {
    match persist() {
        Ok(_) => OnboardingConsentOutcome::Saved,
        Err(error) => {
            eprintln!("telemetry_complete_onboarding failed: {error}");
            OnboardingConsentOutcome::Failed
        }
    }
}
