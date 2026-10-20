//! The platform model over every parity scenario
//! (`crates/accshift-egui/parity/data/*.json`): the registry and adapters
//! build each platform's accounts through `MockBackend` without error, with
//! the account counts and current accounts the scenario holds.

mod support;

use accshift_app::menu::account::{build_account_context_menu, AppearanceInput};
use accshift_app::platforms::adapter::MenuContext;
use accshift_app::platforms::{PlatformRegistry, StartupSnapshot, WarningLoadOptions};
use accshift_app::settings::sanitize_settings;
use accshift_app::storage::ClientStorage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::Backend;
use serde_json::Value;
use std::collections::BTreeMap;

/// Builds every platform's snapshot, its menus, profiles and warnings, and
/// returns the snapshots by platform id.
fn build_model(scenario: &str) -> BTreeMap<String, StartupSnapshot> {
    let backend = support::mock(scenario);
    let registry = PlatformRegistry::shipped();
    let stores = ClientStores::new(ClientStorage::new(), StoreContext::shipped());
    let settings = sanitize_settings(&Value::Null, &StoreContext::shipped());
    let mut out = BTreeMap::new();
    for def in registry.defs() {
        let adapter = registry.adapter(&def.id).expect("adapter");
        let snapshot = adapter
            .startup_snapshot(&backend)
            .unwrap_or_else(|error| panic!("{scenario}/{}: {error}", def.id));
        let ctx = MenuContext {
            current_account_id: Some(&snapshot.current_account),
            stores: &stores,
            input_dialog: true,
        };
        for account in &snapshot.accounts {
            assert!(!account.id.is_empty(), "{scenario}/{}: empty id", def.id);
            let menu = build_account_context_menu(
                account,
                &adapter,
                &ctx,
                &AppearanceInput::default(),
                None,
            );
            assert!(!menu.is_empty(), "{scenario}/{}: empty menu", def.id);
        }
        if def.id == "riot" {
            for account in &snapshot.accounts {
                let profile = adapter.profile_info(&backend, &account.id).unwrap();
                assert!(
                    profile.is_some(),
                    "{scenario}/riot/{}: no profile",
                    account.id
                );
            }
        }
        out.insert(def.id.clone(), snapshot);
    }
    // The Steam warnings run against the mock's bans without error.
    let steam = registry.adapter("steam").unwrap();
    let accounts = out["steam"].accounts.clone();
    let mut stores = stores;
    let load = steam
        .load_warning_states_blocking(
            &backend,
            &mut stores,
            &settings,
            &accounts,
            WarningLoadOptions::default(),
            backend.now() as f64 * 1000.0,
        )
        .expect("steam has warnings");
    // TS settles nothing for an empty list.
    assert_eq!(load.outcome.is_some(), !accounts.is_empty(), "{scenario}");
    assert!(backend.platform_detect_installed().is_ok());
    out
}

fn counts(model: &BTreeMap<String, StartupSnapshot>) -> Vec<(String, usize)> {
    model
        .iter()
        .filter(|(_, snapshot)| !snapshot.accounts.is_empty())
        .map(|(id, snapshot)| (id.clone(), snapshot.accounts.len()))
        .collect()
}

fn expect(model: &BTreeMap<String, StartupSnapshot>, wanted: &[(&str, usize)]) {
    let wanted: Vec<(String, usize)> = wanted
        .iter()
        .map(|(id, count)| (id.to_string(), *count))
        .collect();
    assert_eq!(counts(model), wanted);
}

#[test]
fn scenario_demo() {
    let model = build_model("demo");
    expect(&model, &[("riot", 3), ("roblox", 4), ("steam", 17)]);
    assert_eq!(model["steam"].current_account, "76561198000000001");
    assert_eq!(model["riot"].current_account, "riot-1");
    assert_eq!(model["roblox"].current_account, "1000000001");
}

#[test]
fn scenario_dev() {
    let model = build_model("dev");
    expect(&model, &[("riot", 4), ("steam", 8)]);
    assert_eq!(model["steam"].current_account, "76561198100000001");
    assert_eq!(model["riot"].current_account, "riot-1");
}

#[test]
fn scenario_empty() {
    let model = build_model("empty");
    expect(&model, &[]);
}

#[test]
fn scenario_huge() {
    let model = build_model("huge");
    expect(&model, &[("steam", 200)]);
    assert_eq!(model["steam"].current_account, "76561199000000000");
}

#[test]
fn scenario_slow() {
    let model = build_model("slow");
    expect(&model, &[("riot", 4), ("steam", 8)]);
}

#[test]
fn every_scenario_is_covered() {
    let root = support::repo_root();
    let scenarios = accshift_app::mock::MockBackend::scenarios(
        &accshift_app::mock::MockBackend::data_dir(&root),
    );
    assert_eq!(scenarios, ["demo", "dev", "empty", "huge", "slow"]);
}
