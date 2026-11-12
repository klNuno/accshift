//! Dialogs and the context menu.
//!
//! An input dialog carries an [`InputPurpose`] and a confirm dialog a
//! [`ConfirmPurpose`]: data saying what confirming does.
//! [`AppDialogs::submit_input`] and [`AppDialogs::confirm_current`] apply
//! it: the stores the dialog owns change in place, the rest comes back as
//! [`AppEffect`]s.
//!
//! [`AppDialogs::request_confirm`] returns a [`ConfirmTicket`], and every way
//! out of the dialog emits [`AppEffect::ConfirmSettled`] for it.
//!
//! - [`menu`]: the app's context menu and what its entries do.
//! - [`input`]: the input dialog, its draft and what submitting does.
//! - here: the confirm dialog and the dialog state they share.

mod input;
mod menu;

pub use input::{InputDialogState, InputPurpose};
pub use menu::{
    folder_path_label, AppMenuCommand, AppMenuEntry, AppMenuSwatch, ContextMenuState,
    ContextMenuTarget, MenuInput,
};

use super::{load_options, AppEffect};
use crate::accounts::{Toast, ToastKind};
use crate::backend::Backend;
use crate::error::BackendResult;
use crate::menu::PlatformTask;
use crate::platforms::{PlatformAdapter, PlatformBulkEditResult};
use crate::stores::ClientStores;
use crate::text::Text;
use std::sync::Arc;

/// A confirmation request in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConfirmTicket(pub u64);

/// What confirming a confirm dialog does.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmPurpose {
    /// A platform menu's confirm: run its task.
    PlatformTask(PlatformTask),
    /// The folder's "Delete folder".
    DeleteFolder { folder_id: String },
    /// A confirmation request: nothing to run, the ticket's owner acts.
    Request(ConfirmTicket),
}

/// A confirm dialog and what confirming does.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialogState {
    pub title: Text,
    pub message: Text,
    pub confirm_label: Option<Text>,
    pub confirm_color: String,
    pub purpose: ConfirmPurpose,
}

impl ConfirmDialogState {
    /// The confirm button label, `common.confirm` by default.
    pub fn confirm_label(&self) -> Text {
        self.confirm_label
            .clone()
            .unwrap_or_else(|| Text::key("common.confirm"))
    }
}

/// What a confirmation request asks.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmConfig {
    pub title: Text,
    pub message: Text,
    pub confirm_label: Option<Text>,
    pub confirm_color: Option<String>,
}

/// What the dialogs write and read when they act.
pub struct DialogContext<'a> {
    pub stores: &'a mut ClientStores,
    pub active_tab: &'a str,
    pub current_folder_id: Option<&'a str>,
}

/// The context menu, the input dialog and the confirm dialog.
#[derive(Debug, Clone, Default)]
pub struct AppDialogs {
    context_menu: Option<ContextMenuState>,
    input_dialog: Option<InputDialogState>,
    confirm_dialog: Option<ConfirmDialogState>,
    pending_confirm: Option<ConfirmTicket>,
    next_ticket: u64,
}

impl AppDialogs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn context_menu(&self) -> Option<&ContextMenuState> {
        self.context_menu.as_ref()
    }

    pub fn input_dialog(&self) -> Option<&InputDialogState> {
        self.input_dialog.as_ref()
    }

    /// The draft the input dialog edits.
    pub fn input_dialog_mut(&mut self) -> Option<&mut InputDialogState> {
        self.input_dialog.as_mut()
    }

    pub fn confirm_dialog(&self) -> Option<&ConfirmDialogState> {
        self.confirm_dialog.as_ref()
    }

    /// An input or confirm dialog is up (the `dialog` key scope).
    pub fn has_dialog(&self) -> bool {
        self.input_dialog.is_some() || self.confirm_dialog.is_some()
    }

    /// The ticket a confirm dialog answers, if one is pending.
    pub fn pending_confirm(&self) -> Option<ConfirmTicket> {
        self.pending_confirm
    }

    fn settle_pending(&mut self, allowed: bool, effects: &mut Vec<AppEffect>) {
        if let Some(ticket) = self.pending_confirm.take() {
            effects.push(AppEffect::ConfirmSettled { ticket, allowed });
        }
    }

    /// A dialog replacing a pending request settles it
    /// as refused first.
    fn open_confirm(&mut self, dialog: ConfirmDialogState, effects: &mut Vec<AppEffect>) {
        self.settle_pending(false, effects);
        self.confirm_dialog = Some(dialog);
    }

    /// Opens the confirm dialog; its answer comes back as
    /// [`AppEffect::ConfirmSettled`] for the returned ticket. A request open
    /// before is settled as refused into `effects`.
    pub fn request_confirm(
        &mut self,
        config: ConfirmConfig,
        effects: &mut Vec<AppEffect>,
    ) -> ConfirmTicket {
        self.next_ticket += 1;
        let ticket = ConfirmTicket(self.next_ticket);
        self.open_confirm(
            ConfirmDialogState {
                title: config.title,
                message: config.message,
                confirm_label: config.confirm_label,
                confirm_color: config.confirm_color.unwrap_or_default(),
                purpose: ConfirmPurpose::Request(ticket),
            },
            effects,
        );
        self.pending_confirm = Some(ticket);
        ticket
    }

    /// Cancel or Escape: a pending request is refused.
    pub fn close_confirm_dialog(&mut self) -> Vec<AppEffect> {
        self.confirm_dialog = None;
        let mut effects = Vec::new();
        self.settle_pending(false, &mut effects);
        effects
    }

    /// Runs the purpose, then a pending request is
    /// accepted.
    pub fn confirm_current(&mut self, ctx: &mut DialogContext<'_>) -> Vec<AppEffect> {
        let mut effects = Vec::new();
        if let Some(dialog) = self.confirm_dialog.take() {
            match dialog.purpose {
                ConfirmPurpose::PlatformTask(task) => {
                    effects.push(AppEffect::RunPlatformTask(task))
                }
                ConfirmPurpose::DeleteFolder { folder_id } => {
                    ctx.stores.delete_folder(&folder_id);
                    effects.push(AppEffect::RefreshCurrentItems);
                }
                ConfirmPurpose::Request(_) => {}
            }
        }
        self.settle_pending(true, &mut effects);
        effects
    }
}

/// The toast of a bulk edit: all done, partial, or failed.
pub fn bulk_edit_result_toast(result: &PlatformBulkEditResult) -> Toast {
    if result.failed.is_empty() && result.succeeded > 0 {
        Toast::new(
            Text::with(
                "bulkEdit.toastSuccess",
                [("count", result.succeeded as f64)],
            ),
            ToastKind::Success,
        )
    } else if !result.failed.is_empty() && result.succeeded > 0 {
        Toast::new(
            Text::with(
                "bulkEdit.toastPartial",
                [
                    ("succeeded", result.succeeded as f64),
                    ("failed", result.failed.len() as f64),
                ],
            ),
            ToastKind::Default,
        )
    } else {
        Toast::new(Text::key("bulkEdit.toastFailed"), ToastKind::Error)
    }
}

/// The blocking half of [`AppEffect::RenameAccount`], for the job pool.
pub fn rename_account_blocking(
    backend: &dyn Backend,
    adapter: &Arc<PlatformAdapter>,
    account_id: &str,
    label: &str,
) -> BackendResult<()> {
    adapter.set_account_label(backend, account_id, label)
}

/// The UI half: reload silently, or `toast.renameAccountFailed`.
pub fn finish_rename_account(result: BackendResult<()>) -> Vec<AppEffect> {
    match result {
        Ok(()) => vec![AppEffect::LoadAccounts(load_options(
            true, false, false, false,
        ))],
        Err(error) => vec![AppEffect::Toast(Toast::new(
            Text::with("toast.renameAccountFailed", [("error", error.message)]),
            ToastKind::Error,
        ))],
    }
}
