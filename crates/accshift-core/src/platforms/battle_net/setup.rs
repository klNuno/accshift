//! Adding a Battle.net account: setup jobs, their status polls and cancel.

#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct BattleNetAccountSetupJob {
    pub(super) known_account_keys: HashSet<String>,
    /// `SavedAccountNames` as the launcher left it before setup emptied it,
    /// put back when the setup is cancelled, and by the next switch or forget
    /// when it ends any other way.
    pub(super) previous_saved: Vec<String>,
    pub(super) last_touched_at: u64,
}

pub(super) fn battle_net_setup_jobs() -> &'static Mutex<HashMap<String, BattleNetAccountSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, BattleNetAccountSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn purge_expired_battle_net_setup_jobs(
    jobs: &mut HashMap<String, BattleNetAccountSetupJob>,
) -> Vec<String> {
    let mut expired_saved = Vec::new();
    jobs.retain(|_, job| {
        let expired = crate::platforms::setup_expired(job.last_touched_at, BATTLE_NET_SETUP_TTL_MS);
        if expired {
            expired_saved =
                merge_saved_after_setup(std::mem::take(&mut expired_saved), &job.previous_saved);
        }
        !expired
    });
    expired_saved
}

/// Persists saved names that setups emptied out of `SavedAccountNames` and
/// that no cancel put back. A setup that ends with a sign-in leaves the
/// launcher running on the new account; quitting it then to restore the list
/// would undo what the user just did, so the next switch or forget, which
/// quits the launcher anyway, writes them back — even after an app restart.
pub(super) fn owe_saved_accounts(app_handle: &dyn AppContext, previous: &[String]) {
    if let Err(err) = try_owe_saved_accounts(app_handle, previous) {
        log_platform_error(
            app_handle,
            "battle_net.owe_saved_accounts",
            "Could not persist owed Battle.net saved account names",
            err,
        );
    }
}

/// [`owe_saved_accounts`] for a caller that must not go on when the names
/// could not be saved.
pub(super) fn try_owe_saved_accounts(
    app_handle: &dyn AppContext,
    previous: &[String],
) -> Result<(), String> {
    let incoming = collect_unique_accounts(previous.iter().cloned(), &mut HashSet::new());
    if incoming.is_empty() {
        return Ok(());
    }
    config::update_config(app_handle, |cfg| {
        let current = std::mem::take(&mut cfg.battle_net.owed_saved_accounts);
        cfg.battle_net.owed_saved_accounts = merge_saved_after_setup(current, &incoming);
    })
}

/// Hands the launcher's saved list over to the local config before a setup
/// empties it: `to_owe` (this setup's `previous_saved` plus the lists of
/// expired setups) is saved, then `empty_launcher_list` clears
/// SavedAccountNames. The copy goes first, so a crash between the two leaves
/// the names in both places instead of neither, and a copy that cannot be
/// saved aborts the setup before the launcher's list is touched.
pub(super) fn owe_then_empty_saved_accounts(
    app_handle: &dyn AppContext,
    to_owe: &[String],
    previous_saved: &[String],
    empty_launcher_list: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    try_owe_saved_accounts(app_handle, to_owe)?;
    if let Err(err) = empty_launcher_list() {
        // The launcher still holds this list; only the expired setups' names
        // stay owed.
        settle_owed_saved_accounts(app_handle, previous_saved);
        return Err(err);
    }
    Ok(())
}

/// `fresh` with every persisted owed name appended once, for a write that quit
/// the launcher first. Returns both the merged list and the snapshot of owed
/// names included in it so [`settle_owed_saved_accounts`] clears only what was
/// actually written.
pub(super) fn with_owed_saved_accounts(
    app_handle: &dyn AppContext,
    fresh: Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let owed = config::load_config(app_handle)
        .battle_net
        .owed_saved_accounts;
    if owed.is_empty() {
        return (fresh, owed);
    }
    let merged = merge_saved_after_setup(fresh, &owed);
    (merged, owed)
}

/// Called once a list built by [`with_owed_saved_accounts`] (or a setup
/// restore) is on disk. Removes only `settled` so names owed concurrently stay
/// persisted.
pub(super) fn settle_owed_saved_accounts(app_handle: &dyn AppContext, settled: &[String]) {
    if settled.is_empty() {
        return;
    }
    let settled_keys: HashSet<String> = settled
        .iter()
        .map(|name| normalize_account_key(name))
        .collect();
    if let Err(err) = config::update_config(app_handle, |cfg| {
        cfg.battle_net
            .owed_saved_accounts
            .retain(|name| !settled_keys.contains(&normalize_account_key(name)));
    }) {
        log_platform_error(
            app_handle,
            "battle_net.settle_owed_saved_accounts",
            "Could not settle owed Battle.net saved account names",
            err,
        );
    }
}

/// Runs `work` once `stop` has quit the launcher. When `work` fails, a
/// launcher that was running before is started again, so a setup that gives
/// up does not leave Battle.net closed; the error returned stays `work`'s,
/// and a failed restart is only logged.
pub(super) fn run_with_launcher_stopped<T>(
    app_handle: &dyn AppContext,
    was_running: bool,
    stop: impl FnOnce() -> Result<(), String>,
    work: impl FnOnce() -> Result<T, String>,
    restart: impl FnOnce() -> Result<(), String>,
) -> Result<T, String> {
    stop()?;
    let result = work();
    if result.is_err() && was_running {
        if let Err(err) = restart() {
            log_platform_error(
                app_handle,
                "battle_net.begin_account_setup",
                "Could not start Battle.net again after an aborted setup",
                err,
            );
        }
    }
    result
}

pub fn begin_account_setup(app_handle: AppCtx) -> Result<SetupStatus, String> {
    log_platform_info(
        &app_handle,
        "battle_net.begin_account_setup",
        "Battle.net account setup requested",
        build_battle_net_switch_details(None),
    );
    let known_accounts = known_account_emails(&app_handle).unwrap_or_default();
    let setup_id = format!("battle-net-setup-{}", Uuid::new_v4());
    let created_at = crate::platforms::now_unix_ms();
    let known_account_keys = known_accounts
        .iter()
        .map(|account| normalize_account_key(account))
        .collect::<HashSet<_>>();

    let was_running = is_battle_net_running();
    let previous_saved = run_with_launcher_stopped(
        &*app_handle,
        was_running,
        kill_battle_net,
        || {
            // Read once the launcher is down: it saves the list while it runs.
            let previous_saved = read_saved_accounts().unwrap_or_default();

            let expired = {
                let mut jobs = battle_net_setup_jobs()
                    .lock()
                    .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
                let expired = purge_expired_battle_net_setup_jobs(&mut jobs);
                jobs.insert(
                    setup_id.clone(),
                    BattleNetAccountSetupJob {
                        known_account_keys,
                        previous_saved: previous_saved.clone(),
                        last_touched_at: created_at,
                    },
                );
                expired
            };
            // Saved before SavedAccountNames is emptied so even a restart or
            // crash mid-setup does not lose the user's previously saved
            // accounts.
            let to_owe = merge_saved_after_setup(expired, &previous_saved);
            if let Err(e) =
                owe_then_empty_saved_accounts(&*app_handle, &to_owe, &previous_saved, || {
                    write_saved_accounts(&app_handle, &[])
                })
            {
                log_platform_error(
                    &app_handle,
                    "battle_net.begin_account_setup",
                    "Battle.net account setup aborted before emptying SavedAccountNames",
                    &e,
                );
                if let Ok(mut jobs) = battle_net_setup_jobs().lock() {
                    jobs.remove(&setup_id);
                }
                return Err(e);
            }
            Ok(previous_saved)
        },
        || launch_battle_net(&app_handle),
    )?;

    if let Err(e) = launch_battle_net(&app_handle) {
        log_platform_error(
            &app_handle,
            "battle_net.begin_account_setup",
            "Battle.net account setup launch failed",
            &e,
        );
        // No setup id reaches the UI, so nothing would cancel this one.
        if let Ok(mut jobs) = battle_net_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
        match write_saved_accounts(&app_handle, &previous_saved) {
            Ok(()) => settle_owed_saved_accounts(&*app_handle, &previous_saved),
            Err(restore) => log_platform_error(
                &app_handle,
                "battle_net.begin_account_setup",
                "Could not restore SavedAccountNames after a failed setup launch",
                restore,
            ),
        }
        return Err(e);
    }
    Ok(crate::platforms::make_setup_status(
        &setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

pub fn get_account_setup_status(
    app_handle: AppCtx,
    setup_id: String,
) -> Result<SetupStatus, String> {
    let (job, expired) = {
        let mut jobs = battle_net_setup_jobs()
            .lock()
            .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
        let expired = purge_expired_battle_net_setup_jobs(&mut jobs);
        let job = jobs.get_mut(&setup_id).map(|job| {
            job.last_touched_at = crate::platforms::now_unix_ms();
            job.clone()
        });
        (job, expired)
    };
    owe_saved_accounts(&*app_handle, &expired);
    let Some(job) = job else {
        return Err("Battle.net setup session not found".into());
    };

    let accounts = read_saved_accounts().unwrap_or_default();
    if let Some(account) = accounts.iter().find(|account| {
        !job.known_account_keys
            .contains(&normalize_account_key(account))
    }) {
        if let Ok(mut jobs) = battle_net_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
        owe_saved_accounts(&*app_handle, &job.previous_saved);
        let _ = remember_account_usage(&app_handle, account, true);
        return Ok(crate::platforms::make_setup_status(
            &setup_id,
            "ready",
            account.clone(),
            battle_net_display_name(account),
            "",
        ));
    }

    if is_battle_net_running() {
        return Ok(crate::platforms::make_setup_status(
            &setup_id,
            "waiting_for_login",
            "",
            "",
            "",
        ));
    }

    Ok(crate::platforms::make_setup_status(
        &setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

pub fn cancel_account_setup(app_handle: AppCtx, setup_id: String) -> Result<(), String> {
    let (job, expired) = {
        let mut jobs = battle_net_setup_jobs()
            .lock()
            .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
        let expired = purge_expired_battle_net_setup_jobs(&mut jobs);
        (jobs.remove(&setup_id), expired)
    };
    owe_saved_accounts(&*app_handle, &expired);
    let Some(job) = job else {
        return Ok(());
    };
    if job.previous_saved.is_empty() {
        return Ok(());
    }
    // The cancel itself succeeds either way: the job is gone, and a list left
    // empty only costs the launcher its remembered accounts until the next switch.
    match restore_saved_accounts_after_setup(&app_handle, &job.previous_saved) {
        Ok(()) => settle_owed_saved_accounts(&*app_handle, &job.previous_saved),
        Err(e) => log_platform_error(
            &app_handle,
            "battle_net.cancel_account_setup",
            "Could not restore SavedAccountNames after a cancelled setup",
            e,
        ),
    }
    Ok(())
}

/// Puts back the list setup emptied, keeping in front any account the launcher
/// saved during the setup. The launcher is restarted when it was running,
/// since it rewrites the file on exit.
pub(super) fn restore_saved_accounts_after_setup(
    app_handle: &dyn AppContext,
    previous: &[String],
) -> Result<(), String> {
    let was_running = is_battle_net_running();
    if was_running {
        kill_battle_net()?;
    }
    let current = read_saved_accounts().unwrap_or_default();
    write_saved_accounts(app_handle, &merge_saved_after_setup(current, previous))?;
    if was_running {
        launch_battle_net(app_handle)?;
    }
    Ok(())
}

/// The accounts saved during a setup first, then the ones saved before it,
/// each once.
pub(super) fn merge_saved_after_setup(current: Vec<String>, previous: &[String]) -> Vec<String> {
    collect_unique_accounts(
        current.into_iter().chain(previous.iter().cloned()),
        &mut HashSet::new(),
    )
}
