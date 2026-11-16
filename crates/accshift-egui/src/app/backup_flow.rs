//! The config backup: export, import with its preview and confirm, and the
//! restore, owned by the app rather than the settings panel so closing the
//! panel never drops a backup under way.
//!
//! An export and a restore both run behind a boundary. Once the export path
//! is picked or the restore confirmed, the panel's draft goes to the stores
//! and the boundary waits: for the store writes already running, for the
//! debounced ones it hands out itself, for an external refresh and for the
//! descriptor and theme writes counted by [`WriteGate`], wherever they were
//! started. Meanwhile no new store write, descriptor or theme write and no
//! external refresh starts. Any of these writes that failed there cancels
//! the backup. A store write's edit is then only in memory. A descriptor
//! install goes to a temp file renamed over the descriptor and is never
//! copied over it, so a failed one leaves the descriptor whole. A theme save
//! copies over its file when the rename keeps failing (an antivirus or the
//! indexer holding it), and a copy that fails midway can leave it partial.
//! The job itself records the failure, so it cancels the backup even once
//! its panel is gone, and the boundary clears the failures of the one
//! before it under the lock the jobs record with, before it is published.
//!
//! The export then reads the files, all written. The restore replaces them,
//! reloads the user descriptors and reads the stores and themes back. The
//! app registers the restored descriptors first, so the settings that name a
//! restored platform are sanitized against them, then adopts the whole
//! snapshot, dropping the writes queued meanwhile, and only then lets writes
//! and refreshes run again. When the stores or the custom themes cannot be
//! read back, the boundary stays up and retries: the old stores in memory
//! are never written over the restored files, not even on close, and the
//! restored settings are never adopted without the custom theme they name.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use accshift_app::actions::dialogs::ConfirmTicket;
use accshift_app::backend::Backend;
use accshift_app::error::{BackendError, BackendResult};
use accshift_app::i18n::{translate, Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::text::Text;
use accshift_app::types::{BackupPreview, BackupReport, ClientStorageSnapshot, UserPlatformReport};
use accshift_theme::CustomThemePayload;
use egui::Context;

use super::settings_panel::{apply_report, restore_confirm, BackupState};
use super::AppModel;

/// How long a restore whose stores could not be read back waits before it
/// reads them again.
const READ_BACK_RETRY: Duration = Duration::from_millis(if cfg!(test) { 50 } else { 2000 });

/// The descriptor and custom theme writes, which bypass the client-store
/// queue. Each admitted job counts until its call returns, even when the
/// panel that started it is gone; none is admitted while a backup holds the
/// boundary.
#[derive(Clone, Default)]
pub(super) struct WriteGate(Arc<GateState>);

#[derive(Default)]
struct GateState {
    running: AtomicUsize,
    closed: AtomicBool,
    /// The first write that failed while a backup held the boundary.
    failed: Mutex<Option<String>>,
    /// Test seam: run once the boundary is published.
    #[cfg(test)]
    published: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// One admitted call, uncounted when dropped.
struct Admitted(Arc<GateState>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::SeqCst);
    }
}

impl WriteGate {
    /// `call`, counted until it returns; `None` while a backup holds writes.
    pub(super) fn admit<T, F>(
        &self,
        call: F,
    ) -> Option<impl FnOnce(&dyn Backend) -> T + Send + 'static>
    where
        T: Send + 'static,
        F: FnOnce(&dyn Backend) -> T + Send + 'static,
    {
        if self.0.closed.load(Ordering::SeqCst) {
            return None;
        }
        self.0.running.fetch_add(1, Ordering::SeqCst);
        let admitted = Admitted(Arc::clone(&self.0));
        Some(move |b: &dyn Backend| {
            let _admitted = admitted;
            call(b)
        })
    }

    /// [`Self::admit`] for a call that writes a file: when it fails while a
    /// backup holds the boundary, the backup does not run, since the file
    /// may be left changed.
    pub(super) fn admit_write<R, E, F>(
        &self,
        call: F,
    ) -> Option<impl FnOnce(&dyn Backend) -> Result<R, E> + Send + 'static>
    where
        R: Send + 'static,
        E: std::fmt::Display + Send + 'static,
        F: FnOnce(&dyn Backend) -> Result<R, E> + Send + 'static,
    {
        let state = Arc::clone(&self.0);
        self.admit(move |b| {
            let result = call(b);
            if let Err(error) = &result {
                if state.closed.load(Ordering::SeqCst) {
                    let mut failed = state.failed.lock().unwrap_or_else(|p| p.into_inner());
                    failed.get_or_insert_with(|| error.to_string());
                }
            }
            result
        })
    }

    /// An admitted call has not returned yet.
    fn running(&self) -> bool {
        self.0.running.load(Ordering::SeqCst) > 0
    }

    /// The write that failed since the boundary went up, taken.
    fn take_failure(&self) -> Option<String> {
        self.0
            .failed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }

    fn set_closed(&self, closed: bool) {
        if !closed || self.0.closed.load(Ordering::SeqCst) {
            self.0.closed.store(closed, Ordering::SeqCst);
            return;
        }
        // The failures of an earlier boundary go before this one is
        // published, under the lock a failing write records with: a write
        // that sees the boundary records after this, and is never erased.
        let mut failed = self.0.failed.lock().unwrap_or_else(|p| p.into_inner());
        *failed = None;
        self.0.closed.store(true, Ordering::SeqCst);
        drop(failed);
        #[cfg(test)]
        self.run_published_hook();
    }

    /// Test seam: runs `hook` on the UI thread the moment the next boundary
    /// is published to the write jobs.
    #[cfg(test)]
    pub(super) fn on_published(&self, hook: impl FnOnce() + Send + 'static) {
        *self.0.published.lock().unwrap() = Some(Box::new(hook));
    }

    #[cfg(test)]
    fn run_published_hook(&self) {
        let hook = self.0.published.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }
}

/// What an export or an import job answered.
pub(super) enum BackupDone {
    /// The export path the save dialog returned.
    ExportTo(String),
    /// The picked file and what it holds; `None` when a picker was
    /// cancelled.
    Picked(Option<(String, BackupPreview)>),
}

/// Which step a failure belongs to, for its notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackupStep {
    Export,
    Read,
}

/// What the restore job read back once the files were replaced.
pub(super) struct Restored {
    report: BackupReport,
    /// The user descriptors after the reload; `None` when it failed.
    platforms: Option<UserPlatformReport>,
    read_back: BackendResult<ReadBack>,
}

/// The stores and the custom themes as now on disk, read together: the
/// restored settings may name a restored theme.
pub(super) struct ReadBack {
    snapshot: ClientStorageSnapshot,
    themes: Vec<CustomThemePayload>,
}

/// What a boundary is held for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Export,
    Restore,
}

/// Where a backup behind the boundary is.
#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    /// Waiting for the writes and reads already running.
    Draining {
        path: String,
        purpose: Purpose,
        /// A write that failed while draining: the backup does not run.
        failed: Option<String>,
    },
    Exporting(Pending<BackendResult<BackupReport>>),
    Restoring(Pending<BackendResult<Restored>>),
    /// The files were restored but the stores could not be read back: the
    /// memory still holds the old ones, which must never be written.
    Unadopted {
        /// The notice once the stores are read back.
        done: String,
        job: Option<Pending<BackendResult<ReadBack>>>,
        retry_at: Instant,
    },
}

/// The backup card's jobs and the boundary's stage.
#[derive(Default)]
pub(super) struct BackupFlow {
    pub(super) card: BackupState,
    pub(super) gate: WriteGate,
    job: Option<(Pending<BackendResult<BackupDone>>, BackupStep)>,
    /// The previewed file the confirm is about, and its ticket.
    pending_restore: Option<(String, ConfirmTicket)>,
    stage: Stage,
}

impl BackupFlow {
    /// A backup holds the boundary: store, descriptor and theme writes and
    /// external refreshes wait.
    pub(super) fn holds_writes(&self) -> bool {
        !matches!(self.stage, Stage::Idle)
    }

    /// A store write ended with `error` while the boundary drains.
    pub(super) fn save_failed(&mut self, error: &BackendError) {
        if let Stage::Draining { failed, .. } = &mut self.stage {
            failed.get_or_insert_with(|| error.message.clone());
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.pending_restore.is_some() || self.holds_writes()
    }

    fn set_stage(&mut self, stage: Stage) {
        self.stage = stage;
        self.gate.set_closed(self.holds_writes());
    }

    fn start(
        &mut self,
        jobs: &Jobs,
        step: BackupStep,
        call: impl FnOnce(&dyn Backend) -> BackendResult<BackupDone> + Send + 'static,
    ) {
        if self.busy() {
            return;
        }
        self.set_notice(String::new(), false);
        self.job = Some((jobs.run(call), step));
    }

    fn set_notice(&mut self, text: String, error: bool) {
        self.card.notice = text;
        self.card.error = error;
    }

    /// "Export setup": the save dialog; the export itself runs behind the
    /// boundary.
    pub(super) fn export(&mut self, jobs: &Jobs, title: String, all_files: String) {
        self.start(jobs, BackupStep::Export, move |b| {
            match b.backup_select_export_path(&title, &all_files) {
                Ok(path) if !path.is_empty() => Ok(BackupDone::ExportTo(path)),
                _ => Ok(BackupDone::Picked(None)),
            }
        });
    }

    /// "Restore from a file": the file picker, then the preview.
    pub(super) fn import(&mut self, jobs: &Jobs, title: String, all_files: String) {
        self.start(jobs, BackupStep::Read, move |b| {
            let Ok(path) = b.backup_select_import_file(&title, &all_files) else {
                return Ok(BackupDone::Picked(None));
            };
            if path.is_empty() {
                return Ok(BackupDone::Picked(None));
            }
            let preview = b.backup_preview(&path)?;
            Ok(BackupDone::Picked(Some((path, preview))))
        });
    }
}

/// `{key}` with `{name}` set to `value`.
fn with_one(locale: Locale, key: &str, name: &str, value: String) -> String {
    Text::with(key, [(name, TranslationValue::Str(value))]).resolve(locale)
}

/// The restore job: the files, then the descriptors, the stores and the
/// themes read back.
fn restore_job(b: &dyn Backend, path: &str) -> BackendResult<Restored> {
    let report = b.backup_restore(path)?;
    let platforms = b.reload_user_platforms().ok();
    Ok(Restored {
        report,
        platforms,
        read_back: read_back(b),
    })
}

/// The stores and the custom themes as now on disk; an error when either
/// cannot be read.
fn read_back(b: &dyn Backend) -> BackendResult<ReadBack> {
    let snapshot = b.load_client_storage_snapshot()?;
    let themes = b
        .list_custom_themes()?
        .iter()
        .filter_map(|t| serde_json::to_value(t).ok())
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    Ok(ReadBack { snapshot, themes })
}

/// A backup dialog's title and its "all files" filter, in the app's
/// language.
pub(super) fn dialog_labels(locale: Locale, title_key: &str) -> (String, String) {
    (
        translate(locale, title_key),
        translate(locale, "settings.backupAllFiles"),
    )
}

impl AppModel {
    fn backup_locale(&self) -> Locale {
        self.booted.as_ref().map_or(Locale::En, |b| b.locale)
    }

    /// The card's "Export setup".
    pub(super) fn export_backup(&mut self) {
        let (title, all_files) = dialog_labels(self.backup_locale(), "settings.backupExportDialog");
        self.backup.export(&self.jobs, title, all_files);
    }

    /// The card's "Restore from a file".
    pub(super) fn import_backup(&mut self) {
        let (title, all_files) = dialog_labels(self.backup_locale(), "settings.backupImportDialog");
        self.backup.import(&self.jobs, title, all_files);
    }

    /// Once per frame, panel open or not: the export and import jobs, the
    /// boundary's stages, then the card the panel draws.
    pub(super) fn tick_backup(&mut self, ctx: &Context) {
        self.poll_backup_job(ctx);
        self.drain_boundary();
        self.poll_export();
        self.poll_restore(ctx);
        self.poll_read_back(ctx);
        let busy = self.backup.busy();
        self.backup.card.busy = busy;
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.backup = self.backup.card.clone();
        }
    }

    fn poll_backup_job(&mut self, ctx: &Context) {
        let Some((job, step)) = &mut self.backup.job else {
            return;
        };
        let step = *step;
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(BackendError::new("job panicked")),
        };
        self.backup.job = None;
        let locale = self.backup_locale();
        match result {
            Err(error) => {
                let key = match step {
                    BackupStep::Export => "settings.backupExportFailed",
                    BackupStep::Read => "settings.backupReadFailed",
                };
                let text = with_one(locale, key, "error", error.message);
                self.backup.set_notice(text, true);
            }
            Ok(BackupDone::Picked(None)) => {}
            Ok(BackupDone::ExportTo(path)) => self.hold_boundary(path, Purpose::Export),
            Ok(BackupDone::Picked(Some((path, preview)))) => {
                let mut effects = Vec::new();
                let ticket = self
                    .dialogs
                    .request_confirm(restore_confirm(&preview, locale), &mut effects);
                self.backup.pending_restore = Some((path, ticket));
                self.apply_effects(ctx, effects);
            }
        }
    }

    /// The restore confirm settled: an allowed restore sends the panel's
    /// draft to the stores, then waits behind the boundary.
    pub(super) fn settle_backup_restore(&mut self, ticket: ConfirmTicket, allowed: bool) {
        let Some((path, pending)) = self.backup.pending_restore.take() else {
            return;
        };
        if pending != ticket {
            self.backup.pending_restore = Some((path, pending));
            return;
        }
        if allowed {
            self.start_backup_restore(path);
        }
    }

    /// Starts the restore of `path` behind the boundary.
    pub(super) fn start_backup_restore(&mut self, path: String) {
        if self.backup.holds_writes() {
            return;
        }
        self.hold_boundary(path, Purpose::Restore);
    }

    /// Raises the boundary for an export to, or a restore from, `path`.
    fn hold_boundary(&mut self, path: String, purpose: Purpose) {
        if self.booted.is_none() {
            return;
        }
        // An install under way ends in a relaunch that would cut the
        // backup between two files: it waits for the install to fail.
        if self.updates.installing() {
            let text = translate(self.backup_locale(), "settings.backupUpdateInstalling");
            self.backup.set_notice(text, true);
            return;
        }
        // The draft is part of the setup an export saves; written after a
        // restore, it would put the old setup back.
        self.flush_settings(Instant::now());
        self.backup.set_notice(String::new(), false);
        self.backup.set_stage(Stage::Draining {
            path,
            purpose,
            failed: None,
        });
    }

    /// The boundary: hands out the debounced writes, then waits until no
    /// store, descriptor or theme write and no refresh is running before
    /// the export or restore job starts.
    fn drain_boundary(&mut self) {
        if !matches!(self.backup.stage, Stage::Draining { .. }) {
            return;
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        for save in b.stores.take_all_saves() {
            let store_id = save.store_id;
            let pending = self
                .jobs
                .run(move |backend| backend.save_client_storage_store(store_id, &save.value));
            self.saves.push((store_id, pending));
        }
        if !self.saves.is_empty()
            || b.stores.storage().has_unsaved()
            || self.external_refresh.running()
            || self.backup.gate.running()
        {
            return;
        }
        let Stage::Draining {
            path,
            purpose,
            failed,
        } = std::mem::take(&mut self.backup.stage)
        else {
            return;
        };
        if let Some(error) = failed.or_else(|| self.backup.gate.take_failure()) {
            self.backup.set_stage(Stage::Idle);
            let key = match purpose {
                Purpose::Export => "settings.backupExportFailed",
                Purpose::Restore => "settings.backupRestoreFailed",
            };
            let text = with_one(self.backup_locale(), key, "error", error);
            self.backup.set_notice(text, true);
            return;
        }
        let stage = match purpose {
            Purpose::Export => Stage::Exporting(self.jobs.run(move |b| b.backup_export(&path))),
            Purpose::Restore => Stage::Restoring(self.jobs.run(move |b| restore_job(b, &path))),
        };
        self.backup.set_stage(stage);
    }

    fn poll_export(&mut self) {
        let Stage::Exporting(job) = &mut self.backup.stage else {
            return;
        };
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(BackendError::new("job panicked")),
        };
        self.backup.set_stage(Stage::Idle);
        let locale = self.backup_locale();
        let (text, error) = match result {
            Ok(report) => (
                with_one(
                    locale,
                    "settings.backupExported",
                    "path",
                    report.preview.path,
                ),
                false,
            ),
            Err(error) => (
                with_one(
                    locale,
                    "settings.backupExportFailed",
                    "error",
                    error.message,
                ),
                true,
            ),
        };
        self.backup.set_notice(text, error);
    }

    fn poll_restore(&mut self, ctx: &Context) {
        let Stage::Restoring(job) = &mut self.backup.stage else {
            return;
        };
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(BackendError::new("job panicked")),
        };
        self.backup.set_stage(Stage::Idle);
        self.finish_restore(ctx, result);
    }

    /// The restored stores read again after a failed read back: adopted
    /// once read, retried until then.
    fn poll_read_back(&mut self, ctx: &Context) {
        let Stage::Unadopted { job, retry_at, .. } = &mut self.backup.stage else {
            return;
        };
        let Some(pending) = job else {
            let now = Instant::now();
            if now < *retry_at {
                ctx.request_repaint_after(retry_at.saturating_duration_since(now));
                return;
            }
            *job = Some(self.jobs.run(read_back));
            return;
        };
        let result = match pending.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(BackendError::new("job panicked")),
        };
        match result {
            Ok(read) => {
                let Stage::Unadopted { done, .. } = std::mem::take(&mut self.backup.stage) else {
                    return;
                };
                self.backup.set_stage(Stage::Idle);
                self.adopt_restored(ctx, read);
                self.backup.set_notice(done, false);
            }
            Err(_) => {
                *job = None;
                *retry_at = Instant::now() + READ_BACK_RETRY;
                ctx.request_repaint_after(READ_BACK_RETRY);
            }
        }
    }

    /// On close: waits for an export or a restore job, so the restored
    /// stores are adopted before the last writes. Stores that cannot be
    /// read back drop the writes queued for the old ones. A backup still
    /// draining does not start. Teardown only: it lifts the boundary, so
    /// the update install, the one flush that may leave the app open, is
    /// refused while a backup holds the writes.
    pub(super) fn settle_backup_on_close(&mut self) {
        let snapshot = match std::mem::take(&mut self.backup.stage) {
            Stage::Idle | Stage::Draining { .. } => None,
            Stage::Exporting(job) => {
                let _ = job.wait();
                None
            }
            Stage::Restoring(job) => match job.wait() {
                Some(Ok(restored)) => Some(restored.read_back.map(|read| read.snapshot)),
                // The files were not replaced: the old stores stand.
                _ => None,
            },
            Stage::Unadopted { job, .. } => Some(
                job.and_then(|job| job.wait())
                    .unwrap_or_else(|| Err(BackendError::new("not read back")))
                    .map(|read| read.snapshot),
            ),
        };
        self.backup.set_stage(Stage::Idle);
        let Some(snapshot) = snapshot else {
            return;
        };
        let snapshot = snapshot.or_else(|_| self.backend.load_client_storage_snapshot());
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        match snapshot {
            Ok(snapshot) => {
                b.stores.adopt_snapshot(snapshot);
            }
            Err(_) => b.stores.discard_pending_saves(),
        }
    }

    /// Takes the restored themes, then the restored stores, which may name
    /// one of them, and refreshes what reads them.
    fn adopt_restored(&mut self, ctx: &Context, read: ReadBack) {
        self.apply_theme_payloads(&read.themes);
        let changed = self
            .booted
            .as_mut()
            .map(|b| b.stores.adopt_snapshot(read.snapshot))
            .unwrap_or_default();
        self.apply_external_refresh(ctx, &changed);
    }

    /// A restore job ended: on success the restored descriptors are
    /// registered, then the themes, then the stores adopted whole.
    fn finish_restore(&mut self, ctx: &Context, result: BackendResult<Restored>) {
        let locale = self.backup_locale();
        let restored = match result {
            Ok(restored) => restored,
            Err(error) => {
                let text = with_one(
                    locale,
                    "settings.backupRestoreFailed",
                    "error",
                    error.message,
                );
                self.backup.set_notice(text, true);
                return;
            }
        };
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let platforms_read = restored.platforms.is_some();
        if let Some(report) = &restored.platforms {
            apply_report(b, report);
            if let Some(panel) = self.settings_panel.as_mut() {
                panel.user_platforms = Some(report.clone());
            }
        }
        let done = if platforms_read {
            with_one(
                locale,
                "settings.backupRestored",
                "path",
                restored.report.pre_restore_dir,
            )
        } else {
            translate(locale, "settings.backupRestartNeeded")
        };
        match restored.read_back {
            Ok(read) => {
                self.adopt_restored(ctx, read);
                self.backup.set_notice(done, false);
            }
            Err(_) => {
                // The boundary stays up until the stores are read back.
                self.backup
                    .set_notice(translate(locale, "settings.backupRestartNeeded"), false);
                self.backup.set_stage(Stage::Unadopted {
                    done,
                    job: None,
                    retry_at: Instant::now() + READ_BACK_RETRY,
                });
                ctx.request_repaint_after(READ_BACK_RETRY);
            }
        }
    }
}
