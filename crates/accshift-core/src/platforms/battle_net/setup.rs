//! Adding a Battle.net account: setup jobs, their status polls and cancel.

#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct BattleNetAccountSetupJob {
    pub(super) known_account_keys: HashSet<String>,
    /// `SavedAccountNames` as the launcher left it before setup emptied it,
    /// put back when the setup is cancelled.
    pub(super) previous_saved: Vec<String>,
    pub(super) last_touched_at: u64,
}

pub(super) fn battle_net_setup_jobs() -> &'static Mutex<HashMap<String, BattleNetAccountSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, BattleNetAccountSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn purge_expired_battle_net_setup_jobs(
    jobs: &mut HashMap<String, BattleNetAccountSetupJob>,
) {
    jobs.retain(|_, job| {
        !crate::platforms::setup_expired(job.last_touched_at, BATTLE_NET_SETUP_TTL_MS)
    });
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

    kill_battle_net()?;
    // Read once the launcher is down: it rewrites the list on exit.
    let previous_saved = read_saved_accounts().unwrap_or_default();
    write_saved_accounts(&app_handle, &[])?;

    let mut jobs = battle_net_setup_jobs()
        .lock()
        .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
    purge_expired_battle_net_setup_jobs(&mut jobs);
    jobs.insert(
        setup_id.clone(),
        BattleNetAccountSetupJob {
            known_account_keys,
            previous_saved: previous_saved.clone(),
            last_touched_at: created_at,
        },
    );
    drop(jobs);

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
        if let Err(restore) = write_saved_accounts(&app_handle, &previous_saved) {
            log_platform_error(
                &app_handle,
                "battle_net.begin_account_setup",
                "Could not restore SavedAccountNames after a failed setup launch",
                restore,
            );
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
    let job = {
        let mut jobs = battle_net_setup_jobs()
            .lock()
            .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
        purge_expired_battle_net_setup_jobs(&mut jobs);
        let Some(job) = jobs.get_mut(&setup_id) else {
            return Err("Battle.net setup session not found".into());
        };
        job.last_touched_at = crate::platforms::now_unix_ms();
        job.clone()
    };

    let accounts = read_saved_accounts().unwrap_or_default();
    if let Some(account) = accounts.iter().find(|account| {
        !job.known_account_keys
            .contains(&normalize_account_key(account))
    }) {
        if let Ok(mut jobs) = battle_net_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
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
    let job = {
        let mut jobs = battle_net_setup_jobs()
            .lock()
            .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
        purge_expired_battle_net_setup_jobs(&mut jobs);
        jobs.remove(&setup_id)
    };
    let Some(job) = job else {
        return Ok(());
    };
    if job.previous_saved.is_empty() {
        return Ok(());
    }
    // The cancel itself succeeds either way: the job is gone, and a list left
    // empty only costs the launcher its remembered accounts until the next switch.
    if let Err(e) = restore_saved_accounts_after_setup(&app_handle, &job.previous_saved) {
        log_platform_error(
            &app_handle,
            "battle_net.cancel_account_setup",
            "Could not restore SavedAccountNames after a cancelled setup",
            e,
        );
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
