//! Adding a Steam account: setup jobs, the login launch and cancel.

#[allow(unused_imports)]
use super::*;

pub(super) const STEAM_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Clone)]
pub(super) struct SteamAccountSetupJob {
    pub(super) steam_path: PathBuf,
    pub(super) known_account_ids: HashSet<String>,
    pub(super) launch_started: bool,
    pub(super) error_message: Option<String>,
    /// Steam's autologin value before the setup launch cleared it, so a
    /// cancel can put it back. Recorded by the launch task.
    pub(super) previous_auto_login: Option<String>,
    pub(super) last_touched_at: u64,
}

pub(super) fn steam_setup_jobs() -> &'static Mutex<HashMap<String, SteamAccountSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, SteamAccountSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn begin_account_setup(
    app_handle: AppCtx,
    run_as_admin: bool,
    launch_options: String,
    force_kill: bool,
) -> Result<SetupStatus, PlatformError> {
    let steam_path = resolve_steam_path(&app_handle)?;
    let known_accounts = accounts::get_accounts(&steam_path)
        .map_err(|e| log_platform_failure(&app_handle, "steam.begin_account_setup", e.into()))?;
    let known_account_ids = known_accounts
        .into_iter()
        .map(|account| account.steam_id)
        .collect::<HashSet<_>>();
    let setup_id = format!("steam-setup-{}", Uuid::new_v4());
    let created_at = crate::platforms::now_unix_ms();

    {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| PlatformError::other("Steam setup storage is unavailable"))?;
        jobs.retain(|_, job| {
            !crate::platforms::setup_expired(job.last_touched_at, STEAM_SETUP_TTL_MS)
        });
        jobs.insert(
            setup_id.clone(),
            SteamAccountSetupJob {
                steam_path: steam_path.clone(),
                known_account_ids,
                launch_started: false,
                error_message: None,
                previous_auto_login: None,
                last_touched_at: created_at,
            },
        );
    }

    let setup_id_for_job = setup_id.clone();
    let app_handle_for_job = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        run_steam_setup_launch(
            &app_handle_for_job,
            &setup_id_for_job,
            crate::platforms::SETUP_LAUNCH_LOCK_TIMEOUT,
            || {
                let previous = os::get_auto_login_user().ok();
                let result =
                    accounts::add_account(&steam_path, run_as_admin, &launch_options, force_kill)
                        .map_err(|e| e.to_string());
                (previous, result)
            },
        );
    });

    Ok(crate::platforms::make_setup_status(
        &setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

/// Body of the detached setup launch. The command that started the setup has
/// already released the operation lock by the time this runs, so it takes the
/// lock itself: stopping Steam and rewriting the autologin while a switch does
/// the same would leave Steam on whichever account wrote last.
///
/// `launch` returns the autologin value it found before clearing it, and the
/// launch result. Every outcome, the lock timeout included, lands in the job so
/// the status poll reports it instead of waiting until the TTL.
pub(super) fn run_steam_setup_launch(
    app_handle: &dyn AppContext,
    setup_id: &str,
    lock_timeout: std::time::Duration,
    launch: impl FnOnce() -> (Option<String>, Result<(), String>),
) {
    let job_exists = || {
        steam_setup_jobs()
            .lock()
            .map(|jobs| jobs.contains_key(setup_id))
            .unwrap_or(false)
    };
    let outcome = crate::lock::with_exclusive(app_handle, lock_timeout, || {
        // A cancel that ran while this task waited for the lock removed the
        // job. Launching now would clear the autologin for nobody.
        job_exists().then(launch)
    });
    let (previous_auto_login, result) = match outcome {
        Ok(Some(done)) => done,
        Ok(None) => return,
        Err(error) => (
            None,
            Err(format!("Could not start the Steam account setup: {error}")),
        ),
    };

    if let Err(error) = &result {
        log_platform_error(
            app_handle,
            "steam.begin_account_setup.launch",
            "Steam account setup launch failed",
            error,
        );
    }
    if let Ok(mut jobs) = steam_setup_jobs().lock() {
        if let Some(job) = jobs.get_mut(setup_id) {
            job.launch_started = true;
            job.previous_auto_login = previous_auto_login;
            job.error_message = result.err();
        }
    }
}

/// The autologin value a cancel should put back, if any. Only a launch that
/// went through cleared it (a failed one restores it on its own path), and a
/// value Steam or the user set since the launch wins over the old one.
pub(super) fn autologin_to_restore_on_cancel(
    job: &SteamAccountSetupJob,
    current: &str,
) -> Option<String> {
    if !job.launch_started || job.error_message.is_some() || !current.trim().is_empty() {
        return None;
    }
    job.previous_auto_login
        .as_deref()
        .filter(|previous| !previous.trim().is_empty())
        .map(str::to_string)
}

pub fn get_account_setup_status(
    app_handle: AppCtx,
    setup_id: String,
) -> Result<SetupStatus, PlatformError> {
    let setup_id = setup_id.trim().to_string();
    if setup_id.is_empty() {
        return Err("Invalid Steam setup id".into());
    }

    let job = {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| PlatformError::other("Steam setup storage is unavailable"))?;
        jobs.retain(|_, job| {
            !crate::platforms::setup_expired(job.last_touched_at, STEAM_SETUP_TTL_MS)
        });
        let Some(job) = jobs.get_mut(&setup_id) else {
            // Unknown id here almost always means the TTL purge dropped it.
            return Err(PlatformError::new(
                PlatformErrorKind::SetupExpired,
                "Steam setup not found",
            ));
        };
        job.last_touched_at = crate::platforms::now_unix_ms();
        job.clone()
    };

    if let Some(error) = job.error_message {
        return Ok(crate::platforms::make_setup_status(
            &setup_id, "failed", "", "", error,
        ));
    }

    if !job.launch_started {
        return Ok(crate::platforms::make_setup_status(
            &setup_id,
            "waiting_for_client",
            "",
            "",
            "",
        ));
    }

    let accounts = accounts::get_accounts(&job.steam_path).map_err(|e| {
        log_platform_failure(&app_handle, "steam.get_account_setup_status", e.into())
    })?;
    let maybe_added = accounts
        .into_iter()
        .filter(|account| !job.known_account_ids.contains(&account.steam_id))
        .max_by_key(|account| account.last_login_at.unwrap_or(0));

    if let Some(account) = maybe_added {
        if let Ok(mut jobs) = steam_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
        return Ok(crate::platforms::make_setup_status(
            &setup_id,
            "ready",
            account.steam_id,
            account.persona_name,
            "",
        ));
    }

    Ok(crate::platforms::make_setup_status(
        &setup_id,
        "waiting_for_login",
        "",
        "",
        "",
    ))
}

pub fn cancel_account_setup(app_handle: AppCtx, setup_id: String) -> Result<(), PlatformError> {
    let setup_id = setup_id.trim();
    if setup_id.is_empty() {
        return Ok(());
    }
    let job = {
        let mut jobs = steam_setup_jobs()
            .lock()
            .map_err(|_| PlatformError::other("Steam setup storage is unavailable"))?;
        jobs.retain(|_, job| {
            !crate::platforms::setup_expired(job.last_touched_at, STEAM_SETUP_TTL_MS)
        });
        jobs.remove(setup_id)
    };

    // The setup launch cleared the autologin so Steam would open on its login
    // screen. Put the previous account back, or Steam keeps opening there.
    let Some(job) = job else {
        return Ok(());
    };
    let current = os::get_auto_login_user().unwrap_or_default();
    if let Some(previous) = autologin_to_restore_on_cancel(&job, &current) {
        if let Err(error) = accounts::restore_auto_login_after_setup(&job.steam_path, &previous) {
            log_platform_error(
                &app_handle,
                "steam.cancel_account_setup",
                "Could not restore the Steam autologin after a cancelled setup",
                error.to_string(),
            );
        }
    }
    Ok(())
}
