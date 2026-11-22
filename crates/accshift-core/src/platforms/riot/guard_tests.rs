//! The League guard around closing the Riot client: the forced close, the
//! relaunch after a refusal, the profile a refused setup leaves current and
//! the order of the game checks. No Riot or League process is touched.

use super::*;
use std::cell::Cell;
use std::sync::Arc;

struct TempCtx {
    root: PathBuf,
}

impl AppContext for TempCtx {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
}

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-guard-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn champ_select_refusal() -> String {
    league_phase_guard_error(Some("ChampSelect"), "closing Riot Client").unwrap_err()
}

fn profile(id: &str, state: &str) -> RiotProfileConfig {
    RiotProfileConfig {
        id: id.into(),
        label: format!("label-{id}"),
        account_name: String::new(),
        account_tag_line: String::new(),
        account_puuid: String::new(),
        snapshot_state: state.into(),
        notes: String::new(),
        last_captured_at: None,
        // Fresh, so the cancel's cleanup of expired setups keeps it.
        last_used_at: Some(crate::platforms::now_unix_ms()),
    }
}

// -- S1: the forced close re-reads the League phase ----------------------

#[test]
fn the_forced_close_after_the_quit_wait_rechecks_the_league_phase() {
    // The phase is fine when the quit is asked; the player enters champion
    // select while the client takes its time to exit.
    let guards = Cell::new(0);
    let kills = Cell::new(0);
    let guard = || {
        guards.set(guards.get() + 1);
        if guards.get() == 1 {
            Ok(())
        } else {
            Err(champ_select_refusal())
        }
    };
    let force_close = || {
        kills.set(kills.get() + 1);
        Ok(())
    };
    let io = QuitIo {
        guard: &guard,
        request_quit: &|| QuitRequest::Sent,
        client_running: &|| true,
        wait: &|| {},
        force_close: &force_close,
        after_exit: &|| {},
        polls: 3,
    };

    let result = run_graceful_quit(&io);

    assert_eq!(kills.get(), 0, "League was killed in champion select");
    let error = result.expect_err("the forced close must be refused");
    // A quit was asked already: the caller still relaunches the client.
    assert!(!error.is_refusal(), "{error:?}");
    assert!(String::from(error).starts_with(RIOT_LEAGUE_IN_CHAMP_SELECT));
}

#[test]
fn the_forced_close_without_the_local_api_rechecks_the_league_phase() {
    let guards = Cell::new(0);
    let kills = Cell::new(0);
    let guard = || {
        guards.set(guards.get() + 1);
        if guards.get() == 1 {
            Ok(())
        } else {
            Err(champ_select_refusal())
        }
    };
    let force_close = || {
        kills.set(kills.get() + 1);
        Ok(())
    };
    let io = QuitIo {
        guard: &guard,
        request_quit: &|| QuitRequest::NoApi,
        client_running: &|| true,
        wait: &|| {},
        force_close: &force_close,
        after_exit: &|| {},
        polls: 3,
    };

    let result = run_graceful_quit(&io);

    assert_eq!(kills.get(), 0, "League was killed in champion select");
    // No quit was asked and nothing killed: nothing to relaunch.
    assert_eq!(result, Err(CloseError::Refused(champ_select_refusal())));
}

#[test]
fn the_forced_close_still_runs_when_league_allows_it() {
    let kills = Cell::new(0);
    let force_close = || {
        kills.set(kills.get() + 1);
        Ok(())
    };
    let io = QuitIo {
        guard: &|| Ok(()),
        request_quit: &|| QuitRequest::Sent,
        client_running: &|| true,
        wait: &|| {},
        force_close: &force_close,
        after_exit: &|| {},
        polls: 3,
    };
    assert_eq!(run_graceful_quit(&io), Ok(()));
    assert_eq!(kills.get(), 1);
}

// -- S2: a refusal before the close does not relaunch the client ---------

#[test]
fn a_refused_setup_launch_does_not_relaunch_the_client() {
    let mut launched = 0;
    let result = relaunch_unless_refused(Err(CloseError::Refused(champ_select_refusal())), || {
        launched += 1;
        Ok(())
    });
    assert_eq!(launched, 0, "the client was relaunched after a refusal");
    assert_eq!(result, Err(CloseError::Refused(champ_select_refusal())));

    // A failure after the kill still relaunches.
    let mut launched = 0;
    let result =
        relaunch_unless_refused(Err(CloseError::AfterClose("clear failed".into())), || {
            launched += 1;
            Ok(())
        });
    assert_eq!(launched, 1);
    assert_eq!(result, Err(CloseError::AfterClose("clear failed".into())));
}

#[test]
fn a_refused_setup_capture_does_not_relaunch_the_client() {
    let root = scratch("capture-refused");
    let ctx = TempCtx { root: root.clone() };
    let profile_id = format!("riot-profile-{}", Uuid::new_v4());
    let mut cfg = config::AppConfig::default();
    cfg.riot.profiles = vec![profile(&profile_id, "capturing")];
    cfg.riot.current_profile_id = profile_id.clone();
    config::save_config(&ctx, &cfg).unwrap();

    let mut launched = 0;
    let error = undo_failed_setup_capture(
        &ctx,
        &mut cfg,
        &profile_id,
        "setup_pending",
        CloseError::Refused(champ_select_refusal()),
        || {
            launched += 1;
            Ok(())
        },
    );

    assert_eq!(launched, 0, "the client was relaunched after a refusal");
    assert!(error.starts_with(RIOT_LEAGUE_IN_CHAMP_SELECT), "{error}");
    // The profile still leaves `capturing`, so a cancel removes it.
    assert_eq!(
        find_profile(&config::load_config(&ctx), &profile_id).map(|p| p.snapshot_state.as_str()),
        Some("setup_pending")
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_refused_setup_start_does_not_relaunch_the_client() {
    let root = scratch("start-refused");
    let ctx = TempCtx { root: root.clone() };

    let mut worked = 0;
    let mut launched = 0;
    let result: Result<(), String> = start_setup_after_quit(
        &ctx,
        || Err(CloseError::Refused(champ_select_refusal())),
        || {
            worked += 1;
            Ok(())
        },
        || {
            launched += 1;
            Ok(())
        },
    );
    assert_eq!(result, Err(champ_select_refusal()));
    assert_eq!(worked, 0);
    assert_eq!(launched, 0, "the client was relaunched after a refusal");

    // A quit that closed something, or a backup that failed after it, still
    // relaunches.
    let mut launched = 0;
    let result: Result<(), String> = start_setup_after_quit(
        &ctx,
        || Err(CloseError::AfterClose(champ_select_refusal())),
        || Ok(()),
        || {
            launched += 1;
            Ok(())
        },
    );
    assert_eq!(result, Err(champ_select_refusal()));
    assert_eq!(launched, 1);

    let mut launched = 0;
    let result: Result<(), String> = start_setup_after_quit(
        &ctx,
        || Ok(()),
        || Err("backup failed".into()),
        || {
            launched += 1;
            Ok(())
        },
    );
    assert_eq!(result, Err("backup failed".to_string()));
    assert_eq!(launched, 1);
    let _ = fs::remove_dir_all(&root);
}

// -- S3: a refused setup launch gives the current profile back -----------

#[test]
fn a_refused_setup_launch_keeps_the_signed_in_profile_current_after_a_cancel() {
    let root = scratch("launch-refused");
    let ctx: AppCtx = Arc::new(TempCtx { root: root.clone() });
    let previous_id = format!("riot-profile-{}", Uuid::new_v4());
    let pending_id = format!("riot-profile-{}", Uuid::new_v4());
    let mut cfg = config::AppConfig::default();
    cfg.riot.profiles = vec![
        profile(&previous_id, "ready"),
        profile(&pending_id, "setup_pending"),
    ];
    // `start_setup_profile` made the pending profile current.
    cfg.riot.current_profile_id = pending_id.clone();
    config::save_config(&*ctx, &cfg).unwrap();
    set_setup_launch(&pending_id, SetupLaunch::Running);

    run_riot_setup_launch(
        &*ctx,
        &pending_id,
        &previous_id,
        Duration::from_millis(500),
        || Err(CloseError::Refused(champ_select_refusal())),
    );
    assert_eq!(
        setup_launch(&pending_id),
        Some(SetupLaunch::Failed(champ_select_refusal()))
    );

    cancel_profile_setup(ctx.clone(), pending_id.clone()).unwrap();

    let saved = config::load_config(&*ctx);
    assert!(find_profile(&saved, &pending_id).is_none());
    assert_eq!(
        saved.riot.current_profile_id, previous_id,
        "the still signed-in account lost its profile"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_setup_launch_that_failed_after_the_kill_keeps_the_pending_profile_current() {
    let root = scratch("launch-failed-after-kill");
    let ctx = TempCtx { root: root.clone() };
    let previous_id = format!("riot-profile-{}", Uuid::new_v4());
    let pending_id = format!("riot-profile-{}", Uuid::new_v4());
    let mut cfg = config::AppConfig::default();
    cfg.riot.profiles = vec![
        profile(&previous_id, "ready"),
        profile(&pending_id, "setup_pending"),
    ];
    cfg.riot.current_profile_id = pending_id.clone();
    config::save_config(&ctx, &cfg).unwrap();
    set_setup_launch(&pending_id, SetupLaunch::Running);

    // The live session may be half cleared: it is not the previous one's.
    run_riot_setup_launch(
        &ctx,
        &pending_id,
        &previous_id,
        Duration::from_millis(500),
        || Err(CloseError::AfterClose("clear failed".into())),
    );
    assert_eq!(
        config::load_config(&ctx).riot.current_profile_id,
        pending_id
    );
    forget_setup_launch(&pending_id);
    let _ = fs::remove_dir_all(&root);
}

// -- S4: a running League game still gets the dedicated refusal ----------

#[test]
fn a_league_game_in_progress_is_refused_with_the_league_message() {
    let error = riot_game_guard_error(
        &["League of Legends.exe"],
        || Some("InProgress"),
        "switching Riot account",
    )
    .unwrap_err();
    assert!(error.starts_with(RIOT_LEAGUE_GAME_ACTIVE), "{error}");

    let error = riot_game_guard_error(
        &["VALORANT-Win64-Shipping.exe"],
        || Some("ChampSelect"),
        "switching Riot account",
    )
    .unwrap_err();
    assert!(error.starts_with(RIOT_LEAGUE_IN_CHAMP_SELECT), "{error}");

    // No League phase to read: the generic message names the game.
    let error = riot_game_guard_error(
        &["VALORANT-Win64-Shipping.exe"],
        || None,
        "switching Riot account",
    )
    .unwrap_err();
    assert!(
        error.starts_with("Close Riot game processes before switching Riot account"),
        "{error}"
    );
    assert_eq!(
        riot_game_guard_error(&[], || None, "switching Riot account"),
        Ok(())
    );
}
