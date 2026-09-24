//! Switching the live Riot session to a saved profile.

use super::*;

pub fn switch_profile(app_handle: AppCtx, profile_id: String) -> Result<(), String> {
    log_platform_info(
        &app_handle,
        "riot.switch_profile",
        "Riot profile switch requested",
        build_riot_switch_details(&app_handle, Some(&profile_id)),
    );
    ensure_no_riot_game_running("switching Riot account")?;
    let client_path = resolve_riot_client_path(&app_handle)?;
    let target_id = normalize_profile_id(&profile_id)?;
    let current_live_identity = detect_live_identity().ok();
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    if find_profile(&cfg, &target_id).is_none() {
        return Err("Riot profile not found".into());
    }

    // Riot Client only flushes its in-memory session tokens to
    // RiotGamesPrivateSettings.yaml on a graceful quit. Quit (and wait for the
    // process to exit + settle) BEFORE snapshotting the outgoing profile, so the
    // backup captures rotated tokens rather than the stale pre-rotation ones.
    // Same quit-then-backup order as begin_profile_setup. This is the single
    // quit for the whole switch; the target snapshot is restored afterwards.
    graceful_riot_quit();

    // The client path is resolved once for the whole switch.
    let install_dir = client_path.parent();
    let work = switch_after_quit(
        &app_handle,
        &mut cfg,
        &target_id,
        install_dir,
        current_live_identity.as_ref(),
    );
    // Relaunch even when the work failed, so a failed switch never leaves the
    // client closed.
    let result = relaunch_after_quit(work, || launch_riot_client(&client_path));

    match &result {
        Ok(()) => log_platform_info(
            &app_handle,
            "riot.switch_profile",
            "Riot profile switch completed",
            build_riot_switch_details(&app_handle, Some(&target_id)),
        ),
        Err(error) => log_platform_error(
            &app_handle,
            "riot.switch_profile",
            "Riot profile switch failed",
            format!(
                "error={error}; state={}",
                build_riot_switch_details(&app_handle, Some(&target_id))
            ),
        ),
    }

    result
}

/// Everything a switch does while the client is closed: back the outgoing
/// session up into its own profile, restore the target and save the config.
pub(super) fn switch_after_quit(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    target_id: &str,
    install_dir: Option<&Path>,
    live_identity: Option<&RiotDetectedIdentity>,
) -> Result<(), String> {
    let current_id = cfg.riot.current_profile_id.clone();
    if !current_id.trim().is_empty() && current_id != target_id {
        if !is_valid_profile_id(&current_id) {
            return Err("Invalid Riot profile id in config".into());
        }
        // Only re-backup if the live settings file actually has tokens.
        // begin_profile_setup clears live files to add a new account. Without this
        // check, switching after an add overwrites the good snapshot with a default
        // 484-byte file that has no auth tokens. Checked after the quit so the
        // freshly flushed file is what gates the backup.
        let has_live_tokens = riot_settings_file_ready(install_dir).unwrap_or(false);
        let plan = plan_outgoing_backup(
            find_profile(cfg, &current_id),
            SWITCH_BACKUP_STATES,
            has_live_tokens,
            live_identity,
        );
        match plan {
            OutgoingBackup::Skip => {}
            OutgoingBackup::IdentityMismatch => log_platform_info(
                app_handle,
                "riot.switch_profile",
                "Skipped the outgoing backup: the signed-in Riot account is not this profile's",
                format!("profile={}", crate::platforms::redact_id(&current_id)),
            ),
            OutgoingBackup::Backup { adopt_identity } => {
                backup_live_snapshot(app_handle, &current_id, install_dir)?;
                update_profile_state(
                    cfg,
                    &current_id,
                    Some("ready"),
                    Some(Some(crate::platforms::now_unix_ms())),
                    None,
                    live_identity.filter(|_| adopt_identity),
                )?;
            }
        }
    }

    let restored = restore_live_snapshot(app_handle, target_id, install_dir)?;
    if clear_live_for_target_without_snapshot(restored, &current_id, target_id) {
        // Without this, the client would reopen on the previous account and
        // the next capture would save that account into the target profile.
        clear_live_session_for_login(app_handle, install_dir)?;
        log_platform_info(
            app_handle,
            "riot.switch_profile",
            "Cleared the live session: the target profile has no saved session",
            format!("profile={}", crate::platforms::redact_id(target_id)),
        );
    }

    // Log the restored settings file size to diagnose overwrite issues
    if let Ok(Some(settings_path)) = live_path_for(&RIOT_SNAPSHOT_ITEMS[0], install_dir) {
        let size = fs::metadata(&settings_path).map(|m| m.len()).unwrap_or(0);
        log_platform_info(
            app_handle,
            "riot.switch_profile",
            "Settings file after restore",
            format!("size={size} restored={restored}"),
        );
    }

    cfg.riot.current_profile_id = target_id.to_string();
    let next_state = if restored {
        "ready"
    } else {
        "awaiting_capture"
    };
    update_profile_state(
        cfg,
        target_id,
        Some(next_state),
        None,
        Some(Some(crate::platforms::now_unix_ms())),
        None,
    )?;
    config::save_config(app_handle, cfg)
}
