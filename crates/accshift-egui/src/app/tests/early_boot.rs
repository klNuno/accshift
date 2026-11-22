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

