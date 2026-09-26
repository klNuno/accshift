use super::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique temp directory per test, removed on drop.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "accshift-steam-folder-test-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn with_steam_exe(self) -> Self {
        std::fs::write(self.0.join(os::steam_executable_name()), b"stub")
            .expect("write steam executable stub");
        self
    }

    fn with_login_history(self) -> Self {
        let config_dir = self.0.join("config");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("loginusers.vdf"), b"\"users\"{}")
            .expect("write loginusers.vdf stub");
        self
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct LockCtx(PathBuf);

impl AppContext for LockCtx {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.clone())
    }
}

fn insert_test_job(setup_id: &str) {
    steam_setup_jobs().lock().unwrap().insert(
        setup_id.to_string(),
        SteamAccountSetupJob {
            steam_path: PathBuf::new(),
            known_account_ids: HashSet::new(),
            launch_started: false,
            error_message: None,
            previous_auto_login: None,
            last_touched_at: crate::platforms::now_unix_ms(),
        },
    );
}

fn take_test_job(setup_id: &str) -> Option<SteamAccountSetupJob> {
    steam_setup_jobs().lock().unwrap().remove(setup_id)
}

#[test]
fn setup_launch_waits_for_the_lock_and_reports_a_failure_when_it_cannot_take_it() {
    let tmp = TempRoot::new("setup-lock-contended");
    let ctx = LockCtx(tmp.0.clone());
    let setup_id = format!("steam-setup-test-{}", Uuid::new_v4());
    insert_test_job(&setup_id);

    let _held = crate::lock::acquire_exclusive(&ctx, std::time::Duration::from_millis(500))
        .expect("take the lock");
    let ran = std::thread::scope(|s| {
        s.spawn(|| {
            let mut ran = false;
            run_steam_setup_launch(
                &ctx,
                &setup_id,
                std::time::Duration::from_millis(150),
                || {
                    ran = true;
                    (Some("previous".into()), Ok(()))
                },
            );
            ran
        })
        .join()
        .unwrap()
    });

    let job = take_test_job(&setup_id).expect("job kept");
    assert!(!ran, "Steam was stopped without the operation lock");
    assert!(job.launch_started);
    assert!(job.error_message.is_some(), "the lock failure was dropped");
}

#[test]
fn setup_launch_records_the_previous_autologin_and_its_error() {
    let tmp = TempRoot::new("setup-lock-free");
    let ctx = LockCtx(tmp.0.clone());
    let setup_id = format!("steam-setup-test-{}", Uuid::new_v4());
    insert_test_job(&setup_id);

    run_steam_setup_launch(
        &ctx,
        &setup_id,
        std::time::Duration::from_millis(500),
        || (Some("previous".into()), Err("Steam did not start".into())),
    );

    let job = take_test_job(&setup_id).expect("job kept");
    assert!(job.launch_started);
    assert_eq!(job.previous_auto_login.as_deref(), Some("previous"));
    assert_eq!(job.error_message.as_deref(), Some("Steam did not start"));
}

#[test]
fn setup_launch_skips_a_setup_cancelled_while_it_waited() {
    let tmp = TempRoot::new("setup-cancelled");
    let ctx = LockCtx(tmp.0.clone());
    let setup_id = format!("steam-setup-test-{}", Uuid::new_v4());

    let mut ran = false;
    run_steam_setup_launch(
        &ctx,
        &setup_id,
        std::time::Duration::from_millis(500),
        || {
            ran = true;
            (None, Ok(()))
        },
    );

    assert!(!ran);
    assert!(take_test_job(&setup_id).is_none());
}

#[test]
fn cancel_restores_the_autologin_the_setup_cleared() {
    let launched = |previous: Option<&str>| SteamAccountSetupJob {
        steam_path: PathBuf::new(),
        known_account_ids: HashSet::new(),
        launch_started: true,
        error_message: None,
        previous_auto_login: previous.map(str::to_string),
        last_touched_at: 0,
    };

    assert_eq!(
        autologin_to_restore_on_cancel(&launched(Some("main")), ""),
        Some("main".to_string())
    );
    // Steam or the user already picked an account since: keep it.
    assert_eq!(
        autologin_to_restore_on_cancel(&launched(Some("main")), "newone"),
        None
    );
    // Nothing to restore.
    assert_eq!(
        autologin_to_restore_on_cancel(&launched(Some("")), ""),
        None
    );
    assert_eq!(autologin_to_restore_on_cancel(&launched(None), ""), None);

    // The launch has not run yet, or failed and restored the value itself.
    let mut pending = launched(Some("main"));
    pending.launch_started = false;
    assert_eq!(autologin_to_restore_on_cancel(&pending, ""), None);
    let mut failed = launched(Some("main"));
    failed.error_message = Some("boom".into());
    assert_eq!(autologin_to_restore_on_cancel(&failed, ""), None);
}

#[test]
fn classify_steam_folder_reports_never_signed_in_for_executable_only() {
    // The exact folder the audit found: the picker used to take it and
    // every read then said "not installed".
    let tmp = TempRoot::new("exe-only").with_steam_exe();
    assert_eq!(
        classify_steam_folder(&tmp.0),
        SteamFolder::NeverSignedIn,
        "steam.exe without config/loginusers.vdf means never signed in"
    );
}

#[test]
fn classify_steam_folder_accepts_login_history_alone() {
    // A Steam whose executable sits elsewhere (or a copied config tree)
    // still has the account list every read needs.
    let tmp = TempRoot::new("login-only").with_login_history();
    assert_eq!(classify_steam_folder(&tmp.0), SteamFolder::Usable);
}

#[test]
fn classify_steam_folder_accepts_a_complete_install() {
    let tmp = TempRoot::new("both").with_steam_exe().with_login_history();
    assert_eq!(classify_steam_folder(&tmp.0), SteamFolder::Usable);
}

#[test]
fn classify_steam_folder_rejects_an_unrelated_folder() {
    let tmp = TempRoot::new("neither");
    assert_eq!(classify_steam_folder(&tmp.0), SteamFolder::NotSteam);
}

#[test]
fn classify_steam_folder_rejects_a_missing_path_and_a_plain_file() {
    let tmp = TempRoot::new("not-a-dir");
    assert_eq!(
        classify_steam_folder(&tmp.0.join("does-not-exist")),
        SteamFolder::NotADirectory
    );
    let file = tmp.0.join("Steam");
    std::fs::write(&file, b"not a folder").expect("write file");
    assert_eq!(classify_steam_folder(&file), SteamFolder::NotADirectory);
}

// The webview splits the message on the first '|' and translates the left
// half. Losing that shape would drop it back to matching English prose.
#[test]
fn coded_path_error_carries_the_code_then_the_english_fallback() {
    let err = coded_path_error(STEAM_PATH_NEVER_SIGNED_IN, NEVER_SIGNED_IN_MESSAGE);
    assert_eq!(err.kind, PlatformErrorKind::ClientNotInstalled);
    assert_eq!(
        err.message,
        "steam_path_never_signed_in|Steam found, sign in once so it creates its login history"
    );
    let (code, english) = err.message.split_once('|').expect("code and fallback");
    assert_eq!(code, STEAM_PATH_NEVER_SIGNED_IN);
    assert_eq!(english, NEVER_SIGNED_IN_MESSAGE);
}

#[test]
fn validate_steam_id_accepts_17_digit_numeric() {
    assert!(validate_steam_id("76561198000000000").is_ok());
}

#[test]
fn validate_steam_id_rejects_too_short() {
    assert!(validate_steam_id("7656119800000000").is_err());
}

#[test]
fn validate_steam_id_rejects_too_long() {
    assert!(validate_steam_id("765611980000000001").is_err());
}

#[test]
fn validate_steam_id_rejects_non_numeric() {
    assert!(validate_steam_id("7656119800000000a").is_err());
}

#[test]
fn validate_steam_id_rejects_empty() {
    assert!(validate_steam_id("").is_err());
}

#[test]
fn validate_username_rejects_empty() {
    assert!(validate_username("").is_err());
    assert!(validate_username("   ").is_err());
}

#[test]
fn validate_username_rejects_too_long() {
    let long_name = "a".repeat(129);
    assert!(validate_username(&long_name).is_err());
}

#[test]
fn validate_username_accepts_128_chars() {
    let max_name = "a".repeat(128);
    assert!(validate_username(&max_name).is_ok());
}

#[test]
fn validate_username_rejects_control_char() {
    assert!(validate_username("bad\u{0000}name").is_err());
    assert!(validate_username("bad\nname").is_err());
}

#[test]
fn validate_username_accepts_normal_name() {
    assert!(validate_username("some_user123").is_ok());
}

#[test]
fn secret_rotation_deletes_previous_token_after_persisting_replacement() {
    let deleted = RefCell::new(Vec::new());
    let result = rotate_secret_with(
        "new secret",
        |value| {
            assert_eq!(value, "new secret");
            Ok("new-token".to_string())
        },
        |replacement| {
            assert_eq!(replacement, "new-token");
            Ok(SecretPersistence::Replaced("old-token".to_string()))
        },
        |token| {
            deleted.borrow_mut().push(token.to_string());
            Ok(())
        },
        |_, _| panic!("cleanup should succeed"),
    );

    assert!(result.unwrap());
    assert_eq!(&*deleted.borrow(), &["old-token"]);
}

#[test]
fn secret_rotation_deletes_replacement_when_persist_fails() {
    let deleted = RefCell::new(Vec::new());
    let result = rotate_secret_with(
        "new secret",
        |_| Ok("new-token".to_string()),
        |_| Err("config write failed".to_string()),
        |token| {
            deleted.borrow_mut().push(token.to_string());
            Ok(())
        },
        |_, _| panic!("cleanup should succeed"),
    );

    assert_eq!(result.unwrap_err(), "config write failed");
    assert_eq!(&*deleted.borrow(), &["new-token"]);
}

#[test]
fn clearing_secret_skips_encryption_and_deletes_previous_token() {
    let deleted = RefCell::new(Vec::new());
    let result = rotate_secret_with(
        "   ",
        |_| panic!("empty secrets must not be encrypted"),
        |replacement| {
            assert!(replacement.is_empty());
            Ok(SecretPersistence::Replaced("old-token".to_string()))
        },
        |token| {
            deleted.borrow_mut().push(token.to_string());
            Ok(())
        },
        |_, _| panic!("cleanup should succeed"),
    );

    assert!(result.unwrap());
    assert_eq!(&*deleted.borrow(), &["old-token"]);
}

#[test]
fn unused_secret_replacement_is_deleted_without_touching_current_token() {
    let deleted = RefCell::new(Vec::new());
    let result = rotate_secret_with(
        "legacy secret",
        |_| Ok("unused-token".to_string()),
        |_| Ok(SecretPersistence::Unused),
        |token| {
            deleted.borrow_mut().push(token.to_string());
            Ok(())
        },
        |_, _| panic!("cleanup should succeed"),
    );

    assert!(!result.unwrap());
    assert_eq!(&*deleted.borrow(), &["unused-token"]);
}
