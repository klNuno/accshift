use super::*;
use crate::secrets::backend;

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
        "accshift-riot-session-test-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn profile(id: &str, state: &str, puuid: &str) -> RiotProfileConfig {
    RiotProfileConfig {
        id: id.into(),
        label: format!("label-{id}"),
        account_name: format!("name-{id}"),
        account_tag_line: "EUW".into(),
        account_puuid: puuid.into(),
        snapshot_state: state.into(),
        notes: String::new(),
        last_captured_at: None,
        last_used_at: None,
    }
}

fn live(puuid: &str) -> RiotDetectedIdentity {
    RiotDetectedIdentity {
        account_name: format!("live-{puuid}"),
        account_tag_line: "NA1".into(),
        account_puuid: puuid.into(),
    }
}

// -- B1: identity check before the outgoing backup --------------------

#[test]
fn the_live_identity_is_compared_by_puuid() {
    let owned = profile("p", "ready", "puuid-a");
    assert_eq!(
        check_live_identity(&owned, Some(&live("puuid-a"))),
        IdentityCheck::Match
    );
    assert_eq!(
        check_live_identity(&owned, Some(&live("PUUID-A"))),
        IdentityCheck::Match
    );
    assert_eq!(
        check_live_identity(&owned, Some(&live("puuid-b"))),
        IdentityCheck::Mismatch
    );
    assert_eq!(check_live_identity(&owned, None), IdentityCheck::Unknown);
    // An alias read without the userinfo puuid proves nothing.
    assert_eq!(
        check_live_identity(&owned, Some(&live(""))),
        IdentityCheck::Unknown
    );
    let unclaimed = profile("p", "awaiting_capture", "");
    assert_eq!(
        check_live_identity(&unclaimed, Some(&live("puuid-b"))),
        IdentityCheck::Unclaimed
    );
}

#[test]
fn another_account_signed_in_is_never_backed_up_into_the_profile() {
    // Trigger A: the user signed into Y inside the client while accshift
    // still has X as current.
    let x = profile("x", "ready", "puuid-x");
    assert_eq!(
        plan_outgoing_backup(Some(&x), SWITCH_BACKUP_STATES, true, Some(&live("puuid-y"))),
        OutgoingBackup::IdentityMismatch
    );
}

#[test]
fn the_profile_own_session_is_backed_up_and_its_identity_refreshed() {
    let x = profile("x", "ready", "puuid-x");
    assert_eq!(
        plan_outgoing_backup(Some(&x), SWITCH_BACKUP_STATES, true, Some(&live("puuid-x"))),
        OutgoingBackup::Backup {
            adopt_identity: true
        }
    );
}

#[test]
fn an_unverifiable_session_is_backed_up_without_renaming_the_profile() {
    // The client is closed, so the local API cannot say who is signed in.
    // Skipping the backup here would drop the tokens the client rotated
    // since the last switch; renaming would trust an unknown account.
    let x = profile("x", "ready", "puuid-x");
    assert_eq!(
        plan_outgoing_backup(Some(&x), SWITCH_BACKUP_STATES, true, None),
        OutgoingBackup::Backup {
            adopt_identity: false
        }
    );
}

#[test]
fn a_profile_without_a_puuid_adopts_the_live_account() {
    let t = profile("t", "awaiting_capture", "");
    assert_eq!(
        plan_outgoing_backup(Some(&t), SWITCH_BACKUP_STATES, true, Some(&live("puuid-t"))),
        OutgoingBackup::Backup {
            adopt_identity: true
        }
    );
}

#[test]
fn nothing_is_backed_up_without_tokens_or_from_other_states() {
    let x = profile("x", "ready", "puuid-x");
    assert_eq!(
        plan_outgoing_backup(
            Some(&x),
            SWITCH_BACKUP_STATES,
            false,
            Some(&live("puuid-x"))
        ),
        OutgoingBackup::Skip
    );
    let capturing = profile("c", "capturing", "puuid-x");
    assert_eq!(
        plan_outgoing_backup(
            Some(&capturing),
            SWITCH_BACKUP_STATES,
            true,
            Some(&live("puuid-x"))
        ),
        OutgoingBackup::Skip
    );
    assert_eq!(
        plan_outgoing_backup(None, SWITCH_BACKUP_STATES, true, Some(&live("puuid-x"))),
        OutgoingBackup::Skip
    );
}

#[test]
fn a_detected_identity_never_renames_a_profile_of_another_account() {
    let mut x = profile("x", "ready", "puuid-x");
    x.label = "name-x#EUW".into();
    let before = x.clone();

    let changed = apply_detected_identity(&mut x, &live("puuid-y"));

    assert!(!changed);
    assert_eq!(x.account_puuid, before.account_puuid);
    assert_eq!(x.account_name, before.account_name);
    assert_eq!(x.label, before.label);
}

#[test]
fn a_detected_identity_still_follows_a_riot_id_rename_of_the_same_account() {
    let mut x = profile("x", "ready", "puuid-x");
    x.label = "name-x#EUW".into();
    let renamed = RiotDetectedIdentity {
        account_name: "new-name".into(),
        account_tag_line: "EUW".into(),
        account_puuid: "puuid-x".into(),
    };

    assert!(apply_detected_identity(&mut x, &renamed));
    assert_eq!(x.account_name, "new-name");
    assert_eq!(x.label, "new-name#EUW");
}

#[test]
fn a_setup_adopts_the_account_signed_in_during_the_setup() {
    // The user signed into A in the setup window, then switched to B
    // before the capture: the new profile is B's.
    let mut pending = profile("s", "setup_pending", "puuid-a");
    assert!(adopt_detected_identity(&mut pending, &live("puuid-b")));
    assert_eq!(pending.account_puuid, "puuid-b");
}

// -- B1 trigger B: target without a snapshot ---------------------------

#[test]
fn switching_to_a_profile_without_a_snapshot_opens_the_login_screen() {
    assert!(clear_live_for_target_without_snapshot(false, "p", "t"));
    // No current profile (a cancelled setup, a forgotten profile): the
    // live session belongs to no profile, it must not become the target's.
    assert!(clear_live_for_target_without_snapshot(false, "", "t"));
    // A restored snapshot already replaced the live session.
    assert!(!clear_live_for_target_without_snapshot(true, "p", "t"));
    // Re-selecting the current profile keeps a login waiting for capture.
    assert!(!clear_live_for_target_without_snapshot(false, "t", "t"));
}

// -- B1 trigger C: removing the current profile ------------------------

fn config_with(profiles: Vec<RiotProfileConfig>, current: &str) -> config::AppConfig {
    let mut cfg = config::AppConfig::default();
    cfg.riot.profiles = profiles;
    cfg.riot.current_profile_id = current.into();
    cfg
}

#[test]
fn removing_the_current_profile_leaves_no_profile_current() {
    // The session still live is the removed profile's (or a cancelled
    // setup's new account). Pointing current at another profile made the
    // next switch back it up into that profile.
    let mut cfg = config_with(vec![profile("first", "ready", "puuid-f")], "pending");
    release_current_profile(&mut cfg, &["pending".to_string()]);
    assert_eq!(cfg.riot.current_profile_id, "");
}

#[test]
fn removing_another_profile_keeps_the_current_one() {
    let mut cfg = config_with(
        vec![
            profile("first", "ready", "puuid-f"),
            profile("second", "ready", "puuid-s"),
        ],
        "second",
    );
    release_current_profile(&mut cfg, &["gone".to_string()]);
    assert_eq!(cfg.riot.current_profile_id, "second");
}

// -- B4: setup backs up what it is about to clear ----------------------

#[test]
fn setup_backs_up_an_awaiting_capture_session_before_clearing_it() {
    let t = profile("t", "awaiting_capture", "");
    assert_eq!(
        plan_outgoing_backup(Some(&t), SETUP_BACKUP_STATES, true, Some(&live("puuid-t"))),
        OutgoingBackup::Backup {
            adopt_identity: true
        }
    );
    let ready = profile("r", "ready", "puuid-r");
    assert_eq!(
        plan_outgoing_backup(Some(&ready), SETUP_BACKUP_STATES, true, None),
        OutgoingBackup::Backup {
            adopt_identity: false
        }
    );
    // A setup restarted over its own pending profile has nothing saved.
    let pending = profile("s", "setup_pending", "");
    assert_eq!(
        plan_outgoing_backup(Some(&pending), SETUP_BACKUP_STATES, true, None),
        OutgoingBackup::Skip
    );
}

// -- B4: staged snapshot write -----------------------------------------

struct SnapshotFixture {
    root: PathBuf,
    live: PathBuf,
    snapshot: PathBuf,
}

impl SnapshotFixture {
    fn new(tag: &str) -> Self {
        let root = scratch(tag);
        let live = root.join("live");
        let snapshot = root.join("snapshots").join("riot-profile-a");
        fs::create_dir_all(&live).unwrap();
        fs::create_dir_all(&snapshot).unwrap();
        Self {
            root,
            live,
            snapshot,
        }
    }

    fn ctx(&self) -> TempCtx {
        TempCtx {
            root: self.root.clone(),
        }
    }

    /// Every item lives under `live/<snapshot name>`.
    fn live_path(&self) -> impl Fn(&RiotSnapshotItem) -> Result<Option<PathBuf>, String> + '_ {
        move |item| Ok(Some(self.live.join(item.snapshot_name)))
    }

    fn write_old_snapshot(&self) {
        fs::write(
            self.snapshot.join("RiotGamesPrivateSettings.yaml"),
            b"old-good-session",
        )
        .unwrap();
        fs::create_dir_all(self.snapshot.join("Sessions")).unwrap();
        fs::write(self.snapshot.join("Sessions").join("s.json"), b"old").unwrap();
    }

    fn siblings(&self) -> Vec<String> {
        fs::read_dir(self.snapshot.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect()
    }
}

impl Drop for SnapshotFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_failed_backup_keeps_the_previous_snapshot() {
    let fx = SnapshotFixture::new("backup-fails");
    fx.write_old_snapshot();
    // The settings file copies, then Sessions (a file where a directory
    // is expected) fails: the old snapshot used to be gone by then.
    fs::write(
        fx.live.join("RiotGamesPrivateSettings.yaml"),
        b"new-session",
    )
    .unwrap();
    fs::write(fx.live.join("Sessions"), b"not a directory").unwrap();
    let before = backend::entry_count();

    let result = write_snapshot_atomically(&fx.ctx(), &fx.snapshot, &fx.live_path());

    assert!(result.is_err());
    assert_eq!(
        fs::read(fx.snapshot.join("RiotGamesPrivateSettings.yaml")).unwrap(),
        b"old-good-session"
    );
    assert_eq!(
        fs::read(fx.snapshot.join("Sessions").join("s.json")).unwrap(),
        b"old"
    );
    assert_eq!(fx.siblings(), vec!["riot-profile-a".to_string()]);
    assert_eq!(backend::entry_count(), before, "the staged copy leaked");
}

#[test]
fn a_backup_missing_the_required_file_keeps_the_previous_snapshot() {
    let fx = SnapshotFixture::new("backup-missing");
    fx.write_old_snapshot();
    fs::create_dir_all(fx.live.join("Sessions")).unwrap();
    fs::write(fx.live.join("Sessions").join("s.json"), b"new").unwrap();

    let result = write_snapshot_atomically(&fx.ctx(), &fx.snapshot, &fx.live_path());

    assert!(result.is_err());
    assert_eq!(
        fs::read(fx.snapshot.join("RiotGamesPrivateSettings.yaml")).unwrap(),
        b"old-good-session"
    );
    assert_eq!(fx.siblings(), vec!["riot-profile-a".to_string()]);
}

#[test]
fn a_successful_backup_replaces_the_snapshot_and_frees_the_old_one() {
    let fx = SnapshotFixture::new("backup-ok");
    // An encrypted old snapshot, so freeing its entries is observable.
    fs::write(
        fx.live.join("RiotGamesPrivateSettings.yaml"),
        b"old-session",
    )
    .unwrap();
    write_snapshot_atomically(&fx.ctx(), &fx.snapshot, &fx.live_path()).unwrap();
    let after_first = backend::entry_count();

    fs::write(
        fx.live.join("RiotGamesPrivateSettings.yaml"),
        b"new-session",
    )
    .unwrap();
    fs::create_dir_all(fx.live.join("Sessions")).unwrap();
    fs::write(fx.live.join("Sessions").join("s.json"), b"new").unwrap();
    write_snapshot_atomically(&fx.ctx(), &fx.snapshot, &fx.live_path()).unwrap();

    let restored = fx.root.join("restored.yaml");
    decrypted_copy_file(
        &fx.snapshot.join("RiotGamesPrivateSettings.yaml"),
        &restored,
    )
    .unwrap();
    assert_eq!(fs::read(&restored).unwrap(), b"new-session");
    assert!(fx.snapshot.join("Sessions").join("s.json").exists());
    assert_eq!(fx.siblings(), vec!["riot-profile-a".to_string()]);
    // Old file freed, two new files stored.
    assert_eq!(backend::entry_count(), after_first - 1 + 2);
}

#[test]
fn an_interrupted_swap_is_recovered_before_the_next_backup() {
    // A crash between the two renames left the good copy aside and the
    // snapshot dir empty (profile_snapshot_dir recreates it).
    let fx = SnapshotFixture::new("backup-recover");
    let previous = snapshot_sibling(&fx.snapshot, "previous");
    fs::create_dir_all(&previous).unwrap();
    fs::write(previous.join("RiotGamesPrivateSettings.yaml"), b"good").unwrap();

    recover_interrupted_snapshot_swap(&fx.ctx(), &fx.snapshot);

    assert_eq!(
        fs::read(fx.snapshot.join("RiotGamesPrivateSettings.yaml")).unwrap(),
        b"good"
    );
    assert!(!previous.exists());
}

// -- B5: setup launch under the lock, gated poll ------------------------

#[test]
fn the_setup_poll_waits_for_its_own_clean_launch() {
    assert_eq!(
        setup_capture_gate("setup_pending", Some(&SetupLaunch::Running)),
        SetupGate::Wait
    );
    assert_eq!(
        setup_capture_gate("setup_pending", Some(&SetupLaunch::Launched)),
        SetupGate::Observe
    );
    assert_eq!(
        setup_capture_gate("setup_pending", Some(&SetupLaunch::Failed("boom".into()))),
        SetupGate::Failed("boom".into())
    );
    // No launch recorded by this process: the client running is not one
    // the setup cleared, so its session is someone else's.
    assert_eq!(setup_capture_gate("setup_pending", None), SetupGate::Wait);
    // A profile outside setup keeps its manual capture flow.
    assert_eq!(
        setup_capture_gate("awaiting_capture", None),
        SetupGate::Observe
    );
}

fn save_pending_setup(ctx: &TempCtx, profile_id: &str) {
    let cfg = config_with(vec![profile(profile_id, "setup_pending", "")], profile_id);
    config::save_config(ctx, &cfg).unwrap();
}

#[test]
fn the_setup_launch_does_not_touch_the_client_without_the_lock() {
    let root = scratch("launch-contended");
    let ctx = TempCtx { root: root.clone() };
    let profile_id = format!("riot-profile-{}", Uuid::new_v4());
    save_pending_setup(&ctx, &profile_id);
    set_setup_launch(&profile_id, SetupLaunch::Running);

    let held = crate::lock::acquire_exclusive(&ctx, Duration::from_millis(500)).unwrap();
    let ran = std::thread::scope(|s| {
        s.spawn(|| {
            let mut ran = false;
            run_riot_setup_launch(&ctx, &profile_id, Duration::from_millis(150), || {
                ran = true;
                Ok(())
            });
            ran
        })
        .join()
        .unwrap()
    });
    drop(held);

    assert!(!ran, "the client was quit and cleared without the lock");
    assert!(
        matches!(setup_launch(&profile_id), Some(SetupLaunch::Failed(_))),
        "the lock failure was dropped"
    );
    forget_setup_launch(&profile_id);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_setup_launch_records_success_and_failure() {
    let root = scratch("launch-outcome");
    let ctx = TempCtx { root: root.clone() };
    let ok_id = format!("riot-profile-{}", Uuid::new_v4());
    save_pending_setup(&ctx, &ok_id);
    set_setup_launch(&ok_id, SetupLaunch::Running);
    run_riot_setup_launch(&ctx, &ok_id, Duration::from_millis(500), || Ok(()));
    assert_eq!(setup_launch(&ok_id), Some(SetupLaunch::Launched));
    forget_setup_launch(&ok_id);

    let failed_id = format!("riot-profile-{}", Uuid::new_v4());
    save_pending_setup(&ctx, &failed_id);
    set_setup_launch(&failed_id, SetupLaunch::Running);
    run_riot_setup_launch(&ctx, &failed_id, Duration::from_millis(500), || {
        Err("Could not remove file".into())
    });
    assert_eq!(
        setup_launch(&failed_id),
        Some(SetupLaunch::Failed("Could not remove file".into()))
    );
    forget_setup_launch(&failed_id);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_setup_cancelled_before_its_launch_ran_is_left_alone() {
    let root = scratch("launch-cancelled");
    let ctx = TempCtx { root: root.clone() };
    let profile_id = format!("riot-profile-{}", Uuid::new_v4());
    // Cancel already removed the profile and the launch entry.
    config::save_config(&ctx, &config_with(Vec::new(), "")).unwrap();

    let mut ran = false;
    run_riot_setup_launch(&ctx, &profile_id, Duration::from_millis(500), || {
        ran = true;
        Ok(())
    });

    assert!(!ran);
    assert_eq!(setup_launch(&profile_id), None);
    let _ = fs::remove_dir_all(&root);
}

// -- B11: failures after the quit relaunch the client -------------------

#[test]
fn a_failure_after_the_quit_still_relaunches_the_client() {
    let mut launched = 0;
    let result = relaunch_after_quit(Err("backup failed".into()), || {
        launched += 1;
        Ok(())
    });
    assert_eq!(result, Err("backup failed".to_string()));
    assert_eq!(launched, 1);

    let mut launched = 0;
    let result = relaunch_after_quit(Err("backup failed".into()), || {
        launched += 1;
        Err("launch failed".into())
    });
    assert_eq!(result, Err("backup failed".to_string()));
    assert_eq!(launched, 1);

    assert_eq!(
        relaunch_after_quit(Ok(()), || Err("launch failed".into())),
        Err("launch failed".to_string())
    );
    assert_eq!(relaunch_after_quit(Ok(()), || Ok(())), Ok(()));
}

#[test]
fn a_failed_setup_capture_reopens_the_client_and_leaves_the_setup_cancellable() {
    let root = scratch("capture-failed");
    let ctx = TempCtx { root: root.clone() };
    let profile_id = format!("riot-profile-{}", Uuid::new_v4());
    let mut cfg = config_with(vec![profile(&profile_id, "capturing", "")], &profile_id);
    config::save_config(&ctx, &cfg).unwrap();

    let mut launched = 0;
    let error = undo_failed_setup_capture(
        &ctx,
        &mut cfg,
        &profile_id,
        "setup_pending",
        "backup failed".into(),
        || {
            launched += 1;
            Err("launch failed".into())
        },
    );

    assert_eq!(error, "backup failed");
    assert_eq!(launched, 1);
    // Saved, so the cancel that follows the failed poll removes the profile.
    let saved = config::load_config(&ctx);
    assert_eq!(
        find_profile(&saved, &profile_id).map(|p| p.snapshot_state.as_str()),
        Some("setup_pending")
    );
    let _ = fs::remove_dir_all(&root);
}

// -- Performance: the setup poll logs on change only -------------------

#[test]
fn the_setup_poll_logs_only_when_its_view_changes() {
    let waiting = SetupPollObservation {
        profile_id: "p".into(),
        lockfile: true,
        logged_in: false,
        persist: false,
        settings_ready: false,
        identity: false,
        can_capture: false,
    };
    let mut last = None;
    assert!(observation_changed(&mut last, &waiting));
    assert!(!observation_changed(&mut last, &waiting));
    assert!(!observation_changed(&mut last, &waiting));

    let logged_in = SetupPollObservation {
        logged_in: true,
        ..waiting.clone()
    };
    assert!(observation_changed(&mut last, &logged_in));
    // A new setup logs its first poll even when the view is the same.
    let other_setup = SetupPollObservation {
        profile_id: "q".into(),
        ..logged_in.clone()
    };
    assert!(observation_changed(&mut last, &other_setup));
}
