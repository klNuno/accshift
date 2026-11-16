//! The config backup: export, import with its preview and confirm, and the
//! restore, owned by the app rather than the settings panel so closing the
//! panel never drops a restore under way.
//!
//! A restore runs behind a boundary. Once confirmed, the panel's draft goes
//! to the stores and the restore waits: for the store writes already
//! running, for the debounced ones it hands out itself, for an external
//! refresh and a descriptor task in flight. Meanwhile no new store write and
//! no external refresh starts. A write that failed there cancels the
//! restore. The restore job then replaces the files, reloads the user
//! descriptors and reads the stores and themes back. The app registers the
//! restored descriptors first, so the settings that name a restored platform
//! are sanitized against it, then adopts the whole snapshot, dropping the
//! writes queued meanwhile, and only then lets writes and refreshes run
//! again.

use std::time::Instant;

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

/// What an export or an import job answered.
pub(super) enum BackupDone {
    Exported(BackupReport),
    /// The picked file and what it holds; `None` when the picker was
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
    /// The stores as now on disk.
    snapshot: BackendResult<ClientStorageSnapshot>,
    /// The custom themes as now on disk; `None` when they could not be read.
    themes: Option<Vec<CustomThemePayload>>,
}

/// Where a confirmed restore is.
#[derive(Default)]
enum RestoreStage {
    #[default]
    Idle,
    /// Waiting for the writes and reads already running.
    Draining {
        path: String,
        /// A write that failed while draining: the restore does not run.
        failed: Option<String>,
    },
    Running(Pending<BackendResult<Restored>>),
}

/// The backup card's jobs and the restore's stage.
#[derive(Default)]
pub(super) struct BackupFlow {
    pub(super) card: BackupState,
    job: Option<(Pending<BackendResult<BackupDone>>, BackupStep)>,
    /// The previewed file the confirm is about, and its ticket.
    pending_restore: Option<(String, ConfirmTicket)>,
    restore: RestoreStage,
}

impl BackupFlow {
    /// A confirmed restore is under way: store writes and external
    /// refreshes wait.
    pub(super) fn restoring(&self) -> bool {
        !matches!(self.restore, RestoreStage::Idle)
    }

    /// A store write ended with `error` while the restore drains.
    pub(super) fn save_failed(&mut self, error: &BackendError) {
        if let RestoreStage::Draining { failed, .. } = &mut self.restore {
            failed.get_or_insert_with(|| error.message.clone());
        }
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.pending_restore.is_some() || self.restoring()
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

    /// "Export setup": the save dialog, then the export.
    pub(super) fn export(&mut self, jobs: &Jobs, title: String, all_files: String) {
        self.start(jobs, BackupStep::Export, move |b| {
            let Ok(path) = b.backup_select_export_path(&title, &all_files) else {
                return Ok(BackupDone::Picked(None));
            };
            b.backup_export(&path).map(BackupDone::Exported)
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
    let snapshot = b.load_client_storage_snapshot();
    let themes = b.list_custom_themes().ok().map(|themes| {
        themes
            .iter()
            .filter_map(|t| serde_json::to_value(t).ok())
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect()
    });
    Ok(Restored {
        report,
        platforms,
        snapshot,
        themes,
    })
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
    /// restore's stages, then the card the panel draws.
    pub(super) fn tick_backup(&mut self, ctx: &Context) {
        self.poll_backup_job(ctx);
        self.drain_for_restore();
        self.poll_restore(ctx);
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
            Ok(BackupDone::Exported(report)) => {
                let text = with_one(
                    locale,
                    "settings.backupExported",
                    "path",
                    report.preview.path,
                );
                self.backup.set_notice(text, false);
            }
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
        if self.backup.restoring() || self.booted.is_none() {
            return;
        }
        // Written after the restore, the draft would put the old setup back.
        self.flush_settings(Instant::now());
        self.backup.set_notice(String::new(), false);
        self.backup.restore = RestoreStage::Draining { path, failed: None };
    }

    /// The boundary: hands out the debounced writes, then waits until no
    /// write, refresh or descriptor task is running before the restore job
    /// starts.
    fn drain_for_restore(&mut self) {
        if !matches!(self.backup.restore, RestoreStage::Draining { .. }) {
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
        let descriptor_running = self
            .settings_panel
            .as_ref()
            .is_some_and(|p| p.descriptor_busy);
        if !self.saves.is_empty()
            || b.stores.storage().has_unsaved()
            || self.external_refresh.running()
            || descriptor_running
        {
            return;
        }
        let RestoreStage::Draining { path, failed } = std::mem::take(&mut self.backup.restore)
        else {
            return;
        };
        if let Some(error) = failed {
            let text = with_one(
                self.backup_locale(),
                "settings.backupRestoreFailed",
                "error",
                error,
            );
            self.backup.set_notice(text, true);
            return;
        }
        self.backup.restore = RestoreStage::Running(self.jobs.run(move |b| restore_job(b, &path)));
    }

    fn poll_restore(&mut self, ctx: &Context) {
        let RestoreStage::Running(job) = &mut self.backup.restore else {
            return;
        };
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(BackendError::new("job panicked")),
        };
        self.backup.restore = RestoreStage::Idle;
        self.finish_restore(ctx, result);
    }

    /// Waits for a restore job on close, so the restored stores are adopted
    /// before the last writes; a restore still draining does not start.
    pub(super) fn settle_restore_on_close(&mut self) {
        let RestoreStage::Running(job) = std::mem::take(&mut self.backup.restore) else {
            return;
        };
        let Some(Ok(restored)) = job.wait() else {
            return;
        };
        if let (Ok(snapshot), Some(b)) = (restored.snapshot, self.booted.as_mut()) {
            b.stores.adopt_snapshot(snapshot);
        }
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
        if let Some(themes) = &restored.themes {
            self.apply_theme_payloads(themes);
        }
        let Ok(snapshot) = restored.snapshot else {
            self.backup
                .set_notice(translate(locale, "settings.backupRestartNeeded"), false);
            return;
        };
        let changed = self
            .booted
            .as_mut()
            .map(|b| b.stores.adopt_snapshot(snapshot))
            .unwrap_or_default();
        self.apply_external_refresh(ctx, &changed);
        if !platforms_read {
            self.backup
                .set_notice(translate(locale, "settings.backupRestartNeeded"), false);
            return;
        }
        let text = with_one(
            locale,
            "settings.backupRestored",
            "path",
            restored.report.pre_restore_dir,
        );
        self.backup.set_notice(text, false);
    }
}
