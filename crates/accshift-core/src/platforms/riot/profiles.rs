//! Profile list helpers: visibility, labels, current profile and state updates.

#[allow(unused_imports)]
use super::*;

pub(super) fn next_profile_label(profiles: &[RiotProfileConfig]) -> String {
    let mut next_index = profiles.len() + 1;
    loop {
        let candidate = format!("Riot Profile {next_index}");
        if !profiles
            .iter()
            .any(|profile| profile.label.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        next_index += 1;
    }
}

pub(super) fn current_profile_id(cfg: &config::AppConfig) -> String {
    let configured = cfg.riot.current_profile_id.trim();
    if !configured.is_empty()
        && cfg
            .riot
            .profiles
            .iter()
            .any(|profile| profile.id == configured)
    {
        return configured.to_string();
    }
    cfg.riot
        .profiles
        .first()
        .map(|profile| profile.id.clone())
        .unwrap_or_default()
}

pub(super) fn is_visible_profile(profile: &RiotProfileConfig) -> bool {
    profile.snapshot_state != "setup_pending"
}

pub(super) fn visible_profiles(cfg: &config::AppConfig) -> Vec<RiotProfileConfig> {
    cfg.riot
        .profiles
        .iter()
        .filter(|profile| is_visible_profile(profile))
        .cloned()
        .collect()
}

pub(super) fn visible_current_profile_id(cfg: &config::AppConfig) -> String {
    let current_id = current_profile_id(cfg);
    if current_id.is_empty() {
        return current_id;
    }
    cfg.riot
        .profiles
        .iter()
        .find(|profile| profile.id == current_id && is_visible_profile(profile))
        .map(|profile| profile.id.clone())
        .unwrap_or_default()
}

pub(super) fn find_pending_setup_profile(cfg: &config::AppConfig) -> Option<&RiotProfileConfig> {
    cfg.riot
        .profiles
        .iter()
        .find(|profile| profile.snapshot_state == "setup_pending")
}

pub(super) fn update_profile_state(
    cfg: &mut config::AppConfig,
    profile_id: &str,
    snapshot_state: Option<&str>,
    captured_at: Option<Option<u64>>,
    used_at: Option<Option<u64>>,
    identity: Option<&RiotDetectedIdentity>,
) -> Result<(), String> {
    let Some(profile) = find_profile_mut(cfg, profile_id) else {
        return Err("Riot profile not found".into());
    };

    if let Some(state) = snapshot_state {
        profile.snapshot_state = state.to_string();
    }
    if let Some(captured) = captured_at {
        profile.last_captured_at = captured;
    }
    if let Some(used) = used_at {
        profile.last_used_at = used;
    }
    if let Some(identity) = identity {
        apply_detected_identity(profile, identity);
    }
    Ok(())
}

pub(super) fn capture_profile_into_snapshot(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    profile_id: &str,
    identity: Option<&RiotDetectedIdentity>,
) -> Result<(), String> {
    backup_live_snapshot(
        app_handle,
        profile_id,
        resolve_riot_install_dir(app_handle).as_deref(),
    )?;
    cfg.riot.current_profile_id = profile_id.to_string();
    update_profile_state(
        cfg,
        profile_id,
        Some("ready"),
        Some(Some(crate::platforms::now_unix_ms())),
        Some(Some(crate::platforms::now_unix_ms())),
        identity,
    )?;
    config::save_config(app_handle, cfg)
}
