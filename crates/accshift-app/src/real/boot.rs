//! The backend's part of boot: the boot state, the log session and panic
//! hook, the startup maintenance thread and the shared HTTP client. The
//! window steps (the window, the close handler, the geometry saver, the boot
//! failsafe that shows the window) and the deep links belong to the egui
//! shell.

use super::RealBackend;
use accshift_core::{logging, AppCtx};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `app_runtime::BootState`: whether the UI reported its first frame done.
#[derive(Debug, Default)]
pub struct BootState {
    completed: AtomicBool,
}

impl BootState {
    /// True the first time only.
    pub fn mark_completed(&self) -> bool {
        !self.completed.swap(true, Ordering::SeqCst)
    }

    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }
}

/// The shared HTTP client: 10 s per request, 5 s to connect. The caller
/// gets the error when it cannot be built.
pub(super) fn build_http_client() -> Result<reqwest::Client, String> {
    accshift_core::http::client_builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Fatal: failed to create HTTP client: {e}"))
}

impl RealBackend {
    /// The boot steps that only need the app directories: rotate the log
    /// chain, write
    /// "App setup started", install the panic hook. Call once per process,
    /// before the first frame; tests never call it (it replaces the
    /// process-wide panic hook).
    pub fn begin_session(&self) {
        let _ = logging::begin_log_session(&self.ctx);
        let _ = logging::append_app_log(
            &self.ctx,
            "info",
            "backend.startup",
            "App setup started",
            None,
        );
        logging::install_panic_hook(self.ctx.clone());
    }

    /// The startup maintenance: arms the keyring index, then on a
    /// background thread re-encrypts legacy plaintext snapshots, purges the
    /// plaintext backups, sweeps Riot rollback copies, deletes the WebView2
    /// caches older installs left and frees orphaned keyring entries, each locked
    /// step under the cross-process write lock. Call
    /// once after [`Self::begin_session`]. Touches the real keyring, so
    /// tests never call it.
    pub fn spawn_maintenance(&self) -> std::thread::JoinHandle<()> {
        spawn_snapshot_upgrade(self.ctx.clone())
    }

    /// The telemetry of the first boot (first run, app launched, account
    /// snapshots), in `real/telemetry.rs`.
    pub(super) fn after_first_boot(&self) {
        self.record_first_boot();
    }
}

fn spawn_snapshot_upgrade(upgrade_ctx: AppCtx) -> std::thread::JoinHandle<()> {
    accshift_core::secrets::init(&upgrade_ctx);
    std::thread::spawn(move || {
        let mut maintenance = SystemMaintenance {
            ctx: upgrade_ctx,
            failures: Vec::new(),
            stats: Default::default(),
        };
        run_maintenance(
            &mut maintenance,
            accshift_core::snapshot_crypto::SNAPSHOT_PLATFORM_IDS,
        );
    })
}

/// The steps of the startup maintenance.
trait Maintenance {
    type Lock;
    /// The cross-process write lock, or `None` (logged) on a timeout.
    fn lock(&mut self, step: &str) -> Option<Self::Lock>;
    fn upgrade(&mut self, platform_id: &str);
    fn report_upgrade(&mut self);
    fn purge_backups(&mut self);
    fn sweep_rollbacks(&mut self);
    fn prune_caches(&mut self);
    fn keyring_gc(&mut self);
}

/// A lock that times out skips its own step only: the steps are
/// independent and each one takes the lock again.
fn run_maintenance<M: Maintenance>(maintenance: &mut M, platform_ids: &[&str]) {
    for platform_id in platform_ids {
        let Some(_operation) = maintenance.lock("snapshot upgrade") else {
            continue;
        };
        maintenance.upgrade(platform_id);
    }
    maintenance.report_upgrade();
    if let Some(_operation) = maintenance.lock("snapshot backup purge") {
        maintenance.purge_backups();
    }
    if let Some(_operation) = maintenance.lock("rollback sweep") {
        maintenance.sweep_rollbacks();
    }
    maintenance.prune_caches();
    if let Some(_operation) = maintenance.lock("keyring sweep") {
        maintenance.keyring_gc();
    }
}

struct SystemMaintenance {
    ctx: AppCtx,
    failures: Vec<String>,
    stats: accshift_core::snapshot_crypto::LegacyUpgradeStats,
}

impl Maintenance for SystemMaintenance {
    type Lock = accshift_core::lock::WriteGuard;

    fn lock(&mut self, step: &str) -> Option<Self::Lock> {
        match accshift_core::lock::acquire_for_write(&self.ctx, Duration::from_secs(2)) {
            Ok(guard) => Some(guard),
            Err(error) => {
                let _ = logging::append_app_log(
                    &self.ctx,
                    "warn",
                    "backend.snapshot-upgrade",
                    &format!("Skipped {step} because the store could not be locked"),
                    Some(&error.to_string()),
                );
                None
            }
        }
    }

    fn upgrade(&mut self, platform_id: &str) {
        let failures = &mut self.failures;
        self.stats.merge(
            accshift_core::snapshot_crypto::upgrade_legacy_plaintext_platform(
                &self.ctx,
                platform_id,
                &mut |message, detail| failures.push(format!("{message} ({detail})")),
            ),
        );
    }

    fn report_upgrade(&mut self) {
        if !self.stats.touched_anything() {
            return;
        }
        let level = if self.stats.failed > 0 {
            "warn"
        } else {
            "info"
        };
        let _ = logging::append_app_log(
            &self.ctx,
            level,
            "backend.snapshot-upgrade",
            &format!(
                "Re-encrypted {} legacy plaintext snapshot file(s), {} failed",
                self.stats.upgraded, self.stats.failed
            ),
            (!self.failures.is_empty())
                .then(|| self.failures.join("; "))
                .as_deref(),
        );
    }

    fn purge_backups(&mut self) {
        let mut purge_failures: Vec<String> = Vec::new();
        let purged = accshift_core::storage::purge_migrated_snapshot_backups(
            &self.ctx,
            &mut |message, detail| purge_failures.push(format!("{message} ({detail})")),
        );
        if purged > 0 || !purge_failures.is_empty() {
            let _ = logging::append_app_log(
                &self.ctx,
                if purge_failures.is_empty() {
                    "info"
                } else {
                    "warn"
                },
                "backend.snapshot-upgrade",
                &format!("Deleted {purged} plaintext pre-migration snapshot backup(s)"),
                (!purge_failures.is_empty())
                    .then(|| purge_failures.join("; "))
                    .as_deref(),
            );
        }
    }

    fn sweep_rollbacks(&mut self) {
        sweep_riot_rollback_copies(&self.ctx);
    }

    fn prune_caches(&mut self) {
        prune_webview_caches(&self.ctx);
    }

    fn keyring_gc(&mut self) {
        let mut sweep_failures: Vec<String> = Vec::new();
        let swept = accshift_core::secrets::gc(&self.ctx, &mut |message, detail| {
            sweep_failures.push(format!("{message} ({detail})"))
        });
        if swept.touched_anything() {
            let level = if swept.failed > 0 { "warn" } else { "info" };
            let _ = logging::append_app_log(
                &self.ctx,
                level,
                "backend.secrets-gc",
                &format!(
                    "Freed {} orphaned keyring entry(ies), {} failed",
                    swept.freed, swept.failed
                ),
                (!sweep_failures.is_empty())
                    .then(|| sweep_failures.join("; "))
                    .as_deref(),
            );
        }
    }
}

#[cfg(windows)]
fn sweep_riot_rollback_copies(upgrade_ctx: &AppCtx) {
    let mut failures: Vec<String> = Vec::new();
    let swept = accshift_core::platforms::riot::sweep_rollback_dirs(upgrade_ctx, &mut |m, d| {
        failures.push(format!("{m} ({d})"))
    });
    if !swept.touched_anything() {
        return;
    }
    let level = if swept.failed > 0 { "warn" } else { "info" };
    let _ = logging::append_app_log(
        upgrade_ctx,
        level,
        "backend.riot-rollback-sweep",
        &format!(
            "Removed {} leftover Riot rollback copy(ies), {} failed",
            swept.removed, swept.failed
        ),
        (!failures.is_empty())
            .then(|| failures.join("; "))
            .as_deref(),
    );
}

#[cfg(not(windows))]
fn sweep_riot_rollback_copies(_upgrade_ctx: &AppCtx) {}

/// The WebView2 caches older installs left, dead weight now
/// (`legacy_web_storage::prune_webview2_caches`).
#[cfg(windows)]
fn prune_webview_caches(upgrade_ctx: &AppCtx) {
    use accshift_core::AppContext;
    let Ok(local) = upgrade_ctx.app_local_data_dir() else {
        return;
    };
    let (freed, failures) = crate::legacy_web_storage::prune_webview2_caches(&local);
    if freed == 0 && failures.is_empty() {
        return;
    }
    let _ = logging::append_app_log(
        upgrade_ctx,
        if failures.is_empty() { "info" } else { "warn" },
        "backend.webview-cache-prune",
        &format!(
            "Deleted {:.1} MB of WebView2 cache left by the Tauri app",
            freed as f64 / 1e6
        ),
        (!failures.is_empty())
            .then(|| failures.join("; "))
            .as_deref(),
    );
}

#[cfg(not(windows))]
fn prune_webview_caches(_upgrade_ctx: &AppCtx) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the steps; the locks whose index is in `busy` time out.
    struct Recorder {
        busy: Vec<usize>,
        locks: usize,
        steps: Vec<String>,
    }

    impl Maintenance for Recorder {
        type Lock = ();

        fn lock(&mut self, step: &str) -> Option<()> {
            let index = self.locks;
            self.locks += 1;
            if self.busy.contains(&index) {
                self.steps.push(format!("skipped {step}"));
                return None;
            }
            Some(())
        }

        fn upgrade(&mut self, platform_id: &str) {
            self.steps.push(format!("upgrade {platform_id}"));
        }

        fn report_upgrade(&mut self) {
            self.steps.push("report".into());
        }

        fn purge_backups(&mut self) {
            self.steps.push("purge".into());
        }

        fn sweep_rollbacks(&mut self) {
            self.steps.push("rollbacks".into());
        }

        fn prune_caches(&mut self) {
            self.steps.push("caches".into());
        }

        fn keyring_gc(&mut self) {
            self.steps.push("keyring".into());
        }
    }

    #[test]
    fn a_busy_lock_skips_only_its_own_maintenance_step() {
        let mut recorder = Recorder {
            busy: vec![0, 3],
            locks: 0,
            steps: Vec::new(),
        };
        run_maintenance(&mut recorder, &["riot", "epic"]);
        assert_eq!(
            recorder.steps,
            [
                "skipped snapshot upgrade",
                "upgrade epic",
                "report",
                "purge",
                "skipped rollback sweep",
                "caches",
                "keyring",
            ]
        );
    }
}
