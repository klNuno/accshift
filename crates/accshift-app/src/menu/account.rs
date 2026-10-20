//! The account card menu, ported from
//! `src/lib/shared/contextMenu/{accountAppearanceActions,accountFolderActions,
//! accountMenuBuilder}.ts`.

use super::{
    build_context_menu_items, InputRequest, MenuAction, MenuActionBody, MenuCommand, MenuEntry,
    MenuSwatch, ACCOUNT_CONTEXT_GROUP_ORDER,
};
use crate::platforms::adapter::MenuContext;
use crate::platforms::{PlatformAccount, PlatformAdapter};
use crate::stores::ACCOUNT_CARD_COLOR_PRESETS;
use crate::text::Text;

/// `COLOR_LABEL_KEYS`: preset id to its label key.
pub const COLOR_LABEL_KEYS: [(&str, &str); 11] = [
    ("none", "color.none"),
    ("blue", "color.blue"),
    ("cyan", "color.cyan"),
    ("green", "color.green"),
    ("lime", "color.lime"),
    ("yellow", "color.yellow"),
    ("orange", "color.orange"),
    ("red", "color.red"),
    ("pink", "color.pink"),
    ("violet", "color.violet"),
    ("gray", "color.gray"),
];

/// The label key of a colour preset.
pub fn color_label_key(preset_id: &str) -> Option<&'static str> {
    COLOR_LABEL_KEYS
        .iter()
        .find(|(id, _)| *id == preset_id)
        .map(|(_, key)| *key)
}

/// What the appearance entries read (`getCurrentColor`, `getExistingNote`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppearanceInput {
    pub current_color: String,
    pub existing_note: String,
}

/// `getAccountAppearanceContextActions`: the note and colour submenu.
pub fn account_appearance_actions(
    account: &PlatformAccount,
    input: &AppearanceInput,
) -> Vec<MenuAction> {
    let note_label = if input.existing_note.is_empty() {
        "context.menu.addNote"
    } else {
        "context.menu.editNote"
    };
    let swatches = ACCOUNT_CARD_COLOR_PRESETS
        .iter()
        .map(|preset| MenuSwatch {
            id: preset.id.to_string(),
            label: Text::key(color_label_key(preset.id).unwrap_or(preset.id)),
            color: preset.color.to_string(),
            active: input.current_color == preset.color,
            command: MenuCommand::SetCardColor {
                account_id: account.id.clone(),
                color: preset.color.to_string(),
            },
        })
        .collect();
    vec![MenuAction::submenu(
        format!("account.appearance.{}", account.id),
        Some("account.appearance"),
        Text::key("context.menu.editCardAndColor"),
        vec![
            MenuAction::item(
                format!("account.appearance.note.{}", account.id),
                None,
                Text::key(note_label),
                MenuCommand::EditNote {
                    account_id: account.id.clone(),
                    initial_note: input.existing_note.clone(),
                },
            ),
            MenuAction {
                id: format!("account.appearance.color.{}", account.id),
                group: None,
                label: Text::key("context.menu.cardColor"),
                body: MenuActionBody::Swatches(swatches),
            },
        ],
    )]
}

/// `AccountFolderChoice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFolderChoice {
    pub id: String,
    /// Full path of the folder, deepest last ("Parent / Child").
    pub label: String,
}

/// What the folder entry reads (`getFolders`, `getCurrentFolderId`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderInput {
    /// Every folder of the active platform, in listing order.
    pub folders: Vec<AccountFolderChoice>,
    /// The folder the account is in, `None` at the platform root.
    pub current_folder_id: Option<String>,
}

/// `getAccountFolderContextActions`: "Move to folder", minus wherever the
/// account already is; nothing when there is nowhere to go.
pub fn account_folder_actions(account: &PlatformAccount, input: &FolderInput) -> Vec<MenuAction> {
    let current = input.current_folder_id.as_deref();
    let mut destinations: Vec<(Option<String>, Text)> = Vec::new();
    if current.is_some() {
        destinations.push((None, Text::key("context.menu.moveToRoot")));
    }
    for folder in &input.folders {
        if Some(folder.id.as_str()) == current {
            continue;
        }
        destinations.push((Some(folder.id.clone()), Text::raw(folder.label.clone())));
    }
    if destinations.is_empty() {
        return Vec::new();
    }
    let submenu = destinations
        .into_iter()
        .map(|(folder_id, label)| {
            MenuAction::item(
                format!(
                    "account.folder.{}.{}",
                    account.id,
                    folder_id.as_deref().unwrap_or("root")
                ),
                None,
                label,
                MenuCommand::MoveToFolder {
                    account_id: account.id.clone(),
                    folder_id,
                },
            )
        })
        .collect();
    vec![MenuAction::submenu(
        format!("account.folder.{}", account.id),
        Some("account.folder"),
        Text::key("context.menu.moveToFolder"),
        submenu,
    )]
}

/// `collapseCopyActions`: every `platform.copy` entry moves into one "Copy"
/// submenu placed after the others.
fn collapse_copy_actions(actions: Vec<MenuAction>) -> Vec<MenuAction> {
    let (copy, mut rest): (Vec<MenuAction>, Vec<MenuAction>) = actions
        .into_iter()
        .partition(|action| action.group.as_deref() == Some("platform.copy"));
    if copy.is_empty() {
        return rest;
    }
    let submenu = copy
        .into_iter()
        .map(|mut action| {
            action.group = None;
            action
        })
        .collect();
    rest.push(MenuAction::submenu(
        "platform.copy.root",
        Some("platform.copy"),
        Text::key("context.menu.copy"),
        submenu,
    ));
    rest
}

/// `getRenameAction`: only when the platform can label accounts and the UI
/// offers an input dialog.
fn rename_action(
    account: &PlatformAccount,
    adapter: &PlatformAdapter,
    ctx: &MenuContext<'_>,
) -> Option<MenuAction> {
    if !adapter.supports_account_labels() || !ctx.input_dialog {
        return None;
    }
    Some(MenuAction::item(
        format!("platform.rename.{}", account.id),
        Some("platform.primary"),
        Text::key("platform.rename"),
        MenuCommand::Input(Box::new(InputRequest {
            title: Text::key("platform.renameTitle"),
            placeholder: Text::key("platform.renamePlaceholder"),
            initial_value: account.display_name.clone(),
            allow_empty: true,
            platform_id: adapter.id().to_string(),
            account_id: account.id.clone(),
        })),
    ))
}

/// `buildAccountContextMenuItems`: rename, folder move, the platform's own
/// entries and the appearance submenu, copies collapsed, grouped in
/// [`ACCOUNT_CONTEXT_GROUP_ORDER`]. `folders` is `None` for callers with no
/// folder world.
pub fn build_account_context_menu(
    account: &PlatformAccount,
    adapter: &PlatformAdapter,
    ctx: &MenuContext<'_>,
    appearance: &AppearanceInput,
    folders: Option<&FolderInput>,
) -> Vec<MenuEntry> {
    let mut actions = Vec::new();
    actions.extend(rename_action(account, adapter, ctx));
    if let Some(folders) = folders {
        actions.extend(account_folder_actions(account, folders));
    }
    actions.extend(adapter.context_menu_actions(account, ctx));
    actions.extend(account_appearance_actions(account, appearance));
    build_context_menu_items(collapse_copy_actions(actions), &ACCOUNT_CONTEXT_GROUP_ORDER)
}
