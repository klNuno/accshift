//! Dialogs and the context menu, ported from `src/lib/app/useAppDialogs.svelte.ts`,
//! with the submit rule of `src/lib/shared/components/InputDialog.svelte`.
//!
//! TS dialogs carry an `onConfirm` closure. Here an input dialog carries an
//! [`InputPurpose`] and a confirm dialog a [`ConfirmPurpose`]: data saying
//! what confirming does. [`AppDialogs::submit_input`] and
//! [`AppDialogs::confirm_current`] apply it: the stores the dialog owns change
//! in place, the rest comes back as [`AppEffect`]s.
//!
//! `requestConfirm` returned a promise; here it returns a [`ConfirmTicket`]
//! and every way out of the dialog emits [`AppEffect::ConfirmSettled`] for it.

use super::{load_options, AppEffect};
use crate::accounts::{Toast, ToastKind};
use crate::backend::Backend;
use crate::error::BackendResult;
use crate::folders::{FolderInfo, ItemRef};
use crate::js;
use crate::menu::account::{
    build_account_context_menu, color_label_key, AccountFolderChoice, AppearanceInput, FolderInput,
};
use crate::menu::{InputRequest, MenuCommand, MenuEntry, MenuSwatch, PlatformTask, SubmenuLoader};
use crate::platforms::adapter::MenuContext;
use crate::platforms::{
    PlatformAccount, PlatformAdapter, PlatformBulkEditResult, PlatformRegistry,
};
use crate::stores::{ClientStores, ACCOUNT_CARD_COLOR_PRESETS, MAX_NOTE_LENGTH};
use crate::text::Text;
use std::sync::Arc;

// ------------------------------------------------------------- menu

/// What clicking an entry of the app's context menu does: the platform
/// menu's own [`MenuCommand`]s plus the folder and background entries
/// `useAppDialogs` builds itself.
#[derive(Debug, Clone, PartialEq)]
pub enum AppMenuCommand {
    Platform(MenuCommand),
    /// "Rename": the folder rename dialog.
    RenameFolder(FolderInfo),
    /// A folder colour swatch.
    SetFolderColor {
        folder_id: String,
        color: String,
    },
    /// "Delete folder": its confirm dialog.
    DeleteFolder(FolderInfo),
    /// Background "Refresh": `loadAccounts(false, true, false, true)`.
    RefreshAccounts,
    /// Background "New folder".
    NewFolder,
    /// An entry of a caller-built menu, by the id that caller gave it.
    Custom(String),
}

/// `ContextMenuSwatchAction` with an app command.
#[derive(Debug, Clone, PartialEq)]
pub struct AppMenuSwatch {
    pub id: String,
    pub label: Text,
    pub color: String,
    pub active: bool,
    pub command: AppMenuCommand,
}

/// `ContextMenuItem`: what the menu draws, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum AppMenuEntry {
    Separator,
    Item {
        id: String,
        label: Text,
        command: Option<AppMenuCommand>,
        submenu: Option<Vec<AppMenuEntry>>,
        loader: Option<SubmenuLoader>,
    },
    Swatches {
        id: String,
        label: Text,
        swatches: Vec<AppMenuSwatch>,
    },
}

impl AppMenuEntry {
    /// A plain entry.
    pub fn item(id: impl Into<String>, label: Text, command: AppMenuCommand) -> Self {
        AppMenuEntry::Item {
            id: id.into(),
            label,
            command: Some(command),
            submenu: None,
            loader: None,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            AppMenuEntry::Separator => None,
            AppMenuEntry::Item { id, .. } | AppMenuEntry::Swatches { id, .. } => Some(id),
        }
    }

    pub fn label(&self) -> Option<&Text> {
        match self {
            AppMenuEntry::Separator => None,
            AppMenuEntry::Item { label, .. } | AppMenuEntry::Swatches { label, .. } => Some(label),
        }
    }

    pub fn command(&self) -> Option<&AppMenuCommand> {
        match self {
            AppMenuEntry::Item { command, .. } => command.as_ref(),
            _ => None,
        }
    }
}

impl From<MenuSwatch> for AppMenuSwatch {
    fn from(swatch: MenuSwatch) -> Self {
        Self {
            id: swatch.id,
            label: swatch.label,
            color: swatch.color,
            active: swatch.active,
            command: AppMenuCommand::Platform(swatch.command),
        }
    }
}

impl From<MenuEntry> for AppMenuEntry {
    /// The platform menu (and a loaded submenu) as app entries.
    fn from(entry: MenuEntry) -> Self {
        match entry {
            MenuEntry::Separator => AppMenuEntry::Separator,
            MenuEntry::Item {
                id,
                label,
                command,
                submenu,
                loader,
            } => AppMenuEntry::Item {
                id,
                label,
                command: command.map(AppMenuCommand::Platform),
                submenu: submenu.map(|entries| entries.into_iter().map(Into::into).collect()),
                loader,
            },
            MenuEntry::Swatches {
                id,
                label,
                swatches,
            } => AppMenuEntry::Swatches {
                id,
                label,
                swatches: swatches.into_iter().map(Into::into).collect(),
            },
        }
    }
}

/// What the menu was opened on.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuTarget {
    Account(PlatformAccount),
    Folder(FolderInfo),
    Background,
    /// Pre-built entries from outside the account and folder world.
    Custom(Vec<AppMenuEntry>),
}

/// `ContextMenuState`.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenuState {
    pub x: f32,
    pub y: f32,
    pub target: ContextMenuTarget,
}

/// What building the menu reads.
#[derive(Clone, Copy)]
pub struct MenuInput<'a> {
    pub stores: &'a ClientStores,
    /// `shell.adapter`: the active tab's adapter.
    pub adapter: Option<&'a PlatformAdapter>,
    pub active_tab: &'a str,
    pub active_tab_usable: bool,
    pub current_account_id: Option<&'a str>,
}

/// `folderPathLabel`: "Parent / Child", the id when the folder is unknown.
pub fn folder_path_label(stores: &ClientStores, folder_id: &str) -> String {
    let path = stores.folders().folder_path(Some(folder_id));
    if path.is_empty() {
        return folder_id.to_string();
    }
    path.iter()
        .map(|folder| folder.name.as_str())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn color_label(preset_id: &str) -> Text {
    Text::key(color_label_key(preset_id).unwrap_or(preset_id))
}

/// The folder card menu.
fn folder_menu(folder: &FolderInfo, stores: &ClientStores) -> Vec<AppMenuEntry> {
    let current_color = stores.folder_card_color(&folder.id);
    let swatches = ACCOUNT_CARD_COLOR_PRESETS
        .iter()
        .map(|preset| AppMenuSwatch {
            id: preset.id.to_string(),
            label: color_label(preset.id),
            color: preset.color.to_string(),
            active: current_color == preset.color,
            command: AppMenuCommand::SetFolderColor {
                folder_id: folder.id.clone(),
                color: preset.color.to_string(),
            },
        })
        .collect();
    vec![
        AppMenuEntry::item(
            "folder.rename",
            Text::key("context.menu.rename"),
            AppMenuCommand::RenameFolder(folder.clone()),
        ),
        AppMenuEntry::Swatches {
            id: "folder.color".to_string(),
            label: Text::key("context.menu.folderColor"),
            swatches,
        },
        AppMenuEntry::item(
            "folder.delete",
            Text::key("context.menu.deleteFolder"),
            AppMenuCommand::DeleteFolder(folder.clone()),
        ),
    ]
}

/// The account card menu: the platform menu with the folder world wired in.
fn account_menu(
    account: &PlatformAccount,
    adapter: &PlatformAdapter,
    input: &MenuInput<'_>,
) -> Vec<AppMenuEntry> {
    let stores = input.stores;
    let folders = FolderInput {
        folders: stores
            .folders()
            .list_folders(input.active_tab)
            .into_iter()
            .map(|folder| AccountFolderChoice {
                id: folder.id.clone(),
                label: folder_path_label(stores, &folder.id),
            })
            .collect(),
        current_folder_id: stores
            .folders()
            .find_item_folder_id(&ItemRef::account(account.id.clone()), input.active_tab)
            .map(str::to_string),
    };
    let appearance = AppearanceInput {
        current_color: stores.account_card_color(&account.id).to_string(),
        existing_note: stores.account_card_note(&account.id).to_string(),
    };
    let ctx = MenuContext {
        current_account_id: input.current_account_id,
        stores,
        input_dialog: true,
    };
    build_account_context_menu(account, adapter, &ctx, &appearance, Some(&folders))
        .into_iter()
        .map(Into::into)
        .collect()
}

// ----------------------------------------------------------- dialogs

/// What confirming an input dialog does.
#[derive(Debug, Clone, PartialEq)]
pub enum InputPurpose {
    /// `openNewFolderDialog`: a folder in the current folder of the active
    /// tab, both read when confirming.
    NewFolder,
    /// `openRenameFolderDialog`.
    RenameFolder { folder_id: String },
    /// The note editor of the appearance submenu: set, or clear when blank.
    CardNote { account_id: String },
    /// A platform menu's rename (`openInputDialog` from the adapter).
    PlatformInput(InputRequest),
    /// `openRenameAccountDialog` (F2).
    RenameAccount {
        platform_id: String,
        account_id: String,
    },
    /// `promptRenameNewAccount`: a blank answer keeps the detected name.
    RenameNewAccount {
        platform_id: String,
        account_id: String,
    },
}

/// `InputDialogConfig` plus the draft `InputDialog.svelte` edits.
#[derive(Debug, Clone, PartialEq)]
pub struct InputDialogState {
    pub title: Text,
    pub placeholder: Text,
    pub initial_value: String,
    pub allow_empty: bool,
    /// `maxlength`, in UTF-16 units as the HTML input counts.
    pub maxlength: Option<usize>,
    pub purpose: InputPurpose,
    value: String,
}

impl InputDialogState {
    fn new(
        title: Text,
        placeholder: Text,
        initial_value: String,
        allow_empty: bool,
        maxlength: Option<usize>,
        purpose: InputPurpose,
    ) -> Self {
        Self {
            title,
            placeholder,
            value: initial_value.clone(),
            initial_value,
            allow_empty,
            maxlength,
            purpose,
        }
    }

    /// The draft.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Typing: the input refuses what goes past `maxlength`.
    pub fn set_value(&mut self, value: impl Into<String>) {
        let value = value.into();
        self.value = match self.maxlength {
            Some(max) if js::utf16_len(&value) > max => js::utf16_prefix(&value, max),
            _ => value,
        };
    }

    /// The OK button: enabled with `allowEmpty` or a non-blank draft.
    pub fn can_submit(&self) -> bool {
        self.allow_empty || !js::trim(&self.value).is_empty()
    }

    /// The `n/max` counter, shown in the last 20 characters.
    pub fn char_count(&self) -> Option<(usize, usize)> {
        let max = self.maxlength.filter(|max| *max > 0)?;
        let len = js::utf16_len(&self.value);
        (len + 20 >= max).then_some((len, max))
    }

    /// `submit`: the value handed to `onConfirm` (trimmed unless
    /// `allowEmpty`), `None` when it cannot submit.
    pub fn submitted_value(&self) -> Option<String> {
        if !self.can_submit() {
            return None;
        }
        Some(if self.allow_empty {
            self.value.clone()
        } else {
            js::trim(&self.value).to_string()
        })
    }
}

/// A `requestConfirm` in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConfirmTicket(pub u64);

/// What confirming a confirm dialog does.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmPurpose {
    /// A platform menu's confirm: run its task.
    PlatformTask(PlatformTask),
    /// The folder's "Delete folder".
    DeleteFolder { folder_id: String },
    /// `requestConfirm`: nothing to run, the ticket's owner acts.
    Request(ConfirmTicket),
}

/// `PlatformContextMenuConfirmConfig`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialogState {
    pub title: Text,
    pub message: Text,
    pub confirm_label: Option<Text>,
    pub confirm_color: String,
    pub purpose: ConfirmPurpose,
}

impl ConfirmDialogState {
    /// `confirmDialogConfirmLabel`.
    pub fn confirm_label(&self) -> Text {
        self.confirm_label
            .clone()
            .unwrap_or_else(|| Text::key("common.confirm"))
    }
}

/// What a `requestConfirm` asks.
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

/// `createAppDialogsController`.
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

    // ------------------------------------------------- context menu

    /// `contextMenuItems`.
    pub fn context_menu_items(&self, input: &MenuInput<'_>) -> Vec<AppMenuEntry> {
        let Some(menu) = &self.context_menu else {
            return Vec::new();
        };
        match &menu.target {
            ContextMenuTarget::Custom(items) => items.clone(),
            ContextMenuTarget::Account(account) => match input.adapter {
                Some(adapter) => account_menu(account, adapter, input),
                None => Vec::new(),
            },
            ContextMenuTarget::Folder(folder) => folder_menu(folder, input.stores),
            ContextMenuTarget::Background => {
                let mut items = Vec::new();
                if input.active_tab_usable && input.adapter.is_some() {
                    items.push(AppMenuEntry::item(
                        "background.refresh",
                        Text::key("context.menu.refresh"),
                        AppMenuCommand::RefreshAccounts,
                    ));
                }
                items.push(AppMenuEntry::item(
                    "background.newFolder",
                    Text::key("context.menu.newFolder"),
                    AppMenuCommand::NewFolder,
                ));
                items
            }
        }
    }

    pub fn open_background_context_menu(&mut self, x: f32, y: f32) {
        self.open_menu(x, y, ContextMenuTarget::Background);
    }

    pub fn open_account_context_menu(&mut self, x: f32, y: f32, account: PlatformAccount) {
        self.open_menu(x, y, ContextMenuTarget::Account(account));
    }

    pub fn open_folder_context_menu(&mut self, x: f32, y: f32, folder: FolderInfo) {
        self.open_menu(x, y, ContextMenuTarget::Folder(folder));
    }

    /// `openCustomContextMenu`: pre-built entries (the persona cards).
    pub fn open_custom_context_menu(&mut self, x: f32, y: f32, items: Vec<AppMenuEntry>) {
        self.open_menu(x, y, ContextMenuTarget::Custom(items));
    }

    fn open_menu(&mut self, x: f32, y: f32, target: ContextMenuTarget) {
        self.context_menu = Some(ContextMenuState { x, y, target });
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// A click on an entry or swatch: the menu closes and the command runs.
    pub fn activate(
        &mut self,
        command: &AppMenuCommand,
        ctx: &mut DialogContext<'_>,
    ) -> Vec<AppEffect> {
        self.close_context_menu();
        let mut effects = Vec::new();
        match command {
            AppMenuCommand::Platform(command) => self.platform_command(command, ctx, &mut effects),
            AppMenuCommand::RenameFolder(folder) => self.open_rename_folder_dialog(folder),
            AppMenuCommand::SetFolderColor { folder_id, color } => {
                ctx.stores.set_folder_card_color(folder_id, color);
                effects.push(AppEffect::CardColorsChanged);
            }
            AppMenuCommand::DeleteFolder(folder) => self.open_confirm(
                ConfirmDialogState {
                    title: Text::with("dialog.deleteFolderTitle", [("name", folder.name.as_str())]),
                    message: Text::key("dialog.deleteFolderMessage"),
                    confirm_label: Some(Text::key("context.menu.deleteFolder")),
                    confirm_color: String::new(),
                    purpose: ConfirmPurpose::DeleteFolder {
                        folder_id: folder.id.clone(),
                    },
                },
                &mut effects,
            ),
            AppMenuCommand::RefreshAccounts => effects.push(AppEffect::LoadAccounts(load_options(
                false, true, false, true,
            ))),
            AppMenuCommand::NewFolder => self.open_new_folder_dialog(),
            AppMenuCommand::Custom(id) => effects.push(AppEffect::CustomMenu(id.clone())),
        }
        effects
    }

    fn platform_command(
        &mut self,
        command: &MenuCommand,
        ctx: &mut DialogContext<'_>,
        effects: &mut Vec<AppEffect>,
    ) {
        match command {
            MenuCommand::Close => {}
            MenuCommand::Copy { value, label } => effects.push(AppEffect::CopyToClipboard {
                text: value.clone(),
                label: label.clone(),
            }),
            MenuCommand::Confirm(request) => self.open_confirm(
                ConfirmDialogState {
                    title: request.title.clone(),
                    message: request.message.clone(),
                    confirm_label: request.confirm_label.clone(),
                    confirm_color: String::new(),
                    purpose: ConfirmPurpose::PlatformTask(request.on_confirm.clone()),
                },
                effects,
            ),
            MenuCommand::Input(request) => {
                self.input_dialog = Some(InputDialogState::new(
                    request.title.clone(),
                    request.placeholder.clone(),
                    request.initial_value.clone(),
                    request.allow_empty,
                    None,
                    InputPurpose::PlatformInput((**request).clone()),
                ));
            }
            MenuCommand::MoveToFolder {
                account_id,
                folder_id,
            } => {
                let item = ItemRef::account(account_id.clone());
                let from = ctx
                    .stores
                    .folders()
                    .find_item_folder_id(&item, ctx.active_tab)
                    .map(str::to_string);
                ctx.stores.move_item(
                    &item,
                    from.as_deref(),
                    folder_id.as_deref(),
                    ctx.active_tab,
                    None,
                );
                effects.push(AppEffect::RefreshCurrentItems);
            }
            MenuCommand::EditNote {
                account_id,
                initial_note,
            } => {
                self.input_dialog = Some(InputDialogState::new(
                    Text::key("dialog.cardNoteTitle"),
                    Text::key("dialog.cardNotePlaceholder"),
                    initial_note.clone(),
                    true,
                    Some(MAX_NOTE_LENGTH),
                    InputPurpose::CardNote {
                        account_id: account_id.clone(),
                    },
                ));
            }
            MenuCommand::SetCardColor { account_id, color } => {
                ctx.stores.set_account_card_color(account_id, color);
                effects.push(AppEffect::CardColorsChanged);
            }
            MenuCommand::Task(task) => effects.push(AppEffect::RunPlatformTask(task.clone())),
        }
    }

    // ------------------------------------------------- input dialog

    /// `openNewFolderDialog`.
    pub fn open_new_folder_dialog(&mut self) {
        self.input_dialog = Some(InputDialogState::new(
            Text::key("dialog.newFolderTitle"),
            Text::key("dialog.folderNamePlaceholder"),
            String::new(),
            false,
            None,
            InputPurpose::NewFolder,
        ));
    }

    /// `openRenameFolderDialog`.
    pub fn open_rename_folder_dialog(&mut self, folder: &FolderInfo) {
        self.input_dialog = Some(InputDialogState::new(
            Text::key("dialog.renameFolderTitle"),
            Text::key("dialog.folderNamePlaceholder"),
            folder.name.clone(),
            false,
            None,
            InputPurpose::RenameFolder {
                folder_id: folder.id.clone(),
            },
        ));
    }

    /// `openRenameAccountDialog`: nothing when the active adapter cannot
    /// label accounts.
    pub fn open_rename_account_dialog(
        &mut self,
        adapter: Option<&PlatformAdapter>,
        account: &PlatformAccount,
    ) {
        let Some(adapter) = adapter.filter(|adapter| adapter.supports_account_labels()) else {
            return;
        };
        self.input_dialog = Some(InputDialogState::new(
            Text::key("platform.renameTitle"),
            Text::key("platform.renamePlaceholder"),
            account.display_name.clone(),
            true,
            None,
            InputPurpose::RenameAccount {
                platform_id: adapter.id().to_string(),
                account_id: account.id.clone(),
            },
        ));
    }

    /// `promptRenameNewAccount`: prefilled with the identity the platform
    /// detected, when the platform can label accounts.
    pub fn prompt_rename_new_account(
        &mut self,
        registry: &PlatformRegistry,
        platform_id: &str,
        account_id: &str,
        display_name: Option<&str>,
    ) {
        let supported = registry
            .adapter(platform_id)
            .is_some_and(|adapter| adapter.supports_account_labels());
        if !supported {
            return;
        }
        self.input_dialog = Some(InputDialogState::new(
            Text::key("platform.renameNewAccount"),
            Text::key("platform.renamePlaceholder"),
            display_name.map(js::trim).unwrap_or_default().to_string(),
            true,
            None,
            InputPurpose::RenameNewAccount {
                platform_id: platform_id.to_string(),
                account_id: account_id.to_string(),
            },
        ));
    }

    /// `closeInputDialog` (Cancel, Escape).
    pub fn close_input_dialog(&mut self) {
        self.input_dialog = None;
    }

    /// The OK button or Enter: closes the dialog and applies its purpose with
    /// the submitted value. Nothing while it cannot submit.
    pub fn submit_input(&mut self, ctx: &mut DialogContext<'_>) -> Vec<AppEffect> {
        let Some(value) = self
            .input_dialog
            .as_ref()
            .and_then(InputDialogState::submitted_value)
        else {
            return Vec::new();
        };
        let Some(dialog) = self.input_dialog.take() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        match dialog.purpose {
            InputPurpose::NewFolder => {
                ctx.stores
                    .create_folder(&value, ctx.current_folder_id, ctx.active_tab);
                effects.push(AppEffect::RefreshCurrentItems);
            }
            InputPurpose::RenameFolder { folder_id } => {
                ctx.stores.rename_folder(&folder_id, &value);
                effects.push(AppEffect::RefreshCurrentItems);
            }
            InputPurpose::CardNote { account_id } => {
                if js::trim(&value).is_empty() {
                    ctx.stores.clear_account_card_note(&account_id);
                } else {
                    ctx.stores.set_account_card_note(&account_id, &value);
                }
                effects.push(AppEffect::CardNotesChanged);
            }
            InputPurpose::PlatformInput(request) => {
                effects.push(AppEffect::RunPlatformTask(request.confirm(&value)));
            }
            InputPurpose::RenameAccount {
                platform_id,
                account_id,
            } => effects.push(AppEffect::RenameAccount {
                platform_id,
                account_id,
                label: value,
            }),
            InputPurpose::RenameNewAccount {
                platform_id,
                account_id,
            } => {
                if !js::trim(&value).is_empty() {
                    effects.push(AppEffect::RenameAccount {
                        platform_id,
                        account_id,
                        label: value,
                    });
                }
            }
        }
        effects
    }

    // ----------------------------------------------- confirm dialog

    fn settle_pending(&mut self, allowed: bool, effects: &mut Vec<AppEffect>) {
        if let Some(ticket) = self.pending_confirm.take() {
            effects.push(AppEffect::ConfirmSettled { ticket, allowed });
        }
    }

    /// `openConfirmDialog`: a dialog replacing a pending request settles it
    /// as refused first.
    fn open_confirm(&mut self, dialog: ConfirmDialogState, effects: &mut Vec<AppEffect>) {
        self.settle_pending(false, effects);
        self.confirm_dialog = Some(dialog);
    }

    /// `requestConfirm`: opens the confirm dialog; its answer comes back as
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

    /// `closeConfirmDialog` (Cancel, Escape): a pending request is refused.
    pub fn close_confirm_dialog(&mut self) -> Vec<AppEffect> {
        self.confirm_dialog = None;
        let mut effects = Vec::new();
        self.settle_pending(false, &mut effects);
        effects
    }

    /// `confirmCurrentDialog`: runs the purpose, then a pending request is
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

/// `handleBulkEditResult`: all done, partial, or failed.
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
