use crate::context::AppContext;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

mod migrate;
mod raw;
mod split;
mod store;
mod window;

#[allow(unused_imports)]
use self::migrate::*;
#[allow(unused_imports)]
use self::raw::*;
#[allow(unused_imports)]
use self::split::*;
#[cfg(test)]
use self::store::*;

pub use self::migrate::migrate_legacy_config;
#[cfg(test)]
pub(crate) use self::store::config_io_test_mutex;
pub use self::store::{load_config, save_config, update_config};
pub use self::window::{
    clamp_window_position, clamp_window_size, load_window, load_window_physical_position,
    load_window_position, load_window_size, logical_from_physical, save_window_geometry,
    save_window_size, SavedWindow,
};

// Every window measurement here is in LOGICAL pixels, which is what
// `WebviewWindowBuilder::inner_size` and `::position` consume. Callers that
// read a live window get physical pixels and must divide by the scale factor
// first (see `logical_from_physical`), or the window grows by that factor at
// every launch on a scaled display.
pub const DEFAULT_WINDOW_WIDTH: f64 = 1000.0;
/// Sized for the first-run onboarding, which opens at this size on every fresh
/// install. At 520 its short-window breakpoints hid both the telemetry
/// explanation (below 641) and the clip (at 520 and under). 680 is the first
/// height that shows the whole deal step and four full rows of cards.
pub const DEFAULT_WINDOW_HEIGHT: f64 = 680.0;
pub const MIN_WINDOW_WIDTH: f64 = 400.0;
pub const MIN_WINDOW_HEIGHT: f64 = 300.0;
/// Upper bound on a restored window, in logical pixels. Windows itself refuses
/// to create a window wider or taller than this, so a config claiming more is
/// corrupt whatever produced it.
pub const MAX_WINDOW_WIDTH: f64 = 16_384.0;
pub const MAX_WINDOW_HEIGHT: f64 = 16_384.0;
/// Upper bound on a restored window origin, in logical pixels. A multi-monitor
/// desktop can put a window at a negative coordinate, so this bounds the
/// magnitude and not the sign. Whether the saved spot is still on a monitor is
/// a question only the GUI can answer.
const MAX_WINDOW_ORIGIN: f64 = 32_768.0;
const WINDOW_SIZE_EPSILON: f64 = 1.0;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SteamConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key_encrypted: String,
    #[serde(default)]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "is_default_cs2_bridge_config")]
    pub cs2_bridge: Cs2BridgeConfig,
}

/// Connection to an external CS2 account manager exposing level/XP/weekly
/// drop data over HTTP (see the cs2_bridge platform module). `url` is the
/// full endpoint URL (it may embed a secret link key), fetched as-is.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Cs2BridgeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub token_encrypted: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RiotProfileConfig {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_tag_line: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_puuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub snapshot_state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_captured_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RiotConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<RiotProfileConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_profile_id: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BattleNetConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<BattleNetAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BattleNetAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub battle_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct UbisoftConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<UbisoftAccountConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forgotten_uuids: Vec<String>,
    /// The account the engine last switched to, and when. The launcher logs
    /// its sign-in some time after it starts, so until the log is newer than
    /// this the log still names the previous account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_switch: Option<LastSwitch>,
}

/// One switch the engine made, for identity sources that lag behind it.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct LastSwitch {
    pub account_id: String,
    /// Unix milliseconds at the moment the session files were in place.
    pub at: u64,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct UbisoftAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RobloxAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookie_encrypted: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RobloxConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<RobloxAccountConfig>,
}

/// The account record shared by every platform whose entry is just an id, a
/// user label and a last-used stamp: Epic, GOG, Jagex, Discord, and every
/// platform a user descriptor adds. The field names are the ones already on
/// disk, so the aliases below keep each platform's config file byte-identical.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SimpleAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

pub type EpicAccountConfig = SimpleAccountConfig;
pub type GogAccountConfig = SimpleAccountConfig;
pub type JagexAccountConfig = SimpleAccountConfig;
pub type DiscordAccountConfig = SimpleAccountConfig;
pub type CustomAccountConfig = SimpleAccountConfig;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct EpicConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<EpicAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct GogConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<GogAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct JagexConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<JagexAccountConfig>,
    /// Jagex exposes no readable account id, so the id last switched to (or
    /// captured during setup) is the only record of which session is live.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_account: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DiscordConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<DiscordAccountConfig>,
    // Discord exposes no readable current-account id (we never parse leveldb),
    // so the last account switched to is tracked here instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_account_id: String,
}

/// The section of a platform this build was never compiled to know about.
///
/// Every shipped platform has a typed section written before its descriptor
/// existed, and keeps it so nobody's accounts move. A platform the user added
/// has no such history, so one shape carries every field the engine asks the
/// config for: giving each new platform its own struct would mean compiling to
/// add one, which is the thing descriptors exist to avoid.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct CustomPlatformConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<CustomAccountConfig>,
    /// Set only when the descriptor says the launcher exposes no readable
    /// account id, mirroring `jagex.current_account`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_account: String,
    /// Ids forgotten while still on disk, mirroring `ubisoft.forgotten_uuids`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forgotten_ids: Vec<String>,
    /// Mirrors `ubisoft.last_switch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_switch: Option<LastSwitch>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub mode_a_enabled: bool,
    #[serde(default)]
    pub mode_b_enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub install_id: String,
    /// Install identifiers whose server-side Mode B data still needs to be
    /// deleted. Kept locally until `/forget` succeeds so an offline opt-out is
    /// both immediate and retryable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_forget_install_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub anonymous_id: String,
    #[serde(default)]
    pub onboarding_completed: bool,
    /// Whether the one-shot `first_run` event has already been emitted.
    ///
    /// Existing installations default to false and will report a `first_run`
    /// on their next launch, so the event means "first launch that knew how
    /// to report one". Dashboards must read it against the release that
    /// introduced it, not as an install date for the whole population.
    #[serde(default)]
    pub first_run_reported: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            mode_a_enabled: true,
            mode_b_enabled: false,
            install_id: String::new(),
            pending_forget_install_ids: Vec::new(),
            anonymous_id: String::new(),
            onboarding_completed: false,
            first_run_reported: false,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "is_default_steam_config")]
    pub steam: SteamConfig,
    #[serde(default, skip_serializing_if = "is_default_riot_config")]
    pub riot: RiotConfig,
    #[serde(
        default,
        skip_serializing_if = "is_default_battle_net_config",
        rename = "battleNet"
    )]
    pub battle_net: BattleNetConfig,
    #[serde(default, skip_serializing_if = "is_default_ubisoft_config")]
    pub ubisoft: UbisoftConfig,
    #[serde(default, skip_serializing_if = "is_default_roblox_config")]
    pub roblox: RobloxConfig,
    #[serde(default, skip_serializing_if = "is_default_epic_config")]
    pub epic: EpicConfig,
    #[serde(default, skip_serializing_if = "is_default_gog_config")]
    pub gog: GogConfig,
    #[serde(default, skip_serializing_if = "is_default_jagex_config")]
    pub jagex: JagexConfig,
    #[serde(default, skip_serializing_if = "is_default_discord_config")]
    pub discord: DiscordConfig,
    /// Platforms that arrived as a user descriptor, keyed by their id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom_platforms: BTreeMap<String, CustomPlatformConfig>,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub window_width: Option<f64>,
    #[serde(default)]
    pub window_height: Option<f64>,
    /// Window origin in logical pixels. Absent means "no saved placement", and
    /// the GUI centers the window, which is also what every config written
    /// before this field existed says.
    #[serde(default)]
    pub window_x: Option<f64>,
    #[serde(default)]
    pub window_y: Option<f64>,
    /// Scale factor of the monitor the origin was measured on. The origin
    /// times this scale is the exact physical position; without it the
    /// builder converts with the primary monitor's scale, which puts a window
    /// saved on a 150% screen beside a 100% primary a third of the way back.
    #[serde(default)]
    pub window_scale: Option<f64>,
}

#[cfg(test)]
mod tests;
