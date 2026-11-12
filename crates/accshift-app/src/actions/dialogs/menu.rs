//! The app's context menu: the platform menu of an account card, the folder
//! menu, the background menu, and what clicking an entry does.

use super::{
    AppDialogs, ConfirmDialogState, ConfirmPurpose, DialogContext, InputDialogState, InputPurpose,
};
use crate::actions::{load_options, AppEffect};
use crate::folders::{FolderInfo, ItemRef};
use crate::menu::account::{
    build_account_context_menu, color_label_key, AccountFolderChoice, AppearanceInput, FolderInput,
};
use crate::menu::{MenuCommand, MenuEntry, MenuSwatch, SubmenuLoader};
use crate::platforms::adapter::MenuContext;
use crate::platforms::{PlatformAccount, PlatformAdapter};
use crate::stores::{ClientStores, ACCOUNT_CARD_COLOR_PRESETS, MAX_NOTE_LENGTH};
use crate::text::Text;

/// What clicking an entry of the app's context menu does: the platform
/// menu's own [`MenuCommand`]s plus the folder and background entries
/// the app builds itself.
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
    /// Background "Refresh": a visible reload that checks bans.
    RefreshAccounts,
    /// Background "New folder".
    NewFolder,
    /// An entry of a caller-built menu, by the id that caller gave it.
    Custom(String),
}

/// A colour swatch with an app command.
#[derive(Debug, Clone, PartialEq)]
pub struct AppMenuSwatch {
    pub id: String,
    pub label: Text,
    pub color: String,
    pub active: bool,
    pub command: AppMenuCommand,
}

/// What the menu draws, in order.
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

/// The open context menu: where, and on what.
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
    /// The active tab's adapter.
    pub adapter: Option<&'a PlatformAdapter>,
    pub active_tab: &'a str,
    pub active_tab_usable: bool,
    pub current_account_id: Option<&'a str>,
}

/// "Parent / Child", the id when the folder is unknown.
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

impl AppDialogs {
    /// The entries of the open menu, empty when none is open.
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

    /// A menu of pre-built entries (the persona cards).
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
}
