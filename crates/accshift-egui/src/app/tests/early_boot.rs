//! The boot job `main` starts before the window exists
//! ([`super::super::early_boot`]): the model polls it as its own.

use super::super::early_boot;
use super::*;

fn mock() -> Arc<MockBackend> {
    let fixture = include_str!("../../../parity/data/empty.json");
    Arc::new(MockBackend::from_json(fixture, None).unwrap())
}

fn frame(app: &mut AppModel, ctx: &Context) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.frame(ui));
    output.textures_delta.clear();
}

#[test]
fn a_boot_read_before_the_model_lands_on_its_first_frame() {
    let ctx = Context::default();
    let mock = mock();
    let (early, task) = early_boot();
    task.run(mock.as_ref());
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    frame(&mut app, &ctx);
    assert!(app.booted().is_some());
    assert!(app.boot_error().is_none());
}

#[test]
fn a_boot_read_after_the_model_exists_asks_for_a_frame() {
    let ctx = Context::default();
    let repaints = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::clone(&repaints);
    ctx.set_request_repaint_callback(move |_| {
        seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let mock = mock();
    let (early, task) = early_boot();
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    let before = repaints.load(std::sync::atomic::Ordering::Relaxed);
    std::thread::spawn(move || task.run(mock.as_ref()))
        .join()
        .unwrap();
    assert!(repaints.load(std::sync::atomic::Ordering::Relaxed) > before);
    app.wait_boot(&ctx).unwrap();
    assert!(app.booted().is_some());
}

#[test]
fn a_boot_thread_that_dies_reports_the_boot_as_failed() {
    let ctx = Context::default();
    let mock = mock();
    let (early, task) = early_boot();
    drop(task);
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    assert!(app.wait_boot(&ctx).is_err());
    assert_eq!(app.boot_error(), Some("boot job panicked"));
}

// ------------------------------------------------- a release's folder, migrated

const LEGACY_CONFIG: &str =
    include_str!("../../../../accshift-app/tests/data/real/legacy-config.json");
const THEME: &str = include_str!("../../../../accshift-app/tests/data/real/theme.json");

/// A scratch app folder as a 1.0.5 or 2.0.0 install leaves it (settings,
/// view mode and a custom theme in their stores), plus the pre-1.0 config
/// file the boot payload migrates.
fn release_folder(tag: &str) -> Arc<Scratch> {
    use accshift_core::storage::{legacy_config_path, save_client_store, STORE_SETTINGS};
    let root =
        std::env::temp_dir().join(format!("accshift-egui-early-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let scratch = Arc::new(Scratch { root });
    let settings = serde_json::json!({
        "enabledPlatforms": [],
        "animations": "off",
        "themeId": "egui-fixture-theme",
    });
    save_client_store(scratch.as_ref(), STORE_SETTINGS, &settings).unwrap();
    let backend = RealBackend::with_context(scratch.clone());
    let theme: accshift_app::types::CustomTheme = serde_json::from_str(THEME).unwrap();
    backend.save_custom_theme(&theme).unwrap();
    backend
        .save_client_storage_store(
            accshift_core::storage::STORE_VIEW_MODE,
            &serde_json::json!("grid"),
        )
        .unwrap();
    let legacy = legacy_config_path(scratch.as_ref()).unwrap();
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, LEGACY_CONFIG).unwrap();
    scratch
}

/// Every app-state file under `root`, by relative path, with its bytes.
fn contents(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    fingerprint(root, &mut found);
    found
        .into_keys()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path.strip_prefix(root).unwrap().to_path_buf(), bytes)
        })
        .collect()
}

#[test]
fn a_release_folder_migrates_and_boots_the_same_on_the_session_thread() {
    let ctx = Context::default();
    // As `main` runs it: the boot job on its own thread, started before
    // the model exists.
    let early_dir = release_folder("session");
    let early_backend = Arc::new(RealBackend::with_context(early_dir.clone()));
    let (early, task) = early_boot();
    let on_thread = Arc::clone(&early_backend);
    std::thread::spawn(move || task.run(on_thread.as_ref()))
        .join()
        .unwrap();
    let mut early_app = AppModel::from_backend(&ctx, early_backend, Clock::wall(), Some(early));
    early_app.wait_boot(&ctx).expect("early boot");

    // As before: the model's own boot job.
    let job_dir = release_folder("job");
    let job_backend: Arc<dyn Backend> = Arc::new(RealBackend::with_context(job_dir.clone()));
    let mut job_app = AppModel::with_backend(&ctx, job_backend, None, Clock::wall());
    job_app.wait_boot(&ctx).expect("job boot");

    // The legacy config was migrated and kept aside.
    for dir in [&early_dir, &job_dir] {
        let legacy = accshift_core::storage::legacy_config_path(dir.as_ref()).unwrap();
        assert!(!legacy.exists());
        let config = accshift_core::config::load_config(dir.as_ref());
        let labels: Vec<&str> = config
            .riot
            .profiles
            .iter()
            .map(|p| p.label.as_str())
            .collect();
        assert_eq!(labels, ["Main", "Smurf"]);
    }
    // Both boots read the same state and left the same files.
    let (early, job) = (early_app.booted().unwrap(), job_app.booted().unwrap());
    assert_eq!(early.stores.settings(), job.stores.settings());
    assert_eq!(early.stores.settings().theme_id, "egui-fixture-theme");
    assert!(early.registry.document("egui-fixture-theme").is_some());
    assert_eq!(early.nav.active_tab, job.nav.active_tab);
    assert_eq!(early.locale, job.locale);
    assert!(early.init_error.is_none() && job.init_error.is_none());
    let files = contents(&early_dir.root);
    assert!(files.len() >= 4, "{:?}", files.keys());
    assert_eq!(files, contents(&job_dir.root));
}

#[test]
fn a_release_folder_keeps_its_window_geometry_through_the_session_thread() {
    let ctx = Context::default();
    let dir = release_folder("geometry");
    let backend = Arc::new(RealBackend::with_context(dir.clone()));
    let session = super::super::session::start(&backend, |_| None);
    // The pre-1.0 file's size, not the default one.
    assert_eq!(session.saved.size, Some((1180.0, 760.0)));
    assert_eq!(session.legacy.recv().unwrap().map(|read| read.notes), None);
    session.thread.join().unwrap();

    let mut app = AppModel::from_backend(&ctx, backend, Clock::wall(), Some(session.early));
    app.wait_boot(&ctx).expect("early boot");
    // Migrated into the split files, which hold the same geometry.
    let legacy = accshift_core::storage::legacy_config_path(dir.as_ref()).unwrap();
    assert!(!legacy.exists());
    let after = accshift_core::config::load_window(dir.as_ref());
    assert_eq!(after, session.saved);
}

#[test]
fn the_telemetry_consent_is_read_once_the_release_folder_is_migrated() {
    let ctx = Context::default();
    let dir = release_folder("telemetry");
    // A pre-1.0 file that consented: the queue sets an anonymous id.
    let legacy = accshift_core::storage::legacy_config_path(dir.as_ref()).unwrap();
    let mut config: serde_json::Value = serde_json::from_str(LEGACY_CONFIG).unwrap();
    config["telemetry"] = serde_json::json!({
        "onboarding_completed": true,
        "mode_a_enabled": true,
    });
    std::fs::write(&legacy, config.to_string()).unwrap();
    // A closed port: the queue starts, nothing is ever sent.
    let backend = Arc::new(RealBackend::with_telemetry_endpoint(
        dir.clone(),
        "http://127.0.0.1:9",
    ));
    let session = super::super::session::start(&backend, |_| None);
    // As `main` does: the telemetry starts once the legacy read is in,
    // before the maintenance and while the boot runs.
    let _ = session.legacy.recv().unwrap();
    assert!(!legacy.exists(), "read before the migration");
    assert!(backend.start_telemetry());
    session.thread.join().unwrap();
    let mut app = AppModel::from_backend(&ctx, backend.clone(), Clock::wall(), Some(session.early));
    app.wait_boot(&ctx).expect("early boot");
    let config = accshift_core::config::load_config(dir.as_ref());
    assert!(config.telemetry.onboarding_completed);
    assert!(accshift_core::telemetry::install_id::is_valid(
        &config.telemetry.anonymous_id
    ));
}

#[test]
fn the_legacy_read_is_handed_over_once_the_config_is_migrated() {
    let ctx = Context::default();
    let dir = release_folder("order");
    let legacy = accshift_core::storage::legacy_config_path(dir.as_ref()).unwrap();
    let backend = Arc::new(RealBackend::with_context(dir.clone()));
    let session = super::super::session::start(&backend, |_| None);
    // What `main` waits for before the telemetry and the maintenance.
    assert_eq!(session.legacy.recv().unwrap().map(|read| read.notes), None);
    assert!(!legacy.exists(), "the migration was still to come");
    session.thread.join().unwrap();
    let mut app = AppModel::from_backend(&ctx, backend, Clock::wall(), Some(session.early));
    app.wait_boot(&ctx).expect("early boot");
    // The boot still reports the migration it no longer runs itself.
    let toasts = format!("{:?}", app.toasts().toasts());
    assert!(toasts.contains("legacyConfigMigrated"), "{toasts}");
}

#[test]
fn an_in_flight_early_boot_finishing_shortly_after_frame_zero_starts_lands_on_frame_zero() {
    let ctx = Context::default();
    let mock = mock();
    let (early, task) = early_boot();
    let mut app = AppModel::from_backend(&ctx, mock.clone(), Clock::wall(), Some(early));
    let worker = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        task.run(mock.as_ref());
    });
    frame(&mut app, &ctx);
    worker.join().unwrap();
    assert!(app.booted().is_some());
    assert!(app.boot_error().is_none());
}

#[test]
fn real_model_marks_boot_state_ready_synchronously_on_first_frame() {
    let ctx = Context::default();
    let dir = release_folder("sync-ready");
    let backend = Arc::new(RealBackend::with_context(dir));
    let (early, task) = early_boot();
    task.run(backend.as_ref());
    let mut app = AppModel::real(&ctx, Arc::clone(&backend), None, Some(early));
    assert!(!backend.boot_state().is_completed());
    frame(&mut app, &ctx);
    assert!(app.booted().is_some());
    assert!(backend.boot_state().is_completed());
}

#[test]
fn session_start_with_runs_hooks_in_order_around_config_migration() {
    use std::sync::Mutex;

    let ctx = Context::default();
    let dir = release_folder("start-with");
    let legacy_path = accshift_core::storage::legacy_config_path(dir.as_ref()).unwrap();
    let backend = Arc::new(RealBackend::with_context(dir));
    let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    let e1 = Arc::clone(&events);
    let e2 = Arc::clone(&events);
    let e3 = Arc::clone(&events);
    let legacy_in_after = legacy_path.clone();

    let session = super::super::session::start_with(
        &backend,
        move |_| {
            e1.lock().unwrap().push("read_legacy");
            None
        },
        move |_| {
            assert!(
                !legacy_in_after.exists(),
                "after_migrate must see the migrated config"
            );
            e2.lock().unwrap().push("after_migrate");
        },
        move |_| {
            e3.lock().unwrap().push("after_ready");
        },
    );

    assert_eq!(session.legacy.recv().unwrap().map(|read| read.notes), None);
    {
        let seen = events.lock().unwrap();
        assert!(seen.starts_with(&["read_legacy", "after_migrate"]), "{seen:?}");
    }
    session.thread.join().unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec!["read_legacy", "after_migrate", "after_ready"]
    );

    let mut app = AppModel::real(&ctx, backend, None, Some(session.early));
    frame(&mut app, &ctx);
    assert!(app.booted().is_some());
}
