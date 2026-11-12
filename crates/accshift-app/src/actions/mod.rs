//! What happens when the user acts: dialogs, card actions, navigation,
//! personas, the command palette, the keyboard model and toasts.
//!
//! Each concern is a state struct with methods. A method that reaches into
//! something another module owns (the account loader, the add flow, bulk
//! edit, the shell's active tab, the clipboard) does not call it: it returns
//! [`AppEffect`]s, in call order, and the egui `AppModel` applies them after
//! the frame. What these modules own themselves (dialog state, the history
//! stack, the toast queue, the stores they write) changes in place.
//!
//! - [`toasts::ToastQueue`]: the toast queue and each toast's countdown.
//! - [`keyboard`]: the binding table as data, combo parsing, scope
//!   resolution and dispatch through a [`keyboard::KeyHost`].
//! - [`palette`]: the command palette's fuzzy matching, registry and list.
//! - [`dialogs::AppDialogs`]: the confirm and input dialogs, with the input
//!   validation.
//! - [`card_actions`] holds the bulk edit bar actions on the selected cards.
//! - [`navigation`] holds the settings panel tab bar.
//! - [`extension::ExtensionContent`]: the extension panel of each card.
//! - [`personas`]: the saved personas and their switch.
//! - [`onboarding::OnboardingTour`]: the first-run onboarding and its tour.
//! - [`avatar_fallback`]: initials and gradient of a card without a picture.

pub mod avatar_fallback;
pub mod card_actions;
pub mod dialogs;
pub mod extension;
pub mod keyboard;
pub mod navigation;
pub mod onboarding;
pub mod palette;
pub mod personas;
pub mod toasts;

use crate::accounts::loader::LoadOptions;
use crate::accounts::Toast;
use crate::menu::PlatformTask;
use crate::platforms::PlatformAccount;
use crate::stores::ViewMode;
use crate::text::Text;

/// The options of an account load (`silent`, `show_refreshed_toast`,
/// `force_refresh`, `check_bans`), with background work deferred.
pub fn load_options(
    silent: bool,
    show_refreshed_toast: bool,
    force_refresh: bool,
    check_bans: bool,
) -> LoadOptions {
    LoadOptions {
        silent,
        show_refreshed_toast,
        force_refresh,
        check_bans,
        defer_background: true,
    }
}

/// One call into something another module owns, in call order.
#[derive(Debug, Clone, PartialEq)]
pub enum AppEffect {
    /// Show a toast.
    Toast(Toast),
    /// `loadAccounts(...)` on the active tab.
    LoadAccounts(LoadOptions),
    /// Reload the active tab's accounts.
    RefreshAccounts,
    /// Start adding an account.
    AddAccount,
    /// `handleAccountSwitch(account)`.
    SwitchAccount(PlatformAccount),
    /// `handleNavigateToFolder(folderId)`.
    NavigateToFolder(Option<String>),
    /// `navigation.refreshCurrentItems()`.
    RefreshCurrentItems,
    /// `loader.prepareVisibleAccounts()`.
    PrepareVisibleAccounts,
    /// `grid.queueCalculatePadding()`.
    QueueGridPadding,
    /// `loader.clearForPlatformChange()`.
    ClearForPlatformChange,
    /// `visiblePriming.reset()`.
    ResetVisiblePrimeState,
    /// `shell.setActiveTab(tab)`.
    SetActiveTab(String),
    /// `shell.refreshSettings()`: re-read the settings store.
    RefreshSettings,
    /// `secureScreen.handleSettingsClosed()`.
    SettingsClosed,
    /// `appNavigation.closeSettingsPanel()`.
    CloseSettingsPanel,
    /// `appNavigation.toggleSettingsPanel()`.
    ToggleSettingsPanel,
    /// `appNavigation.handleTabChange(tab)`.
    ChangeTab(String),
    /// Go to the parent folder (its add flow cancel comes first in the list).
    NavigateToParentFolder,
    /// Remove an account from the grid: loader, folder sync, items.
    RemoveAccount(String),
    /// `copyToClipboard(text, label)`: write it, then
    /// [`toasts::copy_result_toast`] with the label resolved.
    CopyToClipboard { text: String, label: Text },
    /// Write several lines to the clipboard, then
    /// [`card_actions::bulk_urls_copied_toast`].
    CopyBulkUrls(Vec<String>),
    /// `cardColorVersion += 1`: card colours changed in the stores.
    CardColorsChanged,
    /// `cardNoteVersion += 1`: card notes changed in the stores.
    CardNotesChanged,
    /// A menu task for `PlatformAdapter::run_task` on the job pool.
    RunPlatformTask(PlatformTask),
    /// `adapter.setAccountLabel` then `loadAccounts(true)`, a
    /// `toast.renameAccountFailed` on error: run
    /// [`dialogs::rename_account_blocking`] on the pool and hand the result to
    /// [`dialogs::finish_rename_account`].
    RenameAccount {
        platform_id: String,
        account_id: String,
        label: String,
    },
    /// A confirmation settled: route it to whoever holds the ticket.
    ConfirmSettled {
        ticket: dialogs::ConfirmTicket,
        allowed: bool,
    },
    /// `addFlow.cancel()`.
    AddFlowCancel,
    /// `addFlow.cancelIfConflicting(platformId, accountId?)`.
    AddFlowCancelIfConflicting {
        platform_id: String,
        account_id: Option<String>,
    },
    /// `bulkEdit.closeBulkEdit()`.
    CloseBulkEdit,
    /// `bulkEdit.toggleBulkEdit()`.
    ToggleBulkEdit,
    /// `bulkEdit.toggleBulkEditAccount(id)`.
    BulkEditToggleAccount(String),
    /// `bulkEdit.handlePaintMouseDown(event)`.
    BulkEditPaintStart,
    /// `bulkEdit.bulkEditSelectAll()`.
    BulkEditSelectAll,
    /// `bulkEdit.bulkEditDeselectAll()`.
    BulkEditDeselectAll,
    /// `drag.handleGridMouseDown(event)`.
    DragGridMouseDown,
    /// `uiScale.zoomIn()`.
    ZoomIn,
    /// `uiScale.zoomOut()`.
    ZoomOut,
    /// `uiScale.resetZoom()`.
    ResetZoom,
    /// `updates.applyReadyUpdate()`.
    ApplyUpdate,
    /// The view mode changed and was stored; the grid recomputes its padding
    /// when it is the grid.
    ViewModeChanged(ViewMode),
    /// An entry of a caller-built menu (a custom context menu, the persona
    /// cards) was clicked: the id that caller gave it.
    CustomMenu(String),
    /// Count a persona switch in telemetry, errors ignored.
    TrackPersonaSwitch { platforms: u64, succeeded: u64 },
}
