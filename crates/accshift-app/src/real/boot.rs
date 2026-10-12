//! What `src-tauri/src/lib.rs` (`run`, the `setup` closure) and
//! `src-tauri/src/boot.rs` do for the backend, without the window: the boot
//! state, the log session and panic hook, the startup maintenance thread and
//! the shared HTTP client. The window steps (`build_main_window`, the close
//! handler, the geometry saver, the boot failsafe that shows the window) and
//! the deep links belong to the egui shell.

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

/// `boot::build_http_client`, boot.rs lines 117-133: 10 s per request, 5 s to
/// connect. The Tauri app exits when it cannot build one; here the caller
/// gets the error.
pub(super) fn build_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Fatal: failed to create HTTP client: {e}"))
}

impl RealBackend {
    /// The steps of the Tauri `setup` closure that only need the app
    /// directories (lib.rs lines 121-153): rotate the log chain, write
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

    /// `boot::spawn_snapshot_upgrade` (boot.rs lines 747-858) and the Riot
    /// rollback sweep (lines 860-885): arms the keyring index, then on a
    /// background thread re-encrypts legacy plaintext snapshots, purges the
    /// plaintext backups, sweeps Riot rollback copies and frees orphaned
    /// keyring entries, each step under the cross-process write lock. Call
    /// once after [`Self::begin_session`]. Touches the real keyring, so
    /// tests never call it.
    pub fn spawn_maintenance(&self) -> std::thread::JoinHandle<()> {
        spawn_snapshot_upgrade(self.ctx.clone())
    }

    /// The telemetry half of `finish_boot` (app.rs lines 153-167:
    /// `emit_first_run_once`, `AppLaunched`, `emit_accounts_snapshots`). It
    /// needs the telemetry runtime (`telemetry_runtime.rs`), which part 2 of
    /// the port brings; until then a first boot sends nothing, which is what
    /// the Tauri app does without consent.
    pub(super) fn after_first_boot(&self) {}
}

fn spawn_snapshot_upgrade(upgrade_ctx: AppCtx) -> std::thread::JoinHandle<()> {
    accshift_core::secrets::init(&upgrade_ctx);
    std::thread::spawn(move || {
        let locked = |step: &str| match accshift_core::lock::acquire_for_write(
            &upgrade_ctx,
            Duration::from_secs(2),
        ) {
            Ok(guard) => Some(guard),
            Err(error) => {
                let _ = logging::append_app_log(
                    &upgrade_ctx,
                    "warn",
                    "backend.snapshot-upgrade",
                    &format!("Skipped {step} because the store could not be locked"),
                    Some(&error.to_string()),
                );
                None
            }
        };

        let mut failures: Vec<String> = Vec::new();
        let mut stats = accshift_core::snapshot_crypto::LegacyUpgradeStats::default();
        for platform_id in accshift_core::snapshot_crypto::SNAPSHOT_PLATFORM_IDS {
            let Some(_operation) = locked("snapshot upgrade") else {
                return;
            };
            stats.merge(
                accshift_core::snapshot_crypto::upgrade_legacy_plaintext_platform(
                    &upgrade_ctx,
                    platform_id,
                    &mut |message, detail| failures.push(format!("{message} ({detail})")),
                ),
            );
        }
        if stats.touched_anything() {
            let level = if stats.failed > 0 { "warn" } else { "info" };
            let _ = logging::append_app_log(
                &upgrade_ctx,
                level,
                "backend.snapshot-upgrade",
                &format!(
                    "Re-encrypted {} legacy plaintext snapshot file(s), {} failed",
                    stats.upgraded, stats.failed
                ),
                (!failures.is_empty())
                    .then(|| failures.join("; "))
                    .as_deref(),
            );
        }

        if let Some(_operation) = locked("snapshot backup purge") {
            let mut purge_failures: Vec<String> = Vec::new();
            let purged = accshift_core::storage::purge_migrated_snapshot_backups(
                &upgrade_ctx,
                &mut |message, detail| purge_failures.push(format!("{message} ({detail})")),
            );
            if purged > 0 || !purge_failures.is_empty() {
                let _ = logging::append_app_log(
                    &upgrade_ctx,
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

        if let Some(_operation) = locked("rollback sweep") {
            sweep_riot_rollback_copies(&upgrade_ctx);
        }

        let Some(_operation) = locked("keyring sweep") else {
            return;
        };
        let mut sweep_failures: Vec<String> = Vec::new();
        let swept = accshift_core::secrets::gc(&upgrade_ctx, &mut |message, detail| {
            sweep_failures.push(format!("{message} ({detail})"))
        });
        if swept.touched_anything() {
            let level = if swept.failed > 0 { "warn" } else { "info" };
            let _ = logging::append_app_log(
                &upgrade_ctx,
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
    })
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
