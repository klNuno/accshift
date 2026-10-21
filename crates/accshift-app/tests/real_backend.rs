//! `RealBackend` part 1 on scratch app directories: storage, legacy
//! migration, themes, descriptors, the PIN session, logs and boot. Nothing
//! here opens a window, a dialog, a URL or a folder, and nothing reads or
//! writes the real `%APPDATA%\com.accshift.desktop`.

use accshift_app::dispatch::dispatch;
use accshift_app::real::{RealBackend, UI_DETAILS_MAX_BYTES, UI_LOG_CAP};
use accshift_app::types::CustomTheme;
use accshift_app::{Backend, BackendError};
use accshift_core::pin::{hash_pin_code, PIN_LOCKED_MESSAGE};
use accshift_core::storage::{STORE_FOLDERS, STORE_SETTINGS, STORE_VIEW_MODE};
use accshift_core::AppContext;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// A context on a scratch folder, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Arc<Self> {
        let root =
            std::env::temp_dir().join(format!("accshift-app-real-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Arc::new(Self { root })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl AppContext for Scratch {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("config"))
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("data"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("cache"))
    }
}

fn backend(tag: &str) -> (Arc<Scratch>, RealBackend) {
    let scratch = Scratch::new(tag);
    let real = RealBackend::with_context(scratch.clone());
    (scratch, real)
}

/// `reload_user_platforms` swaps a process-wide registry: the tests that
/// reload take turns.
fn registry_turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/real")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn app_log(scratch: &Scratch) -> String {
    let path = accshift_core::logging::log_file_path(scratch).unwrap();
    std::fs::read_to_string(path).unwrap_or_default()
}

// ---------------------------------------------------------------- storage

#[test]
fn a_saved_store_comes_back_in_the_snapshot_and_the_manifest() {
    let (scratch, real) = backend("storage");

    // A fresh install lists every store, each `null`.
    let empty = real.load_client_storage_snapshot().unwrap();
    assert_eq!(empty.stores.len(), 12);
    assert!(
        empty.stores.values().all(Value::is_null),
        "{:?}",
        empty.stores
    );

    let folders = json!({ "version": 2, "folders": { "f1": { "name": "Main" } } });
    let fingerprint = real
        .save_client_storage_store(STORE_FOLDERS, &folders)
        .unwrap()
        .expect("the real command answers the fingerprint");
    let view = json!("grid");
    real.save_client_storage_store(STORE_VIEW_MODE, &view)
        .unwrap();

    let snapshot = real.load_client_storage_snapshot().unwrap();
    assert_eq!(snapshot.stores.get(STORE_FOLDERS), Some(&folders));
    assert_eq!(snapshot.stores.get(STORE_VIEW_MODE), Some(&view));

    let manifest = real.get_storage_manifest().unwrap();
    assert_eq!(manifest.stores.get(STORE_FOLDERS), Some(&fingerprint));
    assert!(manifest.stores.contains_key(STORE_VIEW_MODE));
    assert_eq!(snapshot.manifest.stores, manifest.stores);
    assert_eq!(snapshot.manifest.schema_version, manifest.schema_version);

    // A null value deletes the store: it reads back as `null`.
    real.save_client_storage_store(STORE_VIEW_MODE, &Value::Null)
        .unwrap();
    let snapshot = real.load_client_storage_snapshot().unwrap();
    assert_eq!(snapshot.stores.get(STORE_VIEW_MODE), Some(&Value::Null));

    // Same log lines as the Tauri commands.
    let log = app_log(&scratch);
    assert!(log.contains("Saved client storage store"), "{log}");
    assert!(log.contains("Loaded client storage snapshot"), "{log}");
    assert!(log.contains("Built storage manifest"), "{log}");
}

#[test]
fn an_unknown_store_id_is_refused_with_the_core_message() {
    let (_scratch, real) = backend("storage-unknown");
    let error = real
        .save_client_storage_store("client.nope", &json!({}))
        .unwrap_err();
    assert_eq!(error.message, "Unknown client store id: client.nope");
}

#[test]
fn the_manifest_and_snapshot_dispatch_to_the_webview_shape() {
    let (_scratch, real) = backend("storage-dispatch");
    let saved = dispatch(
        &real,
        "save_client_storage_store",
        &json!({ "storeId": STORE_VIEW_MODE, "value": "list" }),
    )
    .unwrap()
    .unwrap();
    assert!(saved.is_string(), "{saved}");
    let manifest = dispatch(&real, "get_storage_manifest", &json!({}))
        .unwrap()
        .unwrap();
    assert!(manifest["schemaVersion"].is_u64(), "{manifest}");
    assert_eq!(manifest["stores"][STORE_VIEW_MODE], saved);
}

// ---------------------------------------------------------------- legacy migration and boot

#[test]
fn a_legacy_config_is_migrated_once_and_kept_aside() {
    let (scratch, real) = backend("legacy");
    let legacy = accshift_core::storage::legacy_config_path(&*scratch).unwrap();
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, fixture("legacy-config.json")).unwrap();

    assert_eq!(real.migrate_legacy_config().unwrap(), "migrated");

    assert!(!legacy.exists());
    let mut retired = legacy.clone().into_os_string();
    retired.push(".migrated");
    assert_eq!(
        std::fs::read_to_string(retired).unwrap(),
        fixture("legacy-config.json")
    );
    let config = accshift_core::config::load_config(&*scratch);
    let labels: Vec<&str> = config
        .riot
        .profiles
        .iter()
        .map(|profile| profile.label.as_str())
        .collect();
    assert_eq!(labels, ["Main", "Smurf"]);

    assert_eq!(real.migrate_legacy_config().unwrap(), "none");
}

#[test]
fn a_corrupt_legacy_config_answers_an_error_string() {
    let (scratch, real) = backend("legacy-corrupt");
    let legacy = accshift_core::storage::legacy_config_path(&*scratch).unwrap();
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, b"{ not json").unwrap();

    let answer = real.migrate_legacy_config().unwrap();
    assert!(
        answer.starts_with("error:Legacy config is corrupt: "),
        "{answer}"
    );
    assert!(legacy.exists(), "the only copy stays");
}

#[test]
fn the_boot_payload_migrates_then_reads_everything() {
    let _turn = registry_turn();
    let (scratch, real) = backend("boot");
    let legacy = accshift_core::storage::legacy_config_path(&*scratch).unwrap();
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, fixture("legacy-config.json")).unwrap();
    let theme: CustomTheme = serde_json::from_str(&fixture("theme.json")).unwrap();
    real.save_custom_theme(&theme).unwrap();
    real.save_client_storage_store(STORE_VIEW_MODE, &json!("grid"))
        .unwrap();

    let payload = real.get_boot_payload().unwrap();
    assert_eq!(payload.migration, "migrated");
    assert_eq!(payload.runtime_os, std::env::consts::OS);
    assert_eq!(
        payload.storage_snapshot.stores.get(STORE_VIEW_MODE),
        Some(&json!("grid"))
    );
    assert_eq!(payload.custom_themes.len(), 1);
    assert_eq!(payload.custom_themes[0].id, "egui-fixture-theme");
    assert!(payload.user_platforms.loaded.is_empty());
    assert!(payload
        .user_platforms
        .dir
        .ends_with(&format!("{}descriptors", std::path::MAIN_SEPARATOR)));

    let json = serde_json::to_value(&payload).unwrap();
    for key in [
        "migration",
        "runtimeOs",
        "storageSnapshot",
        "customThemes",
        "userPlatforms",
    ] {
        assert!(json.get(key).is_some(), "{key} missing from {json}");
    }
}

#[test]
fn finish_boot_completes_once_and_logs_both_calls() {
    let (scratch, real) = backend("finish-boot");
    assert!(!real.boot_state().is_completed());

    real.finish_boot("app.boot", Some(&json!({ "firstFrameMs": 120 })))
        .unwrap();
    assert!(real.boot_state().is_completed());
    real.finish_boot("app.boot", None).unwrap();

    let log = app_log(&scratch);
    assert!(log.contains("Boot completed"), "{log}");
    assert!(log.contains("Boot completion requested again"), "{log}");
    assert!(log.contains("Frontend startup profile"), "{log}");
}

#[test]
fn runtime_os_and_streaming_detection_answer() {
    let (_scratch, real) = backend("runtime");
    assert_eq!(real.get_runtime_os().unwrap(), std::env::consts::OS);
    // A process scan: the answer depends on the machine, only its shape is
    // checked.
    let _: bool = real.detect_streaming_software().unwrap();
}

// ---------------------------------------------------------------- themes

#[test]
fn a_custom_theme_is_saved_listed_and_deleted() {
    let (_scratch, real) = backend("themes");
    assert!(real.list_custom_themes().unwrap().is_empty());

    let theme: CustomTheme = serde_json::from_str(&fixture("theme.json")).unwrap();
    real.save_custom_theme(&theme).unwrap();
    let mut second = theme.clone();
    second.id = "another-fixture".into();
    second.name = "Another".into();
    real.save_custom_theme(&second).unwrap();

    let listed = real.list_custom_themes().unwrap();
    let names: Vec<&str> = listed.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Another", "Fixture Violet"], "sorted by name");
    assert_eq!(
        serde_json::to_value(&listed[1]).unwrap(),
        serde_json::from_str::<Value>(&fixture("theme.json")).unwrap()
    );

    real.delete_custom_theme("another-fixture").unwrap();
    let listed = real.list_custom_themes().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "egui-fixture-theme");
    // Deleting what is not there is not an error.
    real.delete_custom_theme("another-fixture").unwrap();
}

#[test]
fn unsafe_theme_ids_are_refused_with_the_core_messages() {
    let (_scratch, real) = backend("themes-bad");
    let mut theme: CustomTheme = serde_json::from_str(&fixture("theme.json")).unwrap();
    theme.id = "../escape".into();
    assert_eq!(
        real.save_custom_theme(&theme).unwrap_err().message,
        "Invalid theme ID: only alphanumeric characters, hyphens, and underscores are allowed"
    );
    assert_eq!(
        real.delete_custom_theme("../escape").unwrap_err().message,
        "Invalid theme ID"
    );
}

// ---------------------------------------------------------------- descriptors

#[test]
fn a_descriptor_is_previewed_installed_and_removed() {
    let _turn = registry_turn();
    let (scratch, real) = backend("descriptors");
    let live = scratch.root.join("live");
    let body = fixture("descriptor.json")
        .replace("${LIVE}", &live.display().to_string().replace('\\', "/"));
    let picked = scratch.root.join("picked-by-user.json");
    std::fs::write(&picked, &body).unwrap();
    let picked_path = picked.display().to_string();

    let preview = real.descriptor_preview_file(&picked_path).unwrap();
    assert_eq!(preview.descriptor.id, "egui-fixture");
    assert_eq!(preview.file_name, "egui-fixture.json");
    assert!(preview.blocked.is_empty(), "{}", preview.blocked);
    assert!(!preview.replaces);

    let installed = real.descriptor_install_file(&picked_path).unwrap();
    let ids: Vec<&str> = installed
        .loaded
        .iter()
        .map(|descriptor| descriptor.id.as_str())
        .collect();
    assert_eq!(ids, ["egui-fixture"]);
    let in_folder = Path::new(&installed.dir).join("egui-fixture.json");
    assert_eq!(std::fs::read_to_string(&in_folder).unwrap(), body);
    assert!(picked.exists(), "the picked file stays the user's");
    assert_eq!(real.reload_user_platforms().unwrap().loaded.len(), 1);
    assert!(
        real.descriptor_preview_file(&picked_path).unwrap().replaces,
        "a second preview says it would replace the installed one"
    );

    let removed = real.descriptor_remove("egui-fixture").unwrap();
    assert!(removed.loaded.is_empty());
    assert!(!in_folder.exists());
    assert!(real.reload_user_platforms().unwrap().loaded.is_empty());
}

#[test]
fn descriptor_errors_keep_the_core_messages() {
    let _turn = registry_turn();
    let (scratch, real) = backend("descriptors-bad");
    assert_eq!(
        real.descriptor_remove("steam").unwrap_err().message,
        "`steam` is a platform this build ships"
    );
    assert_eq!(
        real.descriptor_remove("../x").unwrap_err().message,
        "`../x` is not a platform id"
    );
    let missing = scratch.root.join("missing.json");
    let error = real
        .descriptor_install_file(&missing.display().to_string())
        .unwrap_err();
    assert!(error.message.contains("could not be read"), "{error}");
}

// ---------------------------------------------------------------- PIN

fn settings_with_pin(enabled: bool, pin: &str) -> Value {
    json!({
        "pinEnabled": enabled,
        "pinHash": if enabled { hash_pin_code(pin).unwrap() } else { String::new() },
    })
}

#[test]
fn without_a_pin_unlock_is_not_configured() {
    let (_scratch, real) = backend("pin-none");
    let result = real.pin_unlock("1234").unwrap();
    assert_eq!(result.status, "not_configured");
    assert_eq!(result.retry_after_ms, 0);
    assert!(real.pin_session().ensure_unlocked(real.context()).is_ok());
}

#[test]
fn the_pin_session_locks_unlocks_and_guards_the_settings_store() {
    let (scratch, real) = backend("pin");
    // Saved through the backend: no PIN on disk yet, so the session decides
    // "unlocked" on this first write, as in the Tauri app.
    real.save_client_storage_store(STORE_SETTINGS, &settings_with_pin(true, "1234"))
        .unwrap();
    assert!(real.pin_session().ensure_unlocked(real.context()).is_ok());

    real.pin_lock().unwrap();
    let refused = real
        .pin_session()
        .ensure_unlocked(real.context())
        .unwrap_err();
    assert_eq!(refused.message, PIN_LOCKED_MESSAGE);
    assert!(BackendError::from(refused).is_pin_locked());

    // Locked: the PIN cannot be turned off through the store.
    let error = real
        .save_client_storage_store(STORE_SETTINGS, &settings_with_pin(false, ""))
        .unwrap_err();
    assert_eq!(error.message, PIN_LOCKED_MESSAGE);

    // A code of the wrong length fails before PBKDF2, which takes up to a
    // second unoptimized on a slow runner: the retry right after it then
    // always lands inside the one second wait, which counts from the start of
    // the failed attempt.
    let wrong = real.pin_unlock("00").unwrap();
    assert_eq!(wrong.status, "invalid");
    assert_eq!(wrong.retry_after_ms, 1000);
    let too_soon = real.pin_unlock("1234").unwrap();
    assert_eq!(too_soon.status, "retry_later");
    assert!(too_soon.retry_after_ms > 0 && too_soon.retry_after_ms <= 1000);

    std::thread::sleep(std::time::Duration::from_millis(1050));
    let right = dispatch(&real, "pin_unlock", &json!({ "code": "1234" }))
        .unwrap()
        .unwrap();
    assert_eq!(
        right,
        json!({ "status": "unlocked", "legacy": false, "retryAfterMs": 0 })
    );
    assert!(real.pin_session().ensure_unlocked(real.context()).is_ok());

    // Unlocked: the same write goes through.
    real.save_client_storage_store(STORE_SETTINGS, &settings_with_pin(false, ""))
        .unwrap();

    // A new process with a PIN on disk starts locked.
    real.save_client_storage_store(STORE_SETTINGS, &settings_with_pin(true, "4321"))
        .unwrap();
    let next = RealBackend::with_context(scratch.clone());
    assert!(next.pin_session().ensure_unlocked(next.context()).is_err());
    assert_eq!(next.pin_unlock("4321").unwrap().status, "unlocked");
}

// ---------------------------------------------------------------- logs and URLs

#[test]
fn ui_log_records_are_trimmed_and_capped_per_session() {
    let (scratch, real) = backend("logs");
    let long = "x".repeat(UI_DETAILS_MAX_BYTES * 2);
    real.log_app_event("info", "ui.test", "first record", Some(&long))
        .unwrap();
    let log = app_log(&scratch);
    assert!(log.contains("first record"), "{log}");
    assert!(
        !log.contains(&long),
        "details are cut to {UI_DETAILS_MAX_BYTES} bytes"
    );

    for i in 1..UI_LOG_CAP {
        real.log_app_event("debug", "ui.test", &format!("record {i}"), None)
            .unwrap();
    }
    real.log_app_event("info", "ui.test", "over the cap", None)
        .unwrap();
    real.log_app_event("info", "ui.test", "still over", None)
        .unwrap();
    let log = app_log(&scratch);
    assert_eq!(
        log.matches("Webview log cap reached").count(),
        1,
        "warned once"
    );
    assert!(!log.contains("over the cap"));
    assert!(!log.contains("still over"));
}

#[test]
fn open_url_refuses_what_the_tauri_command_refuses() {
    let (_scratch, real) = backend("open-url");
    // Refused before anything is launched: nothing opens.
    for url in [
        "file:///C:/Windows/System32/calc.exe",
        "javascript:alert(1)",
        "not a url",
    ] {
        let error = real.open_url(url).unwrap_err();
        assert_eq!(
            error.message,
            format!("Could not start process: Refusing to open URL with disallowed scheme: {url}")
        );
    }
}

// ---------------------------------------------------------------- wallpaper

/// Captures the real desktop picture (read only, no window). Ignored by
/// default: run with `--ignored` on a desktop session.
#[test]
#[ignore]
fn the_wallpaper_capture_answers_a_jpeg_data_url() {
    let (_scratch, real) = backend("wallpaper");
    let snapshot = real.get_desktop_wallpaper().unwrap();
    if cfg!(windows) {
        let snapshot = snapshot.expect("a capture on a Windows desktop session");
        assert!(snapshot.data_url.starts_with("data:image/jpeg;base64,"));
        assert!(snapshot.width > 0 && snapshot.height > 0);
        println!(
            "wallpaper: {}x{} at {},{}, {} bytes of data URL",
            snapshot.width,
            snapshot.height,
            snapshot.x,
            snapshot.y,
            snapshot.data_url.len()
        );
        // The second call is served from the cache: same answer.
        assert_eq!(real.get_desktop_wallpaper().unwrap(), Some(snapshot));
    } else {
        assert_eq!(snapshot, None);
    }
}
