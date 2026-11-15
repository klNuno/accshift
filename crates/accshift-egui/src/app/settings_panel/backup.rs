//! The Privacy tab's backup card: export the setup to one file, and restore
//! one after a preview.
//!
//! Export runs the save dialog and the export in one job. Import runs the
//! file picker and the preview in one job, then asks the confirm dialog with
//! what the file holds (its date, its app version, counts per kind). An
//! allowed restore first writes the panel's draft and every store write still
//! owed to disk, then runs the restore in a job. Once it is done the app reads
//! its stores again through the external refresh and the descriptor folder
//! through a reload, without a restart; a store write that is still owed to
//! disk at that point would overwrite the restored file, so the notice then
//! asks for a restart instead.

use std::time::Instant;

use accshift_app::actions::dialogs::{ConfirmConfig, ConfirmTicket};
use accshift_app::error::BackendResult;
use accshift_app::i18n::{translate, Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::text::Text;
use accshift_app::types::{BackupPreview, BackupReport};
use egui::Context;

use super::super::AppModel;
use super::jobs::{DescriptorDone, DescriptorTask};
use super::state::SettingsPanel;

/// What a backup job answered.
pub(in crate::app) enum BackupDone {
    Exported(BackupReport),
    /// The picked file and what it holds; `None` when the picker was
    /// cancelled.
    Picked(Option<(String, BackupPreview)>),
    Restored(BackupReport),
}

/// Which step a failure belongs to, for its notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackupStep {
    Export,
    Read,
    Restore,
}

/// The backup card's state.
#[derive(Default)]
pub struct BackupState {
    /// A job is under way: the buttons are disabled.
    pub busy: bool,
    /// The last outcome, shown under the buttons.
    pub notice: String,
    /// The notice is an error.
    pub error: bool,
    job: Option<(Pending<BackendResult<BackupDone>>, BackupStep)>,
    /// The previewed file the confirm is about, and its ticket.
    pending_restore: Option<(String, ConfirmTicket)>,
}

impl BackupState {
    fn start(
        &mut self,
        jobs: &Jobs,
        step: BackupStep,
        call: impl FnOnce(&dyn accshift_app::backend::Backend) -> BackendResult<BackupDone>
            + Send
            + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        self.busy = true;
        self.notice.clear();
        self.error = false;
        self.job = Some((jobs.run(call), step));
    }

    fn set_notice(&mut self, text: String, error: bool) {
        self.notice = text;
        self.error = error;
    }
}

/// `{key}` with `{name}` set to `value`.
fn with_one(locale: Locale, key: &str, name: &str, value: String) -> String {
    Text::with(key, [(name, TranslationValue::Str(value))]).resolve(locale)
}

/// The confirm a picked backup asks, with what it holds.
pub fn restore_confirm(preview: &BackupPreview, locale: Locale) -> ConfirmConfig {
    let date = accshift_app::time::format_absolute_date_time(
        Some(preview.created_at as f64),
        locale,
        &accshift_app::time::system_zone(),
    );
    let settings = translate(
        locale,
        if preview.has_settings {
            "settings.backupSettingsIncluded"
        } else {
            "settings.backupSettingsMissing"
        },
    );
    let n = |count: usize| TranslationValue::Str(count.to_string());
    ConfirmConfig {
        title: Text::key("settings.backupRestoreTitle"),
        message: Text::with(
            "settings.backupRestoreMessage",
            [
                ("date", TranslationValue::Str(date)),
                (
                    "version",
                    TranslationValue::Str(preview.app_version.clone()),
                ),
                ("folders", n(preview.folders)),
                ("notes", n(preview.notes)),
                ("colors", n(preview.colors)),
                ("personas", n(preview.personas)),
                ("themes", n(preview.themes)),
                ("platforms", n(preview.descriptors)),
                ("settings", TranslationValue::Str(settings)),
            ],
        ),
        confirm_label: Some(Text::key("settings.backupRestoreConfirm")),
        confirm_color: None,
    }
}

impl SettingsPanel {
    /// "Export setup": the save dialog, then the export.
    pub(super) fn export_backup(&mut self, jobs: &Jobs) {
        self.backup.start(jobs, BackupStep::Export, |b| {
            let Ok(path) = b.backup_select_export_path() else {
                return Ok(BackupDone::Picked(None));
            };
            b.backup_export(&path).map(BackupDone::Exported)
        });
    }

    /// "Restore from a file": the file picker, then the preview.
    pub(super) fn import_backup(&mut self, jobs: &Jobs) {
        self.backup.start(jobs, BackupStep::Read, |b| {
            let Ok(path) = b.backup_select_import_file() else {
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

impl AppModel {
    /// Polls the backup job: a picked file opens the confirm, a finished
    /// restore reloads the stores.
    pub(in crate::app) fn tick_backup(&mut self, ctx: &Context) {
        let Some(panel) = self.settings_panel.as_mut() else {
            return;
        };
        let Some((job, step)) = &mut panel.backup.job else {
            return;
        };
        let step = *step;
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(accshift_app::error::BackendError::new("job panicked")),
        };
        panel.backup.job = None;
        panel.backup.busy = false;
        let locale = panel.draft.language;
        match result {
            Err(error) => {
                let key = match step {
                    BackupStep::Export => "settings.backupExportFailed",
                    BackupStep::Read => "settings.backupReadFailed",
                    BackupStep::Restore => "settings.backupRestoreFailed",
                };
                let text = with_one(locale, key, "error", error.message);
                panel.backup.set_notice(text, true);
            }
            Ok(BackupDone::Picked(None)) => {}
            Ok(BackupDone::Exported(report)) => {
                let text = with_one(
                    locale,
                    "settings.backupExported",
                    "path",
                    report.preview.path,
                );
                panel.backup.set_notice(text, false);
            }
            Ok(BackupDone::Picked(Some((path, preview)))) => {
                let mut effects = Vec::new();
                let ticket = self
                    .dialogs
                    .request_confirm(restore_confirm(&preview, locale), &mut effects);
                panel.backup.pending_restore = Some((path, ticket));
                self.apply_effects(ctx, effects);
            }
            Ok(BackupDone::Restored(report)) => self.after_restore(locale, report),
        }
    }

    /// The restore confirm settled: an allowed restore writes what is owed
    /// to disk, then runs.
    pub(in crate::app) fn settle_backup_restore(&mut self, ticket: ConfirmTicket, allowed: bool) {
        let Some(panel) = self.settings_panel.as_mut() else {
            return;
        };
        let Some((path, pending)) = panel.backup.pending_restore.take() else {
            return;
        };
        if pending != ticket {
            panel.backup.pending_restore = Some((path, pending));
            return;
        }
        if !allowed {
            return;
        }
        // The draft and the debounced store writes land first: written
        // after the restore, they would put the old setup back.
        self.persist_settings(Instant::now());
        if let Some(b) = self.booted.as_mut() {
            let _ = b.stores.storage_mut().flush_blocking(self.backend.as_ref());
        }
        let jobs = &self.jobs;
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.backup.start(jobs, BackupStep::Restore, move |b| {
                b.backup_restore(&path).map(BackupDone::Restored)
            });
        }
    }

    /// A restore landed: the stores, the themes and the descriptor folder
    /// are read again, or the notice asks for a restart when a store write
    /// still owed to disk would overwrite a restored file.
    fn after_restore(&mut self, locale: Locale, report: BackupReport) {
        let unsafe_reload = self
            .booted
            .as_ref()
            .is_some_and(|b| b.stores.storage().has_unsaved());
        let jobs = &self.jobs;
        let Some(panel) = self.settings_panel.as_mut() else {
            return;
        };
        if unsafe_reload {
            panel
                .backup
                .set_notice(translate(locale, "settings.backupRestartNeeded"), false);
            return;
        }
        let text = with_one(
            locale,
            "settings.backupRestored",
            "path",
            report.pre_restore_dir,
        );
        panel.backup.set_notice(text, false);
        panel.run_descriptor_task(jobs, DescriptorTask::Reload, |b| {
            b.reload_user_platforms().map(DescriptorDone::Report)
        });
        self.request_external_refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview() -> BackupPreview {
        BackupPreview {
            path: "C:/backups/accshift-backup-2026-11-15.json".into(),
            format: 1,
            created_at: 1_791_000_000,
            app_version: "2.0.0".into(),
            folders: 3,
            notes: 5,
            colors: 2,
            personas: 1,
            themes: 4,
            descriptors: 0,
            has_settings: true,
        }
    }

    #[test]
    fn the_restore_confirm_lists_what_the_file_holds_and_that_sessions_stay_out() {
        for locale in accshift_app::i18n::LOCALES {
            let config = restore_confirm(&preview(), locale);
            let message = config.message.resolve(locale);
            for part in ["2.0.0", "3", "5", "4", "user/backups"] {
                assert!(message.contains(part), "{locale:?}: {part} in {message}");
            }
            assert!(!message.contains('{'), "{locale:?}: {message}");
        }
        let en = restore_confirm(&preview(), Locale::En)
            .message
            .resolve(Locale::En);
        assert!(en.contains("Saved sessions are not included"), "{en}");
        assert!(en.contains("Settings: included"), "{en}");
    }
}
