//! Switching the live Riot session to a saved profile.

#[allow(unused_imports)]
use super::*;

pub fn switch_profile(app_handle: AppCtx, profile_id: String) -> Result<(), String> {
    run_switch(
        &app_handle,
        &profile_id,
        "riot.switch_profile",
        RIOT_CLIENT_PRODUCT,
    )
}

/// The switch, logged under `source`, that ends by starting `product_id`
/// (the bare client for a plain switch). A failed switch starts the bare
/// client instead, so a game never opens on the previous account.
pub(super) fn run_switch(
    app_handle: &AppCtx,
    profile_id: &str,
    source: &'static str,
    product_id: &str,
) -> Result<(), String> {
    let details = build_riot_switch_details(app_handle, Some(profile_id));
    log_platform_info(
        app_handle,
        source,
        "Riot profile switch requested",
        if product_id == RIOT_CLIENT_PRODUCT {
            details
        } else {
            format!("product={product_id}; state={details}")
        },
    );
    ensure_no_riot_game_running("switching Riot account")?;
    let client_path = resolve_riot_client_path(app_handle)?;
    let target_id = normalize_profile_id(profile_id)?;
    let current_live_identity = detect_live_identity().ok();
    let mut cfg = config::load_config(app_handle);
    cleanup_expired_pending_profiles(app_handle, &mut cfg)?;
    if find_profile(&cfg, &target_id).is_none() {
        return Err("Riot profile not found".into());
    }

    // Riot Client only flushes its in-memory session tokens to
    // RiotGamesPrivateSettings.yaml on a graceful quit. Quit (and wait for the
    // process to exit + settle) BEFORE snapshotting the outgoing profile, so the
    // backup captures rotated tokens rather than the stale pre-rotation ones.
    // Same quit-then-backup order as begin_profile_setup. This is the single
    // quit for the whole switch; the target snapshot is restored afterwards.
    // A client that could not be closed fails the switch here, before any
    // session file is touched.
    if let Err(error) = graceful_riot_quit() {
        log_platform_error(
            app_handle,
            source,
            "Riot profile switch failed",
            format!(
                "error={error}; state={}",
                build_riot_switch_details(app_handle, Some(&target_id))
            ),
        );
        return Err(error);
    }

    // The client path is resolved once for the whole switch.
    let install_dir = client_path.parent();
    let launch = |product: &str| launch_riot_product(&client_path, product);
    let identity = || detect_live_identity().ok();
    let io = SwitchIo {
        launch: &launch,
        quit: &graceful_riot_quit,
        identity: &identity,
        wait: UNCONFIRMED_IDENTITY_WAIT,
        poll: UNCONFIRMED_IDENTITY_POLL,
    };
    let result = finish_switch(
        app_handle,
        &mut cfg,
        &target_id,
        install_dir,
        current_live_identity.as_ref(),
        product_id,
        &io,
    );

    match &result {
        Ok(()) => log_platform_info(
            app_handle,
            source,
            "Riot profile switch completed",
            build_riot_switch_details(app_handle, Some(&target_id)),
        ),
        Err(error) => log_platform_error(
            app_handle,
            source,
            "Riot profile switch failed",
            format!(
                "error={error}; state={}",
                build_riot_switch_details(app_handle, Some(&target_id))
            ),
        ),
    }

    result
}

/// Start of the error a game launch returns when the account signed in to the
/// client could not be confirmed. The app matches it to show its own toast.
pub(super) const RIOT_ACCOUNT_UNCONFIRMED: &str = "Riot account not confirmed";

/// What `switch_after_quit` left live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SwitchWork {
    /// The target's session is live: restored, or kept and refreshed.
    Switched,
    /// The target is the current profile and the live session has tokens, but
    /// no identity check could say whose they are (client closed, local API
    /// down). Neither the live files nor the snapshot were touched.
    KeptUnconfirmed,
}

/// How long a switch to the current profile waits for the reopened client
/// to say who is signed in, and how often it asks.
const UNCONFIRMED_IDENTITY_WAIT: Duration = Duration::from_secs(45);
const UNCONFIRMED_IDENTITY_POLL: Duration = Duration::from_secs(1);

/// The process side of a switch once the client is closed, injected so the
/// orchestration runs in tests without a Riot client.
pub(super) struct SwitchIo<'a> {
    /// Starts the client with a product (`riot-client` for the bare client).
    pub launch: &'a dyn Fn(&str) -> Result<(), String>,
    /// Quits the client gracefully, so it flushes its session files.
    pub quit: &'a dyn Fn() -> Result<(), String>,
    /// The account the running client reports, `None` while it cannot say.
    pub identity: &'a dyn Fn() -> Option<RiotDetectedIdentity>,
    pub wait: Duration,
    pub poll: Duration,
}

/// The session work and the relaunch, once the client is closed. A failed
/// work starts the bare client, so a game never opens on the wrong account.
pub(super) fn finish_switch(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    target_id: &str,
    install_dir: Option<&Path>,
    live_identity: Option<&RiotDetectedIdentity>,
    product_id: &str,
    io: &SwitchIo,
) -> Result<(), String> {
    match switch_after_quit(app_handle, cfg, target_id, install_dir, live_identity) {
        Ok(SwitchWork::KeptUnconfirmed) => {
            resolve_unconfirmed(app_handle, cfg, target_id, install_dir, product_id, io)
        }
        work => {
            // Relaunch even when the work failed, so a failed switch never
            // leaves the client closed.
            let work = work.map(|_| ());
            let launched = product_after_switch(work.is_ok(), product_id);
            relaunch_after_quit(work, || (io.launch)(launched))
        }
    }
}

/// The current profile was re-selected over a session nobody could
/// attribute. Open the bare client on it, untouched, and ask who is signed
/// in: the profile's account gets the product in the same request, another
/// account is quit and the profile restored, and no answer is an error with
/// both sessions kept.
fn resolve_unconfirmed(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    target_id: &str,
    install_dir: Option<&Path>,
    product_id: &str,
    io: &SwitchIo,
) -> Result<(), String> {
    let Some(profile) = find_profile(cfg, target_id).cloned() else {
        return Err("Riot profile not found".into());
    };
    (io.launch)(RIOT_CLIENT_PRODUCT)?;
    let Some(identity) = wait_for_identity(&profile, io) else {
        return Err(format!(
            "{RIOT_ACCOUNT_UNCONFIRMED}: Riot Client did not report the signed-in account in time; nothing was changed."
        ));
    };
    if check_live_identity(&profile, Some(&identity)) == IdentityCheck::Match {
        log_platform_info(
            app_handle,
            "riot.switch_profile",
            "Confirmed the kept live session as the target profile's",
            format!("profile={}", crate::platforms::redact_id(target_id)),
        );
        update_profile_state(
            cfg,
            target_id,
            None,
            None,
            Some(Some(crate::platforms::now_unix_ms())),
            Some(&identity),
        )?;
        config::save_config(app_handle, cfg)?;
        if product_id == RIOT_CLIENT_PRODUCT {
            return Ok(());
        }
        return (io.launch)(product_id);
    }
    // Another account, or an account a profile without one can adopt: the
    // regular switch knows what to do now that the identity is known.
    (io.quit)()?;
    let work = match switch_after_quit(app_handle, cfg, target_id, install_dir, Some(&identity)) {
        Ok(SwitchWork::Switched) => Ok(()),
        Ok(SwitchWork::KeptUnconfirmed) => Err(format!(
            "{RIOT_ACCOUNT_UNCONFIRMED}: the signed-in account could not be checked; nothing was changed."
        )),
        Err(error) => Err(error),
    };
    let launched = product_after_switch(work.is_ok(), product_id);
    relaunch_after_quit(work, || (io.launch)(launched))
}

/// Polls the client until its account can be compared with `profile`, at
/// most `io.wait`.
fn wait_for_identity(profile: &RiotProfileConfig, io: &SwitchIo) -> Option<RiotDetectedIdentity> {
    let deadline = std::time::Instant::now() + io.wait;
    loop {
        let identity = (io.identity)();
        if check_live_identity(profile, identity.as_ref()) != IdentityCheck::Unknown {
            return identity;
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        thread::sleep(io.poll);
    }
}

/// Everything a switch does while the client is closed: back the outgoing
/// session up into its own profile, restore the target and save the config.
///
/// Re-selecting the current profile (a game launch on it, a click on its
/// card) never restores its older snapshot over a live session with tokens:
/// - the identity check confirms the profile's account: the snapshot is
///   refreshed from the live session, which is kept;
/// - no identity is available: both are left as they are, since the live
///   tokens may be the profile's newer ones or another account's
///   (`finish_switch` then asks the reopened client who is signed in);
/// - another account is proven signed in: the profile's snapshot is restored.
pub(super) fn switch_after_quit(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    target_id: &str,
    install_dir: Option<&Path>,
    live_identity: Option<&RiotDetectedIdentity>,
) -> Result<SwitchWork, String> {
    let current_id = cfg.riot.current_profile_id.clone();
    let mut kept_live = false;
    if !current_id.trim().is_empty() {
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
            OutgoingBackup::Backup {
                adopt_identity: false,
            } if current_id == target_id => {
                log_platform_info(
                    app_handle,
                    "riot.switch_profile",
                    "Kept the live session unchanged: the signed-in Riot account could not be confirmed",
                    format!("profile={}", crate::platforms::redact_id(target_id)),
                );
                config::save_config(app_handle, cfg)?;
                return Ok(SwitchWork::KeptUnconfirmed);
            }
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
                kept_live = current_id == target_id;
            }
        }
    }

    let restored = if kept_live {
        log_platform_info(
            app_handle,
            "riot.switch_profile",
            "Kept the live session: the target is the current profile's signed-in account",
            format!("profile={}", crate::platforms::redact_id(target_id)),
        );
        true
    } else {
        restore_live_snapshot(app_handle, target_id, install_dir)?
    };
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
    config::save_config(app_handle, cfg)?;
    Ok(SwitchWork::Switched)
}
