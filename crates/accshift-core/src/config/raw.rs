//! The on-disk shape: raw serde types, default checks and normalisation into `AppConfig`.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, Deserialize, Default)]
pub(super) struct RawAppConfig {
    #[serde(default)]
    pub(super) steam: Option<SteamConfig>,
    #[serde(default)]
    pub(super) riot: Option<RawRiotConfig>,
    #[serde(default, rename = "battleNet", alias = "battle_net")]
    pub(super) battle_net: Option<BattleNetConfig>,
    #[serde(default)]
    pub(super) ubisoft: Option<UbisoftConfig>,
    #[serde(default)]
    pub(super) roblox: Option<RobloxConfig>,
    #[serde(default)]
    pub(super) epic: Option<EpicConfig>,
    #[serde(default)]
    pub(super) gog: Option<GogConfig>,
    #[serde(default)]
    pub(super) jagex: Option<JagexConfig>,
    pub(super) discord: Option<DiscordConfig>,
    #[serde(default)]
    pub(super) custom_platforms: Option<BTreeMap<String, CustomPlatformConfig>>,
    #[serde(default)]
    pub(super) telemetry: Option<TelemetryConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) steam_api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) steam_api_key_encrypted: String,
    #[serde(default)]
    pub(super) steam_path_override: String,
    #[serde(default)]
    pub(super) window_width: Option<f64>,
    #[serde(default)]
    pub(super) window_height: Option<f64>,
    #[serde(default)]
    pub(super) window_x: Option<f64>,
    #[serde(default)]
    pub(super) window_y: Option<f64>,
    #[serde(default)]
    pub(super) window_scale: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub(super) struct RawRiotProfileConfig {
    #[serde(default)]
    pub(super) id: String,
    #[serde(default)]
    pub(super) label: String,
    #[serde(default)]
    pub(super) account_name: String,
    #[serde(default)]
    pub(super) account_tag_line: String,
    #[serde(default)]
    pub(super) account_puuid: String,
    #[serde(default)]
    pub(super) snapshot_state: String,
    #[serde(default)]
    pub(super) notes: String,
    #[serde(default)]
    pub(super) last_captured_at: Option<u64>,
    #[serde(default)]
    pub(super) last_used_at: Option<u64>,
    #[serde(default)]
    pub(super) username: String,
    #[serde(default)]
    pub(super) display_name: String,
    #[serde(default)]
    pub(super) region: String,
    #[serde(default)]
    pub(super) tag_line: String,
    #[serde(default)]
    pub(super) last_login_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub(super) struct RawRiotConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) path_override: String,
    #[serde(default)]
    pub(super) profiles: Vec<RawRiotProfileConfig>,
    #[serde(default)]
    pub(super) accounts: Vec<RawRiotProfileConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) current_profile_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) current_account_id: String,
}

pub(super) fn is_default_steam_config(value: &SteamConfig) -> bool {
    value.api_key.is_empty()
        && value.api_key_encrypted.is_empty()
        && value.path_override.is_empty()
        && is_default_cs2_bridge_config(&value.cs2_bridge)
}

pub(super) fn is_default_cs2_bridge_config(value: &Cs2BridgeConfig) -> bool {
    !value.enabled && value.url.is_empty() && value.token_encrypted.is_empty()
}

pub(super) fn is_default_riot_config(value: &RiotConfig) -> bool {
    value.path_override.is_empty()
        && value.profiles.is_empty()
        && value.current_profile_id.is_empty()
}

pub(super) fn is_default_battle_net_config(value: &BattleNetConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty()
}

pub(super) fn is_default_ubisoft_config(value: &UbisoftConfig) -> bool {
    // The blocklist counts: forgetting the last Ubisoft account leaves a
    // section that holds nothing else, and skipping it here would drop the
    // forget on the next save and rediscover the account from disk.
    value.path_override.is_empty()
        && value.accounts.is_empty()
        && value.forgotten_uuids.is_empty()
        && value.last_switch.is_none()
}

pub(super) fn is_default_roblox_config(value: &RobloxConfig) -> bool {
    value.accounts.is_empty()
}

pub(super) fn is_default_epic_config(value: &EpicConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty()
}

pub(super) fn is_default_gog_config(value: &GogConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty()
}

pub(super) fn is_default_jagex_config(value: &JagexConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty() && value.current_account.is_empty()
}
pub(super) fn is_default_discord_config(value: &DiscordConfig) -> bool {
    value.path_override.is_empty()
        && value.accounts.is_empty()
        && value.current_account_id.is_empty()
}

pub(super) fn normalize_riot_profile(raw: RawRiotProfileConfig) -> RiotProfileConfig {
    let label = if raw.label.trim().is_empty() {
        let legacy = raw.display_name.trim();
        if legacy.is_empty() {
            raw.username.trim().to_string()
        } else {
            legacy.to_string()
        }
    } else {
        raw.label.trim().to_string()
    };

    let snapshot_state = if raw.snapshot_state.trim().is_empty() {
        if raw.last_login_at.is_some()
            || !raw.region.trim().is_empty()
            || !raw.tag_line.trim().is_empty()
        {
            "ready".to_string()
        } else {
            "awaiting_capture".to_string()
        }
    } else {
        raw.snapshot_state.trim().to_string()
    };

    let account_name = if raw.account_name.trim().is_empty() {
        raw.display_name.trim().to_string()
    } else {
        raw.account_name.trim().to_string()
    };

    let account_tag_line = if raw.account_tag_line.trim().is_empty() {
        raw.tag_line.trim().to_string()
    } else {
        raw.account_tag_line.trim().to_string()
    };

    let account_puuid = raw.account_puuid.trim().to_string();

    RiotProfileConfig {
        id: raw.id,
        label,
        account_name,
        account_tag_line,
        account_puuid,
        snapshot_state,
        notes: raw.notes,
        last_captured_at: raw.last_captured_at.or(raw.last_login_at),
        last_used_at: raw.last_used_at.or(raw.last_login_at),
    }
}

pub(super) fn normalize_riot_config(raw: Option<RawRiotConfig>) -> RiotConfig {
    let Some(raw) = raw else {
        return RiotConfig::default();
    };

    let source_profiles = if raw.profiles.is_empty() {
        raw.accounts
    } else {
        raw.profiles
    };

    RiotConfig {
        path_override: raw.path_override,
        profiles: source_profiles
            .into_iter()
            .map(normalize_riot_profile)
            .collect(),
        current_profile_id: if raw.current_profile_id.trim().is_empty() {
            raw.current_account_id
        } else {
            raw.current_profile_id
        },
    }
}

pub(super) fn normalize_config(raw: RawAppConfig) -> AppConfig {
    let mut steam = raw.steam.unwrap_or_default();
    if steam.api_key.is_empty() {
        steam.api_key = raw.steam_api_key;
    }
    if steam.api_key_encrypted.is_empty() {
        steam.api_key_encrypted = raw.steam_api_key_encrypted;
    }
    if steam.path_override.is_empty() {
        steam.path_override = raw.steam_path_override;
    }
    let riot = normalize_riot_config(raw.riot);
    let battle_net = raw.battle_net.unwrap_or_default();
    let ubisoft = raw.ubisoft.unwrap_or_default();
    let roblox = raw.roblox.unwrap_or_default();
    let epic = raw.epic.unwrap_or_default();
    let gog = raw.gog.unwrap_or_default();
    let jagex = raw.jagex.unwrap_or_default();
    let discord = raw.discord.unwrap_or_default();
    let custom_platforms = raw.custom_platforms.unwrap_or_default();
    let telemetry = raw.telemetry.unwrap_or_default();
    AppConfig {
        steam,
        riot,
        battle_net,
        ubisoft,
        roblox,
        epic,
        gog,
        jagex,
        discord,
        custom_platforms,
        telemetry,
        window_width: raw.window_width,
        window_height: raw.window_height,
        window_x: raw.window_x,
        window_y: raw.window_y,
        window_scale: raw.window_scale,
    }
}
