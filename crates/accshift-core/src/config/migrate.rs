//! The legacy single-file config and its one-time migration.

use super::store::{read_config_file, set_config_unreadable};
#[allow(unused_imports)]
use super::*;

/// Check for a legacy config.json, migrate it to portable+local, and delete it.
/// Returns `Some("ok")` if migrated, `Some(error)` if failed, `None` if no legacy.
pub fn migrate_legacy_config(app_handle: &dyn AppContext) -> Option<Result<(), String>> {
    let legacy_path = crate::storage::legacy_config_path(app_handle).ok()?;
    match legacy_path.try_exists() {
        Ok(true) => {}
        Ok(false) => return None,
        Err(e) => {
            // Present or not, nothing may replace it until a read succeeds.
            set_config_unreadable(&legacy_path, true);
            return Some(Err(format!("Could not check legacy config: {e}")));
        }
    }

    let portable_path = match crate::storage::portable_config_path(app_handle) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };

    let local_path = match crate::storage::local_config_path(app_handle) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };

    // Legacy is stale only once both split files are on disk. A portable file
    // without its local twin is a migration that stopped after the first
    // write. Retiring here would throw away the only copy of the other half.
    if portable_path.exists() && local_path.exists() {
        retire_legacy_config(app_handle, &legacy_path);
        return None;
    }

    // Migrate: load legacy, save as portable+local, retire legacy.
    //
    // Parse the legacy file explicitly here instead of via load_legacy_config:
    // migrating the defaults a failed read returns would save empty split
    // files over the only real copy. If the legacy file is corrupt we must
    // NOT destroy it: keep a backup next to it, poison saves, and surface an
    // error so the user can recover their accounts and API key. The poison
    // matters when one split file exists: the load then reads that file and
    // never looks at legacy, so nothing else would stop a save of defaults.
    let data = match fs::read_to_string(&legacy_path) {
        Ok(d) => d,
        Err(e) => {
            set_config_unreadable(&legacy_path, true);
            return Some(Err(format!("Could not read legacy config: {e}")));
        }
    };
    let raw = match serde_json::from_str::<RawAppConfig>(&data) {
        Ok(raw) => raw,
        Err(e) => {
            set_config_unreadable(&legacy_path, true);
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
    set_config_unreadable(&legacy_path, false);
    let legacy = normalize_config(raw);
    // A split file already on disk is kept and legacy fills only the missing
    // half. Split files are written atomically, so a migration that stopped
    // after the portable write left the same data legacy holds. A legacy file
    // that outlived a failed retire is older than the split pair, and
    // migrating it whole would roll the surviving half back.
    let portable = read_config_file(
        app_handle,
        &portable_path,
        "Portable config unreadable, refusing to overwrite it to avoid wiping accounts",
    );
    let local = read_config_file(
        app_handle,
        &local_path,
        "Local config unreadable, refusing to overwrite it to avoid wiping secrets",
    );
    let migrated = merge_split_configs(
        portable.unwrap_or_else(|| legacy.clone()),
        local.unwrap_or(legacy),
    );
    if let Err(e) = save_config(app_handle, &migrated) {
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

    // Absence is a first run. A present file that cannot be read or parsed is
    // the only copy of every account and secret: the in-memory value stays
    // the defaults the caller needs, but saves stay refused until a later
    // read succeeds. Otherwise a window-geometry save writes empty split
    // files and the next boot retires this one. Only a read that reports
    // NotFound counts as absent; a failed metadata probe does not.
    match fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str::<RawAppConfig>(&data) {
            Ok(raw) => {
                set_config_unreadable(&path, false);
                normalize_config(raw)
            }
            Err(e) => {
                set_config_unreadable(&path, true);
                let _ = crate::logging::append_app_log(
                    app_handle,
                    "error",
                    "config.load_legacy",
                    "Legacy config corrupted; refusing saves so it is not replaced by defaults",
                    Some(&e.to_string()),
                );
                AppConfig::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            set_config_unreadable(&path, false);
            AppConfig::default()
        }
        Err(e) => {
            set_config_unreadable(&path, true);
            let _ = crate::logging::append_app_log(
                app_handle,
                "error",
                "config.load_legacy",
                "Legacy config unreadable; refusing saves so it is not replaced by defaults",
                Some(&e.to_string()),
            );
            AppConfig::default()
        }
    }
}
