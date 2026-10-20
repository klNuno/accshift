//! Steam account menu entries, ported from
//! `src/lib/platforms/steam/contextMenu.ts`.

use super::friend_code::encode_friend_code;
use super::to_profile_url;
use crate::menu::platform::{build_platform_context_menu, CopyItemConfig, ForgetConfig};
use crate::menu::{
    ConfirmRequest, MenuAction, MenuActionBody, MenuCommand, PlatformTask, SubmenuLoader,
};
use crate::platforms::PlatformAccount;
use crate::stores::DefaultGame;
use crate::text::Text;
use crate::types::CopyableGame;

fn loader_item(
    id: String,
    group: &str,
    label: Text,
    command: Option<MenuCommand>,
    loader: SubmenuLoader,
) -> MenuAction {
    MenuAction {
        id,
        group: Some(group.to_string()),
        label,
        body: MenuActionBody::Item {
            command,
            submenu: None,
            loader: Some(loader),
        },
    }
}

/// `(account.displayName || account.username).trim() || account.username`.
fn display_value(account: &PlatformAccount) -> String {
    let base = if account.display_name.is_empty() {
        &account.username
    } else {
        &account.display_name
    };
    let trimmed = crate::js::trim(base);
    if trimmed.is_empty() {
        account.username.clone()
    } else {
        trimmed.to_string()
    }
}

/// `getSteamContextMenuItems`: launch entries, copies, data entries, forget.
/// `default_game` is `getDefaultGame(account.id)`.
pub fn steam_context_menu_actions(
    account: &PlatformAccount,
    current_account_id: Option<&str>,
    default_game: Option<&DefaultGame>,
) -> Vec<MenuAction> {
    let id = &account.id;
    let is_current = current_account_id == Some(id.as_str());
    let mode = |mode: &str| {
        MenuCommand::Task(PlatformTask::SteamSwitchMode {
            username: account.username.clone(),
            steam_id: id.clone(),
            mode: mode.to_string(),
        })
    };
    let run_label = match default_game {
        Some(game) => Text::with(
            if is_current {
                "steam.runGame"
            } else {
                "steam.switchAndRunGame"
            },
            [("game", game.name.as_str())],
        ),
        None => Text::key(if is_current {
            "steam.runPick"
        } else {
            "steam.switchAndRunPick"
        }),
    };
    // Always the combined backend command: it checks the live Steam state and
    // skips the restart when the account is already logged in.
    let run_command = default_game.map(|game| {
        MenuCommand::Task(PlatformTask::SteamRunGame {
            username: account.username.clone(),
            steam_id: id.clone(),
            app_id: game.app_id.clone(),
            remember: None,
        })
    });
    let mut actions = vec![
        MenuAction::item(
            format!("steam.launch.online.{id}"),
            Some("platform.primary"),
            Text::key("steam.launchOnline"),
            mode("online"),
        ),
        MenuAction::item(
            format!("steam.launch.invisible.{id}"),
            Some("platform.primary"),
            Text::key("steam.launchInvisible"),
            mode("invisible"),
        ),
        loader_item(
            format!("steam.switch.run.{id}"),
            "platform.primary",
            run_label,
            run_command,
            SubmenuLoader::SteamAccountGames {
                username: account.username.clone(),
                steam_id: id.clone(),
            },
        ),
    ];
    if default_game.is_some() {
        actions.push(MenuAction::item(
            format!("steam.switch.run.clear.{id}"),
            Some("platform.data"),
            Text::key("steam.clearDefaultGame"),
            MenuCommand::Task(PlatformTask::SteamClearDefaultGame {
                steam_id: id.clone(),
            }),
        ));
    }

    let copy_items = [
        CopyItemConfig::new(
            "username",
            account.username.clone(),
            "steam.copyLabelUsername",
        ),
        CopyItemConfig::new("id64", id.clone(), "steam.copyLabelSteamId64"),
        CopyItemConfig::new(
            "friendCode",
            encode_friend_code(id),
            "steam.copyLabelFriendCode",
        ),
        CopyItemConfig::new(
            "profileUrl",
            to_profile_url(id),
            "steam.copyLabelProfileUrl",
        ),
    ];
    let mut forget = ForgetConfig::keys(
        "steam.forgetConfirmTitle",
        "steam.forgetConfirmMessage",
        "steam.forget",
        "steam.forgotAccount",
    );
    forget.toast_params = vec![("username".to_string(), account.username.as_str().into())];
    let copy_and_forget = build_platform_context_menu(
        "steam",
        account,
        &copy_items,
        &forget,
        Some(display_value(account)),
    );
    let (copies, forget_items): (Vec<MenuAction>, Vec<MenuAction>) = copy_and_forget
        .into_iter()
        .filter(|action| {
            matches!(
                action.group.as_deref(),
                Some("platform.copy" | "platform.danger")
            )
        })
        .partition(|action| action.group.as_deref() == Some("platform.copy"));

    let mut data = vec![MenuAction::item(
        format!("steam.open.userdata.{id}"),
        Some("platform.data"),
        Text::key("steam.openUserdataFolder"),
        MenuCommand::Task(PlatformTask::SteamOpenUserdata {
            steam_id: id.clone(),
        }),
    )];
    if is_current {
        data.push(MenuAction::item(
            format!("steam.clear.browserCache.{id}"),
            Some("platform.data"),
            Text::key("steam.clearIntegratedBrowserCache"),
            MenuCommand::Confirm(Box::new(ConfirmRequest {
                title: Text::key("steam.clearIntegratedBrowserCacheConfirmTitle"),
                message: Text::key("steam.clearIntegratedBrowserCacheConfirmMessage"),
                confirm_label: Some(Text::key("steam.clearIntegratedBrowserCache")),
                on_confirm: PlatformTask::SteamClearBrowserCache,
            })),
        ));
    }
    if let Some(target) = current_account_id.filter(|target| !target.is_empty() && *target != id) {
        data.push(loader_item(
            format!("steam.copy.settings.{id}"),
            "platform.data",
            Text::key("steam.copySettingsFrom"),
            None,
            SubmenuLoader::SteamCopyableGames {
                from_steam_id: id.clone(),
                to_steam_id: target.to_string(),
            },
        ));
    }

    actions.extend(copies);
    actions.extend(data);
    actions.extend(forget_items);
    actions
}

/// The `submenuLoader` of the run entry, once the account's games are in.
/// `remember` sets the picked game as the account's default before running.
pub fn account_games_submenu(
    username: &str,
    steam_id: &str,
    games: &[CopyableGame],
) -> Vec<MenuAction> {
    if games.is_empty() {
        return vec![MenuAction::item(
            format!("steam.switch.run.{steam_id}.empty"),
            None,
            Text::key("steam.switchAndRunNoGames"),
            MenuCommand::Close,
        )];
    }
    games
        .iter()
        .map(|game| {
            MenuAction::item(
                format!("steam.switch.run.{steam_id}.{}", game.app_id),
                None,
                Text::raw(game.name.clone()),
                MenuCommand::Task(PlatformTask::SteamRunGame {
                    username: username.to_string(),
                    steam_id: steam_id.to_string(),
                    app_id: game.app_id.clone(),
                    remember: Some(DefaultGame {
                        app_id: game.app_id.clone(),
                        name: game.name.clone(),
                    }),
                }),
            )
        })
        .collect()
}

/// The `submenuLoader` of "copy settings from", once the games are in.
pub fn copyable_games_submenu(
    from_steam_id: &str,
    to_steam_id: &str,
    games: &[CopyableGame],
) -> Vec<MenuAction> {
    games
        .iter()
        .map(|game| {
            MenuAction::item(
                format!("steam.copy.settings.{from_steam_id}.{}", game.app_id),
                None,
                Text::raw(game.name.clone()),
                MenuCommand::Task(PlatformTask::SteamCopyGameSettings {
                    from_steam_id: from_steam_id.to_string(),
                    to_steam_id: to_steam_id.to_string(),
                    app_id: game.app_id.clone(),
                    game_name: game.name.clone(),
                }),
            )
        })
        .collect()
}
