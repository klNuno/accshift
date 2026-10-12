//! What a click, a right click or a key does to an account or folder card,
//! and the bulk edit bar actions on the selected cards. Ported from
//! `src/lib/app/useCardActions.ts`.

use super::dialogs::AppDialogs;
use super::AppEffect;
use crate::accounts::{Toast, ToastKind};
use crate::folders::{ItemKind, ItemRef};
use crate::platforms::{PlatformAccount, PlatformAdapter};
use crate::stores::ClientStores;
use crate::text::Text;

/// What the card handlers read.
pub struct CardContext<'a> {
    /// `getActiveTab()`.
    pub active_tab: &'a str,
    /// `getIsSearching()`.
    pub is_searching: bool,
    /// `getAdapter()`.
    pub adapter: Option<&'a PlatformAdapter>,
    /// `bulkEdit.bulkEditMode`.
    pub bulk_edit_mode: bool,
    /// `addFlow.isPendingSetupAccount(id)`: the account a running add flow
    /// is still setting up.
    pub pending_setup_account_id: Option<&'a str>,
    /// `display.renderedAccountMap`.
    pub rendered_accounts: &'a [PlatformAccount],
}

impl CardContext<'_> {
    fn is_pending_setup(&self, account_id: &str) -> bool {
        self.pending_setup_account_id == Some(account_id)
    }

    fn rendered(&self, account_id: &str) -> Option<&PlatformAccount> {
        self.rendered_accounts
            .iter()
            .find(|account| account.id == account_id)
    }

    fn cancel_conflicting(&self, account_id: Option<&str>) -> AppEffect {
        AppEffect::AddFlowCancelIfConflicting {
            platform_id: self.active_tab.to_string(),
            account_id: account_id.map(str::to_string),
        }
    }
}

/// The focused card for the keyboard handlers: `cardFocus.focusedItem` and
/// the centre of its element (`findElement(item).getBoundingClientRect()`),
/// `None` when the card is not on screen.
#[derive(Debug, Clone, Copy)]
pub struct FocusedCard<'a> {
    pub item: &'a ItemRef,
    pub center: Option<(f32, f32)>,
}

/// `handleBackgroundContextMenu`.
pub fn background_context_menu(
    dialogs: &mut AppDialogs,
    ctx: &CardContext<'_>,
    x: f32,
    y: f32,
) -> Vec<AppEffect> {
    let effects = vec![ctx.cancel_conflicting(None)];
    dialogs.open_background_context_menu(x, y);
    effects
}

/// `handleWorkspaceMouseDown`: in selection mode a press starts a paint
/// selection, otherwise the drag manager (never while searching).
pub fn workspace_mouse_down(ctx: &CardContext<'_>) -> Vec<AppEffect> {
    if ctx.bulk_edit_mode {
        return vec![AppEffect::BulkEditPaintStart];
    }
    if !ctx.is_searching {
        return vec![AppEffect::DragGridMouseDown];
    }
    Vec::new()
}

/// `handleWorkspaceAccountActivate`.
pub fn account_activate(ctx: &CardContext<'_>, account: &PlatformAccount) -> Vec<AppEffect> {
    if ctx.bulk_edit_mode {
        return Vec::new();
    }
    vec![ctx.cancel_conflicting(Some(&account.id))]
}

/// `handleWorkspaceAccountSwitch`.
pub fn account_switch(ctx: &CardContext<'_>, account: &PlatformAccount) -> Vec<AppEffect> {
    if ctx.bulk_edit_mode {
        return vec![AppEffect::BulkEditToggleAccount(account.id.clone())];
    }
    if ctx.is_pending_setup(&account.id) {
        return Vec::new();
    }
    vec![
        ctx.cancel_conflicting(Some(&account.id)),
        AppEffect::SwitchAccount(account.clone()),
    ]
}

/// `handleWorkspaceAccountContextMenu`.
pub fn account_context_menu(
    dialogs: &mut AppDialogs,
    ctx: &CardContext<'_>,
    account: &PlatformAccount,
    x: f32,
    y: f32,
) -> Vec<AppEffect> {
    if ctx.bulk_edit_mode {
        return vec![AppEffect::BulkEditToggleAccount(account.id.clone())];
    }
    if ctx.is_pending_setup(&account.id) {
        return Vec::new();
    }
    let effects = vec![ctx.cancel_conflicting(Some(&account.id))];
    dialogs.open_account_context_menu(x, y, account.clone());
    effects
}

/// `handleWorkspaceFolderContextMenu`.
pub fn folder_context_menu(
    dialogs: &mut AppDialogs,
    ctx: &CardContext<'_>,
    stores: &ClientStores,
    folder_id: &str,
    x: f32,
    y: f32,
) -> Vec<AppEffect> {
    let effects = vec![ctx.cancel_conflicting(None)];
    if let Some(folder) = stores.folders().folder(folder_id) {
        dialogs.open_folder_context_menu(x, y, folder.clone());
    }
    effects
}

/// `activateFocusedCard`: open a folder, or activate then switch an
/// account. `None` when nothing is focused or the card is gone (the key
/// then passes).
pub fn activate_focused_card(
    ctx: &CardContext<'_>,
    focused: Option<&ItemRef>,
) -> Option<Vec<AppEffect>> {
    let item = focused?;
    if item.kind == ItemKind::Folder {
        return Some(vec![AppEffect::NavigateToFolder(Some(item.id.clone()))]);
    }
    let account = ctx.rendered(&item.id)?;
    let mut effects = account_activate(ctx, account);
    effects.extend(account_switch(ctx, account));
    Some(effects)
}

/// `openFocusedCardContextMenu`: the menu opens at the card's centre.
pub fn open_focused_card_context_menu(
    dialogs: &mut AppDialogs,
    ctx: &CardContext<'_>,
    stores: &ClientStores,
    focused: Option<FocusedCard<'_>>,
) -> Option<Vec<AppEffect>> {
    let focused = focused?;
    let (x, y) = focused.center?;
    let item = focused.item;
    if item.kind == ItemKind::Folder {
        stores.folders().folder(&item.id)?;
        return Some(folder_context_menu(dialogs, ctx, stores, &item.id, x, y));
    }
    let account = ctx.rendered(&item.id)?.clone();
    Some(account_context_menu(dialogs, ctx, &account, x, y))
}

/// `renameFocusedCard`: true when a rename dialog opened.
pub fn rename_focused_card(
    dialogs: &mut AppDialogs,
    ctx: &CardContext<'_>,
    stores: &ClientStores,
    focused: Option<&ItemRef>,
) -> bool {
    let Some(item) = focused else {
        return false;
    };
    if item.kind == ItemKind::Folder {
        let Some(folder) = stores.folders().folder(&item.id) else {
            return false;
        };
        dialogs.open_rename_folder_dialog(folder);
        return true;
    }
    let Some(account) = ctx.rendered(&item.id) else {
        return false;
    };
    let Some(adapter) = ctx
        .adapter
        .filter(|adapter| adapter.supports_account_labels())
    else {
        return false;
    };
    dialogs.open_rename_account_dialog(Some(adapter), account);
    true
}

/// `copyBulkEditUrls`: nothing for an empty list, otherwise the clipboard
/// write, one URL per line.
pub fn copy_bulk_edit_urls(urls: &[String]) -> Vec<AppEffect> {
    if urls.is_empty() {
        return Vec::new();
    }
    vec![AppEffect::CopyBulkUrls(urls.to_vec())]
}

/// The toast after [`AppEffect::CopyBulkUrls`]: `bulkEdit.urlsCopied` with the
/// count, `toast.copyFailed` when the write failed.
pub fn bulk_urls_copied_toast(count: usize, write_ok: bool) -> Toast {
    if write_ok {
        Toast::new(
            Text::with("bulkEdit.urlsCopied", [("count", count as f64)]),
            ToastKind::Success,
        )
    } else {
        Toast::new(Text::key("toast.copyFailed"), ToastKind::Error)
    }
}

/// `applyBulkEditCardColor`: writes every selected id in the store (an empty
/// colour clears), then the version bump and the toast.
pub fn apply_bulk_edit_card_color(
    stores: &mut ClientStores,
    selected_ids: &[String],
    color: &str,
) -> Vec<AppEffect> {
    if selected_ids.is_empty() {
        return Vec::new();
    }
    stores.set_account_card_colors(selected_ids, color);
    let key = if color.is_empty() {
        "bulkEdit.colorCleared"
    } else {
        "bulkEdit.colorApplied"
    };
    vec![
        AppEffect::CardColorsChanged,
        AppEffect::Toast(Toast::new(
            Text::with(key, [("count", selected_ids.len() as f64)]),
            ToastKind::Success,
        )),
    ]
}
