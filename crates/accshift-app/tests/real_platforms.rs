//! `RealBackend` part 2 on scratch app directories: platform commands,
//! Steam on a fixture folder, the operation lock, Riot and Roblox argument
//! checks. Nothing here switches an account, stops Steam, writes the
//! keyring or the registry, or opens a network connection: the Steam folder
//! is a fixture set as the path override (so no registry lookup), and every
//! call that would reach a server or a secret is stopped by its argument
//! check first.

use accshift_app::dispatch::dispatch;
use accshift_app::real::{RealBackend, LOCK_TIMEOUT, PORTED};
use accshift_app::{Backend, COMMANDS};
use accshift_core::pin::{hash_pin_code, PIN_LOCKED_MESSAGE};
use accshift_core::storage::STORE_SETTINGS;
use accshift_core::AppContext;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Arc<Self> {
        let root =
            std::env::temp_dir().join(format!("accshift-app-real2-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Arc::new(Self { root })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl AppContext for Scratch {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("config"))
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("data"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("cache"))
    }
}

fn backend(tag: &str) -> (Arc<Scratch>, RealBackend) {
    let scratch = Scratch::new(tag);
    let real = RealBackend::with_context(scratch.clone());
    (scratch, real)
}

/// The descriptor tests swap the process-wide user platform registry.
fn registry_turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

const ALPHA: &str = "76561198000000001";
const BRAVO: &str = "76561198000000002";

/// SteamID64 to the userdata folder name.
fn account_id(steam_id: &str) -> u64 {
    steam_id.parse::<u64>().unwrap() - 76_561_197_960_265_728
}

/// A Steam folder Steam never saw: `config/loginusers.vdf` with two
/// accounts, one game folder in alpha's userdata, its app manifest.
fn steam_fixture(scratch: &Scratch) -> PathBuf {
    let steam = scratch.root.join("steam");
    std::fs::create_dir_all(steam.join("config")).unwrap();
    std::fs::write(
        steam.join("config").join("loginusers.vdf"),
        format!(
            "\"users\"\n{{\n\t\"{ALPHA}\"\n\t{{\n\t\t\"AccountName\"\t\"alpha\"\n\t\t\"PersonaName\"\t\"Alpha\"\n\t\t\"MostRecent\"\t\"1\"\n\t\t\"Timestamp\"\t\"1700000000\"\n\t}}\n\t\"{BRAVO}\"\n\t{{\n\t\t\"AccountName\"\t\"bravo\"\n\t\t\"PersonaName\"\t\"Bravo\"\n\t\t\"MostRecent\"\t\"0\"\n\t\t\"Timestamp\"\t\"1600000000\"\n\t}}\n}}\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(
        steam
            .join("userdata")
            .join(account_id(ALPHA).to_string())
            .join("730"),
    )
    .unwrap();
    std::fs::create_dir_all(steam.join("userdata").join(account_id(BRAVO).to_string())).unwrap();
    std::fs::create_dir_all(steam.join("steamapps")).unwrap();
    std::fs::write(
        steam.join("steamapps").join("appmanifest_730.acf"),
        "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n}\n",
    )
    .unwrap();
    steam
}

/// Steam pointed at the fixture through the path override, so nothing asks
/// the registry where the real Steam lives.
fn with_steam(tag: &str) -> (Arc<Scratch>, RealBackend, PathBuf) {
    let (scratch, real) = backend(tag);
    let steam = steam_fixture(&scratch);
    real.platform_set_path("steam", steam.to_str().unwrap())
        .unwrap();
    (scratch, real, steam)
}

/// Holds the operation lock of `scratch` on another thread, as a second app
/// would, until the returned sender is dropped or sent to.
fn hold_lock(scratch: &Arc<Scratch>) -> mpsc::Sender<()> {
    let (release, wait) = mpsc::channel::<()>();
    let (ready, is_ready) = mpsc::channel::<()>();
    let ctx = scratch.clone();
    std::thread::spawn(move || {
        let _guard = accshift_core::lock::acquire_exclusive(&*ctx, Duration::from_secs(5)).unwrap();
        ready.send(()).unwrap();
        let _ = wait.recv();
    });
    is_ready.recv().unwrap();
    // Safety net: the commands below stop Steam or switch for real when they
    // get the lock. Prove first that this thread cannot.
    assert!(
        matches!(
            accshift_core::lock::acquire_exclusive(&**scratch, Duration::from_millis(100)),
            Err(accshift_core::lock::LockError::Contended)
        ),
        "the held lock must refuse a second handle in this process"
    );
    release
}

const CONTENDED: &str = "Another accshift instance is holding the lock";

// ---------------------------------------------------------------- registry

#[test]
fn every_command_is_ported_and_dispatches_without_not_ported() {
    assert_eq!(PORTED.len(), COMMANDS.len());
    for command in COMMANDS {
        assert!(PORTED.contains(command), "{command} is not in PORTED");
    }
    // Cheap commands through dispatch: none answers "not ported".
    let (_scratch, real) = backend("dispatch");
    for (command, args) in [
        ("telemetry_get_state", json!({})),
        ("cs2_bridge_get_settings", json!({})),
        ("steam_has_api_key", json!({})),
        ("platform_get_accounts", json!({ "platformId": "nope" })),
        ("steam_get_profile_infos", json!({ "steamIds": []})),
        ("telemetry_track_streamer_mode", json!({})),
    ] {
        let answer = dispatch(&real, command, &args).expect(command);
        if let Err(error) = &answer {
            assert!(!error.message.contains("not ported"), "{command}: {error}");
        }
    }
}

#[test]
fn an_unknown_platform_is_refused_by_every_platform_command() {
    let (_scratch, real) = backend("unknown");
    let expected = "Unknown platform: nope";
    let errors = [
        real.platform_get_accounts("nope").unwrap_err(),
        real.platform_get_startup_snapshot("nope").unwrap_err(),
        real.platform_get_current_account("nope").unwrap_err(),
        real.platform_switch_account("nope", "x", &json!({}))
            .unwrap_err(),
        real.platform_forget_account("nope", "x").unwrap_err(),
        real.platform_begin_setup("nope", &json!({})).unwrap_err(),
        real.platform_get_setup_status("nope", "s").unwrap_err(),
        real.platform_cancel_setup("nope", "s").unwrap_err(),
        real.platform_get_path("nope").unwrap_err(),
        real.platform_set_path("nope", "C:/x").unwrap_err(),
        real.platform_select_path("nope").unwrap_err(),
        real.platform_set_account_label("nope", "x", "l")
            .unwrap_err(),
        real.platform_dry_run("nope", "x").unwrap_err(),
    ];
    for error in errors {
        assert_eq!(error.message, expected);
    }
    // Through dispatch too, the shape the shell uses.
    let error = dispatch(
        &real,
        "platform_get_accounts",
        &json!({ "platformId": "nope" }),
    )
    .expect("known command")
    .unwrap_err();
    assert_eq!(error.message, expected);
}

// ---------------------------------------------------------------- Steam fixture

#[test]
fn steam_lists_the_fixture_accounts_and_its_path_override() {
    let (_scratch, real, steam) = with_steam("steam-accounts");

    assert_eq!(
        real.platform_get_path("steam").unwrap(),
        steam.to_str().unwrap()
    );
    let accounts = real.platform_get_accounts("steam").unwrap();
    let accounts = accounts.as_array().unwrap();
    assert_eq!(accounts.len(), 2, "{accounts:?}");
    let alpha = accounts
        .iter()
        .find(|a| a["steam_id"] == ALPHA)
        .expect("alpha listed");
    assert_eq!(alpha["account_name"], "alpha");
    assert_eq!(alpha["persona_name"], "Alpha");
    assert_eq!(alpha["last_login_at"], 1_700_000_000u64);

    // Copyable games: alpha's 730 folder named from its app manifest.
    let games = real.steam_get_copyable_games(ALPHA, BRAVO).unwrap();
    assert_eq!(games.len(), 1, "{games:?}");
    assert_eq!(games[0].app_id, "730");
    assert_eq!(games[0].name, "Counter-Strike 2");
    // Same through dispatch, camelCase arguments.
    let value = dispatch(
        &real,
        "steam_get_copyable_games",
        &json!({ "fromSteamId": ALPHA, "toSteamId": BRAVO }),
    )
    .unwrap()
    .unwrap();
    assert_eq!(value[0]["app_id"], "730", "{value}");
}

#[test]
fn steam_path_override_refuses_a_folder_that_is_not_steam() {
    let (scratch, real) = backend("steam-path");
    let plain = scratch.root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let error = real
        .platform_set_path("steam", plain.to_str().unwrap())
        .unwrap_err();
    assert!(
        error
            .message
            .ends_with("|This folder does not look like a Steam installation"),
        "{error}"
    );
    let error = real
        .platform_set_path("steam", scratch.root.join("absent").to_str().unwrap())
        .unwrap_err();
    assert!(
        error
            .message
            .ends_with("|Steam path override must be an existing directory"),
        "{error}"
    );
}

#[test]
fn steam_commands_check_their_ids_before_touching_anything() {
    let (_scratch, real, _steam) = with_steam("steam-ids");
    let invalid = "Invalid SteamID64";
    assert_eq!(
        real.steam_get_profile_info("123").unwrap_err().message,
        invalid
    );
    assert_eq!(
        real.steam_get_profile_infos(&["abc".to_string()])
            .unwrap_err()
            .message,
        invalid
    );
    assert_eq!(
        real.steam_get_player_bans(&["1".to_string()])
            .unwrap_err()
            .message,
        invalid
    );
    assert_eq!(
        real.steam_copy_game_settings("1", BRAVO, "730")
            .unwrap_err()
            .message,
        invalid
    );
    assert_eq!(
        real.steam_get_copyable_games(ALPHA, "x")
            .unwrap_err()
            .message,
        invalid
    );
    assert_eq!(real.steam_open_userdata("x").unwrap_err().message, invalid);
    assert_eq!(
        real.steam_get_account_games("x").unwrap_err().message,
        invalid
    );
    assert_eq!(
        real.steam_bulk_edit(
            serde_json::from_value(json!({ "steamIds": ["nope"], "launchOptions": [] })).unwrap()
        )
        .unwrap_err()
        .message,
        invalid
    );
    assert_eq!(
        real.platform_forget_account("steam", "alpha")
            .unwrap_err()
            .message,
        invalid
    );
    // The game launch checks its arguments under the lock, before Steam.
    assert_eq!(
        real.steam_switch_account_and_launch_game("", "730", false, "", "")
            .unwrap_err()
            .message,
        "Invalid username"
    );
    assert_eq!(
        real.steam_switch_account_and_launch_game("alpha", "7a0", false, "", "")
            .unwrap_err()
            .message,
        "Invalid app id"
    );
    // A switch with a username Steam cannot hold, refused before any write.
    assert_eq!(
        real.platform_switch_account("steam", "a\u{1}b", &json!({}))
            .unwrap_err()
            .message,
        "Invalid username"
    );
}

#[test]
fn steam_network_commands_answer_without_a_request_when_there_is_nothing_to_ask() {
    let (_scratch, real) = backend("steam-empty");
    // No ids: no request, an empty map.
    assert!(real.steam_get_profile_infos(&[]).unwrap().is_empty());
    // No API key in the config: bans answer empty without a request, and
    // the key check reads the config only.
    assert!(!real.steam_has_api_key().unwrap());
    assert!(real
        .steam_get_player_bans(&[ALPHA.to_string()])
        .unwrap()
        .is_empty());
}

#[test]
fn steam_labels_and_dry_runs_are_not_supported() {
    let (_scratch, real, _steam) = with_steam("steam-unsupported");
    assert_eq!(
        real.platform_set_account_label("steam", ALPHA, "Main")
            .unwrap_err()
            .message,
        "Account labeling not supported"
    );
    assert_eq!(
        real.platform_dry_run("steam", "alpha").unwrap_err().message,
        "Dry run is only available for platforms described by a descriptor"
    );
}

// ---------------------------------------------------------------- lock and PIN

#[test]
fn a_second_app_holding_the_lock_blocks_every_mutating_command() {
    let (scratch, real, _steam) = with_steam("lock");
    let release = hold_lock(&scratch);

    let started = Instant::now();
    let error = real
        .platform_switch_account("steam", "alpha", &json!({}))
        .unwrap_err();
    let waited = started.elapsed();
    assert_eq!(error.message, CONTENDED);
    assert!(
        waited >= LOCK_TIMEOUT && waited < LOCK_TIMEOUT + Duration::from_secs(2),
        "{waited:?}"
    );

    #[allow(unused_mut)]
    let mut refused = vec![
        real.platform_forget_account("steam", ALPHA).unwrap_err(),
        real.platform_begin_setup("steam", &json!({})).unwrap_err(),
        real.steam_switch_account_and_launch_game("alpha", "730", false, "", "")
            .unwrap_err(),
        real.steam_copy_game_settings(ALPHA, BRAVO, "730")
            .unwrap_err(),
        real.steam_clear_browser_cache().unwrap_err(),
        real.steam_bulk_edit(
            serde_json::from_value(json!({ "steamIds": [ALPHA], "launchOptions": [] })).unwrap(),
        )
        .unwrap_err(),
    ];
    #[cfg(windows)]
    refused.push(real.riot_capture_profile("profile").unwrap_err());
    for error in refused {
        assert_eq!(error.message, CONTENDED);
    }

    // The status poll reports `busy` instead of failing the wizard.
    let status = real.platform_get_setup_status("steam", "setup-1").unwrap();
    assert_eq!(status.state, "busy");
    assert_eq!(status.setup_id, "setup-1");
    assert!(status.account_id.is_empty() && status.error_message.is_empty());
    let value = dispatch(
        &real,
        "platform_get_setup_status",
        &json!({ "platformId": "steam", "setupId": "setup-1" }),
    )
    .unwrap()
    .unwrap();
    assert_eq!(value["state"], "busy", "{value}");

    // Released: the lock is free again and the reads never needed it.
    drop(release);
    std::thread::sleep(Duration::from_millis(100));
    assert!(accshift_core::lock::acquire_exclusive(&*scratch, Duration::from_secs(1)).is_ok());
    assert_eq!(
        real.platform_get_accounts("steam")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn cancel_setup_waits_out_a_lock_held_for_less_than_its_budget() {
    let (scratch, real, _steam) = with_steam("cancel");
    let release = hold_lock(&scratch);
    let releaser = std::thread::spawn(move || {
        // Longer than the 2 s budget of the other commands.
        std::thread::sleep(LOCK_TIMEOUT + Duration::from_millis(800));
        drop(release);
    });
    let started = Instant::now();
    // An unknown setup id is a no-op once the lock is free: Ok after the wait.
    real.platform_cancel_setup("steam", "no-such-setup")
        .unwrap();
    assert!(started.elapsed() > LOCK_TIMEOUT, "{:?}", started.elapsed());
    releaser.join().unwrap();
}

#[test]
fn a_locked_pin_refuses_the_switch_and_the_game_launch() {
    let (_scratch, real, _steam) = with_steam("pin");
    real.save_client_storage_store(
        STORE_SETTINGS,
        &json!({ "pinEnabled": true, "pinHash": hash_pin_code("1234").unwrap() }),
    )
    .unwrap();
    real.pin_lock().unwrap();
    // Safety net: the switches below are real ones if the PIN lets them by.
    assert!(real.pin_session().ensure_unlocked(real.context()).is_err());

    let error = real
        .platform_switch_account("steam", "alpha", &json!({}))
        .unwrap_err();
    assert_eq!(error.message, PIN_LOCKED_MESSAGE);
    assert!(error.is_pin_locked());
    let error = real
        .steam_switch_account_and_launch_game("alpha", "730", false, "", "")
        .unwrap_err();
    assert_eq!(error.message, PIN_LOCKED_MESSAGE);
}

// ---------------------------------------------------------------- descriptors

#[test]
fn a_descriptor_platform_answers_a_dry_run_and_keeps_labels() {
    let _turn = registry_turn();
    let (scratch, real) = backend("descriptor");
    let live = scratch.root.join("live");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(live.join("session.json"), b"live").unwrap();
    let template = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/real/descriptor.json"),
    )
    .unwrap();
    let live_json = live.to_string_lossy().replace('\\', "/");
    let descriptor = template.replace("${LIVE}", &live_json);
    let picked = scratch.root.join("picked.json");
    std::fs::write(&picked, descriptor).unwrap();
    real.descriptor_install_file(picked.to_str().unwrap())
        .unwrap();

    let plan = real.platform_dry_run("egui-fixture", "a3f0c2d1").unwrap();
    assert_eq!(plan.platform_id, "egui-fixture");
    assert!(!plan.applied);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("No snapshot stored for account a3f0c2d1")),
        "{:?}",
        plan.warnings
    );
    // The plan wrote nothing: the live file is untouched.
    assert_eq!(std::fs::read(live.join("session.json")).unwrap(), b"live");
    let value = dispatch(
        &real,
        "platform_dry_run",
        &json!({ "platformId": "egui-fixture", "accountId": "a3f0c2d1" }),
    )
    .unwrap()
    .unwrap();
    assert_eq!(value["platformId"], "egui-fixture", "{value}");

    // An id outside the declared charset is refused.
    assert!(real.platform_dry_run("egui-fixture", "not ok!").is_err());
    assert_eq!(
        real.platform_get_accounts("egui-fixture").unwrap(),
        json!([])
    );

    real.descriptor_remove("egui-fixture").unwrap();
    assert_eq!(
        real.platform_get_accounts("egui-fixture")
            .unwrap_err()
            .message,
        "Unknown platform: egui-fixture"
    );
}

// ---------------------------------------------------------------- Riot, Roblox

#[cfg(windows)]
#[test]
fn riot_and_roblox_check_their_arguments_first() {
    let (_scratch, real) = backend("riot-roblox");
    assert_eq!(
        real.riot_capture_profile(" padded").unwrap_err().message,
        "Invalid Riot profile id"
    );
    assert_eq!(
        real.riot_capture_profile("unknown-profile")
            .unwrap_err()
            .message,
        "Riot profile not found"
    );
    // Empty once the paste prefixes are stripped: refused before Roblox. A
    // bare warning banner is not stripped (cookie.rs lines 32-37) and would
    // reach roblox.com, so it is not in this list.
    for cookie in ["", "   ", ".ROBLOSECURITY=", ".ROBLOSECURITY:\"\""] {
        assert_eq!(
            real.roblox_add_account_by_cookie(cookie)
                .unwrap_err()
                .message,
            "Cookie is empty",
            "{cookie:?}"
        );
    }
    assert_eq!(
        real.roblox_get_profile_info("12a").unwrap_err().message,
        "Invalid Roblox user id"
    );
    // No stored account: nothing to probe.
    assert!(real.roblox_check_sessions().unwrap().is_empty());
    assert_eq!(real.platform_get_accounts("roblox").unwrap(), json!([]));
}

#[cfg(not(windows))]
#[test]
fn riot_and_roblox_are_not_registered_off_windows() {
    let (_scratch, real) = backend("riot-roblox");
    assert_eq!(
        real.riot_capture_profile("p").unwrap_err().message,
        "Command riot_capture_profile not found"
    );
    assert_eq!(
        real.roblox_check_sessions().unwrap_err().message,
        "Command roblox_check_sessions not found"
    );
}

// ---------------------------------------------------------------- CS2 settings

#[test]
fn cs2_bridge_settings_round_trip_without_a_token() {
    let (_scratch, real) = backend("cs2-settings");
    let fresh = real.cs2_bridge_get_settings().unwrap();
    assert!(!fresh.enabled && fresh.url.is_empty() && !fresh.token_configured);

    // No token given and none stored: a config write only, no keyring.
    real.cs2_bridge_set_settings(true, " http://127.0.0.1:9/bridge/ ", None)
        .unwrap();
    let saved = real.cs2_bridge_get_settings().unwrap();
    assert!(saved.enabled);
    assert_eq!(saved.url, "http://127.0.0.1:9/bridge");
    assert!(!saved.token_configured);
    let value = dispatch(&real, "cs2_bridge_get_settings", &json!({}))
        .unwrap()
        .unwrap();
    assert_eq!(
        value,
        json!({ "enabled": true, "url": "http://127.0.0.1:9/bridge", "tokenConfigured": false })
    );

    assert_eq!(
        real.cs2_bridge_set_settings(true, "ftp://example.test", None)
            .unwrap_err()
            .message,
        "Bridge URL must use http or https"
    );
    assert_eq!(
        real.cs2_bridge_set_settings(true, "not a url", None)
            .unwrap_err()
            .message,
        "Invalid bridge URL"
    );
}
