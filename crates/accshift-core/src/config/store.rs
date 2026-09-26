//! Loading and saving the config: the read cache, the unreadable-file guard and write locks.

#[allow(unused_imports)]
use super::*;

pub(super) type FileSig = Option<(std::time::SystemTime, u64)>;

pub(super) struct CachedConfig {
    pub(super) portable_path: std::path::PathBuf,
    pub(super) local_path: std::path::PathBuf,
    pub(super) portable_sig: FileSig,
    pub(super) local_sig: FileSig,
    pub(super) value: AppConfig,
}

/// Parsed-config cache keyed by file signatures. `load_config` is called on
/// hot paths (every `update_config`, several times per switch); a pair of
/// stats replaces a pair of read+parse when nothing changed on disk. External
/// writers (the CLI) bump the mtime, which invalidates naturally.
pub(super) fn config_cache() -> &'static std::sync::Mutex<Option<CachedConfig>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<CachedConfig>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// Poisoned config files: a path lands here when `load_config` finds it
/// present on disk but unreadable/unparseable (and no valid `.bak` to recover
/// from). The local file holds the only copy of the Steam API key, Roblox
/// cookies and path overrides; the portable file holds every non-Steam account
/// list and label. Treating a transient read failure as "empty defaults" and
/// then saving would silently wipe them. While either file is poisoned, every
/// config save is refused, so a corrupt-but-present file is left untouched
/// until the next successful read clears it.
///
/// Keyed by path rather than a single process-global flag. A running app only
/// ever has one local config, so this changes nothing there; the test binary
/// has one per test, and a global flag let any test's successful read clear the
/// poison another test had just set.
pub(super) fn poisoned_configs(
) -> &'static std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>> {
    static POISONED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
    > = std::sync::OnceLock::new();
    POISONED.get_or_init(Default::default)
}

pub(super) fn set_config_unreadable(path: &std::path::Path, unreadable: bool) {
    let mut poisoned = poisoned_configs().lock().unwrap_or_else(|e| e.into_inner());
    if unreadable {
        poisoned.insert(path.to_path_buf());
    } else {
        poisoned.remove(path);
    }
}

pub(super) fn config_unreadable(path: &std::path::Path) -> bool {
    poisoned_configs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(path)
}

/// Serializes every test that reads or writes config, wherever it lives. The
/// poison flag above and the config cache are process-global, so a config read
/// from another module's test clears the flag mid-assertion here and the
/// poisoning tests fail for reasons that have nothing to do with them.
#[cfg(test)]
pub(crate) fn config_io_test_mutex() -> &'static std::sync::Mutex<()> {
    static MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| std::sync::Mutex::new(()))
}

pub(super) fn file_sig(path: &std::path::Path) -> FileSig {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub fn load_config(app_handle: &dyn AppContext) -> AppConfig {
    let portable_path = match crate::storage::portable_config_path(app_handle) {
        Ok(path) => path,
        Err(_) => return load_legacy_config(app_handle),
    };
    let local_path = match crate::storage::local_config_path(app_handle) {
        Ok(path) => path,
        Err(_) => return load_legacy_config(app_handle),
    };

    let portable_sig = file_sig(&portable_path);
    let local_sig = file_sig(&local_path);
    {
        let cache = config_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = cache.as_ref() {
            if cached.portable_path == portable_path
                && cached.local_path == local_path
                && cached.portable_sig == portable_sig
                && cached.local_sig == local_sig
                && portable_sig.is_some()
            {
                return cached.value.clone();
            }
        }
    }

    // Distinguish "file absent" (Ok(None), defaults are fine) from "file
    // present but unreadable" (Err). The latter poisons config writes so a
    // later save can't clobber the only copy with defaults.
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
    // A merge built from a failed read must not outlive this call: the next
    // load retries the read instead of serving the defaults.
    let cacheable = !config_unreadable(&portable_path) && !config_unreadable(&local_path);

    let merged = match (portable, local) {
        (Some(portable), local) => merge_split_configs(portable, local.unwrap_or_default()),
        (None, Some(local)) => merge_split_configs(AppConfig::default(), local),
        // No split config yet, fall back to legacy (pre-migration). Not
        // cached: the next save creates the split files.
        (None, None) => return load_legacy_config(app_handle),
    };
    if !cacheable {
        return merged;
    }

    let mut cache = config_cache().lock().unwrap_or_else(|e| e.into_inner());
    *cache = Some(CachedConfig {
        portable_path,
        local_path,
        portable_sig,
        local_sig,
        value: merged.clone(),
    });
    merged
}

/// Read one split config file, poisoning it on an error other than absence.
pub(super) fn read_config_file(
    app_handle: &dyn AppContext,
    path: &std::path::Path,
    poisoned_message: &str,
) -> Option<AppConfig> {
    match crate::storage::read_json_if_exists::<AppConfig>(path) {
        Ok(config) => {
            set_config_unreadable(path, false);
            config
        }
        Err(e) => {
            set_config_unreadable(path, true);
            let _ = crate::logging::append_app_log(
                app_handle,
                "error",
                "config.load",
                poisoned_message,
                Some(&e),
            );
            None
        }
    }
}

/// Serializes config read-modify-write cycles within this process. The
/// cross-process side is covered by `lock::acquire_for_write` below.
pub(super) fn config_io_mutex() -> &'static std::sync::Mutex<()> {
    static MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    MUTEX.get_or_init(|| std::sync::Mutex::new(()))
}

pub(super) const CONFIG_WRITE_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Run a config write with the cross-process lock acquired before the
/// process-local mutex. Operation-level callers already hold the file lock and
/// nest through `acquire_for_write`; keeping this order everywhere prevents a
/// direct writer from holding the mutex while waiting on that outer lock.
pub(super) fn with_config_write_locks<T>(
    app_handle: &dyn AppContext,
    write: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _write_lock = crate::lock::acquire_for_write(app_handle, CONFIG_WRITE_LOCK_TIMEOUT)
        .map_err(|e| e.to_string())?;
    let _io = config_io_mutex().lock().unwrap_or_else(|e| e.into_inner());
    write()
}

pub fn save_config(app_handle: &dyn AppContext, config: &AppConfig) -> Result<(), String> {
    with_config_write_locks(app_handle, || save_config_unlocked(app_handle, config))
}

pub(super) fn save_config_unlocked(
    app_handle: &dyn AppContext,
    config: &AppConfig,
) -> Result<(), String> {
    // Drop the parsed-config cache: the next load re-reads from disk.
    *config_cache().lock().unwrap_or_else(|e| e.into_inner()) = None;

    let portable = portable_config(config);
    let local = local_config(config);
    let portable_path = crate::storage::portable_config_path(app_handle)?;
    let local_path = crate::storage::local_config_path(app_handle)?;

    // The last read of one of the files failed on an existing file: writing
    // now would overwrite accounts (portable) or the Steam API key, Roblox
    // cookies and path overrides (local) with the empty defaults that the
    // failed read produced. Both files are checked before either is written,
    // so a refused save never leaves half a config on disk. Refuse until a
    // successful read clears the poison flag.
    for (path, what) in [
        (&portable_path, "accounts"),
        (&local_path, "stored secrets"),
    ] {
        if config_unreadable(path) {
            let message = format!(
                "Refusing to write config: the existing file at {} could not be read on the \
                 last load (it may be corrupt or locked). Writing now would wipe {what}. \
                 Fix or remove the file and restart.",
                path.display()
            );
            let _ = crate::logging::append_app_log(
                app_handle,
                "error",
                "config.save",
                "Refused to overwrite an unreadable config file",
                Some(&message),
            );
            return Err(message);
        }
    }

    // Window moves and polls save often with nothing new for one side or
    // both: skip the unchanged file and its log line.
    let wrote_portable = crate::storage::write_json_if_changed(&portable_path, &portable)?;
    let wrote_local = crate::storage::write_json_if_changed(&local_path, &local)?;
    if !wrote_portable && !wrote_local {
        return Ok(());
    }
    // Paths stay out: they embed the OS account name and are the same on
    // every save.
    let details = serde_json::json!({
        "riotProfiles": config.riot.profiles.len(),
        "battleNetAccounts": config.battle_net.accounts.len(),
        "ubisoftAccounts": config.ubisoft.accounts.len(),
        "robloxAccounts": config.roblox.accounts.len(),
        "epicAccounts": config.epic.accounts.len(),
        "gogAccounts": config.gog.accounts.len(),
        "jagexAccounts": config.jagex.accounts.len(),
        "discordAccounts": config.discord.accounts.len(),
        "hasWindowSize": config.window_width.is_some() && config.window_height.is_some(),
        "hasWindowPosition": config.window_x.is_some() && config.window_y.is_some(),
    })
    .to_string();
    let _ = crate::logging::append_app_log(
        app_handle,
        "info",
        "config.save",
        "Saved split app config",
        Some(&details),
    );

    Ok(())
}

/// Load config, apply a mutation, and save in one step.
/// Avoids the scattered load→mutate→save pattern across platform files.
/// The whole cycle runs under both the process-local mutex and the
/// cross-process write lock, so concurrent updates can't lose writes.
pub fn update_config(
    app_handle: &dyn AppContext,
    mutate: impl FnOnce(&mut AppConfig),
) -> Result<(), String> {
    with_config_write_locks(app_handle, || {
        // Read from disk under the lock: another process may have rewritten a
        // file without changing its size inside the mtime granularity, which
        // the cache cannot see, and saving a stale copy would drop its change.
        *config_cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
        let mut cfg = load_config(app_handle);
        mutate(&mut cfg);
        save_config_unlocked(app_handle, &cfg)
    })
}
