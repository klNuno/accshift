//! Riot: the "switch and run a game" menu entries, the installed games
//! submenu and the task, on the mock and a scripted backend. Nothing here
//! reaches a Riot client.

mod support;

use accshift_app::menu::{MenuActionBody, MenuCommand, PlatformTask, SubmenuLoader, TaskEffect};
use accshift_app::platforms::adapter::PlatformAdapter;
use accshift_app::platforms::riot::{
    installed_games_submenu, riot_context_menu_actions, RiotProfile,
};
use accshift_app::platforms::PlatformAccount;
use accshift_app::settings::{sanitize_settings, AppSettings};
use accshift_app::stores::StoreContext;
use accshift_app::text::Text;
use accshift_app::types::RiotGame;
use accshift_app::Backend;
use serde_json::Value;

fn settings() -> AppSettings {
    sanitize_settings(&Value::Null, &StoreContext::shipped())
}

fn account(id: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.to_string(),
        display_name: format!("name-{id}"),
        username: String::new(),
        last_login_at_sec: None,
    }
}

fn profile(id: &str, state: &str) -> RiotProfile {
    RiotProfile {
        id: id.to_string(),
        label: id.to_string(),
        snapshot_state: state.to_string(),
        ..Default::default()
    }
}

fn game(product_id: &str, name: &str) -> RiotGame {
    RiotGame {
        product_id: product_id.to_string(),
        patchline: "live".to_string(),
        name: name.to_string(),
    }
}

fn ids(actions: &[accshift_app::menu::MenuAction]) -> Vec<String> {
    actions.iter().map(|action| action.id.clone()).collect()
}

fn run_task(profile_id: &str, product_id: &str) -> PlatformTask {
    PlatformTask::RiotRunGame {
        profile_id: profile_id.to_string(),
        product_id: product_id.to_string(),
    }
}

#[test]
fn a_ready_profile_offers_switch_and_run_first() {
    let me = account("riot-2");
    let actions = riot_context_menu_actions(&me, Some(&profile("riot-2", "ready")), Some("riot-1"));
    assert_eq!(
        ids(&actions),
        [
            "riot.switch.run.riot-2",
            "riot.capture.riot-2",
            "riot.forget.riot-2"
        ]
    );
    let run = &actions[0];
    assert_eq!(run.group.as_deref(), Some("platform.primary"));
    assert_eq!(run.label, Text::key("riot.switchAndRunPick"));
    assert_eq!(run.command(), None);
    let MenuActionBody::Item { loader, .. } = &run.body else {
        panic!("item");
    };
    assert_eq!(
        loader,
        &Some(SubmenuLoader::RiotInstalledGames {
            profile_id: "riot-2".to_string()
        })
    );

    // On the current profile the entry only runs a game.
    let current = riot_context_menu_actions(&me, Some(&profile("riot-2", "ready")), Some("riot-2"));
    assert_eq!(current[0].label, Text::key("riot.runPick"));
}

#[test]
fn a_profile_without_a_saved_session_has_no_run_entry() {
    let me = account("riot-3");
    for state in ["awaiting_capture", "capturing", "setup_pending"] {
        let actions = riot_context_menu_actions(&me, Some(&profile("riot-3", state)), None);
        assert_eq!(
            ids(&actions),
            ["riot.capture.riot-3", "riot.forget.riot-3"],
            "{state}"
        );
    }
    let unknown = riot_context_menu_actions(&me, None, None);
    assert_eq!(ids(&unknown), ["riot.capture.riot-3", "riot.forget.riot-3"]);
}

#[test]
fn the_games_submenu_runs_each_installed_game_or_says_none() {
    let games = [
        game("valorant", "VALORANT"),
        game("league_of_legends", "League of Legends"),
    ];
    let items = installed_games_submenu("riot-2", &games);
    assert_eq!(
        ids(&items),
        [
            "riot.switch.run.riot-2.valorant",
            "riot.switch.run.riot-2.league_of_legends"
        ]
    );
    assert_eq!(items[0].label, Text::raw("VALORANT"));
    assert_eq!(
        items[1].command(),
        Some(&MenuCommand::Task(run_task("riot-2", "league_of_legends")))
    );

    let empty = installed_games_submenu("riot-2", &[]);
    assert_eq!(ids(&empty), ["riot.switch.run.riot-2.empty"]);
    assert_eq!(empty[0].label, Text::key("riot.noInstalledGames"));
    assert_eq!(empty[0].command(), Some(&MenuCommand::Close));
}

#[test]
fn the_adapter_loads_the_installed_games_from_the_backend() {
    let mock = support::mock("dev");
    let adapter = PlatformAdapter::shipped("riot").unwrap();
    let items = adapter
        .load_submenu(
            &SubmenuLoader::RiotInstalledGames {
                profile_id: "riot-1".to_string(),
            },
            &mock,
        )
        .unwrap();
    assert_eq!(
        ids(&items),
        [
            "riot.switch.run.riot-1.valorant",
            "riot.switch.run.riot-1.league_of_legends"
        ]
    );
    assert_eq!(mock.seen("riot_get_installed_games"), 1);

    // A scenario without the command refuses it rather than reaching Riot.
    let guarded = support::mock("empty");
    assert!(adapter
        .load_submenu(
            &SubmenuLoader::RiotInstalledGames {
                profile_id: "riot-1".to_string(),
            },
            &guarded,
        )
        .is_err());
}

#[test]
fn running_a_game_switches_in_the_mock_and_reloads() {
    let mock = support::mock("dev");
    let adapter = PlatformAdapter::shipped("riot").unwrap();
    assert_eq!(mock.platform_get_current_account("riot").unwrap(), "riot-1");
    let outcome = adapter.run_task(&run_task("riot-3", "valorant"), &mock, &settings());
    assert_eq!(outcome.effects, vec![TaskEffect::RefreshAccounts]);
    assert_eq!(mock.seen("riot_switch_profile_and_launch_game"), 1);
    assert_eq!(mock.platform_get_current_account("riot").unwrap(), "riot-3");
}

#[test]
fn running_a_game_is_logged_and_passes_the_product() {
    let backend = support::scripted("dev");
    let adapter = PlatformAdapter::shipped("riot").unwrap();
    backend.push("riot_switch_profile_and_launch_game", Ok(Value::Null));
    adapter.run_task(&run_task("riot-2", "valorant"), &backend, &settings());
    assert_eq!(
        backend.calls("riot_switch_profile_and_launch_game"),
        [format!("{:?}", (&"riot-2", &"valorant"))]
    );
    let logs = backend.calls("log_app_event");
    assert_eq!(logs.len(), 2, "{logs:?}");
    assert!(logs
        .iter()
        .all(|log| log.contains("frontend.riot.switch_and_launch_game")));
    assert!(
        logs[1].contains("Switch+launch request completed"),
        "{logs:?}"
    );
}

#[test]
fn a_failed_run_toasts_the_error_logs_it_and_does_not_reload() {
    let backend = support::scripted("dev");
    let adapter = PlatformAdapter::shipped("riot").unwrap();
    backend.push(
        "riot_switch_profile_and_launch_game",
        Err("VALORANT is not installed"),
    );
    let outcome = adapter.run_task(&run_task("riot-2", "valorant"), &backend, &settings());
    assert_eq!(
        outcome.effects,
        vec![TaskEffect::Toast(Text::raw("VALORANT is not installed"))]
    );
    let logs = backend.calls("log_app_event");
    assert!(
        logs.last()
            .is_some_and(|log| log.contains("Switch+launch failed")
                && log.contains("VALORANT is not installed")),
        "{logs:?}"
    );

    // Behind the PIN: no toast over the lock screen.
    backend.push(
        "riot_switch_profile_and_launch_game",
        Err("pin_locked: locked"),
    );
    let locked = adapter.run_task(&run_task("riot-2", "valorant"), &backend, &settings());
    assert!(
        locked.toasts().is_empty() && !locked.effects.contains(&TaskEffect::RefreshAccounts),
        "{:?}",
        locked.effects
    );
}

#[test]
fn an_unconfirmed_account_toasts_its_own_text_and_does_not_reload() {
    let backend = support::scripted("dev");
    let adapter = PlatformAdapter::shipped("riot").unwrap();
    // The core's wording when it kept a session it could not attribute.
    backend.push(
        "riot_switch_profile_and_launch_game",
        Err("Riot account not confirmed: Riot Client was opened without the game. Launch the game again once Riot Client shows the signed-in account."),
    );
    let outcome = adapter.run_task(&run_task("riot-2", "valorant"), &backend, &settings());
    assert_eq!(
        outcome.effects,
        vec![TaskEffect::Toast(Text::key("riot.accountNotConfirmed"))]
    );
    assert!(
        backend
            .calls("log_app_event")
            .last()
            .is_some_and(|log| log.contains("Switch+launch failed")),
        "the refusal was not logged"
    );
}

#[test]
fn the_mock_refuses_the_run_behind_its_switch_failure() {
    let mock = support::mock("dev");
    mock.fail(
        "riot_switch_profile_and_launch_game",
        Some("scripted failure"),
    );
    let error = mock
        .riot_switch_profile_and_launch_game("riot-3", "valorant")
        .unwrap_err();
    assert_eq!(error.message, "scripted failure");
    assert_eq!(mock.platform_get_current_account("riot").unwrap(), "riot-1");
}
