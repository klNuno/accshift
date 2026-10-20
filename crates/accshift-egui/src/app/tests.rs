//! `AppModel` on the real backend, over a scratch copy of the user's app
//! folders: the real snapshot is read, never written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use accshift_app::real::{EguiAppContext, RealBackend};
use accshift_core::AppContext;

use super::*;

mod native_regressions;

struct Scratch {
    root: PathBuf,
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
        Ok(self.root.join("config"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("local"))
    }
}

/// Folders a copy skips: the WebView profile and the logs are not app state.
const SKIP: [&str; 2] = ["EBWebView", "logs"];

/// Size and modification time of every app-state file under `dir`.
fn fingerprint(dir: &Path, out: &mut BTreeMap<PathBuf, (u64, Option<SystemTime>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if SKIP.iter().any(|skip| entry.file_name() == *skip) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            fingerprint(&path, out);
        } else {
            out.insert(path, (meta.len(), meta.modified().ok()));
        }
    }
}

/// Copies `from` into `to`, reading only.
fn copy_tree(from: &Path, to: &Path) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    std::fs::create_dir_all(to).unwrap();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|skip| name == *skip) {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            copy_tree(&path, &to.join(&name));
        } else {
            let _ = std::fs::write(to.join(&name), std::fs::read(&path).unwrap_or_default());
        }
    }
}

#[test]
fn real_backend_boots_on_a_copy_of_the_real_snapshot() {
    let real = EguiAppContext::new().unwrap();
    let real_dirs = [
        real.app_config_dir().unwrap(),
        real.app_local_data_dir().unwrap(),
    ];
    let mut before = BTreeMap::new();
    for dir in &real_dirs {
        fingerprint(dir, &mut before);
    }

    let root = std::env::temp_dir().join(format!("accshift-egui-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let scratch = Arc::new(Scratch { root });
    copy_tree(&real_dirs[0], &scratch.root.join("config"));
    copy_tree(&real_dirs[1], &scratch.root.join("local"));

    let ctx = egui::Context::default();
    let backend: Arc<dyn Backend> = Arc::new(RealBackend::with_context(scratch.clone()));
    let mut app = AppModel::with_backend(&ctx, backend, None, Clock::wall());
    app.wait_boot(&ctx).expect("boot");
    let booted = app.booted().expect("booted");
    assert_ne!(booted.runtime_os, RuntimeOs::Unknown);
    // The theme of the stored settings resolved to real values.
    assert_eq!(booted.theme.vars.fg.a, 1.0);
    assert!(booted
        .stores
        .settings()
        .enabled_platforms
        .iter()
        .all(|id| booted.platforms.iter().any(|p| &p.id == id)));
    assert!(
        platforms::usable(&booted.platforms, &booted.nav.active_tab, booted.runtime_os)
            || booted.nav.active_tab.is_empty()
            || booted.stores.settings().enabled_platforms.is_empty(),
        "active tab {} is usable",
        booted.nav.active_tab
    );
    drop(app);

    let mut after = BTreeMap::new();
    for dir in &real_dirs {
        fingerprint(dir, &mut after);
    }
    assert_eq!(before, after, "the real app folders were not touched");
}

#[test]
fn seed_keys_patch_the_scenario_stores() {
    let setup = MockSetup {
        theme: Some("dark".into()),
        locale: Some("fr".into()),
        seed: Some(serde_json::json!({
            "SETTINGS": { "personasEnabled": false },
            "VIEW_MODE": "list",
            "client.folders": [],
        })),
        ..MockSetup::default()
    };
    let mut stores = BTreeMap::new();
    stores.insert(
        "client.settings".to_string(),
        serde_json::json!({ "themeId": "glass-dark", "enabledPlatforms": ["steam"] }),
    );
    setup.patch_stores(&mut stores);
    assert_eq!(
        stores["client.settings"],
        serde_json::json!({
            "themeId": "dark",
            "language": "fr",
            "enabledPlatforms": ["steam"],
            "personasEnabled": false,
        })
    );
    assert_eq!(stores["client.view-mode"], "list");
    assert_eq!(stores["client.folders"], serde_json::json!([]));
}

#[test]
fn seeded_avatar_paths_resolve_like_the_scenarios() {
    let root = PathBuf::from("repo");
    let setup = MockSetup {
        seed: Some(serde_json::json!({
            "cache.steam.profiles": {
                "1": { "url": "crates/accshift-egui/parity/avatars/photo.jpg", "timestamp": 1 },
            },
        })),
        repo_root: Some(root.clone()),
        ..MockSetup::default()
    };
    let mut stores = BTreeMap::new();
    stores.insert(
        "cache.steam.profiles".to_string(),
        serde_json::json!({ "2": { "url": "kept", "timestamp": 1 } }),
    );
    setup.patch_stores(&mut stores);
    let cache = &stores["cache.steam.profiles"];
    let photo = root
        .join("crates")
        .join("accshift-egui")
        .join("parity")
        .join("avatars")
        .join("photo.jpg");
    assert_eq!(cache["1"]["url"], photo.to_string_lossy().as_ref());
    assert_eq!(cache["2"]["url"], "kept");
}

#[test]
fn a_fresh_install_enables_the_detected_platforms() {
    use accshift_app::platforms::{PlatformRegistry, RuntimeOs as AppOs};
    use accshift_app::storage::ClientStorage;
    use accshift_app::types::ClientStorageSnapshot;

    let stores_with = |entries: Vec<(&str, Value)>| {
        let snapshot = ClientStorageSnapshot {
            manifest: Default::default(),
            stores: entries
                .into_iter()
                .map(|(id, value)| (id.to_string(), value))
                .collect(),
        };
        ClientStores::new(
            ClientStorage::from_snapshot(snapshot),
            StoreContext::shipped(),
        )
    };
    let registry = PlatformRegistry::shipped();
    let found = ["riot".to_string(), "nope".to_string(), "steam".to_string()];

    // Nothing stored: the usable detected platforms become the enabled set,
    // the first one the default, and the onboarding names them.
    let mut fresh = stores_with(Vec::new());
    let detected =
        enable_detected_platforms_on_first_run(&mut fresh, &found, &registry, AppOs::Windows);
    assert_eq!(detected, ["riot", "steam"]);
    assert_eq!(fresh.settings().enabled_platforms, ["riot", "steam"]);
    assert_eq!(fresh.settings().default_platform_id, "riot");
    assert!(fresh.has_stored_settings());

    // Detection found nothing: the defaults stay, nothing is written.
    let mut empty = stores_with(Vec::new());
    assert!(
        enable_detected_platforms_on_first_run(&mut empty, &[], &registry, AppOs::Windows)
            .is_empty()
    );
    assert!(!empty.has_stored_settings());

    // Stored settings belong to the user, however few platforms they enable.
    let mut stored = stores_with(vec![(
        storage::CLIENT_STORE_SETTINGS,
        serde_json::json!({ "enabledPlatforms": ["epic"], "defaultPlatformId": "epic" }),
    )]);
    assert!(
        enable_detected_platforms_on_first_run(&mut stored, &found, &registry, AppOs::Windows)
            .is_empty()
    );
    assert_eq!(stored.settings().enabled_platforms, ["epic"]);
}
