//! The legacy single-file config and its one-time migration.

#[allow(unused_imports)]
use super::*;

/// Check for a legacy config.json, migrate it to portable+local, and delete it.
/// Returns `Some("ok")` if migrated, `Some(error)` if failed, `None` if no legacy.
pub fn migrate_legacy_config(app_handle: &dyn AppContext) -> Option<Result<(), String>> {
    let legacy_path = crate::storage::legacy_config_path(app_handle).ok()?;
    if !legacy_path.exists() {
        return None;
    }

    let portable_path = match crate::storage::portable_config_path(app_handle) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };

    // If portable already exists, legacy is stale. Set it aside.
    if portable_path.exists() {
        retire_legacy_config(app_handle, &legacy_path);
        return None;
    }

    // Migrate: load legacy, save as portable+local, delete legacy.
    //
    // Parse the legacy file explicitly here instead of via load_legacy_config:
    // that helper returns AppConfig::default() on a parse error (fine for a
    // read-only load fallback), but migrating a default would save empty
    // portable/local files and then delete the only real copy. If the legacy
    // file is corrupt we must NOT destroy it: keep a backup next to it and
    // surface an error so the user can recover their accounts and API key.
    let data = match fs::read_to_string(&legacy_path) {
        Ok(d) => d,
        Err(e) => return Some(Err(format!("Could not read legacy config: {e}"))),
    };
    let raw = match serde_json::from_str::<RawAppConfig>(&data) {
        Ok(raw) => raw,
        Err(e) => {
            let mut backup = legacy_path.clone();
            let name = backup
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "config.json".into());
            backup.set_file_name(format!("{name}.corrupt-backup"));
            let _ = fs::copy(&legacy_path, &backup);
            let _ = crate::logging::append_app_log(
                app_handle,
                "error",
                "config.migrate_legacy",
                "Legacy config is corrupt; kept a backup and skipped migration",
                Some(&e.to_string()),
            );
            return Some(Err(format!("Legacy config is corrupt: {e}")));
        }
    };
    let legacy = normalize_config(raw);
    if let Err(e) = save_config(app_handle, &legacy) {
        return Some(Err(format!("Failed to write migrated config: {e}")));
    }

    retire_legacy_config(app_handle, &legacy_path);

    Some(Ok(()))
}

/// Rename the legacy `config.json` to `config.json.migrated` instead of
/// deleting it, so a split pair that later turns out incomplete still has a
/// third copy to recover from. Nothing reads the renamed file.
pub(super) fn retire_legacy_config(app_handle: &dyn AppContext, legacy_path: &std::path::Path) {
    let mut retired = legacy_path.as_os_str().to_owned();
    retired.push(".migrated");
    let retired = std::path::PathBuf::from(retired);
    let result = if retired.exists() {
        // An older copy is already set aside; this one is the staler twin.
        fs::remove_file(legacy_path)
    } else {
        fs::rename(legacy_path, &retired)
    };
    if let Err(e) = result {
        let _ = crate::logging::append_app_log(
            app_handle,
            "warn",
            "config.migrate_legacy",
            &format!("Migrated config but could not retire legacy file: {e}"),
            None,
        );
    }
}

pub(super) fn load_legacy_config(app_handle: &dyn AppContext) -> AppConfig {
    let path = match crate::storage::legacy_config_path(app_handle) {
        Ok(path) => path,
        Err(_) => return AppConfig::default(),
    };

    match fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str::<RawAppConfig>(&data) {
            Ok(raw) => normalize_config(raw),
            Err(e) => {
                // Falling back to defaults silently would hide that the whole
                // legacy config was dropped.
                let _ = crate::logging::append_app_log(
                    app_handle,
                    "error",
                    "config.load_legacy",
                    "Legacy config corrupted, using defaults",
                    Some(&e.to_string()),
                );
                AppConfig::default()
            }
        },
        Err(_) => AppConfig::default(),
    }
}
