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
    /// The legacy web storage read, sent before the boot runs.
    pub legacy: mpsc::Receiver<Option<LegacyRead>>,
    /// The thread, joined before the process ends: the boot may be
    /// migrating the config.
    pub thread: JoinHandle<()>,
}

/// Reads the saved window geometry, then starts the session thread:
/// `before_boot` (the legacy web storage read, the log session), the boot
/// payload, then `after_boot` (the telemetry queue) before the model sees
/// the payload.
///
/// The boot migrates a pre-1.0 config into two files. Nothing may read the
/// config while it does: a read between the writes, or between the split
/// files' absence and the legacy file's, gets the defaults. The geometry is
/// therefore read before the thread starts, and the telemetry consent after
/// the payload, still before the model reports the first boot.
pub fn start(
    backend: &Arc<RealBackend>,
    before_boot: impl FnOnce(&RealBackend) -> Option<LegacyRead> + Send + 'static,
    after_boot: impl FnOnce(&RealBackend) + Send + 'static,
) -> Session {
    let saved = config::load_window(backend.context().as_ref());
    let (early, task) = early_boot();
    let (tx, legacy) = mpsc::channel();
    let backend = Arc::clone(backend);
    let thread = std::thread::spawn(move || {
        let _ = tx.send(before_boot(&backend));
        task.run_then(backend.as_ref(), || after_boot(&backend));
    });
    Session {
        saved,
        early,
        legacy,
        thread,
    }
}
