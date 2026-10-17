//! Round trip against the former TypeScript stores: every fixture of
//! `tests/data/stores/roundtrip.json` is a stored value, an operation and
//! what the TS store saved after it. The
//! Rust store loads the same value, runs the same operation and must save the
//! same JSON, byte for byte once serialized the way the backend writes it.

use accshift_app::folders::ItemRef;
use accshift_app::personas::{PersonaAssignment, PersonaPatch};
use accshift_app::storage::{ClientStorage, CLIENT_STORE_IDS};
use accshift_app::stores::{
    ClientStores, DefaultGame, StoreContext, BUILT_IN_THEME_IDS, SHIPPED_PLATFORM_IDS,
};
use accshift_app::types::{ClientStorageSnapshot, StorageManifest};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn data(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/stores")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture")).expect("json")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

fn context() -> StoreContext {
    let raw = data("context.json");
    StoreContext {
        platform_ids: strings(&raw["platformIds"]),
        theme_ids: strings(&raw["themeIds"]),
        preferred_languages: strings(&raw["preferredLanguages"]),
    }
}

#[test]
fn shipped_lists_match_the_ts_registry() {
    let ctx = context();
    assert_eq!(ctx.platform_ids, SHIPPED_PLATFORM_IDS.map(String::from));
    assert_eq!(ctx.theme_ids, BUILT_IN_THEME_IDS.map(String::from));
}

fn opt_str(value: &Value) -> Option<&str> {
    value.as_str()
}

fn item_ref(value: &Value) -> ItemRef {
    serde_json::from_value(value.clone()).expect("item ref")
}

fn run(stores: &mut ClientStores, op: &Value) {
    let s = |key: &str| op[key].as_str().unwrap_or_default().to_string();
    match op["kind"].as_str().expect("kind") {
        "saveSettings" => {
            let settings = stores.settings().clone();
            stores.save_settings(&settings);
        }
        "reorderRoot" => {
            let platform = s("platform");
            let items = stores.items_in_folder(None, &platform).to_vec();
            stores.reorder_items(None, &platform, items);
        }
        "moveItem" => stores.move_item(
            &item_ref(&op["item"]),
            opt_str(&op["from"]),
            opt_str(&op["to"]),
            &s("platform"),
            op["index"].as_u64().map(|index| index as usize),
        ),
        "deleteFolder" => stores.delete_folder(&s("id")),
        "renameFolder" => stores.rename_folder(&s("id"), &s("name")),
        "syncAccounts" => stores.sync_accounts(&strings(&op["ids"]), &s("platform")),
        "deletePersona" => stores.delete_persona(&s("id")),
        "updatePersona" => {
            let patch = &op["patch"];
            let assignments: Option<Vec<PersonaAssignment>> = patch
                .get("assignments")
                .map(|value| serde_json::from_value(value.clone()).expect("assignments"));
            stores.update_persona(
                &s("id"),
                &PersonaPatch {
                    name: patch["name"].as_str().map(String::from),
                    color: patch["color"].as_str().map(String::from),
                    image: patch
                        .get("image")
                        .map(|image| image.as_str().map(String::from)),
                    assignments,
                },
            );
        }
        "clearNote" => stores.clear_account_card_note(&s("id")),
        "setNote" => stores.set_account_card_note(&s("id"), &s("note")),
        "setColor" => stores.set_account_card_color(&s("id"), &s("color")),
        "setColors" => stores.set_account_card_colors(&strings(&op["ids"]), &s("color")),
        "setFolderColor" => stores.set_folder_card_color(&s("id"), &s("color")),
        "setDefaultGame" => {
            let game: DefaultGame = serde_json::from_value(op["game"].clone()).expect("game");
            stores.set_default_game(&s("id"), &game);
        }
        "resaveViewMode" => {
            let mode = stores.view_mode();
            stores.set_view_mode(mode);
        }
        other => panic!("unknown op {other}"),
    }
}

/// `value` with every object's keys sorted. Key order is not part of the
/// contract: the stores write their maps from `BTreeMap`s, where the TS wrote
/// insertion order, and serde_json keeps the fixture's order only when another
/// crate of the build turns on `preserve_order` (accshift-theme does).
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, Value> =
                map.iter().map(|(k, v)| (k, sorted(v))).collect();
            Value::Object(sorted.into_iter().map(|(k, v)| (k.clone(), v)).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

#[test]
fn every_fixture_saves_what_the_ts_store_saved() {
    let fixtures = data("roundtrip.json");
    let fixtures = fixtures.as_array().expect("fixtures");
    let ctx = context();
    let mut failures = Vec::new();
    let mut by_store: BTreeMap<String, usize> = BTreeMap::new();
    for fixture in fixtures {
        let name = fixture["name"].as_str().expect("name");
        let store_id = fixture["storeId"].as_str().expect("store id");
        *by_store.entry(store_id.to_string()).or_default() += 1;
        let snapshot = ClientStorageSnapshot {
            manifest: StorageManifest {
                schema_version: 1,
                stores: BTreeMap::new(),
            },
            stores: BTreeMap::from([(store_id.to_string(), fixture["input"].clone())]),
        };
        let mut stores = ClientStores::new(ClientStorage::from_snapshot(snapshot), ctx.clone());
        run(&mut stores, &fixture["op"]);
        let saves = stores.take_all_saves();
        let wrote = fixture["wrote"].as_bool().expect("wrote");
        let saved = saves.iter().find(|save| save.store_id == store_id);
        match (wrote, saved) {
            (false, None) => {}
            (false, Some(save)) => failures.push(format!("{name}: wrote {}", save.value)),
            (true, None) => failures.push(format!("{name}: wrote nothing")),
            (true, Some(save)) => {
                let expected = serde_json::to_string_pretty(&sorted(&fixture["expected"])).unwrap();
                let actual = serde_json::to_string_pretty(&sorted(&save.value)).unwrap();
                if expected != actual {
                    failures.push(format!("{name}:\n expected {expected}\n actual   {actual}"));
                }
            }
        }
        assert!(
            saves.iter().all(|save| save.store_id == store_id),
            "{name}: wrote another store"
        );
    }
    eprintln!("round-trip fixtures: {}", fixtures.len());
    eprintln!("by store: {by_store:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let typed = CLIENT_STORE_IDS.len() - 4;
    assert_eq!(by_store.len(), typed, "every typed store has fixtures");
}

#[test]
fn a_fixture_with_no_write_is_listed() {
    let fixtures = data("roundtrip.json");
    let silent: Vec<&str> = fixtures
        .as_array()
        .unwrap()
        .iter()
        .filter(|fixture| fixture["wrote"] == Value::Bool(false))
        .map(|fixture| fixture["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        silent,
        [
            "folders move folder into its own subtree",
            "folders delete missing folder",
            "folders sync unchanged",
            "colors set many empty list",
            "default game invalid set",
        ]
    );
}
