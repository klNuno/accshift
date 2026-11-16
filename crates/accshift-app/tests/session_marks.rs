//! Session marks shared by every platform, over the scripted mock: Steam
//! "password needed" and after-the-fact "expired" beside the ban warnings,
//! Riot marks beside the offline expiry, Epic's chip, the clear on a new
//! capture, and missing or unreadable data that never marks anything.

mod support;

use accshift_app::backend::Backend;
use accshift_app::platforms::adapter::PlatformAdapter;
use accshift_app::platforms::warnings::{WarningPresentation, WarningTone};
use accshift_app::platforms::WarningLoadOptions;
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use serde_json::json;
use std::collections::BTreeMap;

/// In the `dev` scenario's mock: one Steam account Steam does not remember,
/// one whose session was refused after a switch, and one banned.
const STEAM_PASSWORD: &str = "76561198100000006";
const STEAM_EXPIRED: &str = "76561198100000007";
const STEAM_BANNED: &str = "76561198100000003";

fn stores(backend: &dyn Backend) -> ClientStores {
    let (storage, report) = storage::initialize(backend, None, None, true);
    assert!(report.load_error.is_none(), "{:?}", report.load_error);
    ClientStores::new(storage, StoreContext::shipped())
}

/// A platform's account load followed by its warning load, as the shell
/// runs them on the job pool.
fn warnings(
    platform: &str,
    backend: &support::scripted::Scripted,
) -> (PlatformAdapter, BTreeMap<String, WarningPresentation>) {
    let adapter = PlatformAdapter::shipped(platform).unwrap();
    let mut stores = stores(backend);
    let accounts = adapter.load_accounts(backend).unwrap_or_default();
    let settings = stores.settings().clone();
    let load = adapter
        .load_warning_states_blocking(
            backend,
            &mut stores,
            &settings,
            &accounts,
            WarningLoadOptions::default(),
            1_791_000_000_000.0,
        )
        .expect("the platform has warnings");
    (adapter, load.warnings)
}

fn chips(warning: &WarningPresentation) -> Vec<Text> {
    warning.chips.iter().map(|chip| chip.text.clone()).collect()
}

/// Whether the account shows a `session.*` chip; Steam lists every account,
/// clean ones with an empty warning.
fn has_session_chip(warnings: &BTreeMap<String, WarningPresentation>, id: &str) -> bool {
    warnings.get(id).is_some_and(|warning| {
        warning
            .chips
            .iter()
            .any(|chip| matches!(&chip.text, Text::Key { key, .. } if key.starts_with("session.")))
    })
}

fn assert_orange_session_chip(warning: &WarningPresentation, chip: &str, tooltip: &str) {
    assert_eq!(chips(warning), vec![Text::key(chip)]);
    assert_eq!(warning.tooltip, Text::key(tooltip));
    assert_eq!(warning.card_outline_tone, Some(WarningTone::Orange));
    assert!(warning.list_has_orange && !warning.list_has_red);
    assert!(warning.chips.iter().all(|c| c.tone == WarningTone::Orange));
}

#[test]
fn steam_shows_password_needed_and_an_after_the_fact_mark_in_orange() {
    let backend = support::scripted("dev");
    let (_, warnings) = warnings("steam", &backend);
    assert_eq!(backend.calls("session_marks"), vec!["steam".to_string()]);
    assert_orange_session_chip(
        &warnings[STEAM_PASSWORD],
        "session.passwordNeeded",
        "session.passwordNeededTooltip",
    );
    assert_orange_session_chip(
        &warnings[STEAM_EXPIRED],
        "session.expired",
        "session.expiredTooltip",
    );
}

#[test]
fn a_session_chip_joins_a_ban_warning_without_hiding_it() {
    let backend = support::scripted("dev");
    let (_, plain) = warnings("steam", &backend);
    let banned_before = plain.get(STEAM_BANNED).cloned();

    let backend = support::scripted("dev");
    backend.push(
        "session_marks",
        Ok(json!([{ "accountId": STEAM_BANNED, "state": "passwordNeeded" }])),
    );
    let (_, marked) = warnings("steam", &backend);
    let banned = &marked[STEAM_BANNED];
    let chip = Text::key("session.passwordNeeded");
    assert!(chips(banned).contains(&chip));
    assert!(banned.list_has_orange);
    if let Some(before) = banned_before {
        // The ban keeps its own tooltip, outline and chips first.
        assert_eq!(banned.tooltip, before.tooltip);
        assert_eq!(banned.card_outline_tone, before.card_outline_tone);
        assert_eq!(banned.list_has_red, before.list_has_red);
        assert_eq!(banned.chips[..before.chips.len()], before.chips[..]);
    }
}

#[test]
fn a_failed_or_odd_answer_never_marks_anything() {
    let backend = support::scripted("dev");
    backend.push("session_marks", Err("loginusers.vdf unreadable"));
    let (_, after_failure) = warnings("steam", &backend);
    assert!(!has_session_chip(&after_failure, STEAM_PASSWORD));
    assert!(!has_session_chip(&after_failure, STEAM_EXPIRED));

    let backend = support::scripted("dev");
    backend.push(
        "session_marks",
        Ok(json!([
            { "accountId": STEAM_PASSWORD, "state": "melted" },
            { "accountId": "", "state": "expired" }
        ])),
    );
    let (adapter, odd) = warnings("steam", &backend);
    assert!(!has_session_chip(&odd, STEAM_PASSWORD));
    assert!(!odd.contains_key(""));
    assert!(!adapter.session_expired(STEAM_PASSWORD));
}

#[test]
fn a_failed_reload_keeps_the_marks_already_known() {
    let backend = support::scripted("dev");
    let (adapter, _) = warnings("steam", &backend);
    assert!(adapter.session_expired(STEAM_EXPIRED));
    backend.push("session_marks", Err("config locked"));
    adapter.load_accounts(&backend).unwrap();
    assert!(adapter.session_expired(STEAM_EXPIRED));
}

#[test]
fn a_riot_mark_after_the_fact_joins_the_offline_expiry() {
    let backend = support::scripted("dev");
    backend.push(
        "session_marks",
        Ok(json!([{ "accountId": "riot-1", "state": "expired" }])),
    );
    let (adapter, warnings) = warnings("riot", &backend);
    assert_orange_session_chip(
        &warnings["riot-1"],
        "session.expired",
        "session.expiredTooltip",
    );
    // The offline read of the first pass still marks its profile.
    assert!(warnings.contains_key("riot-5"));
    assert!(adapter.session_expired("riot-1"));
    assert_eq!(
        adapter.session_warning_states().unwrap().len(),
        warnings.len()
    );
}

#[test]
fn epic_shows_password_needed_from_its_snapshot() {
    let backend = support::scripted("dev");
    backend.push(
        "session_marks",
        Ok(json!([{ "accountId": "0123456789abcdef0123456789abcdef", "state": "passwordNeeded" }])),
    );
    let adapter = PlatformAdapter::shipped("epic").unwrap();
    assert!(adapter.has_warnings());
    let mut stores = stores(&backend);
    let settings = stores.settings().clone();
    let load = adapter
        .load_warning_states_blocking(
            &backend,
            &mut stores,
            &settings,
            &[],
            WarningLoadOptions::default(),
            0.0,
        )
        .unwrap();
    assert_orange_session_chip(
        &load.warnings["0123456789abcdef0123456789abcdef"],
        "session.passwordNeeded",
        "session.passwordNeededTooltip",
    );
    // A password-needed account is not "expired": no switch notice.
    assert!(!adapter.session_expired("0123456789abcdef0123456789abcdef"));
}

#[test]
fn platforms_without_session_sources_ask_nothing() {
    let backend = support::scripted("dev");
    for platform in ["battle-net", "gog", "discord", "ubisoft", "jagex", "roblox"] {
        let adapter = PlatformAdapter::shipped(platform).expect(platform);
        assert!(!adapter.has_session_marks(), "{platform}");
        assert!(adapter.session_warning_states().is_none(), "{platform}");
    }
    assert!(backend.calls("session_marks").is_empty());
}

#[test]
fn a_new_capture_clears_the_mark() {
    let backend = support::scripted("dev");
    let (adapter, _) = warnings("steam", &backend);
    assert!(adapter.session_expired(STEAM_EXPIRED));
    backend.push(
        "platform_get_setup_status",
        Ok(json!({
            "setupId": "setup-1",
            "state": "ready",
            "accountId": STEAM_EXPIRED,
            "accountDisplayName": "nested_one",
            "errorMessage": ""
        })),
    );
    let status = adapter.poll_add_flow(&backend, "setup-1").unwrap();
    assert_eq!(status.state, "ready");
    assert!(!adapter.session_expired(STEAM_EXPIRED));
    let mut stores = stores(&backend);
    assert!(!adapter
        .cached_warning_states(&stores)
        .contains_key(STEAM_EXPIRED));
    // A capture still pending keeps it.
    let backend = support::scripted("dev");
    let (adapter, _) = warnings("steam", &backend);
    backend.push(
        "platform_get_setup_status",
        Ok(json!({
            "setupId": "setup-1",
            "state": "waiting_for_login",
            "accountId": STEAM_EXPIRED,
            "accountDisplayName": "",
            "errorMessage": ""
        })),
    );
    adapter.poll_add_flow(&backend, "setup-1").unwrap();
    assert!(adapter.session_expired(STEAM_EXPIRED));
    let _ = &mut stores;
}
