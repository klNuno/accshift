//! `shared/platformPathError.test.ts`, `shared/contextMenu/
//! accountFolderActions.test.ts`, `riot/adapter.test.ts` and
//! `roblox/adapter.test.ts` ported, plus checks of the shared menu model.

mod support;

use accshift_app::i18n::{translate, Locale};
use accshift_app::menu::account::{
    account_folder_actions, build_account_context_menu, AccountFolderChoice, AppearanceInput,
    FolderInput,
};
use accshift_app::menu::{
    build_context_menu_items, MenuAction, MenuCommand, MenuEntry, PlatformTask,
    ACCOUNT_CONTEXT_GROUP_ORDER,
};
use accshift_app::platforms::adapter::{MenuContext, PlatformAdapter};
use accshift_app::platforms::path_error::{parse_platform_path_error, platform_path_error_message};
use accshift_app::platforms::riot::{to_riot_account, RiotProfile};
use accshift_app::platforms::roblox::{is_session_expired_error, to_roblox_account};
use accshift_app::platforms::PlatformAccount;
use accshift_app::storage::ClientStorage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use serde_json::json;

const GENERIC: &str = "Couldn't save the Steam path";

fn en(text: &Text) -> String {
    text.resolve(Locale::En)
}

// ------------------------------------------------ platformPathError.test.ts

#[test]
fn path_error_splits_code_and_fallback() {
    let parsed = parse_platform_path_error(
        "steam_path_never_signed_in|Steam found, sign in once so it creates its login history",
    );
    assert_eq!(parsed.code.as_deref(), Some("steam_path_never_signed_in"));
    assert_eq!(
        parsed.fallback,
        "Steam found, sign in once so it creates its login history"
    );
}

#[test]
fn path_error_plain_message_has_no_code() {
    let parsed = parse_platform_path_error("Could not locate Steam installation");
    assert_eq!(parsed.code, None);
    assert_eq!(parsed.fallback, "Could not locate Steam installation");
}

#[test]
fn path_error_prose_pipe_is_not_a_code() {
    let message = "Could not write config | retry as administrator";
    let parsed = parse_platform_path_error(message);
    assert_eq!(parsed.code, None);
    assert_eq!(parsed.fallback, message);
}

#[test]
fn path_error_non_string_rejection() {
    // `String(new Error(...))` and `undefined`, as the backend hands them.
    assert_eq!(
        parse_platform_path_error("Error: steam_path_not_steam|nope").code,
        None
    );
    let empty = parse_platform_path_error("");
    assert_eq!(empty.code, None);
    assert_eq!(empty.fallback, "");
}

#[test]
fn path_error_never_signed_in_translated() {
    let message = platform_path_error_message(
        "steam_path_never_signed_in|Steam found, sign in once so it creates its login history",
        Text::raw(GENERIC),
    );
    assert_eq!(
        en(&message),
        "Steam found, sign in once so it creates its login history"
    );
}

#[test]
fn path_error_other_codes_translated() {
    assert_eq!(
        en(&platform_path_error_message(
            "steam_path_not_steam|whatever",
            Text::raw(GENERIC)
        )),
        translate(Locale::En, "settings.pathNotSteamFolder")
    );
    assert_eq!(
        en(&platform_path_error_message(
            "steam_path_not_a_directory|whatever",
            Text::raw(GENERIC)
        )),
        translate(Locale::En, "settings.pathNotADirectory")
    );
}

#[test]
fn path_error_unknown_code_is_generic() {
    assert_eq!(
        en(&platform_path_error_message(
            "steam_path_from_the_future|nope",
            Text::raw(GENERIC)
        )),
        GENERIC
    );
}

#[test]
fn path_error_uncoded_failure_is_generic() {
    assert_eq!(
        en(&platform_path_error_message(
            "disk full",
            Text::raw(GENERIC)
        )),
        GENERIC
    );
}

// ------------------------------------------------ accountFolderActions.test.ts

fn acc1() -> PlatformAccount {
    PlatformAccount {
        id: "acc-1".to_string(),
        display_name: "Main".to_string(),
        username: "main".to_string(),
        last_login_at_sec: None,
    }
}

fn folders(current: Option<&str>, list: &[(&str, &str)]) -> FolderInput {
    FolderInput {
        folders: list
            .iter()
            .map(|(id, label)| AccountFolderChoice {
                id: id.to_string(),
                label: label.to_string(),
            })
            .collect(),
        current_folder_id: current.map(str::to_string),
    }
}

const TWO: [(&str, &str); 2] = [("f1", "Smurfs"), ("f2", "Smurfs / Old")];

fn labels(actions: &[MenuAction]) -> Vec<String> {
    actions[0]
        .submenu_actions()
        .unwrap()
        .iter()
        .map(|action| match &action.label {
            Text::Raw(raw) => raw.clone(),
            Text::Key { key, .. } => key.clone(),
        })
        .collect()
}

#[test]
fn folder_lists_every_folder_at_root() {
    let actions = account_folder_actions(&acc1(), &folders(None, &TWO));
    assert_eq!(actions.len(), 1);
    assert_eq!(labels(&actions), ["Smurfs", "Smurfs / Old"]);
}

#[test]
fn folder_offers_root_and_hides_current() {
    let actions = account_folder_actions(&acc1(), &folders(Some("f1"), &TWO));
    assert_eq!(
        labels(&actions),
        ["context.menu.moveToRoot", "Smurfs / Old"]
    );
}

#[test]
fn folder_nothing_without_folders_at_root() {
    assert!(account_folder_actions(&acc1(), &folders(None, &[])).is_empty());
}

#[test]
fn folder_root_is_only_way_out() {
    let actions = account_folder_actions(&acc1(), &folders(Some("f1"), &[("f1", "Smurfs")]));
    assert_eq!(labels(&actions), ["context.menu.moveToRoot"]);
}

#[test]
fn folder_moves_to_picked_and_root() {
    let actions = account_folder_actions(&acc1(), &folders(Some("f1"), &TWO));
    let submenu = actions[0].submenu_actions().unwrap();
    assert_eq!(
        submenu[0].command(),
        Some(&MenuCommand::MoveToFolder {
            account_id: "acc-1".to_string(),
            folder_id: None
        })
    );
    assert_eq!(
        submenu[1].command(),
        Some(&MenuCommand::MoveToFolder {
            account_id: "acc-1".to_string(),
            folder_id: Some("f2".to_string())
        })
    );
}

// ------------------------------------------------ riot/adapter.test.ts

fn riot(extra: serde_json::Value) -> RiotProfile {
    let mut base = json!({ "id": "riot-1", "label": "main", "snapshot_state": "ready" });
    for (key, value) in extra.as_object().unwrap() {
        base[key] = value.clone();
    }
    RiotProfile::from_value(&base).unwrap()
}

#[test]
fn riot_converts_ms_to_seconds() {
    let account = to_riot_account(&riot(json!({ "last_used_at": 1_760_000_000_999u64 })));
    assert_eq!(account.last_login_at_sec, Some(1_760_000_000.0));
}

#[test]
fn riot_falls_back_to_capture_stamp() {
    let account = to_riot_account(&riot(json!({ "last_captured_at": 1_759_000_000_500u64 })));
    assert_eq!(account.last_login_at_sec, Some(1_759_000_000.0));
}

#[test]
fn riot_missing_stamp_is_none() {
    assert_eq!(to_riot_account(&riot(json!({}))).last_login_at_sec, None);
}

// ------------------------------------------------ roblox/adapter.test.ts

#[test]
fn roblox_converts_ms_to_seconds() {
    let account = to_roblox_account(&json!({
        "userId": "1234", "username": "player", "displayName": "Player",
        "lastLoginAt": 1_760_000_000_999u64
    }));
    assert_eq!(account.last_login_at_sec, Some(1_760_000_000.0));
}

#[test]
fn roblox_missing_stamp_is_none() {
    let account = to_roblox_account(
        &json!({ "userId": "1234", "username": "player", "displayName": "Player" }),
    );
    assert_eq!(account.last_login_at_sec, None);
}

// ------------------------------------------------ Rust-only: shared menu model

#[test]
fn riot_alias_and_status_labels() {
    let pending = to_riot_account(&riot(json!({
        "snapshot_state": "setup_pending", "account_name": "Nova", "account_tag_line": "EUW"
    })));
    assert_eq!(pending.display_name, "Nova#EUW");
    assert_eq!(pending.username, "main \u{b7} Waiting for connection");
    let awaiting = to_riot_account(&riot(json!({ "snapshot_state": "awaiting_capture" })));
    assert_eq!(awaiting.display_name, "main");
    assert_eq!(awaiting.username, "Capture required");
}

#[test]
fn roblox_session_expiry_pattern() {
    assert!(is_session_expired_error(
        "Auth ticket request failed (HTTP 401 Unauthorized)"
    ));
    assert!(!is_session_expired_error(
        "auth ticket request failed (http 500"
    ));
}

#[test]
fn groups_sort_and_separate() {
    let close = || MenuCommand::Close;
    let actions = vec![
        MenuAction::item("a", Some("platform.danger"), Text::key("a"), close()),
        MenuAction::item("b", Some("custom"), Text::key("b"), close()),
        MenuAction::item("c", Some("platform.primary"), Text::key("c"), close()),
        MenuAction::item("d", None, Text::key("d"), close()),
        MenuAction::item("e", Some("platform.danger"), Text::key("e"), close()),
    ];
    let entries = build_context_menu_items(actions, &ACCOUNT_CONTEXT_GROUP_ORDER);
    let shape: Vec<&str> = entries
        .iter()
        .map(|entry| entry.id().unwrap_or("|"))
        .collect();
    assert_eq!(shape, ["c", "|", "a", "e", "|", "b", "|", "d"]);
    assert!(entries
        .iter()
        .filter(|e| !matches!(e, MenuEntry::Separator))
        .all(MenuEntry::is_enabled));
}

#[test]
fn account_menu_rename_copy_and_appearance() {
    let stores = ClientStores::new(ClientStorage::new(), StoreContext::shipped());
    let ctx = MenuContext {
        current_account_id: None,
        stores: &stores,
        input_dialog: true,
    };
    let gog = PlatformAdapter::shipped("gog").unwrap();
    let account = PlatformAccount {
        id: "g1".to_string(),
        display_name: "Gamer".to_string(),
        username: "g1".to_string(),
        last_login_at_sec: None,
    };
    let entries = build_account_context_menu(
        &account,
        &gog,
        &ctx,
        &AppearanceInput::default(),
        Some(&folders(None, &TWO)),
    );
    let ids: Vec<&str> = entries.iter().map(|e| e.id().unwrap_or("|")).collect();
    assert_eq!(
        ids,
        [
            "platform.rename.g1",
            "|",
            "account.folder.g1",
            "|",
            "platform.copy.root",
            "|",
            "gog.forget.g1",
            "|",
            "account.appearance.g1",
        ]
    );
    let MenuEntry::Item { command, .. } = &entries[0] else {
        panic!("rename");
    };
    let Some(MenuCommand::Input(request)) = command else {
        panic!("input");
    };
    assert_eq!(
        request.confirm("New"),
        PlatformTask::SetAccountLabel {
            platform_id: "gog".to_string(),
            account_id: "g1".to_string(),
            label: "New".to_string()
        }
    );
    // Battle.net has no labels: no rename entry.
    let bnet = PlatformAdapter::shipped("battle-net").unwrap();
    let entries =
        build_account_context_menu(&account, &bnet, &ctx, &AppearanceInput::default(), None);
    assert_ne!(entries[0].id(), Some("platform.rename.g1"));
}
