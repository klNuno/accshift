//! What happens when the user acts: dialogs, card actions, navigation,
//! personas, the command palette, the keyboard model and toasts.
//!
//! Ported from the orchestration composables of `src/lib/app/` and the
//! feature stores they drive. Each TS composable becomes a state struct with
//! methods. A method that reaches into something another module owns (the
//! account loader, the add flow, bulk edit, the shell's active tab, the
//! clipboard) does not call it: it returns [`AppEffect`]s, in the order TS
//! performs those calls, and the egui `AppModel` applies them after the frame.
//! What these modules own themselves (dialog state, the history stack, the
//! toast queue, the stores they write) changes in place.
//!
//! - [`toasts::ToastQueue`] ports `features/notifications/store.svelte.ts` and the
//!   timer of `Toast.svelte`.
//! - [`keyboard`] ports `shared/keyboard/{controller,types}.ts` and
//!   `app/keyboardBindings.ts`: the binding table as data, combo parsing,
//!   scope resolution and dispatch through a [`keyboard::KeyHost`].
//! - [`palette`] ports `features/commandPalette/{fuzzy,registry}.ts` and the
//!   filtering of `CommandPalette.svelte`.
//! - [`dialogs::AppDialogs`] ports `app/useAppDialogs.svelte.ts`, the validation of
//!   `InputDialog.svelte`.
//! - [`card_actions`] ports `app/useCardActions.ts`.
//! - [`navigation`] ports `app/useAppNavigation.svelte.ts` with the browser
//!   history as a stack, `app/tabCycle.ts`, `app/useSettingsPanel.svelte.ts`,
//!   `features/settings/useSettingsTabBar.svelte.ts`.
//! - [`extension::ExtensionContent`] ports `app/useExtensionContent.svelte.ts`.
//! - [`personas`] ports `app/usePersonas.svelte.ts`, `app/usePersonaSwitch.svelte.ts`.
//! - [`onboarding::OnboardingTour`] ports `app/useOnboardingTour.svelte.ts`.
//! - [`avatar_fallback`] ports `shared/avatarFallback.ts`.
//! - [`shell`] ports the handlers `src/App.svelte` defines inline over several of
//!   these.

pub mod avatar_fallback;
pub mod card_actions;
pub mod dialogs;
pub mod extension;
pub mod keyboard;
pub mod navigation;
pub mod onboarding;
pub mod palette;
pub mod personas;
pub mod shell;
pub mod toasts;

use crate::accounts::loader::LoadOptions;
use crate::accounts::Toast;
use crate::menu::PlatformTask;
use crate::platforms::PlatformAccount;
use crate::stores::ViewMode;
use crate::text::Text;

/// `loadAccounts(silent, showRefreshedToast, forceRefresh, checkBans,
/// deferBackground)` with the TS defaults (`deferBackground` true).
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

/// One call into something another module owns, in the order TS makes it.
#[derive(Debug, Clone, PartialEq)]
pub enum AppEffect {
    /// `addToast` / `showToast`.
    Toast(Toast),
    /// `loadAccounts(...)` on the active tab.
    LoadAccounts(LoadOptions),
    /// `handleRefreshClick`.
    RefreshAccounts,
    /// `handleAddAccountClick`.
    AddAccount,
    /// `handleAccountSwitch(account)`.
    SwitchAccount(PlatformAccount),
    /// `handleNavigateToFolder(folderId)`:
    /// [`navigation::AppNavigation::navigate_to`] with history.
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
    /// `appNavigation.closeSettingsPanel()`:
    /// [`navigation::AppNavigation::close_settings_panel`].
    CloseSettingsPanel,
    /// `appNavigation.toggleSettingsPanel()`.
    ToggleSettingsPanel,
    /// `appNavigation.handleTabChange(tab)`.
    ChangeTab(String),
    /// `handleNavigateBack`: [`navigation::AppNavigation::navigate_to_parent_folder`]
    /// (its add flow cancel comes first in the list).
    NavigateToParentFolder,
    /// The removal of an account from the grid (`removeAccount` glue of
    /// App.svelte: loader, folder sync, items).
    RemoveAccount(String),
    /// `copyToClipboard(text, label)`: write it, then
    /// [`shell::copy_result_toast`] with the label resolved.
    CopyToClipboard { text: String, label: Text },
    /// Write several lines to the clipboard, then
    /// [`card_actions::bulk_urls_copied_toast`] (`copyBulkEditUrls`).
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
    /// A `requestConfirm` settled: route it to whoever holds the ticket.
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
    /// An entry of a caller-built menu (`openCustomContextMenu`, the persona
    /// cards) was clicked: the id that caller gave it.
    CustomMenu(String),
    /// `invoke("telemetry_track_persona_switch", ...)`, errors ignored.
    TrackPersonaSwitch { platforms: u64, succeeded: u64 },
}
