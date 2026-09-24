//! Adding a Riot profile: the setup launch state machine, polling and expiry.

#[allow(unused_imports)]
use super::*;

/// Where the detached setup launch of a profile stands. Kept in memory: a
/// setup does not survive an app restart anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SetupLaunch {
    /// Waiting for the lock, or quitting the client and clearing the session.
    Running,
    /// The client was relaunched on a cleared session.
    Launched,
    /// The launch failed. The message is shown to the user.
    Failed(String),
}

pub(super) fn setup_launches() -> &'static Mutex<HashMap<String, SetupLaunch>> {
    static LAUNCHES: OnceLock<Mutex<HashMap<String, SetupLaunch>>> = OnceLock::new();
    LAUNCHES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn setup_launch(profile_id: &str) -> Option<SetupLaunch> {
    setup_launches()
        .lock()
        .ok()
        .and_then(|launches| launches.get(profile_id).cloned())
}

pub(super) fn set_setup_launch(profile_id: &str, state: SetupLaunch) {
    if let Ok(mut launches) = setup_launches().lock() {
        launches.insert(profile_id.to_string(), state);
    }
}

/// Record the end of a launch, unless a cancel already dropped the setup.
pub(super) fn finish_setup_launch(profile_id: &str, state: SetupLaunch) {
    if let Ok(mut launches) = setup_launches().lock() {
        if let Some(entry) = launches.get_mut(profile_id) {
            *entry = state;
        }
    }
}

pub(super) fn forget_setup_launch(profile_id: &str) {
    if let Ok(mut launches) = setup_launches().lock() {
        launches.remove(profile_id);
    }
}

/// What the setup poll may do with the live client right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SetupGate {
    /// The client running now is not the one the setup launched.
    Wait,
    /// The setup launch failed.
    Failed(String),
    /// Read the client, and capture once the login is complete.
    Observe,
}

pub(super) fn setup_capture_gate(snapshot_state: &str, launch: Option<&SetupLaunch>) -> SetupGate {
    match launch {
        Some(SetupLaunch::Running) => SetupGate::Wait,
        Some(SetupLaunch::Failed(error)) => SetupGate::Failed(error.clone()),
        Some(SetupLaunch::Launched) => SetupGate::Observe,
        // A pending setup this process never launched: whatever client runs
        // now still holds the previous session.
        None if snapshot_state == "setup_pending" => SetupGate::Wait,
        None => SetupGate::Observe,
    }
}

/// Whether the setup that spawned a launch still wants it once the launch
/// holds the lock. A cancel may have run in between.
pub(super) fn setup_launch_still_wanted(cfg: &config::AppConfig, profile_id: &str) -> bool {
    setup_launch(profile_id) == Some(SetupLaunch::Running)
        && cfg.riot.current_profile_id == profile_id
        && find_profile(cfg, profile_id)
            .is_some_and(|profile| profile.snapshot_state == "setup_pending")
}

/// Body of the detached setup launch: take the operation lock, check the
/// setup still wants its launch, run it, record the outcome.
///
/// The command that started the setup released the lock when it returned, so
/// without taking it here the quit and the session wipe could run in the
/// middle of a switch. The outcome is recorded before the lock is released,
/// so the setup poll (which takes the same lock) never sees a stale state.
pub(super) fn run_riot_setup_launch(
    app_handle: &dyn AppContext,
    profile_id: &str,
    lock_timeout: Duration,
    launch: impl FnOnce() -> Result<(), String>,
) {
    let outcome = crate::lock::with_exclusive(app_handle, lock_timeout, || {
        let cfg = config::load_config(app_handle);
        if !setup_launch_still_wanted(&cfg, profile_id) {
            forget_setup_launch(profile_id);
            return Ok(());
        }
        let result = launch();
        match &result {
            Ok(()) => finish_setup_launch(profile_id, SetupLaunch::Launched),
            Err(error) => finish_setup_launch(profile_id, SetupLaunch::Failed(error.clone())),
        }
        result
    });
    let error = match outcome {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(lock_error) => {
            let error = format!("Could not start the Riot account setup: {lock_error}");
            finish_setup_launch(profile_id, SetupLaunch::Failed(error.clone()));
            error
        }
    };
    log_platform_error(
        app_handle,
        "riot.setup_launch",
        "Riot setup launch failed",
        error,
    );
}

/// Finish an operation that quit the client: relaunch it whether or not the
/// work in between succeeded, so a failure never leaves it closed. The work's
/// error wins over the launch's.
pub(super) fn relaunch_after_quit(
    work: Result<(), String>,
    launch: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let launched = launch();
    work.and(launched)
}

/// One setup poll's view of the client. Logged only when it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetupPollObservation {
    pub(super) profile_id: String,
    pub(super) lockfile: bool,
    pub(super) logged_in: bool,
    pub(super) persist: bool,
    pub(super) settings_ready: bool,
    pub(super) identity: bool,
    pub(super) can_capture: bool,
}

pub(super) fn observation_changed(
    last: &mut Option<SetupPollObservation>,
    next: &SetupPollObservation,
) -> bool {
    if last.as_ref() == Some(next) {
        return false;
    }
    *last = Some(next.clone());
    true
}

/// Process-wide memory of the last logged setup poll.
pub(super) fn setup_poll_changed(next: &SetupPollObservation) -> bool {
    static LAST: OnceLock<Mutex<Option<SetupPollObservation>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
        .lock()
        .map(|mut last| observation_changed(&mut last, next))
        .unwrap_or(true)
}

/// Point `current_profile_id` away from profiles that are being removed.
///
/// It is left empty rather than moved to another profile: the session still
/// live belongs to the removed profile (or to the new account of a cancelled
/// setup), and the next switch backs the live session up into whatever
/// profile is current. With none current, that switch saves nothing.
pub(super) fn release_current_profile(cfg: &mut config::AppConfig, removed_ids: &[String]) {
    if removed_ids
        .iter()
        .any(|id| id == &cfg.riot.current_profile_id)
    {
        cfg.riot.current_profile_id = String::new();
    }
}

/// A switch to a profile with no saved session opens the client's login
/// screen, instead of relaunching whoever is signed in now under that profile.
pub(super) fn clear_live_for_target_without_snapshot(
    restored: bool,
    current_id: &str,
    target_id: &str,
) -> bool {
    // Re-selecting the current profile keeps a login that waits for capture.
    !restored && current_id != target_id
}

/// Wipe the live session so the client opens on its login screen, with the
/// same rollback copy a restore uses in case the wipe stops halfway.
pub(super) fn clear_live_session_for_login(
    app_handle: &dyn AppContext,
    install_dir: Option<&Path>,
) -> Result<(), String> {
    let rollback_dir = backup_live_state_for_rollback(app_handle, install_dir)?;
    let result = clear_live_riot_setup_state(install_dir);
    if result.is_err() {
        restore_live_state_from_rollback(app_handle, &rollback_dir, install_dir);
    }
    discard_rollback_dir(app_handle, &rollback_dir);
    result
}

pub(super) fn prepare_clean_riot_launch(install_dir: Option<&Path>) -> Result<(), String> {
    graceful_riot_quit();
    clear_live_riot_setup_state(install_dir)?;
    kill_riot_client_processes();
    thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
    Ok(())
}

/// Quit the client, clear its session and relaunch it on the login screen, in
/// the background. The setup poll refuses to capture until this records that
/// its own launch finished, so the previous account's session can never be
/// captured into the new profile.
pub(super) fn spawn_riot_setup_launch(
    app_handle: AppCtx,
    profile_id: String,
    client_path: PathBuf,
) {
    set_setup_launch(&profile_id, SetupLaunch::Running);
    tokio::task::spawn_blocking(move || {
        run_riot_setup_launch(
            &app_handle,
            &profile_id,
            crate::platforms::SETUP_LAUNCH_LOCK_TIMEOUT,
            || {
                let prepared = prepare_clean_riot_launch(client_path.parent());
                // Relaunch even when the clear stopped halfway, so the client
                // is not left closed. The failure still fails the setup.
                relaunch_after_quit(prepared, || launch_riot_client(&client_path))
            },
        );
    });
}

pub(super) fn riot_setup_expired(last_touched_at: Option<u64>) -> bool {
    let Some(last_touched_at) = last_touched_at else {
        return true;
    };
    crate::platforms::setup_expired(last_touched_at, RIOT_SETUP_TTL_MS)
}

pub(super) fn cleanup_expired_pending_profiles(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
) -> Result<(), String> {
    let mut changed = false;

    // Profiles that have a detected identity (account_name set) but are still
    // in setup_pending should transition to awaiting_capture instead of being
    // deleted. The user logged in (possibly via 2FA) but session files weren't
    // written in time. They can still re-capture manually.
    for profile in cfg.riot.profiles.iter_mut() {
        if profile.snapshot_state == "setup_pending"
            && riot_setup_expired(profile.last_used_at)
            && !profile.account_name.trim().is_empty()
        {
            profile.snapshot_state = "awaiting_capture".into();
            changed = true;
        }
    }

    // Only delete truly empty pending profiles (no identity detected at all).
    let expired_ids = cfg
        .riot
        .profiles
        .iter()
        .filter(|profile| profile.snapshot_state == "setup_pending")
        .filter(|profile| riot_setup_expired(profile.last_used_at))
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();

    if !expired_ids.is_empty() {
        cfg.riot
            .profiles
            .retain(|profile| !expired_ids.iter().any(|id| id == &profile.id));
        release_current_profile(cfg, &expired_ids);
        for profile_id in &expired_ids {
            forget_setup_launch(profile_id);
        }
        changed = true;
    }

    if changed {
        config::save_config(app_handle, cfg)?;
    }

    for profile_id in expired_ids {
        let snapshot_dir = profile_snapshot_path(app_handle, &profile_id)?;
        if snapshot_dir.exists() {
            free_snapshot_secrets(app_handle, &snapshot_dir);
            fs::remove_dir_all(&snapshot_dir).map_err(|e| {
                format!(
                    "Could not remove expired Riot profile snapshot {}: {e}",
                    snapshot_dir.display()
                )
            })?;
        }
    }

    Ok(())
}

pub(super) fn get_profile_setup_status_internal(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    profile_id: &str,
) -> Result<RiotProfileSetupStatus, String> {
    let Some(profile) = find_profile(cfg, profile_id).cloned() else {
        return Err("Riot profile not found".into());
    };

    if profile.snapshot_state == "ready" {
        return Ok(make_setup_status(&profile, "ready", ""));
    }

    // Until the setup's own launch has quit the client and cleared its
    // session, the client running is still signed in as the previous account.
    // Reading it now would name the new profile after that account and
    // capture its session.
    match setup_capture_gate(&profile.snapshot_state, setup_launch(profile_id).as_ref()) {
        SetupGate::Wait => return Ok(make_setup_status(&profile, "waiting_for_client", "")),
        SetupGate::Failed(error) => return Ok(make_setup_status(&profile, "failed", error)),
        SetupGate::Observe => {}
    }

    let access = read_riot_local_api_access().ok();
    let has_lockfile = access.is_some();

    // Identity detection is optional, used to label the profile, not to gate capture.
    // The alias endpoint fails during 2FA, so we must not require it.
    let identity = access
        .as_ref()
        .and_then(|a| detect_live_identity_with_access(a).ok());
    let mut identity_changed = false;
    if let Some(ref id) = identity {
        if let Some(target) = find_profile_mut(cfg, profile_id) {
            identity_changed = if target.snapshot_state == "setup_pending" {
                adopt_detected_identity(target, id)
            } else {
                apply_detected_identity(target, id)
            };
        }
    }

    // Login status API is the official way to detect completed auth (including 2FA).
    // persist=true ("Stay signed in") is required. Without it, tokens are session-only
    // and won't survive a Riot Client restart, making the captured profile useless.
    let login_state = access
        .as_ref()
        .map(read_riot_login_state)
        .unwrap_or(RiotLoginState {
            logged_in: false,
            persist: false,
        });
    let settings_ready =
        riot_settings_file_ready(resolve_riot_install_dir(app_handle).as_deref()).unwrap_or(false);
    let can_capture = login_state.logged_in && login_state.persist && settings_ready;

    // The poll runs about once a second for up to ten minutes: log the view
    // when it changes, not every tick.
    let observation = SetupPollObservation {
        profile_id: profile_id.to_string(),
        lockfile: has_lockfile,
        logged_in: login_state.logged_in,
        persist: login_state.persist,
        settings_ready,
        identity: identity.is_some(),
        can_capture,
    };
    if setup_poll_changed(&observation) {
        log_platform_info(
            app_handle,
            "riot.setup_poll",
            "Riot setup poll",
            format!(
                "lockfile={has_lockfile} logged_in={} persist={} settings_ready={settings_ready} identity={} can_capture={can_capture}",
                login_state.logged_in, login_state.persist, identity.is_some()
            ),
        );
    }

    if !can_capture {
        // This branch is the steady state of the 1s setup poll, and save_config
        // rewrites both config files and drops the parsed-config cache. Only the
        // detected identity can have changed here (cleanup_expired_pending_profiles
        // persists its own edits), so write only when it actually did.
        if identity_changed {
            let _ = config::save_config(app_handle, cfg);
        }
        let (state, error_msg) = if !has_lockfile {
            ("waiting_for_client", "")
        } else if login_state.logged_in && !login_state.persist {
            (
                "waiting_for_login",
                "Check 'Stay signed in' in the Riot Client, then sign out and sign back in.",
            )
        } else {
            ("waiting_for_login", "")
        };
        let updated = find_profile(cfg, profile_id).cloned().unwrap_or(profile);
        return Ok(make_setup_status(&updated, state, error_msg));
    }

    // Graceful quit flushes the Riot Client's in-memory tokens to disk.
    // Without this, the YAML file contains pre-rotation tokens that the server
    // has already invalidated, making the captured snapshot useless.
    if let Some(target) = find_profile_mut(cfg, profile_id) {
        target.snapshot_state = "capturing".into();
    }
    config::save_config(app_handle, cfg)?;

    graceful_riot_quit();
    capture_profile_into_snapshot(app_handle, cfg, profile_id, identity.as_ref())?;
    forget_setup_launch(profile_id);
    let updated = find_profile(cfg, profile_id).cloned().unwrap_or(profile);
    Ok(make_setup_status(&updated, "ready", ""))
}

pub fn begin_profile_setup(app_handle: AppCtx) -> Result<RiotProfileSetupStatus, String> {
    ensure_no_riot_game_running("starting Riot account setup")?;
    let client_path = resolve_riot_client_path(&app_handle)?;
    let install_dir = client_path.parent();
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;

    // The setup launch clears the live session. Save it first into the
    // profile it belongs to. Graceful quit flushes in-memory tokens to disk,
    // then we backup. Without this, the file contains pre-rotation tokens that
    // are invalid.
    let prev_id = cfg.riot.current_profile_id.clone();
    let prev_needs_backup = !prev_id.is_empty()
        && find_profile(&cfg, &prev_id)
            .is_some_and(|p| SETUP_BACKUP_STATES.contains(&p.snapshot_state.as_str()));
    if prev_needs_backup {
        let identity = detect_live_identity().ok();
        graceful_riot_quit();
        // From here the client is closed: every failure relaunches it.
        let saved = backup_before_setup(&app_handle, &mut cfg, &prev_id, install_dir, identity);
        let started = saved.and_then(|()| start_setup_profile(&app_handle, &mut cfg, &client_path));
        if started.is_err() {
            if let Err(error) = launch_riot_client(&client_path) {
                log_platform_error(
                    &app_handle,
                    "riot.begin_setup",
                    "Could not relaunch Riot Client after a failed setup start",
                    error,
                );
            }
        }
        return started;
    }

    start_setup_profile(&app_handle, &mut cfg, &client_path)
}

/// Save the current profile's live session before a setup clears it. A
/// failure stops the setup: the session would otherwise exist nowhere.
pub(super) fn backup_before_setup(
    app_handle: &dyn AppContext,
    cfg: &mut config::AppConfig,
    prev_id: &str,
    install_dir: Option<&Path>,
    identity: Option<RiotDetectedIdentity>,
) -> Result<(), String> {
    let has_live_tokens = riot_settings_file_ready(install_dir).unwrap_or(false);
    let plan = plan_outgoing_backup(
        find_profile(cfg, prev_id),
        SETUP_BACKUP_STATES,
        has_live_tokens,
        identity.as_ref(),
    );
    match plan {
        OutgoingBackup::Skip => Ok(()),
        OutgoingBackup::IdentityMismatch => {
            log_platform_info(
                app_handle,
                "riot.begin_setup",
                "Skipped the backup before setup: the signed-in Riot account is not the current profile's",
                format!("profile={}", crate::platforms::redact_id(prev_id)),
            );
            Ok(())
        }
        OutgoingBackup::Backup { adopt_identity } => {
            if let Err(e) = backup_live_snapshot(app_handle, prev_id, install_dir) {
                log_platform_error(
                    app_handle,
                    "riot.begin_setup",
                    "Failed to backup current profile before setup",
                    format!("profile={} error={e}", crate::platforms::redact_id(prev_id)),
                );
                return Err(format!(
                    "Could not save the current Riot session before adding an account, so the setup did not start: {e}"
                ));
            }
            let identity = identity.as_ref().filter(|_| adopt_identity);
            let _ = update_profile_state(
                cfg,
                prev_id,
                Some("ready"),
                Some(Some(crate::platforms::now_unix_ms())),
                None,
                identity,
            );
            Ok(())
        }
    }
}

/// Create (or reuse) the pending setup profile, make it current and start the
/// clean launch.
pub(super) fn start_setup_profile(
    app_handle: &AppCtx,
    cfg: &mut config::AppConfig,
    client_path: &Path,
) -> Result<RiotProfileSetupStatus, String> {
    if let Some(existing) = find_pending_setup_profile(cfg).cloned() {
        cfg.riot.current_profile_id = existing.id.clone();
        config::save_config(app_handle, cfg)?;
        spawn_riot_setup_launch(
            app_handle.clone(),
            existing.id.clone(),
            client_path.to_path_buf(),
        );
        return Ok(make_setup_status(&existing, "waiting_for_client", ""));
    }

    let profile_id = format!("riot-profile-{}", Uuid::new_v4());
    let label = next_profile_label(&cfg.riot.profiles);

    cfg.riot.profiles.push(RiotProfileConfig {
        id: profile_id.clone(),
        label,
        account_name: String::new(),
        account_tag_line: String::new(),
        account_puuid: String::new(),
        snapshot_state: "setup_pending".into(),
        notes: String::new(),
        last_captured_at: None,
        last_used_at: Some(crate::platforms::now_unix_ms()),
    });
    cfg.riot.current_profile_id = profile_id.clone();
    config::save_config(app_handle, cfg)?;

    profile_snapshot_dir(app_handle, &profile_id)?;
    spawn_riot_setup_launch(
        app_handle.clone(),
        profile_id.clone(),
        client_path.to_path_buf(),
    );
    let created = find_profile(cfg, &profile_id)
        .cloned()
        .ok_or_else(|| "Riot profile not found".to_string())?;
    Ok(make_setup_status(&created, "waiting_for_client", ""))
}

pub fn get_profile_setup_status(
    app_handle: AppCtx,
    profile_id: String,
) -> Result<RiotProfileSetupStatus, String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    let previous_used_at = find_profile(&cfg, &profile_id).and_then(|p| p.last_used_at);
    let _ = update_profile_state(
        &mut cfg,
        &profile_id,
        None,
        None,
        Some(Some(crate::platforms::now_unix_ms())),
        None,
    );
    // The bump stays in memory on every tick, but persisting it once a second
    // rewrites both config files and drops the parsed-config cache. 60s of
    // granularity is nothing against RIOT_SETUP_TTL_MS (10 minutes): each poll
    // reloads the persisted state, so an actively polled setup can never expire.
    // Only an abandoned setup may expire up to 60s earlier after an app restart.
    let should_persist_used_at = previous_used_at
        .is_none_or(|used_at| crate::platforms::now_unix_ms().saturating_sub(used_at) >= 60_000);
    if should_persist_used_at {
        let _ = config::save_config(&app_handle, &cfg);
    }
    get_profile_setup_status_internal(&app_handle, &mut cfg, &profile_id)
}

pub fn cancel_profile_setup(app_handle: AppCtx, profile_id: String) -> Result<(), String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    let should_remove = cfg
        .riot
        .profiles
        .iter()
        .any(|profile| profile.id == profile_id && profile.snapshot_state == "setup_pending");

    if !should_remove {
        forget_setup_launch(&profile_id);
        return Ok(());
    }

    cfg.riot.profiles.retain(|profile| profile.id != profile_id);
    // The live session may be the new account signed in during the setup:
    // with no profile current, the next switch does not save it anywhere.
    release_current_profile(&mut cfg, std::slice::from_ref(&profile_id));
    config::save_config(&app_handle, &cfg)?;
    forget_setup_launch(&profile_id);

    let snapshot_dir = profile_snapshot_path(&app_handle, &profile_id)?;
    if snapshot_dir.exists() {
        free_snapshot_secrets(&app_handle, &snapshot_dir);
        fs::remove_dir_all(&snapshot_dir).map_err(|e| {
            format!(
                "Could not remove Riot profile snapshot {}: {e}",
                snapshot_dir.display()
            )
        })?;
    }

    Ok(())
}

pub fn capture_profile(app_handle: AppCtx, profile_id: String) -> Result<(), String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    let live_identity = detect_live_identity().ok();
    let mut cfg = config::load_config(&app_handle);
    let Some(profile) = find_profile(&cfg, &profile_id) else {
        return Err("Riot profile not found".into());
    };
    if check_live_identity(profile, live_identity.as_ref()) == IdentityCheck::Mismatch {
        log_platform_info(
            &app_handle,
            "riot.capture_profile",
            "Refused a capture: the signed-in Riot account is not this profile's",
            format!("profile={}", crate::platforms::redact_id(&profile_id)),
        );
        return Err("The Riot Client is signed in to a different account than this profile. Sign in to this profile's account, then capture again.".into());
    }

    capture_profile_into_snapshot(&app_handle, &mut cfg, &profile_id, live_identity.as_ref())
}
