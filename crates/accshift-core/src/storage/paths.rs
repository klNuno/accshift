//! Where each kind of data lives: config, data, cache and log roots, and the paths under them.

#[allow(unused_imports)]
use super::*;

pub fn app_config_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(app_handle.app_config_dir()?))
}

pub fn app_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(app_handle.app_data_dir()?))
}

pub fn app_local_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(app_handle.app_local_data_dir()?))
}

pub fn app_cache_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(app_handle.app_cache_dir()?))
}

pub fn app_log_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(app_handle.app_config_dir()?.join("logs")))
}

/// Root of the pre-layout data (the single config file and the old snapshot
/// and theme folders). It is scoped like every other root: the un-scoped tree
/// belongs to the installed release build, and a debug build that migrated
/// from it would move that build's data into `dev/`.
pub(super) fn legacy_app_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    app_data_root(app_handle)
}

/// A debug build keeps all its data under `dev/`, apart from an installed
/// release build's, and never reads or writes outside it.
pub(super) fn scope_root(path: PathBuf) -> PathBuf {
    if cfg!(debug_assertions) {
        path.join(DEV_SCOPE_DIR)
    } else {
        path
    }
}

pub fn portable_config_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(app_data_root(app_handle)?
        .join("state")
        .join("portable-config.json"))
}

pub fn local_config_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(app_local_data_root(app_handle)?
        .join("state")
        .join("local-config.json"))
}

/// Where the list of OS keyring entry ids the app created lives. It sits in
/// the state directory next to the local config, because like it, it describes
/// this machine and never moves with the user's data.
pub fn secrets_index_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(app_local_data_root(app_handle)?
        .join("state")
        .join("secret-entries.txt"))
}

/// Where a Riot restore stages the encrypted copy of the live session it may
/// have to put back. It sits in the state directory next to the local config,
/// because like it, it describes this machine and never moves with the user's
/// data. Each restore creates one subdirectory in it and removes it again on
/// every exit path; a leftover means the process died mid-restore and is swept
/// on the next launch.
pub fn riot_rollback_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(app_local_data_root(app_handle)?
        .join("state")
        .join("riot-rollback"))
}

pub fn legacy_config_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(legacy_app_data_root(app_handle)?.join("config.json"))
}

pub fn roblox_accounts_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(app_local_data_root(app_handle)?
        .join("platforms")
        .join("roblox")
        .join("accounts.json"))
}

pub fn themes_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_config_root(app_handle)?.join("themes");
    let old_legacy = legacy_app_data_root(app_handle)?.join("themes");
    backup_and_migrate_dir(app_handle, &old_legacy, &target)?;
    Ok(target)
}

/// Pre-layout-migration snapshot location under the legacy root, per platform.
/// Platforms added after the layout migration (jagex) have none.
pub(super) fn old_legacy_snapshots_name(platform_id: &str) -> Option<&'static str> {
    match platform_id {
        "riot" => Some("riot-profiles"),
        "ubisoft" => Some("ubisoft_cache"),
        "epic" => Some("epic_cache"),
        "gog" => Some("gog_cache"),
        "discord" => Some("discord_cache"),
        _ => None,
    }
}

/// Snapshot directory for a platform (`<local data>/platforms/<id>/snapshots`),
/// migrating any legacy locations to it on first access.
pub fn platform_snapshots_dir(
    app_handle: &dyn AppContext,
    platform_id: &str,
) -> Result<PathBuf, String> {
    let subpath = Path::new("platforms").join(platform_id).join("snapshots");
    let target = app_local_data_root(app_handle)?.join(subpath);
    if let Some(name) = old_legacy_snapshots_name(platform_id) {
        let old_legacy = legacy_app_data_root(app_handle)?.join(name);
        backup_and_migrate_dir(app_handle, &old_legacy, &target)?;
    }
    Ok(target)
}
