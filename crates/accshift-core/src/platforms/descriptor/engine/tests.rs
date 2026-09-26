use super::super::schema::RegistryHive;
use super::*;
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
        "accshift-descriptor-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// A platform whose whole world is two directories under the scratch root:
/// no launcher, no registry, nothing installed.
fn fixture(live_root: &Path) -> String {
    let live = live_root.display().to_string().replace('\\', "/");
    format!(
        r#"{{
          "id": "gog",
          "schemaVersion": 1,
          "name": "Test Launcher",
          "shortName": "Test",
          "os": {{
            "windows": {{
              "roots": {{ "files": ["{live}"] }},
              "detect": {{ "pathExists": ["{live}"] }},
              "identity": {{
                "source": {{ "kind": "synthetic" }},
                "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                "current": "config"
              }},
              "state": {{
                "files": [
                  {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true, "clearOnSetup": true }}
                ],
                "directories": [
                  {{ "live": "{live}/auth", "snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true }}
                ]
              }},
              "close": {{ "processes": ["nothing-here.exe"] }},
              "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
            }},
            "linux": {{
              "roots": {{ "files": ["{live}"] }},
              "detect": {{ "pathExists": ["{live}"] }},
              "identity": {{
                "source": {{ "kind": "synthetic" }},
                "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                "current": "config"
              }},
              "state": {{
                "files": [
                  {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true, "clearOnSetup": true }}
                ],
                "directories": [
                  {{ "live": "{live}/auth", "snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true }}
                ]
              }},
              "close": {{ "processes": ["nothing-here"] }},
              "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
            }},
            "macos": {{
              "roots": {{ "files": ["{live}"] }},
              "detect": {{ "pathExists": ["{live}"] }},
              "identity": {{
                "source": {{ "kind": "synthetic" }},
                "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                "current": "config"
              }},
              "state": {{
                "files": [
                  {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true, "clearOnSetup": true }}
                ],
                "directories": [
                  {{ "live": "{live}/auth", "snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true }}
                ]
              }},
              "close": {{ "processes": ["nothing-here"] }},
              "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
            }}
          }}
        }}"#
    )
}

/// The config cache and the poisoned-local flag are process-global, so any
/// test that reaches config through the engine takes the same lock as
/// `config`'s own tests instead of clearing state under them.
fn config_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn service(live_root: &Path) -> DescriptorService {
    let descriptor = Descriptor::parse("test", &fixture(live_root)).unwrap();
    DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
}

fn seed_live_session(live_root: &Path, marker: &[u8]) {
    fs::create_dir_all(live_root.join("auth").join("nested")).unwrap();
    fs::write(live_root.join("session.json"), marker).unwrap();
    fs::write(
        live_root.join("auth").join("nested").join("token.bin"),
        marker,
    )
    .unwrap();
}

#[test]
fn account_ids_are_checked_against_the_declared_charset() {
    let root = scratch("id-validation");
    let service = service(&root.join("live"));

    assert_eq!(
        service.validate_account_id("").unwrap_err(),
        "Empty Test account ID"
    );
    assert_eq!(
        service.validate_account_id("../../evil").unwrap_err(),
        "Invalid Test account ID: ../../evil"
    );
    assert_eq!(
        service.validate_account_id("  a3f0c2d1  ").unwrap(),
        "a3f0c2d1"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn detect_reads_the_declared_paths_rather_than_a_launcher() {
    let _config = config_guard();
    let root = scratch("detect");
    let live = root.join("live");
    let ctx: AppCtx = Arc::new(TempCtx { root: root.clone() });
    let service = service(&live);

    assert!(!service.is_installed(ctx.clone()));
    fs::create_dir_all(&live).unwrap();
    assert!(service.is_installed(ctx));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn restoring_without_a_snapshot_names_the_account_and_the_way_out() {
    let _config = config_guard();
    let root = scratch("missing-snapshot");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&root.join("live"));

    let err = service.restore_snapshot(&ctx, "a3f0c2d1").unwrap_err();
    assert_eq!(
        err,
        "No auth snapshot found for account a3f0c2d1. Add this account through setup first."
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_live_path_outside_the_roots_never_reaches_the_engine() {
    let root = scratch("sandbox");
    let live = root.join("live");
    let live_text = live.display().to_string().replace('\\', "/");

    // Same descriptor, but the session file now sits next to the declared
    // root instead of inside it. Validation refuses it at load, so the
    // engine is never handed a descriptor that could escape.
    let json = fixture(&live).replace(
        &format!("{live_text}/session.json"),
        &format!("{live_text}-elsewhere/session.json"),
    );
    let err = Descriptor::parse("test", &json).unwrap_err();
    assert!(err.problem.contains("declared roots"), "{err}");
    assert!(err.field.ends_with("state.files[0].live"), "{err}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn dry_run_lists_every_file_and_folder_without_writing_any() {
    let _config = config_guard();
    let root = scratch("dry-run");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    seed_live_session(&live, b"live-session");

    let service = service(&live);
    let plan = service.plan_switch(&ctx, "a3f0c2d1").unwrap();

    assert_eq!(plan.platform_id, "gog");
    assert!(!plan.applied);
    assert!(plan
        .steps
        .iter()
        .any(|s| s.action == PlanAction::Restore && s.target.ends_with("session.json")));
    assert!(plan
        .steps
        .iter()
        .any(|s| s.action == PlanAction::Close && s.target.contains("nothing-here")));
    assert!(plan
        .warnings
        .iter()
        .any(|w| w.contains("No snapshot stored for account a3f0c2d1")));

    // Nothing was captured: the account's snapshot directory is still absent.
    assert!(!service.snapshot_root(&ctx, "a3f0c2d1").unwrap().exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn dry_run_reports_the_roots_it_would_stay_inside() {
    let _config = config_guard();
    let root = scratch("dry-run-roots");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(&live).unwrap();

    let plan = service(&live).plan_switch(&ctx, "a3f0c2d1").unwrap();
    assert_eq!(plan.roots.len(), 1);
    assert!(plan.roots[0].to_lowercase().contains("live"));
    let _ = fs::remove_dir_all(&root);
}

// The tests below capture a snapshot, which encrypts through the OS
// backend. That is DPAPI on Windows and always available; elsewhere it is
// the login keyring, which a headless build has no way to reach. Windows
// is also the only OS any shipped descriptor targets today.
#[cfg(windows)]
#[test]
fn capture_then_restore_brings_a_session_back_byte_for_byte() {
    let _config = config_guard();
    let root = scratch("round-trip");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"account-one");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    assert!(service.has_snapshot(&ctx, "aaaa1111"));

    // A second account signs in and overwrites the live session.
    seed_live_session(&live, b"account-two");
    service.save_snapshot(&ctx, "bbbb2222").unwrap();

    service.restore_snapshot(&ctx, "aaaa1111").unwrap();
    assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-one");
    assert_eq!(
        fs::read(live.join("auth").join("nested").join("token.bin")).unwrap(),
        b"account-one"
    );

    service.restore_snapshot(&ctx, "bbbb2222").unwrap();
    assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-two");
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_snapshot_never_holds_the_session_in_plaintext() {
    let _config = config_guard();
    let root = scratch("encrypted");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"super-secret-token");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
    let stored = fs::read(snapshot.join("session.json")).unwrap();
    assert!(stored.starts_with(crate::snapshot_crypto::ENCRYPTED_HEADER));
    assert!(!stored.windows(18).any(|w| w == b"super-secret-token"));
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn capture_drops_a_stale_snapshot_when_the_live_file_is_gone() {
    // Otherwise a later restore resurrects the previous account's file.
    let _config = config_guard();
    let root = scratch("stale");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"first");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    fs::remove_file(live.join("session.json")).unwrap();
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
    assert!(!snapshot.join("session.json").exists());
    let _ = fs::remove_dir_all(&root);
}

/// The variable a root is written against on the OS this build targets.
/// Each one is a placeholder the loader accepts there, and none of them is
/// in the fabricated environment the test hands the service.
const ROOT_VAR: &str = if cfg!(windows) {
    "LOCALAPPDATA"
} else {
    "HOME"
};

/// A descriptor whose only root is written against a variable the caller
/// can leave out of the environment.
fn env_rooted_fixture() -> String {
    fn profile(var: &str) -> String {
        format!(
            r#"{{
          "roots": {{ "files": ["${{{var}}}/Demo"] }},
          "detect": {{ "pathExists": ["${{{var}}}/Demo"] }},
          "identity": {{
            "source": {{ "kind": "synthetic" }},
            "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
            "current": "config"
          }},
          "state": {{
            "files": [
              {{ "live": "${{{var}}}/Demo/session.json", "snapshot": "session.json", "snapshotMarker": true }}
            ]
          }},
          "close": {{ "processes": ["nothing-here"] }},
          "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
        }}"#
        )
    }
    format!(
        r#"{{
          "id": "gog",
          "schemaVersion": 1,
          "name": "Test Launcher",
          "shortName": "Test",
          "os": {{
            "windows": {},
            "linux": {},
            "macos": {}
          }}
        }}"#,
        profile("LOCALAPPDATA"),
        profile("HOME"),
        profile("HOME")
    )
}

#[test]
fn a_root_that_does_not_resolve_refuses_every_path_instead_of_allowing_all() {
    // The sandbox used to drop a root it could not resolve, and an empty
    // root list meant "allow everything". One unset variable was enough to
    // let a switch write anywhere on the disk.
    let _config = config_guard();
    let root = scratch("unresolved-root");
    let ctx = TempCtx { root: root.clone() };
    let descriptor = Descriptor::parse("test", &env_rooted_fixture()).unwrap();
    let service = DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
        .with_environment(Vec::<(String, String)>::new());

    let err = service.save_snapshot(&ctx, "aaaa1111").unwrap_err();
    assert!(err.contains("Refused to run without a sandbox"), "{err}");
    assert!(err.contains(&format!("${{{ROOT_VAR}}}/Demo")), "{err}");
    assert!(service.plan_switch(&ctx, "aaaa1111").is_err());
    assert!(service.read_accounts(&ctx).is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn saved_accounts_stay_listed_when_the_roots_do_not_resolve() {
    // An update that stops a root from resolving (a variable this session
    // lacks) must not make the saved accounts vanish. Switching still refuses.
    let _config = config_guard();
    let root = scratch("unresolved-root-listing");
    let ctx = TempCtx { root: root.clone() };
    let descriptor = Descriptor::parse("test", &env_rooted_fixture()).unwrap();
    let service = DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
        .with_environment(Vec::<(String, String)>::new());

    // Nothing saved yet: the refusal is the only useful answer.
    assert!(service.list_accounts(&ctx).is_err());

    let id = service.descriptor.id.clone();
    config_bridge::touch_account(&ctx, &id, "aaaa1111", 1234).unwrap();
    let (accounts, current) = service.list_accounts(&ctx).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "aaaa1111");
    assert_eq!(accounts[0].last_used_at, Some(1234));
    assert_eq!(current, None);
    assert!(service.plan_switch(&ctx, "aaaa1111").is_err());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn recapturing_frees_the_previous_capture_keyring_entries() {
    // Every encrypted file owns a keyring entry on Linux and macOS, and the
    // directory that held the ids is what gets removed. Freeing them has to
    // happen before the removal or a switch leaks one entry per file.
    let _config = config_guard();
    let root = scratch("keyring-growth");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"first");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    let after_first = crate::secrets::backend::entry_count();
    assert_eq!(after_first, 2, "one entry per encrypted snapshot file");

    seed_live_session(&live, b"second");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    assert_eq!(crate::secrets::backend::entry_count(), after_first);

    // The snapshot still reads back, so nothing live was freed either.
    service.restore_snapshot(&ctx, "aaaa1111").unwrap();
    assert_eq!(
        fs::read(live.join("auth").join("nested").join("token.bin")).unwrap(),
        b"second"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_missing_live_directory_keeps_the_previous_snapshot_when_told_to() {
    let _config = config_guard();
    let root = scratch("dir-keep");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let json = fixture(&live).replace(
        r#""snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true"#,
        r#""snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true, "clearSnapshotWhenSourceMissing": false"#,
    );
    let service = DescriptorService::new(
        Descriptor::parse("test", &json).unwrap(),
        DescriptorOrigin::Embedded,
    );

    seed_live_session(&live, b"first");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    // The launcher has not written its auth folder back yet.
    fs::remove_dir_all(live.join("auth")).unwrap();
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
    assert!(snapshot
        .join("auth")
        .join("nested")
        .join("token.bin")
        .exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_missing_live_directory_drops_the_snapshot_by_default() {
    let _config = config_guard();
    let root = scratch("dir-drop");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"first");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    fs::remove_dir_all(live.join("auth")).unwrap();
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
    assert!(!snapshot.join("auth").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clearing_the_live_state_removes_exactly_what_setup_declares() {
    let _config = config_guard();
    let root = scratch("clear");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"session");
    fs::write(live.join("keep-me.txt"), b"preferences").unwrap();

    service.clear_live_state(&ctx).unwrap();

    assert!(!live.join("session.json").exists());
    assert!(!live.join("auth").exists());
    assert!(live.join("keep-me.txt").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_setup_plan_lists_what_adding_an_account_deletes_and_deletes_nothing() {
    let _config = config_guard();
    let root = scratch("setup-plan");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);
    seed_live_session(&live, b"session");

    let mut plan = DryRunPlan::new("gog", "setup", "sample");
    service.plan_setup_clear(&ctx, &mut plan);

    let deleted: Vec<&str> = plan
        .steps
        .iter()
        .filter(|s| s.action == PlanAction::Delete)
        .map(|s| s.target.as_str())
        .collect();
    assert_eq!(deleted.len(), 2, "{deleted:?}");
    assert!(deleted.iter().any(|t| t.ends_with("session.json")));
    assert!(deleted.iter().any(|t| t.ends_with("auth")));
    assert!(live.join("session.json").exists());
    assert!(live.join("auth").exists());
    let _ = fs::remove_dir_all(&root);
}

/// The fixture with a launcher binary and launch arguments on this OS's
/// profile. The binary is found through the user's path override.
fn launching_service(live_root: &Path, launch: serde_json::Value) -> DescriptorService {
    let os_key = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let anchor = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    };
    let mut json: serde_json::Value = serde_json::from_str(&fixture(live_root)).unwrap();
    let profile = &mut json["os"][os_key];
    profile["executable"] = serde_json::json!({
        "fileName": "Demo.exe",
        "candidates": [{ "kind": "path", "template": format!("${{{anchor}}}/Demo/Demo.exe") }],
    });
    profile["launch"] = launch;
    let descriptor = Descriptor::parse("test", &json.to_string()).unwrap();
    DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
}

fn planned_launch_note(service: &DescriptorService, ctx: &TempCtx) -> String {
    let plan = service.plan_switch(ctx, "aaaa1111").unwrap();
    let launch = plan
        .steps
        .iter()
        .find(|step| step.action == PlanAction::Launch)
        .unwrap_or_else(|| panic!("no launch step: {:?}", plan.steps));
    assert!(launch.target.ends_with("Demo.exe"), "{}", launch.target);
    launch.note.clone()
}

#[test]
fn the_switch_plan_shows_the_arguments_the_launcher_gets() {
    let _config = config_guard();
    let root = scratch("launch-args-plan");
    let live = root.join("live");
    let install = root.join("install");
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("Demo.exe"), b"").unwrap();
    let ctx = TempCtx { root: root.clone() };
    config_bridge::set_path_override(&ctx, "gog", &install.display().to_string()).unwrap();

    let always = launching_service(
        &live,
        serde_json::json!({ "args": ["--silent", "--from-accshift"] }),
    );
    assert_eq!(
        planned_launch_note(&always, &ctx),
        "with arguments: --silent --from-accshift"
    );

    // Arguments meant for an updater stub are not shown for the real binary.
    let stub_only = launching_service(
        &live,
        serde_json::json!({ "args": ["--silent"], "argsOnlyFor": "Updater.exe" }),
    );
    assert_eq!(planned_launch_note(&stub_only, &ctx), "");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_candidate_file_with_another_name_than_the_binary_is_not_launched() {
    let root = scratch("binary-name");
    let other = root.join("cmd.exe");
    let named = root.join("Demo.exe");
    fs::write(&other, b"").unwrap();
    fs::write(&named, b"").unwrap();
    let executable: Executable = serde_json::from_value(serde_json::json!({
        "fileName": "demo.exe",
        "candidates": [],
    }))
    .unwrap();

    assert_eq!(locate_binary(&other, &executable), None);
    assert_eq!(locate_binary(&named, &executable), Some(named.clone()));
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn forgetting_an_account_removes_its_snapshot_directory() {
    let _config = config_guard();
    let root = scratch("forget");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"session");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
    assert!(snapshot.exists());

    service.forget(&ctx, "aaaa1111").unwrap();
    assert!(!snapshot.exists());
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn restore_leaves_no_staging_directory_behind() {
    let _config = config_guard();
    let root = scratch("staging");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"session");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    service.restore_snapshot(&ctx, "aaaa1111").unwrap();

    assert!(!live.join("auth.accshift-restore-tmp").exists());
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn accounts_list_what_the_config_holds_plus_anything_with_a_snapshot() {
    let _config = config_guard();
    let root = scratch("accounts");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    assert!(service.read_accounts(&ctx).unwrap().is_empty());

    seed_live_session(&live, b"session");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    config_bridge::touch_account(&ctx, "gog", "aaaa1111", 1234).unwrap();

    let accounts = service.read_accounts(&ctx).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "aaaa1111");
    assert!(accounts[0].snapshot_saved);
    assert_eq!(accounts[0].last_used_at, Some(1234));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn freshness_rejects_missing_empty_and_stale_files() {
    let root = scratch("freshness");
    assert!(!file_is_fresh(&root.join("missing"), 60_000));

    let empty = root.join("empty");
    fs::write(&empty, b"").unwrap();
    assert!(!file_is_fresh(&empty, 60_000));

    let recent = root.join("recent");
    fs::write(&recent, b"data").unwrap();
    assert!(file_is_fresh(&recent, 60_000));

    // Same file, written ten minutes ago: the launcher never flushed the
    // new session, so the account it describes is the previous one.
    let stale = root.join("stale");
    fs::write(&stale, b"data").unwrap();
    let handle = fs::OpenOptions::new().write(true).open(&stale).unwrap();
    handle
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(600))
        .unwrap();
    assert!(!file_is_fresh(&stale, 60_000));
    let _ = fs::remove_dir_all(&root);
}

/// A platform that reads its account id out of a log, finds accounts by
/// listing a directory, and keeps its session file in one of two places.
/// Modelled on Ubisoft, whose id is `ubisoft` so the config bridge reaches
/// the section that holds the forget blocklist.
fn log_fixture(live_root: &Path) -> String {
    let live = live_root.display().to_string().replace('\\', "/");
    format!(
        r#"{{
          "id": "ubisoft",
          "schemaVersion": 1,
          "name": "Test Connect",
          "shortName": "Test",
          "os": {{
            "windows": {{ {profile} }},
            "linux": {{ {profile} }},
            "macos": {{ {profile} }}
          }}
        }}"#,
        profile = log_profile(&live)
    )
}

fn log_profile(live: &str) -> String {
    format!(
        r#"
          "roots": {{ "files": ["{live}"] }},
          "detect": {{ "pathExists": ["{live}"] }},
          "identity": {{
            "source": {{
              "kind": "logTail",
              "path": "{live}/logs/launcher_log.txt",
              "lineContains": "AccountStartupUser.cpp",
              "prefix": "User: ",
              "nearWord": "User"
            }},
            "format": {{
              "charset": "uuid",
              "maxLength": 36,
              "minLength": 36,
              "lowercase": true,
              "invalidMessage": "Invalid Test account UUID"
            }},
            "current": "identity",
            "discovery": [
              {{ "kind": "directoryEntries", "path": "{live}/savegames", "entries": "directories" }}
            ],
            "blocklistOnForget": true
          }},
          "state": {{
            "files": [
              {{ "live": "{live}/user.dat", "snapshot": "user.dat", "snapshotMarker": true }},
              {{
                "live": ["{live}/Config/Windows/settings.ini", "{live}/Config/WindowsEditor/settings.ini"],
                "snapshot": "settings.ini"
              }}
            ]
          }},
          "close": {{ "processes": ["nothing-here.exe"] }},
          "setup": {{ "missingSnapshotHint": "Sign in to this account once first." }}
        "#
    )
}

fn log_service(live_root: &Path) -> DescriptorService {
    let descriptor = Descriptor::parse("test", &log_fixture(live_root)).unwrap();
    DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
}

const UUID_ONE: &str = "a9da419c-1234-5678-9abc-def012345678";
const UUID_TWO: &str = "deadbeef-0000-1111-2222-333344445555";

#[test]
fn the_account_id_comes_from_the_last_matching_line_of_the_log() {
    let _config = config_guard();
    let root = scratch("log-tail");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(live.join("logs")).unwrap();
    fs::write(
        live.join("logs").join("launcher_log.txt"),
        format!(
            "[00:00] AccountStartupUser.cpp - User: {UUID_ONE} logged in\n\
             [00:01] Something.cpp - User: 00000000-0000-0000-0000-000000000000\n\
             [00:02] AccountStartupUser.cpp - User: {UUID_TWO} logged in\n"
        ),
    )
    .unwrap();

    let service = log_service(&live);
    assert_eq!(service.read_identity(&ctx).as_deref(), Some(UUID_TWO));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_id_written_in_the_launchers_own_case_reads_back_as_one_account() {
    let _config = config_guard();
    let root = scratch("log-case");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(live.join("logs")).unwrap();
    fs::write(
        live.join("logs").join("launcher_log.txt"),
        format!(
            "AccountStartupUser.cpp - User={} done\n",
            UUID_ONE.to_uppercase()
        ),
    )
    .unwrap();

    // No `User: ` prefix on that line: the nearby-word fallback finds it,
    // and `lowercase` folds it to the one spelling the engine stores.
    let service = log_service(&live);
    assert_eq!(service.read_identity(&ctx).as_deref(), Some(UUID_ONE));
    let _ = fs::remove_dir_all(&root);
}

fn write_log(live: &Path, id: &str, age: Duration) {
    let log = live.join("logs").join("launcher_log.txt");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    fs::write(
        &log,
        format!("[00:00] AccountStartupUser.cpp - User: {id} logged in\n"),
    )
    .unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&log)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

#[test]
fn a_log_older_than_the_last_switch_does_not_name_the_current_account() {
    // The launcher logs a sign-in some time after it starts, so right
    // after a switch the last line still names the previous account.
    let _config = config_guard();
    let root = scratch("log-lag");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = log_service(&live);
    let minute_ago = now_unix_ms() - 60_000;
    config_bridge::set_last_switch(&ctx, "ubisoft", UUID_TWO, minute_ago).unwrap();

    write_log(&live, UUID_ONE, Duration::from_secs(600));
    assert_eq!(service.current_account_id(&ctx).as_deref(), Some(UUID_TWO));

    // The launcher wrote since: the log is the better witness again.
    write_log(&live, UUID_ONE, Duration::ZERO);
    assert_eq!(service.current_account_id(&ctx).as_deref(), Some(UUID_ONE));
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_switch_records_the_account_it_put_in_place() {
    let _config = config_guard();
    let root = scratch("log-switch-record");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = log_service(&live);

    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("user.dat"), b"account-two").unwrap();
    service.save_snapshot(&ctx, UUID_TWO).unwrap();
    fs::write(live.join("user.dat"), b"account-one").unwrap();
    write_log(&live, UUID_ONE, Duration::from_secs(600));

    service.switch(&ctx, UUID_TWO).unwrap();
    // Nothing new in the log yet: the account is the one just put there,
    // so the next capture cannot file its session under the previous one.
    assert_eq!(service.current_account_id(&ctx).as_deref(), Some(UUID_TWO));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_startup_read_resolves_the_launcher_and_reads_the_log_once() {
    let _config = config_guard();
    let root = scratch("log-read-once");
    let live = root.join("live");
    let ctx: AppCtx = Arc::new(TempCtx { root: root.clone() });
    let service = log_service(&live);
    write_log(&live, UUID_ONE, Duration::ZERO);

    let snapshot = service.get_startup_snapshot(ctx).unwrap();
    assert_eq!(snapshot["currentAccount"], UUID_ONE);
    assert_eq!(count(&service.probes.runtimes), 1);
    assert_eq!(count(&service.probes.log_reads), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_platform_keeps_its_own_wording_for_a_bad_account_id() {
    let root = scratch("id-wording");
    let service = log_service(&root.join("live"));
    assert_eq!(
        service.validate_account_id("nope").unwrap_err(),
        "Invalid Test account UUID"
    );
    assert_eq!(
        service.validate_account_id("").unwrap_err(),
        "Invalid Test account UUID"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn accounts_added_outside_the_app_are_discovered_and_stay_forgotten_once_forgotten() {
    let _config = config_guard();
    let root = scratch("discovery");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(live.join("savegames").join(UUID_ONE)).unwrap();
    fs::create_dir_all(live.join("savegames").join(UUID_TWO)).unwrap();
    // A file, not a directory, and not an id: neither may be listed.
    fs::write(live.join("savegames").join("readme.txt"), b"x").unwrap();

    let service = log_service(&live);
    let mut ids: Vec<String> = service
        .read_accounts(&ctx)
        .unwrap()
        .into_iter()
        .map(|a| a.account_id)
        .collect();
    ids.sort();
    assert_eq!(ids, vec![UUID_ONE.to_string(), UUID_TWO.to_string()]);

    // The directory is still on disk, so without the blocklist the next
    // listing would put the account straight back.
    service.forget(&ctx, UUID_ONE).unwrap();
    let after: Vec<String> = service
        .read_accounts(&ctx)
        .unwrap()
        .into_iter()
        .map(|a| a.account_id)
        .collect();
    assert_eq!(after, vec![UUID_TWO.to_string()]);

    // Using it again is a clear statement that it should come back.
    config_bridge::touch_account(&ctx, "ubisoft", UUID_ONE, 1234).unwrap();
    assert_eq!(service.read_accounts(&ctx).unwrap().len(), 2);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_path_with_several_candidates_uses_the_one_that_exists() {
    let _config = config_guard();
    let root = scratch("first-existing");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(live.join("Config").join("WindowsEditor")).unwrap();
    fs::write(
        live.join("Config")
            .join("WindowsEditor")
            .join("settings.ini"),
        b"editor",
    )
    .unwrap();

    let plan = log_service(&live).plan_switch(&ctx, UUID_ONE).unwrap();
    let restored: Vec<&str> = plan
        .steps
        .iter()
        .filter(|s| s.action == PlanAction::Restore)
        .map(|s| s.target.as_str())
        .collect();
    assert!(
        restored.iter().any(|t| t.contains("WindowsEditor")),
        "{restored:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_path_with_several_candidates_falls_back_to_the_first_when_none_exist() {
    // Otherwise a file the launcher has not written yet would have nowhere
    // to be restored to.
    let _config = config_guard();
    let root = scratch("first-listed");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(&live).unwrap();

    let plan = log_service(&live).plan_switch(&ctx, UUID_ONE).unwrap();
    let restored: Vec<&str> = plan
        .steps
        .iter()
        .filter(|s| s.action == PlanAction::Restore)
        .map(|s| s.target.as_str())
        .collect();
    assert!(
        restored
            .iter()
            .any(|t| t.contains("Config") && !t.contains("WindowsEditor")),
        "{restored:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_failed_restore_leaves_every_live_file_as_it_was() {
    // The session files are one credential set: a half-applied restore
    // would leave the incoming account's file next to the outgoing one's.
    let _config = config_guard();
    let root = scratch("staged-restore");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    fs::create_dir_all(live.join("Config").join("Windows")).unwrap();
    fs::write(live.join("user.dat"), b"incoming").unwrap();
    fs::write(
        live.join("Config").join("Windows").join("settings.ini"),
        b"incoming",
    )
    .unwrap();

    let service = log_service(&live);
    service.save_snapshot(&ctx, UUID_ONE).unwrap();

    // The second snapshot file is replaced by a directory, so decrypting it
    // fails after the first one has already been staged.
    let snapshot = service.snapshot_root(&ctx, UUID_ONE).unwrap();
    fs::remove_file(snapshot.join("settings.ini")).unwrap();
    fs::create_dir_all(snapshot.join("settings.ini")).unwrap();

    fs::write(live.join("user.dat"), b"outgoing").unwrap();
    fs::write(
        live.join("Config").join("Windows").join("settings.ini"),
        b"outgoing",
    )
    .unwrap();

    assert!(service.restore_snapshot(&ctx, UUID_ONE).is_err());
    assert_eq!(fs::read(live.join("user.dat")).unwrap(), b"outgoing");
    assert_eq!(
        fs::read(live.join("Config").join("Windows").join("settings.ini")).unwrap(),
        b"outgoing"
    );
    assert!(!live.join("user.dat.accshift-restore-tmp").exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn non_empty_check_walks_into_subdirectories_only_when_asked() {
    let root = scratch("non-empty");
    let dir = root.join("auth");
    fs::create_dir_all(dir.join("nested")).unwrap();
    assert!(!path_has_content(&dir, true));

    fs::write(dir.join("nested").join("token.bin"), b"x").unwrap();
    assert!(path_has_content(&dir, true));
    assert!(!path_has_content(&dir, false));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_directory_is_fresh_on_the_newest_file_below_it() {
    // A store of many files is rewritten file by file, so the directory's
    // own mtime says nothing about the session inside it.
    let root = scratch("dir-freshness");
    let store = root.join("leveldb");
    fs::create_dir_all(&store).unwrap();
    assert!(!file_is_fresh(&store, 60_000));

    let old = store.join("000001.ldb");
    fs::write(&old, b"data").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(600))
        .unwrap();
    assert!(!file_is_fresh(&store, 60_000));

    fs::write(store.join("000002.log"), b"data").unwrap();
    assert!(file_is_fresh(&store, 60_000));
    let _ = fs::remove_dir_all(&root);
}

// -----------------------------------------------------------------------
// Native hook: a platform whose id no template can point at
// -----------------------------------------------------------------------

/// Modelled on Discord: the id comes from a compiled hook reading a binary
/// store, the launcher is closed before the outgoing account is captured,
/// and the flow adopts a session nothing tracks yet. The id is `discord` so
/// the allowlist accepts the hook and the config bridge finds its section.
fn hook_fixture(live_root: &Path) -> String {
    let live = live_root.display().to_string().replace('\\', "/");
    format!(
        r#"{{
          "id": "discord",
          "schemaVersion": 1,
          "name": "Test Chat",
          "shortName": "Test",
          "os": {{
            "windows": {{ {profile} }},
            "linux": {{ {profile} }},
            "macos": {{ {profile} }}
          }}
        }}"#,
        profile = hook_profile(&live)
    )
}

fn hook_profile(live: &str) -> String {
    format!(
        r#"
          "roots": {{ "files": ["{live}"] }},
          "detect": {{ "pathExists": ["{live}"] }},
          "identity": {{
            "source": {{
              "kind": "nativeHook",
              "name": "discord-leveldb",
              "paths": {{ "leveldb": "{live}/Local Storage/leveldb" }}
            }},
            "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
            "current": "config"
          }},
          "state": {{
            "directories": [
              {{
                "live": "{live}/Local Storage/leveldb",
                "snapshot": "local_storage_leveldb",
                "snapshotMarker": true,
                "clearOnSetup": true
              }}
            ],
            "captureWhen": [
              {{ "kind": "pathNonEmpty", "path": "{live}/Local Storage/leveldb", "recursive": true }}
            ]
          }},
          "close": {{ "processes": ["nothing-here.exe"], "beforeCapture": true }},
          "setup": {{ "adoptSignedIn": true, "displayNameFromId": true }}
        "#
    )
}

fn hook_service(live_root: &Path) -> DescriptorService {
    let descriptor = Descriptor::parse("test", &hook_fixture(live_root)).unwrap();
    DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
}

const SNOWFLAKE: &str = "123456789012345678";

fn seed_store(live: &Path, user_id: &str, username: Option<&str>) {
    let store = live.join("Local Storage").join("leveldb");
    fs::create_dir_all(&store).unwrap();
    fs::write(
        store.join("000003.log"),
        super::super::hooks::discord::fake_log_bytes(user_id, username),
    )
    .unwrap();
}

#[test]
fn a_native_hook_reports_the_id_and_the_name_that_came_with_it() {
    let _config = config_guard();
    let root = scratch("hook-identity");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    seed_store(&live, SNOWFLAKE, Some("sample-user"));

    let service = hook_service(&live);
    let runtime = service.runtime(&ctx).unwrap();
    let found = service.read_identity_detail(&runtime).unwrap();
    assert_eq!(found.id, SNOWFLAKE);
    assert_eq!(found.display_name.as_deref(), Some("sample-user"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_hook_only_ever_sees_the_paths_the_descriptor_declared() {
    // The store sits where the descriptor says it does, and the hook builds
    // no path of its own: moving it leaves the hook with nothing to read.
    let _config = config_guard();
    let root = scratch("hook-sandbox");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let store = live.join("Session Storage");
    fs::create_dir_all(&store).unwrap();
    fs::write(
        store.join("000003.log"),
        super::super::hooks::discord::fake_log_bytes(SNOWFLAKE, None),
    )
    .unwrap();

    assert!(hook_service(&live).read_identity(&ctx).is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_capture_gate_that_does_not_hold_stops_the_capture() {
    let _config = config_guard();
    let root = scratch("capture-gate");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = hook_service(&live);

    // The store exists but holds nothing: the user signed out by hand.
    fs::create_dir_all(live.join("Local Storage").join("leveldb")).unwrap();
    assert!(!service.capture_worth_running(&ctx));

    seed_store(&live, SNOWFLAKE, None);
    assert!(service.capture_worth_running(&ctx));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_signed_out_session_never_overwrites_the_snapshot_it_would_replace() {
    let _config = config_guard();
    let root = scratch("capture-gate-keeps");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = hook_service(&live);

    config_bridge::set_current_account(&ctx, "discord", SNOWFLAKE).unwrap();
    let snapshot = service.snapshot_root(&ctx, SNOWFLAKE).unwrap();
    fs::create_dir_all(snapshot.join("local_storage_leveldb")).unwrap();
    fs::write(snapshot.join("local_storage_leveldb").join("kept"), b"x").unwrap();

    // Live store present but empty, so the gate refuses.
    fs::create_dir_all(live.join("Local Storage").join("leveldb")).unwrap();
    service.capture_current_account(&ctx).unwrap();
    assert!(snapshot.join("local_storage_leveldb").join("kept").exists());
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_session_switched_inside_the_launcher_never_overwrites_the_marker_snapshot() {
    // The launcher's own switcher moves the live session without telling
    // us, so the marker can name an account that is no longer signed in.
    const OTHER: &str = "876543210987654321";
    let _config = config_guard();
    let root = scratch("hook-marker-drift");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = hook_service(&live);

    seed_store(&live, SNOWFLAKE, None);
    service.save_snapshot(&ctx, SNOWFLAKE).unwrap();
    config_bridge::touch_account(&ctx, "discord", SNOWFLAKE, 1).unwrap();
    config_bridge::set_current_account(&ctx, "discord", SNOWFLAKE).unwrap();
    let kept = service
        .snapshot_root(&ctx, SNOWFLAKE)
        .unwrap()
        .join("local_storage_leveldb")
        .join("000003.log");
    let before = fs::read(&kept).unwrap();

    // An account accshift does not track: nothing is captured at all.
    seed_store(&live, OTHER, None);
    service.capture_current_account(&ctx).unwrap();
    assert_eq!(fs::read(&kept).unwrap(), before);
    assert!(!service.snapshot_root(&ctx, OTHER).unwrap().exists());

    // A tracked one: the session is captured where it belongs.
    config_bridge::touch_account(&ctx, "discord", OTHER, 2).unwrap();
    service.capture_current_account(&ctx).unwrap();
    assert_eq!(fs::read(&kept).unwrap(), before);
    assert!(service.has_snapshot(&ctx, OTHER));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_platform_we_track_ourselves_does_not_list_the_live_id_as_an_account() {
    // The config holds ids we minted, so listing the id the launcher
    // reports would show the same account twice under two names.
    let _config = config_guard();
    let root = scratch("hook-listing");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    seed_store(&live, SNOWFLAKE, Some("sample-user"));

    let accounts = hook_service(&live).read_accounts(&ctx).unwrap();
    assert!(accounts.is_empty(), "{accounts:?}");
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn adding_an_account_adopts_the_session_already_signed_in() {
    // Otherwise adding a first account starts by signing the user out of
    // the one they were already using.
    let _config = config_guard();
    let root = scratch("hook-adopt");
    let live = root.join("live");
    let ctx: AppCtx = Arc::new(TempCtx { root: root.clone() });
    seed_store(&live, SNOWFLAKE, Some("sample-user"));

    let service = hook_service(&live);
    let status = service.begin_setup(ctx.clone(), Value::Null).unwrap();
    assert_eq!(status.state, "ready");
    assert_eq!(status.account_id, SNOWFLAKE);
    assert_eq!(status.account_display_name, "sample-user");

    // The session it adopted is still there, and it is now the account the
    // config points at, labelled with the name the hook read.
    assert!(live
        .join("Local Storage")
        .join("leveldb")
        .join("000003.log")
        .exists());
    assert_eq!(
        config_bridge::current_account(&*ctx, "discord").as_deref(),
        Some(SNOWFLAKE)
    );
    let accounts = service.read_accounts(&*ctx).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "sample-user");
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_session_that_already_belongs_to_an_account_is_not_adopted_twice() {
    let _config = config_guard();
    let root = scratch("hook-adopt-once");
    let live = root.join("live");
    let ctx: AppCtx = Arc::new(TempCtx { root: root.clone() });
    seed_store(&live, SNOWFLAKE, Some("sample-user"));

    let service = hook_service(&live);
    service.begin_setup(ctx.clone(), Value::Null).unwrap();

    // Second run: a current account is recorded, so the flow clears the
    // live session and waits for a sign-in instead of adopting again.
    let status = service.begin_setup(ctx.clone(), Value::Null).unwrap();
    assert_eq!(status.state, "waiting_for_client");
    assert_eq!(service.read_accounts(&*ctx).unwrap().len(), 1);
    assert!(!live.join("Local Storage").join("leveldb").exists());
    let _ = fs::remove_dir_all(&root);
}

// -----------------------------------------------------------------------
// Restore symmetry
// -----------------------------------------------------------------------

/// The plain fixture plus one registry value, served by the in-memory
/// registry so nothing reaches the real hive.
#[cfg(windows)]
fn registry_service(live_root: &Path) -> DescriptorService {
    let json = fixture(live_root)
        .replace(
            r#""roots": { "files": ["#,
            r#""roots": { "registry": [{ "root": "HKCU", "key": "Software\\AccshiftTest" }], "files": ["#,
        )
        .replace(
            r#""state": {"#,
            r#""state": {
                "registryValues": [
                  { "root": "HKCU", "key": "Software\\AccshiftTest", "value": "token", "snapshot": "registry_token.txt" }
                ],"#,
        );
    DescriptorService::new(
        Descriptor::parse("test", &json).unwrap(),
        DescriptorOrigin::Embedded,
    )
}

#[cfg(windows)]
const TEST_KEY: &str = "Software\\AccshiftTest";

#[cfg(windows)]
fn test_value() -> Option<String> {
    reg::read(RegistryHive::CurrentUser, TEST_KEY, "token")
}

#[cfg(windows)]
#[test]
fn a_file_the_account_never_had_is_removed_on_restore() {
    // The capture dropped it because the account had none, so the one
    // left live belongs to the outgoing account.
    let _config = config_guard();
    let root = scratch("restore-absent-file");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"account-one");
    fs::remove_file(live.join("session.json")).unwrap();
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    seed_live_session(&live, b"account-two");
    service.restore_snapshot(&ctx, "aaaa1111").unwrap();

    assert!(!live.join("session.json").exists());
    assert_eq!(
        fs::read(live.join("auth").join("nested").join("token.bin")).unwrap(),
        b"account-one"
    );
    // The outgoing copies kept for an undo are gone once it went through.
    assert!(!live.join("session.json.accshift-restore-old").exists());
    assert!(!live.join("auth.accshift-restore-old").exists());
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_registry_value_the_account_never_had_is_removed_on_restore() {
    let _config = config_guard();
    let _registry = reg::fake::install();
    let root = scratch("restore-absent-value");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = registry_service(&live);

    seed_live_session(&live, b"account-one");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    reg::fake::set(RegistryHive::CurrentUser, TEST_KEY, "token", "outgoing");
    service.restore_snapshot(&ctx, "aaaa1111").unwrap();
    assert_eq!(test_value(), None);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_registry_write_that_fails_fails_the_restore_and_undoes_the_files() {
    let _config = config_guard();
    let _registry = reg::fake::install();
    let root = scratch("restore-write-fails");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = registry_service(&live);

    seed_live_session(&live, b"account-one");
    reg::fake::set(RegistryHive::CurrentUser, TEST_KEY, "token", "incoming");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();

    seed_live_session(&live, b"account-two");
    reg::fake::set(RegistryHive::CurrentUser, TEST_KEY, "token", "outgoing");
    reg::fake::fail_writes(true);
    assert!(service.restore_snapshot(&ctx, "aaaa1111").is_err());

    assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-two");
    assert_eq!(test_value().as_deref(), Some("outgoing"));
    assert!(!live.join("session.json.accshift-restore-old").exists());
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_directory_that_cannot_be_restored_leaves_every_file_as_it_was() {
    // Directories used to be restored last, after the files had already
    // been swapped, so a failure there left half of each account live.
    let _config = config_guard();
    let root = scratch("restore-dir-fails");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"account-one");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    // A header with nothing decryptable behind it.
    let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
    fs::write(
        snapshot.join("auth").join("nested").join("token.bin"),
        [crate::snapshot_crypto::ENCRYPTED_HEADER, b"garbage"].concat(),
    )
    .unwrap();

    seed_live_session(&live, b"account-two");
    assert!(service.restore_snapshot(&ctx, "aaaa1111").is_err());

    assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-two");
    assert_eq!(
        fs::read(live.join("auth").join("nested").join("token.bin")).unwrap(),
        b"account-two"
    );
    assert!(!live.join("auth.accshift-restore-tmp").exists());
    assert!(!live.join("session.json.accshift-restore-tmp").exists());
    let _ = fs::remove_dir_all(&root);
}

// -----------------------------------------------------------------------
// Leaving the launcher running
// -----------------------------------------------------------------------

fn count(counter: &std::sync::atomic::AtomicUsize) -> usize {
    counter.load(std::sync::atomic::Ordering::SeqCst)
}

fn launches(service: &DescriptorService) -> usize {
    count(&service.probes.launches)
}

/// The plain fixture with a sign-in flow: `flag` says the user got past
/// the login screen, and `confirm` is whatever the test needs.
fn setup_service(live_root: &Path, confirm: &str) -> DescriptorService {
    let live = live_root.display().to_string().replace('\\', "/");
    let json = fixture(live_root).replace(
        r#""setup": { "missingSnapshotHint""#,
        &format!(
            r#""setup": {{
                "trigger": [{{ "kind": "pathNonEmpty", "path": "{live}/flag" }}],
                "confirm": [{confirm}],
                "missingSnapshotHint""#
        ),
    );
    DescriptorService::new(
        Descriptor::parse("test", &json).unwrap(),
        DescriptorOrigin::Embedded,
    )
}

#[cfg(windows)]
#[test]
fn switching_to_an_account_with_no_snapshot_changes_nothing() {
    // The launcher used to be closed and the marker cleared before the
    // missing snapshot was noticed, so the next switch captured nobody.
    let _config = config_guard();
    let root = scratch("switch-no-snapshot");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"account-one");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    config_bridge::set_current_account(&ctx, "gog", "aaaa1111").unwrap();

    let err = service.switch(&ctx, "bbbb2222").unwrap_err();
    assert!(err.starts_with("No auth snapshot found"), "{err}");
    assert_eq!(
        config_bridge::current_account(&ctx, "gog").as_deref(),
        Some("aaaa1111")
    );
    assert_eq!(launches(&service), 0);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn a_switch_that_fails_after_closing_the_launcher_starts_it_again() {
    let _config = config_guard();
    let root = scratch("switch-relaunch");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = service(&live);

    seed_live_session(&live, b"account-two");
    service.save_snapshot(&ctx, "bbbb2222").unwrap();
    let snapshot = service.snapshot_root(&ctx, "bbbb2222").unwrap();
    fs::write(
        snapshot.join("session.json"),
        [crate::snapshot_crypto::ENCRYPTED_HEADER, b"garbage"].concat(),
    )
    .unwrap();
    seed_live_session(&live, b"account-one");
    service.save_snapshot(&ctx, "aaaa1111").unwrap();
    config_bridge::set_current_account(&ctx, "gog", "aaaa1111").unwrap();

    assert!(service.switch(&ctx, "bbbb2222").is_err());
    assert_eq!(launches(&service), 1);
    assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-one");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_sign_in_that_does_not_confirm_starts_the_launcher_again() {
    let _config = config_guard();
    let root = scratch("setup-confirm-fails");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let never = live.display().to_string().replace('\\', "/") + "/never";
    let service = setup_service(
        &live,
        &format!(r#"{{ "kind": "pathNonEmpty", "path": "{never}" }}"#),
    );

    let status = service.begin(&ctx).unwrap();
    let launched = launches(&service);
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("flag"), b"x").unwrap();

    let polled = service.setup_status(&ctx, &status.setup_id).unwrap();
    assert_eq!(polled.state, "waiting_for_login");
    assert_eq!(launches(&service), launched + 1);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(windows)]
#[test]
fn an_empty_capture_leaves_no_snapshot_for_the_id_it_minted() {
    // Every rejected poll minted a fresh id and left its folder behind.
    let _config = config_guard();
    let root = scratch("setup-empty-capture");
    let live = root.join("live");
    let ctx = TempCtx { root: root.clone() };
    let service = setup_service(&live, "");

    let status = service.begin(&ctx).unwrap();
    let launched = launches(&service);
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("flag"), b"x").unwrap();

    let polled = service.setup_status(&ctx, &status.setup_id).unwrap();
    assert_eq!(polled.state, "waiting_for_login");
    assert_eq!(launches(&service), launched + 1);
    let snapshots = crate::storage::platform_snapshots_dir(&ctx, "gog").unwrap();
    let left: Vec<_> = fs::read_dir(&snapshots)
        .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_hand_off_argument_is_passed_only_to_the_binary_that_understands_it() {
    let launch: super::super::schema::Launch = serde_json::from_value(serde_json::json!({
        "args": ["--processStart", "Client.exe"],
        "argsOnlyFor": "Update.exe",
    }))
    .unwrap();
    assert_eq!(launch.args_for(Path::new("C:/App/Update.exe")).len(), 2);
    assert!(launch.args_for(Path::new("C:/App/Client.exe")).is_empty());
}
