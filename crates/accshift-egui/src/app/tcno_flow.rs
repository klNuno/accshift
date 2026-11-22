//! The TcNo Account Switcher import: the folder picker and its preview, the
//! confirm, then the import behind the backup's boundary
//! (`app::backup_flow`), so the open app never writes its stores over the
//! imported ones.
//!
//! Once confirmed, the panel's draft goes to the stores and the boundary
//! waits for every write already running, as for a restore. The import
//! then adds the accounts to the config and the notes and order to the
//! stores, which are read back and adopted whole: the writes queued
//! meanwhile were made against the stores before the import and go. The
//! active tab's accounts load again. Stores that cannot be read back ask for
//! the restart that loads them, as a restore does.

use accshift_app::actions::dialogs::ConfirmTicket;
use accshift_app::backend::Backend;
use accshift_app::error::{BackendError, BackendResult};
use accshift_app::i18n::{translate, Locale};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::storage::{STORAGE_TARGET_APP_CONFIG_LOCAL, STORAGE_TARGET_APP_CONFIG_PORTABLE};
use accshift_app::types::{ClientStorageSnapshot, TcNoImportReport, TcNoPreview};
use egui::Context;

use super::backup_flow::{picker_cancelled, with_one, Purpose, RestartWhy};
use super::platforms::RuntimeOs;
use super::settings_panel::{
    tcno_confirm, tcno_nothing, tcno_preview_is_empty, tcno_report, TcnoState,
};
use super::AppModel;

/// The folder the picker returned and what importing it would do; `None`
/// when the picker was cancelled.
type Picked = Option<(String, TcNoPreview)>;

/// What the import job answered: the import's own outcome, then the stores
/// as now on disk, read back even after a failure, which may have written
/// some of them.
pub(super) struct TcnoImported {
    result: BackendResult<TcNoImportReport>,
    read_back: BackendResult<ClientStorageSnapshot>,
}

impl TcnoImported {
    /// The stores read back, for the close path.
    pub(super) fn into_snapshot(self) -> BackendResult<ClientStorageSnapshot> {
        self.read_back
    }
}

/// The TcNo card's picker job and the confirm it waits on.
#[derive(Default)]
pub(super) struct TcnoFlow {
    pub(super) card: TcnoState,
    job: Option<Pending<BackendResult<Picked>>>,
    /// The previewed folder the confirm is about, and its ticket.
    pub(super) pending: Option<(String, ConfirmTicket)>,
}

impl TcnoFlow {
    /// A pick or a confirm is under way.
    pub(super) fn busy(&self) -> bool {
        self.job.is_some() || self.pending.is_some()
    }

    pub(super) fn set_notice(&mut self, text: String, error: bool) {
        self.card.notice = text;
        self.card.error = error;
    }
}

/// The import job: the import, then the stores read back.
pub(super) fn import_job(b: &dyn Backend, path: &str) -> TcnoImported {
    let result = b.tcno_import(path);
    TcnoImported {
        result,
        read_back: b.load_client_storage_snapshot(),
    }
}

impl AppModel {
    fn tcno_locale(&self) -> Locale {
        self.booted.as_ref().map_or(Locale::En, |b| b.locale)
    }

    /// The card's "Choose the TcNo folder": the picker, then the preview.
    pub(super) fn import_tcno(&mut self) {
        if self.backup.busy() {
            return;
        }
        let title = translate(self.tcno_locale(), "settings.tcnoDialog");
        let tcno = &mut self.backup.tcno;
        tcno.set_notice(String::new(), false);
        tcno.job = Some(start_pick(&self.jobs, title));
    }

    /// Once per frame, from `tick_backup`: the picker job, then the card.
    pub(super) fn tick_tcno(&mut self, ctx: &Context) {
        self.poll_tcno_pick(ctx);
        let available = self
            .booted
            .as_ref()
            .is_some_and(|b| b.runtime_os == RuntimeOs::Windows);
        let busy = self.backup.busy();
        let card = &mut self.backup.tcno.card;
        card.available = available;
        card.busy = busy;
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.tcno = card.clone();
        }
    }

    fn poll_tcno_pick(&mut self, ctx: &Context) {
        let Some(job) = &mut self.backup.tcno.job else {
            return;
        };
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => Err(BackendError::new("job panicked")),
        };
        self.backup.tcno.job = None;
        let locale = self.tcno_locale();
        match result {
            Err(error) => {
                let text = with_one(locale, "settings.tcnoReadFailed", "error", error.message);
                self.backup.tcno.set_notice(text, true);
            }
            Ok(None) => {}
            Ok(Some((_, preview))) if tcno_preview_is_empty(&preview) => {
                let text = tcno_nothing(&preview.warnings, locale);
                self.backup.tcno.set_notice(text, false);
            }
            Ok(Some((path, preview))) => {
                let mut effects = Vec::new();
                let ticket = self
                    .dialogs
                    .request_confirm(tcno_confirm(&preview), &mut effects);
                self.backup.tcno.pending = Some((path, ticket));
                self.apply_effects(ctx, effects);
            }
        }
    }

    /// The import confirm settled: an allowed import waits behind the
    /// boundary, as a restore.
    pub(super) fn settle_tcno_import(&mut self, ticket: ConfirmTicket, allowed: bool) {
        let Some((path, pending)) = self.backup.tcno.pending.take() else {
            return;
        };
        if pending != ticket {
            self.backup.tcno.pending = Some((path, pending));
            return;
        }
        if allowed {
            self.start_tcno_import(path);
        }
    }

    /// Starts the import of the TcNo folder at `path` behind the boundary.
    pub(super) fn start_tcno_import(&mut self, path: String) {
        if self.backup.holds_writes() {
            return;
        }
        self.hold_boundary(path, String::new(), Purpose::TcnoImport);
    }

    /// The import job ended: the stores read back are adopted whole, then
    /// what reads them and the active tab's accounts refresh. Stores that
    /// cannot be read back ask for the restart that loads them.
    pub(super) fn finish_tcno_import(&mut self, ctx: &Context, imported: TcnoImported) {
        let locale = self.tcno_locale();
        let (text, error) = match &imported.result {
            Ok(report) => (tcno_report(report, locale), false),
            Err(error) => (
                with_one(
                    locale,
                    "settings.tcnoImportFailed",
                    "error",
                    error.message.clone(),
                ),
                true,
            ),
        };
        let snapshot = match imported.read_back {
            Ok(snapshot) => snapshot,
            Err(_) => {
                self.keep_restored_files(RestartWhy::Imported {
                    error: error.then_some(text),
                });
                let notice = self.backup.restart_message(locale).unwrap_or_default();
                self.backup.tcno.set_notice(notice, error);
                ctx.request_repaint();
                return;
            }
        };
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let mut changed = b.stores.adopt_snapshot(snapshot);
        // The import may have added accounts to the config: the active
        // tab's cards load again.
        changed.push(STORAGE_TARGET_APP_CONFIG_PORTABLE.to_string());
        changed.push(STORAGE_TARGET_APP_CONFIG_LOCAL.to_string());
        self.apply_external_refresh(ctx, &changed);
        self.backup.tcno.set_notice(text, error);
    }
}

/// The picker, then the preview of the folder it returned.
fn start_pick(jobs: &Jobs, title: String) -> Pending<BackendResult<Picked>> {
    jobs.run(move |b| {
        let path = match b.tcno_select_folder(&title) {
            Ok(path) => path,
            Err(error) if picker_cancelled(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        if path.is_empty() {
            return Ok(None);
        }
        let preview = b.tcno_preview(&path)?;
        Ok(Some((path, preview)))
    })
}
