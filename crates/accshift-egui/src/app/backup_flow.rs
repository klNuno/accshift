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
//! and refreshes run again. A custom theme file that cannot be read is left
//! out without changing the theme the settings name, which applies once it
//! reads. When the stores cannot be read back, the descriptors not reloaded
//! whole, the restore job lost once it ran (it panicked), or the restore
//! failed and could not put every file back, the boundary stays up for
//! good: a modal asks for the restart that loads the files, no edit is taken
//! meanwhile, and the old stores in memory are never written over the
//! restored files, not even on close. A restore that failed and put every
//! file back lets the app go on as before.

use std::time::Instant;

use accshift_app::actions::dialogs::ConfirmTicket;
use accshift_app::backend::Backend;
use accshift_app::error::{BackendError, BackendResult};
use accshift_app::i18n::{translate, Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::text::Text;
use accshift_app::types::{
    BackupPreview, BackupReport, ClientStorageSnapshot, RestoreError, UserPlatformReport,
};
use egui::Context;

use super::external_refresh::{read_themes, ThemesRead};
use super::settings_panel::{apply_report, restore_confirm, BackupState};
use super::tcno_flow::{import_job, TcnoFlow, TcnoImported};
use super::write_gate::account_writes_running;
pub(super) use super::write_gate::WriteGate;
use super::AppModel;

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
/// restored settings may name a restored theme. Themes not read whole leave
/// their fingerprint unseen, so the next focus reads them again.
pub(super) struct ReadBack {
    snapshot: ClientStorageSnapshot,
    themes: ThemesRead,
}

/// What a boundary is held for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    Export,
    Restore,
    /// The TcNo import (`app::tcno_flow`).
    TcnoImport,
}

/// Where a backup behind the boundary is.
#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    /// Waiting for the writes and reads already running.
    Draining {
        path: String,
        /// The digest of the previewed file a restore applies.
        digest: String,
        purpose: Purpose,
        /// A write that failed while draining: the backup does not run.
        failed: Option<String>,
    },
    Exporting(Pending<BackendResult<BackupReport>>),
    Restoring(Pending<Result<Restored, RestoreError>>),
    Importing(Pending<TcnoImported>),
    /// The files may hold the restored setup while the memory still holds
    /// the old one, which must never be written. Only a restart leaves it.
    RestartNeeded {
        why: RestartWhy,
        /// Why the last restart attempt failed.
        failed: Option<String>,
    },
}

/// What the files hold when a restart is needed, for the modal's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RestartWhy {
    /// The restore went through; this session could not load all of it.
    Restored,
    /// The restore job was lost once it ran: the files may hold the backup,
    /// the old setup or some of each.
    Unknown,
    /// The restore failed and could not put every file back: `error` says
    /// which ones, and where the copy of the old files is.
    Incomplete { error: String },
    /// The TcNo import ran; this session could not read the stores back.
    Imported,
}

/// The backup card's jobs and the boundary's stage.
#[derive(Default)]
pub(super) struct BackupFlow {
    pub(super) card: BackupState,
    pub(super) gate: WriteGate,
    job: Option<(Pending<BackendResult<BackupDone>>, BackupStep)>,
    /// The previewed file the confirm is about, its digest and its ticket.
    pending_restore: Option<(String, String, ConfirmTicket)>,
    stage: Stage,
    /// The TcNo card's picker and confirm; its import holds this boundary.
    pub(super) tcno: TcnoFlow,
}

impl BackupFlow {
    /// The flow whose gate wakes `ctx` when a write it admitted returns.
    pub(super) fn waking(ctx: &Context) -> Self {
        Self {
            gate: WriteGate::waking(ctx),
            ..Self::default()
        }
    }

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

    /// The restored stores could not be read back: the app takes no edit
    /// and shows the restart modal until it restarts.
    pub(super) fn restart_needed(&self) -> bool {
        matches!(self.stage, Stage::RestartNeeded { .. })
    }

    /// Why the last restart attempt failed, for the modal.
    pub(super) fn restart_failure(&self) -> Option<&str> {
        match &self.stage {
            Stage::RestartNeeded { failed, .. } => failed.as_deref(),
            _ => None,
        }
    }

    /// What the restart modal says the files hold, in `locale`; `None`
    /// when no restart is needed.
    pub(super) fn restart_message(&self, locale: Locale) -> Option<String> {
        let Stage::RestartNeeded { why, .. } = &self.stage else {
            return None;
        };
        Some(match why {
            RestartWhy::Restored => translate(locale, "settings.backupRestartNeeded"),
            RestartWhy::Unknown => translate(locale, "settings.backupRestartUnknown"),
            RestartWhy::Incomplete { error } => with_one(
                locale,
                "settings.backupRestartIncomplete",
                "error",
                error.clone(),
            ),
            RestartWhy::Imported => translate(locale, "settings.tcnoRestartNeeded"),
        })
    }

    /// A backup or a TcNo import is being picked, confirmed or run.
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
            || self.pending_restore.is_some()
            || self.tcno.busy()
            || self.holds_writes()
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

    /// Sets the notice of the card that started `purpose`.
    fn notice_for(&mut self, purpose: Purpose, text: String, error: bool) {
        match purpose {
            Purpose::TcnoImport => self.tcno.set_notice(text, error),
            Purpose::Export | Purpose::Restore => self.set_notice(text, error),
        }
    }

    /// "Export setup": the save dialog; the export itself runs behind the
    /// boundary. A cancel says nothing, a picker that failed says why.
    pub(super) fn export(&mut self, jobs: &Jobs, title: String, all_files: String) {
        self.start(jobs, BackupStep::Export, move |b| {
            match b.backup_select_export_path(&title, &all_files) {
                Ok(path) if !path.is_empty() => Ok(BackupDone::ExportTo(path)),
                Ok(_) => Ok(BackupDone::Picked(None)),
                Err(error) => picker_failed(error),
            }
        });
    }

    /// "Restore from a file": the file picker, then the preview.
    pub(super) fn import(&mut self, jobs: &Jobs, title: String, all_files: String) {
        self.start(jobs, BackupStep::Read, move |b| {
            let path = match b.backup_select_import_file(&title, &all_files) {
                Ok(path) => path,
                Err(error) => return picker_failed(error),
            };
            if path.is_empty() {
                return Ok(BackupDone::Picked(None));
            }
            let preview = b.backup_preview(&path)?;
            Ok(BackupDone::Picked(Some((path, preview))))
        });
    }
}

/// What a backup picker's `error` ends in: nothing for a cancel, the
/// backup notice for any other failure (a path the backend cannot use).
fn picker_failed(error: BackendError) -> BackendResult<BackupDone> {
    if picker_cancelled(&error) {
        Ok(BackupDone::Picked(None))
    } else {
        Err(error)
    }
}

/// The picker that answered `error` was cancelled.
pub(super) fn picker_cancelled(error: &BackendError) -> bool {
    error.message == accshift_core::error::AppError::Cancelled.to_string()
}

/// `{key}` with `{name}` set to `value`.
pub(super) fn with_one(locale: Locale, key: &str, name: &str, value: String) -> String {
    Text::with(key, [(name, TranslationValue::Str(value))]).resolve(locale)
}

/// The restore job: the files, if still the ones of `digest`, then the
/// descriptors, the stores and the themes read back. Descriptors not all
/// read count as not reloaded: the settings would be checked against
/// platforms that may lack one.
fn restore_job(b: &dyn Backend, path: &str, digest: &str) -> Result<Restored, RestoreError> {
    let report = b.backup_restore(path, digest)?;
    let platforms = b
        .reload_user_platforms()
        .ok()
        .filter(|report| !report.incomplete);
    Ok(Restored {
        report,
        platforms,
        read_back: read_back(b),
    })
}

/// The stores and the custom themes as now on disk; an error when the
/// stores cannot be read.
fn read_back(b: &dyn Backend) -> BackendResult<ReadBack> {
    let snapshot = b.load_client_storage_snapshot()?;
    let themes = read_themes(b);
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
        self.poll_tcno_import(ctx);
        self.tick_tcno(ctx);
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
            Ok(BackupDone::ExportTo(path)) => {
                self.hold_boundary(path, String::new(), Purpose::Export)
            }
            Ok(BackupDone::Picked(Some((path, preview)))) => {
                let mut effects = Vec::new();
                let ticket = self
                    .dialogs
                    .request_confirm(restore_confirm(&preview, locale), &mut effects);
                self.backup.pending_restore = Some((path, preview.digest, ticket));
                self.apply_effects(ctx, effects);
            }
        }
    }

    /// The restore confirm settled: an allowed restore sends the panel's
    /// draft to the stores, then waits behind the boundary.
    pub(super) fn settle_backup_restore(&mut self, ticket: ConfirmTicket, allowed: bool) {
        let Some((path, digest, pending)) = self.backup.pending_restore.take() else {
            return;
        };
        if pending != ticket {
            self.backup.pending_restore = Some((path, digest, pending));
            return;
        }
        if allowed {
            self.start_backup_restore(path, digest);
        }
    }

    /// Starts the restore of `path`, the file whose preview answered
    /// `digest`, behind the boundary.
    pub(super) fn start_backup_restore(&mut self, path: String, digest: String) {
        if self.backup.holds_writes() {
            return;
        }
        self.hold_boundary(path, digest, Purpose::Restore);
    }

    /// Raises the boundary for an export to, or a restore from, `path`.
    pub(super) fn hold_boundary(&mut self, path: String, digest: String, purpose: Purpose) {
        if self.booted.is_none() {
            return;
        }
        // An install under way ends in a relaunch that would cut the
        // backup between two files: it waits for the install to fail.
        if self.updates.installing() {
            let text = translate(self.backup_locale(), "settings.backupUpdateInstalling");
            self.backup.notice_for(purpose, text, true);
            return;
        }
        // The draft is part of the setup an export saves; written after a
        // restore, it would put the old setup back.
        self.flush_settings(Instant::now());
        self.backup.notice_for(purpose, String::new(), false);
        // The boundary's refusals name it until the next one is raised.
        let tcno = purpose == Purpose::TcnoImport;
        self.backup.gate.set_tcno(tcno);
        self.backup.set_stage(Stage::Draining {
            path,
            digest,
            purpose,
            failed: None,
        });
    }

    /// The boundary: hands out the debounced writes, then waits until no
    /// store, descriptor, theme or account write and no refresh is running
    /// before the export, restore or import job starts.
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
            || account_writes_running(b)
        {
            return;
        }
        let Stage::Draining {
            path,
            digest,
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
                Purpose::TcnoImport => "settings.tcnoImportFailed",
            };
            let text = with_one(self.backup_locale(), key, "error", error);
            self.backup.notice_for(purpose, text, true);
            return;
        }
        let stage = match purpose {
            Purpose::Export => Stage::Exporting(self.jobs.run(move |b| b.backup_export(&path))),
            Purpose::Restore => {
                Stage::Restoring(self.jobs.run(move |b| restore_job(b, &path, &digest)))
            }
            Purpose::TcnoImport => Stage::Importing(self.jobs.run(move |b| import_job(b, &path))),
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

    fn poll_tcno_import(&mut self, ctx: &Context) {
        let Stage::Importing(job) = &mut self.backup.stage else {
            return;
        };
        let imported = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(imported) => imported,
            // It may have died once the stores were written.
            Poll::Gone => {
                let text = translate(self.backup_locale(), "settings.tcnoRestartNeeded");
                self.backup.tcno.set_notice(text, true);
                self.keep_restored_files(RestartWhy::Imported);
                ctx.request_repaint();
                return;
            }
        };
        self.backup.set_stage(Stage::Idle);
        self.finish_tcno_import(ctx, imported);
    }

    fn poll_restore(&mut self, ctx: &Context) {
        let Stage::Restoring(job) = &mut self.backup.stage else {
            return;
        };
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            // It may have died once the files were replaced.
            Poll::Gone => {
                let text = translate(self.backup_locale(), "settings.backupRestartUnknown");
                self.backup.set_notice(text, true);
                self.keep_restored_files(RestartWhy::Unknown);
                ctx.request_repaint();
                return;
            }
        };
        self.backup.set_stage(Stage::Idle);
        self.finish_restore(ctx, result);
    }

    /// The files may hold the restored setup, as `why` says: the boundary
    /// stays up until the restart, and the writes queued for the old stores
    /// go.
    pub(super) fn keep_restored_files(&mut self, why: RestartWhy) {
        self.backup
            .set_stage(Stage::RestartNeeded { why, failed: None });
        if let Some(b) = self.booted.as_mut() {
            b.stores.discard_pending_saves();
        }
    }

    /// On close: waits for an export or a restore job, so the restored
    /// stores are adopted before the last writes. Stores that cannot be
    /// read back drop the writes queued for the old ones, and a restart
    /// still needed keeps the boundary up: a restart that fails leaves the
    /// app as locked as before. So does a restore that may have left
    /// restored files without its answer: lost, or failed without putting
    /// every file back. A backup still draining does not start.
    /// Teardown only: it lifts any other boundary, so the update install,
    /// the one flush that may leave the app open, is refused while a backup
    /// holds the writes.
    pub(super) fn settle_backup_on_close(&mut self) {
        if self.backup.restart_needed() {
            if let Some(b) = self.booted.as_mut() {
                b.stores.discard_pending_saves();
            }
            return;
        }
        let snapshot = match std::mem::take(&mut self.backup.stage) {
            Stage::Idle | Stage::Draining { .. } => None,
            Stage::Exporting(job) => {
                let _ = job.wait();
                None
            }
            Stage::Restoring(job) => match job.wait() {
                // As finish_restore: settings checked against the old
                // platforms are never adopted, nor written.
                Some(Ok(restored)) if restored.platforms.is_none() => {
                    self.keep_restored_files(RestartWhy::Restored);
                    return;
                }
                Some(Ok(restored)) => Some(restored.read_back.map(|read| read.snapshot)),
                // The files were not replaced, or all put back: the old
                // stores stand.
                Some(Err(error)) if !error.left_restored => None,
                Some(Err(error)) => {
                    self.keep_restored_files(RestartWhy::Incomplete {
                        error: error.message,
                    });
                    return;
                }
                None => {
                    self.keep_restored_files(RestartWhy::Unknown);
                    return;
                }
            },
            // As a restore: the imported stores are adopted before the
            // last writes, or the old ones dropped.
            Stage::Importing(job) => match job.wait() {
                Some(imported) => Some(imported.into_snapshot()),
                None => {
                    self.keep_restored_files(RestartWhy::Imported);
                    return;
                }
            },
            Stage::RestartNeeded { .. } => None,
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
    /// one of them, and refreshes what reads them. A zoom or a panel edit
    /// made during the restore goes with the setup it was made against, as
    /// do their pending saves.
    fn adopt_restored(&mut self, ctx: &Context, read: ReadBack) {
        if let Some(themes) = &read.themes.themes {
            self.apply_theme_payloads(themes);
        }
        let changed = self
            .booted
            .as_mut()
            .map(|b| b.stores.adopt_snapshot(read.snapshot))
            .unwrap_or_default();
        let _ = self.ui_scale.destroy(0);
        self.ui_scale_live = None;
        if let (Some(panel), Some(b)) = (self.settings_panel.as_mut(), self.booted.as_ref()) {
            panel.take_restored(b.stores.settings());
        }
        // The snapshot's manifest holds the restored themes' fingerprint.
        if !read.themes.whole {
            self.unsee_themes();
        }
        self.apply_external_refresh(ctx, &changed);
    }

    /// A restore job ended: on success the restored descriptors are
    /// registered, then the themes, then the stores adopted whole. Stores
    /// that cannot be read back, or descriptors that cannot be reloaded
    /// (the restored settings would be checked against the old platforms
    /// and lose a restored one), ask for the restart that loads them, and
    /// so does a failure that left restored files it could not put back.
    fn finish_restore(&mut self, ctx: &Context, result: Result<Restored, RestoreError>) {
        let locale = self.backup_locale();
        let restored = match result {
            Ok(restored) => restored,
            Err(error) => {
                let left_restored = error.left_restored;
                let text = with_one(
                    locale,
                    "settings.backupRestoreFailed",
                    "error",
                    error.message.clone(),
                );
                self.backup.set_notice(text, true);
                if left_restored {
                    self.keep_restored_files(RestartWhy::Incomplete {
                        error: error.message,
                    });
                    ctx.request_repaint();
                }
                return;
            }
        };
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        // The active tab's descriptor, before the restored ones register.
        let tab = b.nav.active_tab.clone();
        let mut source_changed = false;
        if let Some(report) = &restored.platforms {
            source_changed =
                descriptor_of(b.user_platforms.as_ref(), &tab) != descriptor_of(Some(report), &tab);
            apply_report(b, report);
            if let Some(panel) = self.settings_panel.as_mut() {
                panel.user_platforms = Some(report.clone());
            }
        }
        match (restored.read_back, restored.platforms.is_some()) {
            (Ok(read), true) => {
                self.adopt_restored(ctx, read);
                if source_changed {
                    self.reload_tab_source(&tab);
                }
                let done = with_one(
                    locale,
                    "settings.backupRestored",
                    "path",
                    restored.report.pre_restore_dir,
                );
                self.backup.set_notice(done, false);
            }
            _ => {
                // The old stores must never be written over the restored
                // files.
                self.backup
                    .set_notice(translate(locale, "settings.backupRestartNeeded"), false);
                self.keep_restored_files(RestartWhy::Restored);
                ctx.request_repaint();
            }
        }
    }

    /// The restart modal's "Restart now": the last flush, which writes
    /// nothing over the restored files, then accshift starts again and this
    /// process exits. When no new process starts, the modal stays with the
    /// error.
    pub(super) fn restart_after_restore(&mut self, ctx: &Context) {
        if !self.backup.restart_needed() {
            return;
        }
        let result = self
            .flush_app_state()
            .and_then(|()| self.updates.relaunch(ctx));
        if let Err(error) = result {
            let text = with_one(
                self.backup_locale(),
                "settings.backupRestartFailed",
                "error",
                error,
            );
            if let Stage::RestartNeeded { failed, .. } = &mut self.backup.stage {
                *failed = Some(text);
            }
        }
    }

    /// The active tab `tab` reads its accounts from a restored descriptor
    /// of the same id: its cards load again from it. A tab the restored
    /// settings switched away from already loaded the new one.
    fn reload_tab_source(&mut self, tab: &str) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if b.nav.active_tab != tab {
            return;
        }
        b.accounts.reload_source(&self.jobs, &mut b.stores, tab);
    }
}

/// The descriptor `report` loaded for the platform `id`, as JSON, to tell a
/// changed one apart; `None` for a platform no descriptor declares.
fn descriptor_of(report: Option<&UserPlatformReport>, id: &str) -> Option<serde_json::Value> {
    report?
        .loaded
        .iter()
        .find(|descriptor| descriptor.id == id)
        .and_then(|descriptor| serde_json::to_value(descriptor).ok())
}
