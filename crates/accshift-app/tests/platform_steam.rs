//! Steam: `adapter.test.ts`, `friendCode.test.ts`, `steamIdUtils.test.ts`,
//! `warnings.test.ts`, `steamApi.test.ts` and `shared/url.test.ts` ported,
//! plus checks of the menu and task model.

mod support;

use accshift_app::menu::{MenuActionBody, MenuCommand, PlatformTask, SubmenuLoader, TaskEffect};
use accshift_app::platforms::adapter::{MenuContext, PlatformAdapter};
use accshift_app::platforms::steam::api::ApiKeyMemo;
use accshift_app::platforms::steam::context_menu::steam_context_menu_actions;
use accshift_app::platforms::steam::friend_code::encode_friend_code;
use accshift_app::platforms::steam::warnings::{
    already_checked_ban_ids, load_blocking, AlreadyChecked, BanSession,
};
use accshift_app::platforms::steam::{is_safe_http_url, to_profile_url, to_steam_account};
use accshift_app::platforms::{PlatformAccount, WarningLoadOptions, WarningRefreshOutcome};
use accshift_app::settings::{sanitize_settings, AppSettings};
use accshift_app::storage::{ClientStorage, CLIENT_STORE_STEAM_BAN_CHECK_STATE};
use accshift_app::stores::{ClientStores, DefaultGame, StoreContext};
use accshift_app::text::Text;
use serde_json::{json, Value};

fn settings(ban_check_days: i64) -> AppSettings {
    let mut settings = sanitize_settings(&Value::Null, &StoreContext::shipped());
    settings.data_refresh.ban_check_days = ban_check_days;
    settings
}

fn stores() -> ClientStores {
    ClientStores::new(ClientStorage::new(), StoreContext::shipped())
}

fn account(id: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.to_string(),
        display_name: id.to_string(),
        username: id.to_string(),
        last_login_at_sec: None,
    }
}

fn ban_row(steam_id: &str) -> Value {
    json!({
        "steam_id": steam_id,
        "community_banned": false,
        "vac_banned": false,
        "number_of_vac_bans": 0,
        "days_since_last_ban": 0,
        "number_of_game_bans": 0,
        "economy_ban": "none"
    })
}

const QUIET: WarningLoadOptions = WarningLoadOptions {
    force_refresh: false,
    silent: true,
};
const FORCED: WarningLoadOptions = WarningLoadOptions {
    force_refresh: true,
    silent: true,
};
const NOW_MS: f64 = 1_791_000_000_000.0;

// ------------------------------------------------ adapter.test.ts

#[test]
fn steam_passes_seconds_through() {
    let account = to_steam_account(&json!({
        "steam_id": "76561198000000001", "account_name": "player",
        "persona_name": "Player", "last_login_at": 1_760_000_000u64
    }));
    assert_eq!(account.last_login_at_sec, Some(1_760_000_000.0));
}

#[test]
fn steam_missing_stamp_is_none() {
    let account = to_steam_account(&json!({
        "steam_id": "76561198000000001", "account_name": "player", "persona_name": "Player"
    }));
    assert_eq!(account.last_login_at_sec, None);
}

// ------------------------------------------------ friendCode.test.ts

#[test]
fn friend_code_known_vector() {
    assert_eq!(encode_friend_code("76561197960287930"), "SUCVS-FADA");
}

#[test]
fn friend_code_stable() {
    let id = "76561198123456789";
    assert_eq!(encode_friend_code(id), encode_friend_code(id));
}

#[test]
fn friend_code_non_numeric_is_empty() {
    assert_eq!(encode_friend_code("some-junk-key"), "");
}

#[test]
fn friend_code_empty_is_zero() {
    assert_eq!(encode_friend_code(""), encode_friend_code("0"));
}

#[test]
fn friend_code_decimal_is_empty() {
    assert_eq!(encode_friend_code("1.5"), "");
}

// ------------------------------------------------ steamIdUtils.test.ts

#[test]
fn profile_url_for_steam_id() {
    assert_eq!(
        to_profile_url("76561198123456789"),
        "https://steamcommunity.com/profiles/76561198123456789"
    );
}

#[test]
fn profile_url_for_any_string() {
    assert_eq!(
        to_profile_url("12345"),
        "https://steamcommunity.com/profiles/12345"
    );
}

// ------------------------------------------------ warnings.test.ts

#[test]
fn already_checked_forced_refresh_checks_everything() {
    assert_eq!(
        already_checked_ban_ids(true, 7, true),
        AlreadyChecked::Nothing
    );
    assert_eq!(
        already_checked_ban_ids(true, 0, true),
        AlreadyChecked::Nothing
    );
}

#[test]
fn already_checked_session_without_delay() {
    assert_eq!(
        already_checked_ban_ids(false, 0, true),
        AlreadyChecked::Session
    );
}

#[test]
fn already_checked_cached_inside_window() {
    assert_eq!(
        already_checked_ban_ids(false, 7, true),
        AlreadyChecked::Cached
    );
}

#[test]
fn already_checked_lapsed_window_checks_everything() {
    assert_eq!(
        already_checked_ban_ids(false, 7, false),
        AlreadyChecked::Nothing
    );
}

#[test]
fn bans_asks_again_for_an_omitted_id() {
    let backend = support::scripted("demo");
    let (mut session, mut memo, mut stores) =
        (BanSession::default(), ApiKeyMemo::default(), stores());
    let settings = settings(0);
    let accounts = [account("sess-kept"), account("sess-miss")];
    backend.push("steam_has_api_key", Ok(json!(true)));
    backend.push("steam_get_player_bans", Ok(json!([ban_row("sess-kept")])));
    load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &accounts,
        FORCED,
        NOW_MS,
    );
    backend.clear_log();
    backend.push("steam_get_player_bans", Ok(json!([ban_row("sess-miss")])));
    load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &accounts,
        QUIET,
        NOW_MS,
    );
    assert_eq!(
        backend.calls("steam_get_player_bans"),
        vec!["([\"sess-miss\"],)".to_string()]
    );
}

#[test]
fn bans_omitted_id_not_persisted() {
    let backend = support::scripted("demo");
    let (mut session, mut memo, mut stores) =
        (BanSession::default(), ApiKeyMemo::default(), stores());
    backend.push("steam_has_api_key", Ok(json!(true)));
    backend.push("steam_get_player_bans", Ok(json!([ban_row("disk-kept")])));
    load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings(7),
        &[account("disk-kept"), account("disk-miss")],
        FORCED,
        NOW_MS,
    );
    let persisted = stores
        .storage()
        .get(CLIENT_STORE_STEAM_BAN_CHECK_STATE)
        .cloned()
        .unwrap();
    assert_eq!(persisted["checkedSteamIds"], json!(["disk-kept"]));
}

#[test]
fn bans_report_how_a_load_ended() {
    let backend = support::scripted("demo");
    let settings = settings(0);
    let mut session = BanSession::default();
    let mut stores = stores();

    let mut memo = ApiKeyMemo::default();
    backend.push("steam_has_api_key", Ok(json!(false)));
    let load = load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &[account("out-a")],
        FORCED,
        NOW_MS,
    );
    assert_eq!(load.outcome, Some(WarningRefreshOutcome::NoApiKey));

    // A failed key lookup says nothing about the key: a failure, not a skip.
    let mut memo = ApiKeyMemo::default();
    backend.push("steam_has_api_key", Err("ipc down"));
    let load = load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &[account("out-a")],
        FORCED,
        NOW_MS,
    );
    assert_eq!(load.outcome, Some(WarningRefreshOutcome::Failed));

    let mut memo = ApiKeyMemo::default();
    backend.push("steam_has_api_key", Ok(json!(true)));
    backend.push("steam_get_player_bans", Ok(json!([ban_row("out-a")])));
    let load = load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &[account("out-a"), account("out-b")],
        FORCED,
        NOW_MS,
    );
    assert_eq!(
        load.outcome,
        Some(WarningRefreshOutcome::Partial {
            checked: 1,
            requested: 2
        })
    );

    backend.push("steam_get_player_bans", Err("down"));
    let load = load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &[account("out-a")],
        FORCED,
        NOW_MS,
    );
    assert_eq!(load.outcome, Some(WarningRefreshOutcome::Failed));
}

#[test]
fn bans_stop_after_two_omissions_until_forced() {
    let backend = support::scripted("demo");
    let settings = settings(0);
    let (mut session, mut memo, mut stores) =
        (BanSession::default(), ApiKeyMemo::default(), stores());
    let accounts = [account("gone-id")];
    backend.push("steam_has_api_key", Ok(json!(true)));
    for _ in 0..2 {
        backend.push("steam_get_player_bans", Ok(json!([])));
        load_blocking(
            &mut session,
            &mut memo,
            &backend,
            &mut stores,
            &settings,
            &accounts,
            QUIET,
            NOW_MS,
        );
    }
    backend.clear_log();
    load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &accounts,
        QUIET,
        NOW_MS,
    );
    assert!(backend.calls("steam_get_player_bans").is_empty());

    backend.push("steam_get_player_bans", Ok(json!([])));
    load_blocking(
        &mut session,
        &mut memo,
        &backend,
        &mut stores,
        &settings,
        &accounts,
        FORCED,
        NOW_MS,
    );
    assert_eq!(
        backend.calls("steam_get_player_bans"),
        vec!["([\"gone-id\"],)".to_string()]
    );
}

// ------------------------------------------------ steamApi.test.ts

#[test]
fn api_key_decrypts_once_then_memo() {
    let backend = support::scripted("demo");
    let adapter = PlatformAdapter::shipped("steam").unwrap();
    backend.push("steam_has_api_key", Ok(json!(true)));
    assert!(adapter.steam_has_api_key(&backend).unwrap());
    assert!(adapter.steam_has_api_key(&backend).unwrap());
    assert_eq!(backend.calls("steam_has_api_key").len(), 1);
}

#[test]
fn api_key_set_drops_memo() {
    let backend = support::scripted("demo");
    let adapter = PlatformAdapter::shipped("steam").unwrap();
    backend.push("steam_has_api_key", Ok(json!(false)));
    assert!(!adapter.steam_has_api_key(&backend).unwrap());
    backend.push("steam_set_api_key", Ok(Value::Null));
    adapter.steam_set_api_key(&backend, "new-key").unwrap();
    backend.push("steam_has_api_key", Ok(json!(true)));
    assert!(adapter.steam_has_api_key(&backend).unwrap());
    assert_eq!(backend.calls("steam_has_api_key").len(), 2);
}

#[test]
fn api_key_read_in_flight_during_set_is_dropped() {
    let mut memo = ApiKeyMemo::default();
    let generation = memo.generation();
    memo.invalidate();
    memo.finish_read(generation, false);
    assert_eq!(memo.cached(), None);
    memo.finish_read(memo.generation(), true);
    assert_eq!(memo.cached(), Some(true));
}

#[test]
fn api_key_failed_set_keeps_memo() {
    let backend = support::scripted("demo");
    let adapter = PlatformAdapter::shipped("steam").unwrap();
    backend.push("steam_has_api_key", Ok(json!(true)));
    assert!(adapter.steam_has_api_key(&backend).unwrap());
    backend.push("steam_set_api_key", Err("locked"));
    assert!(adapter.steam_set_api_key(&backend, "x").is_err());
    assert!(adapter.steam_has_api_key(&backend).unwrap());
    assert_eq!(backend.calls("steam_has_api_key").len(), 1);
}

// ------------------------------------------------ url.test.ts

#[test]
fn url_https() {
    assert!(is_safe_http_url("https://example.com"));
    assert!(is_safe_http_url("https://example.com/path?q=1"));
}

#[test]
fn url_http() {
    assert!(is_safe_http_url("http://example.com"));
}

#[test]
fn url_javascript() {
    assert!(!is_safe_http_url("javascript:alert(1)"));
}

#[test]
fn url_data() {
    assert!(!is_safe_http_url("data:text/html,<h1>hi</h1>"));
}

#[test]
fn url_empty() {
    assert!(!is_safe_http_url(""));
}

#[test]
fn url_malformed() {
    assert!(!is_safe_http_url("not a url at all"));
    assert!(!is_safe_http_url("://missing-scheme"));
}

#[test]
fn url_ftp() {
    assert!(!is_safe_http_url("ftp://files.example.com/readme.txt"));
}

// ------------------------------------------------ Rust-only: menu and tasks

fn ids(actions: &[accshift_app::menu::MenuAction]) -> Vec<String> {
    actions.iter().map(|action| action.id.clone()).collect()
}

#[test]
fn steam_menu_order_and_ids() {
    let me = account("7656");
    let game = DefaultGame {
        app_id: "730".to_string(),
        name: "CS2".to_string(),
    };
    let actions = steam_context_menu_actions(&me, Some("other"), Some(&game));
    assert_eq!(
        ids(&actions),
        [
            "steam.launch.online.7656",
            "steam.launch.invisible.7656",
            "steam.switch.run.7656",
            "steam.switch.run.clear.7656",
            "steam.copy.username.7656",
            "steam.copy.id64.7656",
            "steam.copy.friendCode.7656",
            "steam.copy.profileUrl.7656",
            "steam.open.userdata.7656",
            "steam.copy.settings.7656",
            "steam.forget.7656",
        ]
    );
    assert_eq!(
        actions[2].label,
        Text::with("steam.switchAndRunGame", [("game", "CS2")])
    );
    let MenuActionBody::Item { loader, .. } = &actions[9].body else {
        panic!("item");
    };
    assert_eq!(
        loader,
        &Some(SubmenuLoader::SteamCopyableGames {
            from_steam_id: "7656".to_string(),
            to_steam_id: "other".to_string()
        })
    );
}

#[test]
fn steam_menu_current_account() {
    let me = account("7656");
    let actions = steam_context_menu_actions(&me, Some("7656"), None);
    let ids = ids(&actions);
    assert!(ids.contains(&"steam.clear.browserCache.7656".to_string()));
    assert!(!ids.contains(&"steam.copy.settings.7656".to_string()));
    assert!(!ids.contains(&"steam.switch.run.clear.7656".to_string()));
    assert_eq!(actions[2].label, Text::key("steam.runPick"));
    assert_eq!(actions[2].command(), None);
}

#[test]
fn steam_forget_clears_default_game_first() {
    let backend = support::scripted("demo");
    let adapter = PlatformAdapter::shipped("steam").unwrap();
    backend.push("platform_forget_account", Ok(Value::Null));
    let outcome = adapter.run_task(
        &PlatformTask::Forget {
            platform_id: "steam".to_string(),
            account_id: "7656".to_string(),
            toast: Text::key("steam.forgotAccount"),
        },
        &backend,
        &settings(0),
    );
    assert_eq!(
        outcome.effects,
        vec![
            TaskEffect::ClearDefaultGame {
                steam_id: "7656".to_string()
            },
            TaskEffect::Toast(Text::key("steam.forgotAccount")),
            TaskEffect::RemoveAccount("7656".to_string()),
        ]
    );
}

#[test]
fn failed_task_toasts_the_error_but_not_a_pin_refusal() {
    let backend = support::scripted("demo");
    let adapter = PlatformAdapter::shipped("steam").unwrap();
    let task = PlatformTask::SteamOpenUserdata {
        steam_id: "7656".to_string(),
    };
    backend.push("steam_open_userdata", Err("no folder"));
    let outcome = adapter.run_task(&task, &backend, &settings(0));
    assert_eq!(
        outcome.effects,
        vec![TaskEffect::Toast(Text::raw("no folder"))]
    );
    backend.push("steam_open_userdata", Err("pin_locked: locked"));
    assert!(adapter
        .run_task(&task, &backend, &settings(0))
        .effects
        .is_empty());
}

#[test]
fn steam_menu_through_the_adapter_reads_default_game() {
    let mut stores = stores();
    stores.set_default_game(
        "7656",
        &DefaultGame {
            app_id: "730".to_string(),
            name: "CS2".to_string(),
        },
    );
    let adapter = PlatformAdapter::shipped("steam").unwrap();
    let ctx = MenuContext {
        current_account_id: Some("7656"),
        stores: &stores,
        input_dialog: true,
    };
    let actions = adapter.context_menu_actions(&account("7656"), &ctx);
    assert_eq!(
        actions[2].label,
        Text::with("steam.runGame", [("game", "CS2")])
    );
    assert!(matches!(
        actions[2].command(),
        Some(MenuCommand::Task(PlatformTask::SteamRunGame {
            remember: None,
            ..
        }))
    ));
}
