//! The portable and local halves of the config: what each file holds and how they merge.

use super::*;

pub(super) fn portable_config(config: &AppConfig) -> AppConfig {
    let mut portable = config.clone();
    portable.steam.api_key.clear();
    portable.steam.api_key_encrypted.clear();
    portable.steam.path_override.clear();
    // Bridge config is machine-specific (local URL) and holds an encrypted
    // secret bound to this machine: keep it out of the portable file.
    portable.steam.cs2_bridge = Cs2BridgeConfig::default();
    portable.riot.path_override.clear();
    portable.battle_net.path_override.clear();
    portable.ubisoft.path_override.clear();
    portable.epic.path_override.clear();
    portable.gog.path_override.clear();
    portable.jagex.path_override.clear();
    portable.discord.path_override.clear();
    for section in portable.custom_platforms.values_mut() {
        section.path_override.clear();
    }
    portable.telemetry.install_id.clear();
    portable.telemetry.pending_forget_install_ids.clear();
    portable.telemetry.anonymous_id.clear();
    portable.window_width = None;
    portable.window_height = None;
    portable.window_x = None;
    portable.window_y = None;
    portable.window_scale = None;
    for account in &mut portable.roblox.accounts {
        account.cookie_encrypted.clear();
    }
    portable
}

/// The `path_override` of every shipped platform section, in a fixed order. A
/// new shipped platform is one line here instead of one copied line in
/// `local_config` plus one copied `if` in `merge_split_configs`. Platforms that
/// arrived as a descriptor live in `custom_platforms` and are handled by the
/// loops beside these calls.
pub(super) fn path_overrides(config: &AppConfig) -> [&String; 8] {
    [
        &config.steam.path_override,
        &config.riot.path_override,
        &config.battle_net.path_override,
        &config.ubisoft.path_override,
        &config.epic.path_override,
        &config.gog.path_override,
        &config.jagex.path_override,
        &config.discord.path_override,
    ]
}

/// Same sections as [`path_overrides`], in the same order.
pub(super) fn path_overrides_mut(config: &mut AppConfig) -> [&mut String; 8] {
    [
        &mut config.steam.path_override,
        &mut config.riot.path_override,
        &mut config.battle_net.path_override,
        &mut config.ubisoft.path_override,
        &mut config.epic.path_override,
        &mut config.gog.path_override,
        &mut config.jagex.path_override,
        &mut config.discord.path_override,
    ]
}

/// A value the machine-local config file may or may not carry. "Unset" is
/// what [`AppConfig::default`] leaves behind, so an unset local value never
/// overwrites the portable one during the merge.
pub(super) trait LocalOverride {
    fn is_set(&self) -> bool;
}

impl LocalOverride for String {
    fn is_set(&self) -> bool {
        !self.is_empty()
    }
}

impl LocalOverride for Vec<String> {
    fn is_set(&self) -> bool {
        !self.is_empty()
    }
}

impl<T> LocalOverride for Option<T> {
    fn is_set(&self) -> bool {
        self.is_some()
    }
}

impl LocalOverride for Cs2BridgeConfig {
    fn is_set(&self) -> bool {
        !is_default_cs2_bridge_config(self)
    }
}

pub(super) fn overwrite_if_set<T: LocalOverride>(target: &mut T, local: T) {
    if local.is_set() {
        *target = local;
    }
}

pub(super) fn local_config(config: &AppConfig) -> AppConfig {
    let mut local = AppConfig::default();
    local.steam.api_key = config.steam.api_key.clone();
    local.steam.api_key_encrypted = config.steam.api_key_encrypted.clone();
    local.steam.cs2_bridge = config.steam.cs2_bridge.clone();
    for (target, source) in path_overrides_mut(&mut local)
        .into_iter()
        .zip(path_overrides(config))
    {
        target.clone_from(source);
    }
    // Same rule as every shipped section: where a launcher lives is a fact
    // about this machine, so it never travels in the portable file.
    local.custom_platforms = config
        .custom_platforms
        .iter()
        .filter(|(_, section)| !section.path_override.trim().is_empty())
        .map(|(id, section)| {
            (
                id.clone(),
                CustomPlatformConfig {
                    path_override: section.path_override.clone(),
                    ..CustomPlatformConfig::default()
                },
            )
        })
        .collect();
    local.telemetry.install_id = config.telemetry.install_id.clone();
    local.telemetry.pending_forget_install_ids =
        config.telemetry.pending_forget_install_ids.clone();
    local.telemetry.anonymous_id = config.telemetry.anonymous_id.clone();
    // mode_a_enabled / mode_b_enabled / onboarding_completed live in the portable
    // file. Reset the defaults here so they do not pollute the later merge step.
    local.telemetry.mode_a_enabled = false;
    local.telemetry.mode_b_enabled = false;
    local.telemetry.onboarding_completed = false;
    local.window_width = config.window_width;
    local.window_height = config.window_height;
    local.window_x = config.window_x;
    local.window_y = config.window_y;
    local.window_scale = config.window_scale;
    local.roblox.accounts = config
        .roblox
        .accounts
        .iter()
        .filter(|account| !account.user_id.trim().is_empty())
        .map(|account| RobloxAccountConfig {
            user_id: account.user_id.clone(),
            username: String::new(),
            display_name: String::new(),
            cookie_encrypted: account.cookie_encrypted.clone(),
            last_used_at: account.last_used_at,
        })
        .collect();
    local
}

pub(super) fn merge_split_configs(portable: AppConfig, mut local: AppConfig) -> AppConfig {
    let mut merged = portable;

    // Whole-struct borrow, so it has to run before any field is moved out.
    for (target, source) in path_overrides_mut(&mut merged)
        .into_iter()
        .zip(path_overrides_mut(&mut local))
    {
        overwrite_if_set(target, std::mem::take(source));
    }
    overwrite_if_set(&mut merged.steam.api_key, local.steam.api_key);
    overwrite_if_set(
        &mut merged.steam.api_key_encrypted,
        local.steam.api_key_encrypted,
    );
    overwrite_if_set(&mut merged.steam.cs2_bridge, local.steam.cs2_bridge);
    for (id, section) in local.custom_platforms {
        if section.path_override.trim().is_empty() {
            continue;
        }
        // `or_default` and not `get_mut`: the portable file has no section for
        // a platform the user only ever pointed at a path for.
        merged.custom_platforms.entry(id).or_default().path_override = section.path_override;
    }
    overwrite_if_set(&mut merged.telemetry.install_id, local.telemetry.install_id);
    overwrite_if_set(
        &mut merged.telemetry.pending_forget_install_ids,
        local.telemetry.pending_forget_install_ids,
    );
    overwrite_if_set(
        &mut merged.telemetry.anonymous_id,
        local.telemetry.anonymous_id,
    );
    overwrite_if_set(&mut merged.window_width, local.window_width);
    overwrite_if_set(&mut merged.window_height, local.window_height);
    overwrite_if_set(&mut merged.window_x, local.window_x);
    overwrite_if_set(&mut merged.window_y, local.window_y);
    overwrite_if_set(&mut merged.window_scale, local.window_scale);

    for local_account in local.roblox.accounts {
        if local_account.user_id.trim().is_empty() {
            continue;
        }
        if let Some(existing) = merged
            .roblox
            .accounts
            .iter_mut()
            .find(|account| account.user_id == local_account.user_id)
        {
            if !local_account.cookie_encrypted.is_empty() {
                existing.cookie_encrypted = local_account.cookie_encrypted;
            }
            if local_account.last_used_at.is_some() {
                existing.last_used_at = local_account.last_used_at;
            }
        } else {
            merged.roblox.accounts.push(local_account);
        }
    }

    merged
}
