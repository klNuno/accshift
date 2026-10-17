//! The context menu model, ported from `src/lib/shared/contextMenu/*.ts` and
//! the `ContextMenuItem` type of `src/lib/shared/types.ts`.
//!
//! TS entries carry closures. Here an entry carries a [`MenuCommand`]: data
//! describing what a click does. The UI applies the UI-only commands itself
//! (copy, confirm, input dialog, folder move, note, colour) and hands a
//! [`PlatformTask`] to [`crate::platforms::PlatformAdapter::run_task`] on the
//! job pool, then applies the [`TaskEffect`]s it returns. Drawing the menu is
//! the UI's job; ids, grouping, order, separators, labels (as [`Text`]) and
//! enabled state are decided here.

pub mod account;
pub mod actions;
pub mod platform;

use crate::stores::DefaultGame;
use crate::text::Text;

pub use actions::{TaskEffect, TaskOutcome};

/// `ACCOUNT_CONTEXT_GROUP_ORDER`.
pub const ACCOUNT_CONTEXT_GROUP_ORDER: [&str; 6] = [
    "platform.primary",
    "account.folder",
    "platform.copy",
    "platform.data",
    "platform.danger",
    "account.appearance",
];

/// What clicking an entry asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuCommand {
    /// Close the menu and do nothing (`action: () => {}`).
    Close,
    /// `copyToClipboard(value, label)`.
    Copy { value: String, label: Text },
    /// `confirmAction`: on confirm, run the task as a safe action.
    Confirm(Box<ConfirmRequest>),
    /// `openInputDialog` (rename).
    Input(Box<InputRequest>),
    /// `moveToFolder`: `None` is the platform root.
    MoveToFolder {
        account_id: String,
        folder_id: Option<String>,
    },
    /// `openNoteEditor(initialNote)`.
    EditNote {
        account_id: String,
        initial_note: String,
    },
    /// `setColor(color)`: a preset colour, `""` clears.
    SetCardColor { account_id: String, color: String },
    /// A safe action (`createSafeContextAction`) that talks to the backend.
    Task(PlatformTask),
}

/// `PlatformContextMenuConfirmConfig`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmRequest {
    pub title: Text,
    pub message: Text,
    pub confirm_label: Option<Text>,
    pub on_confirm: PlatformTask,
}

/// `PlatformContextMenuInputConfig`. The confirmed value goes into
/// [`PlatformTask::SetAccountLabel`].
#[derive(Debug, Clone, PartialEq)]
pub struct InputRequest {
    pub title: Text,
    pub placeholder: Text,
    pub initial_value: String,
    pub allow_empty: bool,
    pub platform_id: String,
    pub account_id: String,
}

impl InputRequest {
    /// The task to run with the value the user confirmed.
    pub fn confirm(&self, value: &str) -> PlatformTask {
        PlatformTask::SetAccountLabel {
            platform_id: self.platform_id.clone(),
            account_id: self.account_id.clone(),
            label: value.to_string(),
        }
    }
}

/// Work a menu entry hands to the job pool. Each variant is one TS closure.
#[derive(Debug, Clone, PartialEq)]
pub enum PlatformTask {
    /// `switchAccountMode(username, steamId, mode)`: `online` or `invisible`.
    SteamSwitchMode {
        username: String,
        steam_id: String,
        mode: String,
    },
    /// `switchAccountAndLaunchGame`, after remembering the game as the
    /// account's default when `remember` is set.
    SteamRunGame {
        username: String,
        steam_id: String,
        app_id: String,
        remember: Option<DefaultGame>,
    },
    /// `clearDefaultGame`.
    SteamClearDefaultGame { steam_id: String },
    /// `openUserdata`.
    SteamOpenUserdata { steam_id: String },
    /// `clearIntegratedBrowserCache` then its toast.
    SteamClearBrowserCache,
    /// `copyGameSettings` then its toast.
    SteamCopyGameSettings {
        from_steam_id: String,
        to_steam_id: String,
        app_id: String,
        game_name: String,
    },
    /// `captureProfile`, its toast and a reload.
    RiotCapture {
        profile_id: String,
        display_name: String,
    },
    /// The forget entry of `buildPlatformContextMenu`: forget, toast, remove.
    Forget {
        platform_id: String,
        account_id: String,
        toast: Text,
    },
    /// `setAccountLabel` then a reload (rename).
    SetAccountLabel {
        platform_id: String,
        account_id: String,
        label: String,
    },
}

/// A submenu filled on open (`submenuLoader`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmenuLoader {
    /// The account's games, to switch and run one.
    SteamAccountGames { username: String, steam_id: String },
    /// Games whose settings can be copied to the current account.
    SteamCopyableGames {
        from_steam_id: String,
        to_steam_id: String,
    },
}

/// `ContextMenuSwatchAction`.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuSwatch {
    pub id: String,
    pub label: Text,
    pub color: String,
    pub active: bool,
    pub command: MenuCommand,
}

/// `ContextMenuAction`: an entry before grouping.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuAction {
    pub id: String,
    pub group: Option<String>,
    pub label: Text,
    pub body: MenuActionBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuActionBody {
    /// `ContextMenuItemAction`.
    Item {
        command: Option<MenuCommand>,
        submenu: Option<Vec<MenuAction>>,
        loader: Option<SubmenuLoader>,
    },
    /// `ContextMenuSwatchesAction`.
    Swatches(Vec<MenuSwatch>),
}

impl MenuAction {
    /// An entry that runs `command`.
    pub fn item(
        id: impl Into<String>,
        group: Option<&str>,
        label: Text,
        command: MenuCommand,
    ) -> Self {
        Self {
            id: id.into(),
            group: group.map(str::to_string),
            label,
            body: MenuActionBody::Item {
                command: Some(command),
                submenu: None,
                loader: None,
            },
        }
    }

    /// An entry that opens `submenu`.
    pub fn submenu(
        id: impl Into<String>,
        group: Option<&str>,
        label: Text,
        submenu: Vec<MenuAction>,
    ) -> Self {
        Self {
            id: id.into(),
            group: group.map(str::to_string),
            label,
            body: MenuActionBody::Item {
                command: None,
                submenu: Some(submenu),
                loader: None,
            },
        }
    }

    /// The click command of an item entry.
    pub fn command(&self) -> Option<&MenuCommand> {
        match &self.body {
            MenuActionBody::Item { command, .. } => command.as_ref(),
            MenuActionBody::Swatches(_) => None,
        }
    }

    /// The static submenu of an item entry.
    pub fn submenu_actions(&self) -> Option<&[MenuAction]> {
        match &self.body {
            MenuActionBody::Item { submenu, .. } => submenu.as_deref(),
            MenuActionBody::Swatches(_) => None,
        }
    }
}

/// `ContextMenuItem`: what the menu draws, in order, separators included.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuEntry {
    Separator,
    Item {
        id: String,
        label: Text,
        command: Option<MenuCommand>,
        submenu: Option<Vec<MenuEntry>>,
        loader: Option<SubmenuLoader>,
    },
    Swatches {
        id: String,
        label: Text,
        swatches: Vec<MenuSwatch>,
    },
}

impl MenuEntry {
    /// An entry with something behind it. TS has no disabled state; an item
    /// with no action and no submenu does nothing when clicked.
    pub fn is_enabled(&self) -> bool {
        match self {
            MenuEntry::Separator => false,
            MenuEntry::Item {
                command,
                submenu,
                loader,
                ..
            } => command.is_some() || submenu.is_some() || loader.is_some(),
            MenuEntry::Swatches { swatches, .. } => !swatches.is_empty(),
        }
    }

    pub fn label(&self) -> Option<&Text> {
        match self {
            MenuEntry::Separator => None,
            MenuEntry::Item { label, .. } | MenuEntry::Swatches { label, .. } => Some(label),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            MenuEntry::Separator => None,
            MenuEntry::Item { id, .. } | MenuEntry::Swatches { id, .. } => Some(id),
        }
    }
}

fn to_entry(action: MenuAction, group_order: &[&str]) -> MenuEntry {
    match action.body {
        MenuActionBody::Swatches(swatches) => MenuEntry::Swatches {
            id: action.id,
            label: action.label,
            swatches,
        },
        MenuActionBody::Item {
            command,
            submenu,
            loader,
        } => MenuEntry::Item {
            id: action.id,
            label: action.label,
            command,
            submenu: submenu.map(|actions| build_context_menu_items(actions, group_order)),
            loader,
        },
    }
}

/// `buildContextMenuItems`: actions bucketed by group, groups in
/// `group_order` first (in that order) then by first appearance, a separator
/// between groups. Loaded submenus go through this again with the same order.
pub fn build_context_menu_items(actions: Vec<MenuAction>, group_order: &[&str]) -> Vec<MenuEntry> {
    if actions.is_empty() {
        return Vec::new();
    }
    // (group key, first index, actions), in first-seen order.
    let mut buckets: Vec<(String, usize, Vec<MenuAction>)> = Vec::new();
    for (index, action) in actions.into_iter().enumerate() {
        let key = action
            .group
            .clone()
            .filter(|group| !group.is_empty())
            .unwrap_or_else(|| "__default__".to_string());
        match buckets.iter_mut().find(|(bucket, _, _)| *bucket == key) {
            Some((_, _, list)) => list.push(action),
            None => buckets.push((key, index, vec![action])),
        }
    }
    let priority = |key: &str| group_order.iter().position(|group| *group == key);
    buckets.sort_by(
        |(a, a_first, _), (b, b_first, _)| match (priority(a), priority(b)) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a_first.cmp(b_first),
        },
    );
    let mut items = Vec::new();
    for (group_index, (_, _, group_actions)) in buckets.into_iter().enumerate() {
        if group_actions.is_empty() {
            continue;
        }
        if group_index > 0 {
            items.push(MenuEntry::Separator);
        }
        items.extend(
            group_actions
                .into_iter()
                .map(|action| to_entry(action, group_order)),
        );
    }
    items
}
