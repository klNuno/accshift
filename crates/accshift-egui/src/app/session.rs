//! The launch's session thread: the boot read while the window builds.

use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use accshift_app::legacy_web_storage::LegacyRead;
use accshift_app::real::RealBackend;
use accshift_core::config::{self, SavedWindow};

use super::boot::{early_boot, EarlyBoot};

/// What [`start`] hands the window and the model.
pub struct Session {
    /// The saved window geometry.
    pub saved: SavedWindow,
    /// The boot result, for [`super::AppModel::real`].
    pub early: EarlyBoot,
    /// The legacy web storage read, sent once the config is migrated.
    pub legacy: mpsc::Receiver<Option<LegacyRead>>,
    /// The thread, joined before the process ends: the boot may be
    /// migrating the config.
    pub thread: JoinHandle<()>,
}

/// Reads the saved window geometry, then starts the session thread:
/// `before_boot` (the legacy web storage read, the log session), the
/// migration of a pre-1.0 config, then the boot payload.
///
/// Nothing may read the config while it migrates into two files: a read
/// between the writes, or between the split files' absence and the legacy
/// file's, gets the defaults. The geometry is therefore read before the
/// thread starts, and [`Session::legacy`] only arrives once the migration
/// is over: what waits for it (the telemetry consent, then the maintenance
/// and its config lock) reads the migrated config.
pub fn start(
    backend: &Arc<RealBackend>,
    before_boot: impl FnOnce(&RealBackend) -> Option<LegacyRead> + Send + 'static,
) -> Session {
    start_with(backend, before_boot, |_| {}, |_| {})
}

/// Like [`start`], splitting the work before the window shows into three
/// phases that preserve the startup ordering while overlapping disk I/O:
/// 1. `read_legacy` (before the panic hook is installed) and
///    `migrate_config_early` run first so nothing reads the config
///    mid-migration.
/// 2. The boot payload runs on a helper thread in parallel with
///    `after_migrate` (log rotation, panic hook, telemetry startup).
/// 3. [`Session::legacy`] is sent once `after_migrate` completes, then
///    `after_ready` runs off the UI thread before joining the boot helper.
pub fn start_with(
    backend: &Arc<RealBackend>,
    read_legacy: impl FnOnce(&RealBackend) -> Option<LegacyRead> + Send + 'static,
    after_migrate: impl FnOnce(&RealBackend) + Send + 'static,
    after_ready: impl FnOnce(&RealBackend) + Send + 'static,
) -> Session {
    let saved = config::load_window(backend.context().as_ref());
    let (early, task) = early_boot();
    let (tx, legacy) = mpsc::channel();
    let backend = Arc::clone(backend);
    let thread = std::thread::spawn(move || {
        let legacy = read_legacy(&backend);
        backend.migrate_config_early();
        let boot_backend = Arc::clone(&backend);
        let boot_thread = std::thread::spawn(move || {
            task.run(boot_backend.as_ref());
        });
        after_migrate(&backend);
        let _ = tx.send(legacy);
        after_ready(&backend);
        let _ = boot_thread.join();
    });
    Session {
        saved,
        early,
        legacy,
        thread,
    }
}
