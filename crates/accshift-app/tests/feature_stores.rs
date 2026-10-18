//! The TS tests of the ported stores (settings, folders, personas, the
//! small shared stores), as Rust tests.

use accshift_app::folders::{FolderStore, ItemRef};
use accshift_app::personas::{persona_assignments_for_save, AssignmentsForSave, PersonaAssignment};
use accshift_app::settings::{
    api_key_draft_after_successful_save, complete_onboarding_consent, diff_settings,
    merge_settings_draft, AnimationsMode, AppSettings, OnboardingConsentOutcome, SettingsChange,
    StreamerMode,
};
use accshift_app::storage::{
    ClientStorage, CLIENT_STORE_ACCOUNT_CARD_COLORS, CLIENT_STORE_ACCOUNT_CARD_NOTES,
    CLIENT_STORE_FOLDERS,
};
use accshift_app::stores::{
    ClientStores, StoreContext, StringMapStore, ACCOUNT_CARD_COLOR_PRESETS,
};
use accshift_app::types::ClientStorageSnapshot;
use serde_json::{json, Value};
use std::collections::BTreeMap;

// ------------------------------------------------------------ settingsPatch

fn settings(edit: impl FnOnce(&mut Value)) -> AppSettings {
    let mut value = json!({
        "language": "en",
        "themeId": "dark",
        "backgroundOpacity": 100,
        "uiScalePercent": 100,
        "animations": "system",
        "streamerMode": "auto",
        "suspendGraphicsWhenMinimized": true,
        "minimizeOnAccountSwitch": false,
        "startWithSystem": false,
        "closeToTray": false,
        "dataRefresh": { "avatarCacheDays": 7, "banCheckDays": 7 },
        "enabledPlatforms": ["steam", "riot"],
        "healthCheckPerPlatform": {},
        "personasEnabled": true,
        "defaultPlatformId": "steam",
        "inactivityBlurSeconds": 60,
        "deepLinksEnabled": true,
        "cliEnabled": true,
        "platformSettings": {
            "steam": { "runAsAdmin": false, "launchOptions": "", "shutdownMode": "graceful" }
        },
        "accountDisplay": {
            "showUsernames": true,
            "showLastLoginPerPlatform": {},
            "showCardNotesInline": false,
            "expandedFolders": false
        },
        "pinEnabled": true,
        "pinHash": "legacy-hash"
    });
    edit(&mut value);
    serde_json::from_value(value).unwrap()
}

fn base() -> AppSettings {
    settings(|_| {})
}

fn merge(current: &AppSettings, baseline: &AppSettings, draft: &AppSettings) -> AppSettings {
    merge_settings_draft(current, baseline, draft).unwrap()
}

#[test]
fn keeps_a_zoom_change_made_elsewhere_while_the_panel_was_open() {
    let current = settings(|s| s["uiScalePercent"] = json!(120));
    let draft = settings(|s| s["minimizeOnAccountSwitch"] = json!(true));
    let merged = merge(&current, &base(), &draft);
    assert_eq!(merged.ui_scale_percent, 120);
    assert!(merged.minimize_on_account_switch);
}

#[test]
fn keeps_the_streamer_flag_and_a_rehashed_pin_written_elsewhere() {
    let current = settings(|s| {
        s["streamerMode"] = json!("off");
        s["pinHash"] = json!("upgraded-hash");
    });
    let draft = settings(|s| s["themeId"] = json!("light"));
    let merged = merge(&current, &base(), &draft);
    assert_eq!(merged.streamer_mode, StreamerMode::Off);
    assert_eq!(merged.pin_hash, "upgraded-hash");
    assert_eq!(merged.theme_id, "light");
}

#[test]
fn merges_nested_fields_one_leaf_at_a_time() {
    let current = settings(|s| s["accountDisplay"]["expandedFolders"] = json!(true));
    let draft = settings(|s| s["accountDisplay"]["showUsernames"] = json!(false));
    let merged = merge(&current, &base(), &draft);
    assert!(merged.account_display.expanded_folders);
    assert!(!merged.account_display.show_usernames);
}

#[test]
fn replaces_an_edited_list_whole() {
    let current = settings(|s| s["enabledPlatforms"] = json!(["steam", "riot", "roblox"]));
    let draft = settings(|s| {
        s["enabledPlatforms"] = json!(["riot"]);
        s["defaultPlatformId"] = json!("riot");
    });
    let merged = merge(&current, &base(), &draft);
    assert_eq!(merged.enabled_platforms, ["riot"]);
    assert_eq!(merged.default_platform_id, "riot");
}

#[test]
fn applies_the_users_own_edit_over_a_different_value_written_elsewhere() {
    let merged = merge(
        &settings(|s| s["uiScalePercent"] = json!(120)),
        &base(),
        &settings(|s| s["uiScalePercent"] = json!(90)),
    );
    assert_eq!(merged.ui_scale_percent, 90);
}

#[test]
fn leaves_the_current_settings_untouched() {
    let current = base();
    merge(
        &current,
        &base(),
        &settings(|s| s["language"] = json!("fr")),
    );
    assert_eq!(current, base());
}

#[test]
fn rebases_an_open_panel_and_keeps_its_own_edits() {
    let baseline = base();
    let draft = settings(|s| s["language"] = json!("fr"));
    let stored = settings(|s| s["uiScalePercent"] = json!(110));
    let shown = merge(&stored, &baseline, &draft);
    assert_eq!(shown.ui_scale_percent, 110);
    assert_eq!(shown.language.code(), "fr");
    let saved = merge(&stored, &stored, &shown);
    assert_eq!(
        saved,
        settings(|s| {
            s["language"] = json!("fr");
            s["uiScalePercent"] = json!(110);
        })
    );
    assert_eq!(
        diff_settings(Some(&stored.to_value()), Some(&shown.to_value())),
        [SettingsChange {
            path: vec!["language".to_string()],
            value: Some(json!("fr")),
        }]
    );
}

#[test]
fn diff_reports_nothing_when_the_draft_matches() {
    assert!(diff_settings(Some(&base().to_value()), Some(&base().to_value())).is_empty());
}

#[test]
fn diff_reports_the_path_of_each_changed_leaf() {
    let after = settings(|s| {
        s["dataRefresh"] = json!({ "avatarCacheDays": 3, "banCheckDays": 7 });
        s["enabledPlatforms"] = json!(["steam"]);
    });
    assert_eq!(
        diff_settings(Some(&base().to_value()), Some(&after.to_value())),
        [
            SettingsChange {
                path: vec!["dataRefresh".into(), "avatarCacheDays".into()],
                value: Some(json!(3)),
            },
            SettingsChange {
                path: vec!["enabledPlatforms".into()],
                value: Some(json!(["steam"])),
            },
        ]
    );
}

// ------------------------------------------------------------ apiKeyDraft, onboardingConsent

#[test]
fn clears_the_api_key_input_when_it_still_holds_the_saved_key() {
    let draft = api_key_draft_after_successful_save("  steam-key  ", "steam-key");
    assert_eq!(draft.api_key, "");
    assert!(!draft.touched);
}

#[test]
fn keeps_a_replacement_key_typed_while_the_first_save_was_in_flight() {
    let draft = api_key_draft_after_successful_save("next-key", "steam-key");
    assert_eq!(draft.api_key, "next-key");
    assert!(draft.touched);
}

#[test]
fn onboarding_consent_returns_saved_when_persisted() {
    assert_eq!(
        complete_onboarding_consent(|| Ok::<(), String>(())),
        OnboardingConsentOutcome::Saved
    );
}

#[test]
fn onboarding_consent_returns_failed_without_panicking() {
    assert_eq!(
        complete_onboarding_consent(|| Err::<(), _>("disk")),
        OnboardingConsentOutcome::Failed
    );
}

// ------------------------------------------------------------ folders

fn stores_with(store_id: &str, value: Value) -> ClientStores {
    let snapshot = ClientStorageSnapshot {
        manifest: Default::default(),
        stores: BTreeMap::from([(store_id.to_string(), value)]),
    };
    ClientStores::new(
        ClientStorage::from_snapshot(snapshot),
        StoreContext::shipped(),
    )
}

fn saved(stores: &mut ClientStores, store_id: &str) -> Value {
    stores
        .take_all_saves()
        .into_iter()
        .find(|save| save.store_id == store_id)
        .expect("saved")
        .value
}

#[test]
fn drops_an_item_order_bucket_keyed_by_a_folder_that_no_longer_exists() {
    let mut stores = stores_with(
        CLIENT_STORE_FOLDERS,
        json!({
            "version": 1,
            "folders": [{ "id": "live", "name": "Live", "parentId": null, "platform": "steam" }],
            "itemOrder": {
                "root:steam": [{ "type": "folder", "id": "live" }],
                "live": [{ "type": "account", "id": "account-1" }],
                "ghost": [{ "type": "account", "id": "account-2" }]
            }
        }),
    );
    assert_eq!(
        stores.items_in_folder(Some("live"), "steam"),
        [ItemRef::account("account-1")]
    );
    assert!(stores.items_in_folder(Some("ghost"), "steam").is_empty());
    stores.create_folder("New", None, "steam");
    let value = saved(&mut stores, CLIENT_STORE_FOLDERS);
    assert!(value["itemOrder"].get("ghost").is_none());
}

#[test]
fn keeps_every_platform_root_bucket() {
    let stores = stores_with(
        CLIENT_STORE_FOLDERS,
        json!({
            "version": 1,
            "folders": [],
            "itemOrder": {
                "root:steam": [{ "type": "account", "id": "account-1" }],
                "root:riot": [{ "type": "account", "id": "account-2" }]
            }
        }),
    );
    assert_eq!(
        stores.items_in_folder(None, "steam"),
        [ItemRef::account("account-1")]
    );
    assert_eq!(
        stores.items_in_folder(None, "riot"),
        [ItemRef::account("account-2")]
    );
}

#[test]
fn drops_a_bare_root_prefix() {
    let mut stores = stores_with(
        CLIENT_STORE_FOLDERS,
        json!({ "version": 1, "folders": [], "itemOrder": { "root:": [{ "type": "account", "id": "account-1" }] } }),
    );
    stores.create_folder("New", None, "steam");
    assert!(saved(&mut stores, CLIENT_STORE_FOLDERS)["itemOrder"]
        .get("root:")
        .is_none());
}

fn move_store() -> ClientStores {
    stores_with(
        CLIENT_STORE_FOLDERS,
        json!({
            "version": 1,
            "folders": [
                { "id": "smurfs", "name": "Smurfs", "parentId": null, "platform": "steam" },
                { "id": "old", "name": "Old", "parentId": "smurfs", "platform": "steam" },
                { "id": "riot-folder", "name": "Ranked", "parentId": null, "platform": "riot" }
            ],
            "itemOrder": {
                "root:steam": [{ "type": "folder", "id": "smurfs" }, { "type": "account", "id": "at-root" }],
                "smurfs": [{ "type": "folder", "id": "old" }, { "type": "account", "id": "in-smurfs" }],
                "old": [{ "type": "account", "id": "in-old" }],
                "root:riot": [{ "type": "folder", "id": "riot-folder" }]
            }
        }),
    )
}

fn ids(folders: &FolderStore, platform: &str) -> Vec<String> {
    folders
        .list_folders(platform)
        .iter()
        .map(|folder| folder.id.clone())
        .collect()
}

#[test]
fn lists_only_the_folders_of_the_active_platform() {
    let stores = move_store();
    assert_eq!(ids(stores.folders(), "steam"), ["smurfs", "old"]);
    assert_eq!(ids(stores.folders(), "riot"), ["riot-folder"]);
}

#[test]
fn finds_the_folder_holding_an_account() {
    let stores = move_store();
    let folders = stores.folders();
    assert_eq!(
        folders.find_item_folder_id(&ItemRef::account("in-smurfs"), "steam"),
        Some("smurfs")
    );
    assert_eq!(
        folders.find_item_folder_id(&ItemRef::account("in-old"), "steam"),
        Some("old")
    );
}

#[test]
fn reads_an_account_at_the_platform_root_as_no_folder() {
    let stores = move_store();
    let folders = stores.folders();
    assert_eq!(
        folders.find_item_folder_id(&ItemRef::account("at-root"), "steam"),
        None
    );
    assert_eq!(
        folders.find_item_folder_id(&ItemRef::account("unknown"), "steam"),
        None
    );
}

#[test]
fn puts_a_root_account_into_a_folder() {
    let mut stores = move_store();
    stores.move_item(
        &ItemRef::account("at-root"),
        None,
        Some("old"),
        "steam",
        None,
    );
    assert_eq!(
        stores.items_in_folder(Some("old"), "steam"),
        [ItemRef::account("in-old"), ItemRef::account("at-root")]
    );
    assert_eq!(
        stores.items_in_folder(None, "steam"),
        [ItemRef::folder("smurfs")]
    );
}

#[test]
fn sends_an_account_back_to_the_root() {
    let mut stores = move_store();
    stores.move_item(
        &ItemRef::account("in-old"),
        Some("old"),
        None,
        "steam",
        None,
    );
    assert!(stores.items_in_folder(Some("old"), "steam").is_empty());
    assert_eq!(
        stores.items_in_folder(None, "steam"),
        [
            ItemRef::folder("smurfs"),
            ItemRef::account("at-root"),
            ItemRef::account("in-old")
        ]
    );
}

// ------------------------------------------------------------ personaAssignmentsForSave

fn steam() -> PersonaAssignment {
    PersonaAssignment {
        platform_id: "steam".into(),
        account_id: "steam-1".into(),
    }
}

fn discord() -> PersonaAssignment {
    PersonaAssignment {
        platform_id: "discord".into(),
        account_id: "discord-1".into(),
    }
}

struct Save {
    offered: Vec<String>,
    selection: BTreeMap<String, String>,
    accounts: BTreeMap<String, Vec<String>>,
    failed: Vec<String>,
}

impl Save {
    fn new() -> Self {
        Self {
            offered: vec!["steam".into()],
            selection: map(&[("steam", "steam-1")]),
            accounts: BTreeMap::from([("steam".into(), vec!["steam-1".into()])]),
            failed: Vec::new(),
        }
    }
    fn run(&self) -> Vec<PersonaAssignment> {
        let original = [steam(), discord()];
        persona_assignments_for_save(&AssignmentsForSave {
            original: &original,
            offered_platform_ids: &self.offered,
            selection: &self.selection,
            accounts_by_platform: &self.accounts,
            loading: false,
            failed_platform_ids: &self.failed,
        })
    }
}

fn map(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn keeps_an_assignment_for_a_platform_the_wizard_is_not_offering() {
    assert_eq!(Save::new().run(), [steam(), discord()]);
}

#[test]
fn keeps_an_assignment_when_that_platforms_account_read_failed() {
    let mut save = Save::new();
    save.offered = vec!["steam".into(), "discord".into()];
    save.selection = map(&[("steam", "steam-1"), ("discord", "discord-1")]);
    save.accounts.insert("discord".into(), Vec::new());
    save.failed = vec!["discord".into()];
    assert_eq!(save.run(), [steam(), discord()]);
}

#[test]
fn drops_an_assignment_a_successful_read_proved_is_gone() {
    let mut save = Save::new();
    save.offered = vec!["steam".into(), "discord".into()];
    save.selection = map(&[("steam", "steam-1"), ("discord", "discord-1")]);
    save.accounts.insert("discord".into(), Vec::new());
    assert_eq!(save.run(), [steam()]);
}

#[test]
fn drops_a_cleared_platform_and_keeps_the_one_it_could_not_edit() {
    let mut save = Save::new();
    save.selection = map(&[("steam", "")]);
    assert_eq!(save.run(), [discord()]);
}

#[test]
fn appends_a_newly_picked_platform_after_the_original_assignments() {
    let mut save = Save::new();
    save.offered = vec!["steam".into(), "riot".into()];
    save.selection = map(&[("steam", "steam-1"), ("riot", "riot-1")]);
    save.accounts.insert("riot".into(), vec!["riot-1".into()]);
    let riot = PersonaAssignment {
        platform_id: "riot".into(),
        account_id: "riot-1".into(),
    };
    assert_eq!(save.run(), [steam(), discord(), riot]);
}

// ------------------------------------------------------------ cachedMapStore

fn identity(_key: &str, value: &str) -> Option<String> {
    Some(value.to_string())
}

fn refuse_bad(_key: &str, value: &str) -> Option<String> {
    (value != "bad").then(|| value.to_string())
}

#[test]
fn map_store_accepts_valid_entries() {
    fn non_empty(_key: &str, value: &str) -> Option<String> {
        (!value.is_empty()).then(|| value.to_string())
    }
    let mut store = StringMapStore::new(CLIENT_STORE_ACCOUNT_CARD_NOTES, non_empty);
    store.set("a", "hello");
    assert_eq!(store.get("a"), "hello");
}

#[test]
fn map_store_get_returns_empty_for_unknown_keys() {
    let store = StringMapStore::new(CLIENT_STORE_ACCOUNT_CARD_NOTES, identity);
    assert_eq!(store.get("nonexistent"), "");
}

#[test]
fn map_store_set_stores_and_retrieves_values() {
    let mut store = StringMapStore::new(CLIENT_STORE_ACCOUNT_CARD_NOTES, identity);
    store.set("acc1", "blue");
    assert_eq!(store.get("acc1"), "blue");
}

#[test]
fn map_store_refused_value_removes_the_entry() {
    let mut store = StringMapStore::new(CLIENT_STORE_ACCOUNT_CARD_NOTES, refuse_bad);
    store.set("acc1", "good");
    assert_eq!(store.get("acc1"), "good");
    store.set("acc1", "bad");
    assert_eq!(store.get("acc1"), "");
}

#[test]
fn set_many_writes_every_id_in_one_store_write() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_COLORS, json!({}));
    stores.set_account_card_color("keep", "#000");
    let before = stores.storage().revision(CLIENT_STORE_ACCOUNT_CARD_COLORS);
    let abc: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    stores.set_account_card_colors(&abc, "#f00");
    assert_eq!(
        stores.storage().revision(CLIENT_STORE_ACCOUNT_CARD_COLORS),
        before + 1
    );
    let read = |stores: &ClientStores, ids: &[&str]| -> Vec<String> {
        ids.iter()
            .map(|id| stores.account_card_color(id).to_string())
            .collect()
    };
    assert_eq!(
        read(&stores, &["a", "b", "c", "keep"]),
        ["#f00", "#f00", "#f00", "#000"]
    );
    stores.set_account_card_colors(&abc[..2], "bad");
    assert_eq!(read(&stores, &["a", "b", "c"]), ["", "", "#f00"]);
}

#[test]
fn map_store_remove_deletes_entries() {
    let mut store = StringMapStore::new(CLIENT_STORE_ACCOUNT_CARD_NOTES, identity);
    store.set("acc1", "value");
    store.remove("acc1");
    assert_eq!(store.get("acc1"), "");
}

// ------------------------------------------------------------ accountCardColors

#[test]
fn account_card_color_is_empty_for_an_unknown_account() {
    assert_eq!(
        stores_with(CLIENT_STORE_ACCOUNT_CARD_COLORS, Value::Null).account_card_color("unknown"),
        ""
    );
}

#[test]
fn account_card_color_stores_valid_hex_colors() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_COLORS, Value::Null);
    for (id, color) in [("acc1", "#ff0000"), ("acc2", "#abc"), ("acc3", "#aabbccdd")] {
        stores.set_account_card_color(id, color);
        assert_eq!(stores.account_card_color(id), color);
    }
}

#[test]
fn account_card_color_rejects_invalid_colors() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_COLORS, Value::Null);
    for (id, color) in [
        ("acc1", "red"),
        ("acc2", "rgb(255,0,0)"),
        ("acc3", "#xyz"),
        ("acc4", "not-a-color"),
    ] {
        stores.set_account_card_color(id, color);
        assert_eq!(stores.account_card_color(id), "");
    }
}

#[test]
fn account_card_color_presets_have_the_expected_structure() {
    assert!(!ACCOUNT_CARD_COLOR_PRESETS.is_empty());
    let none = ACCOUNT_CARD_COLOR_PRESETS
        .iter()
        .find(|preset| preset.id == "none")
        .unwrap();
    assert_eq!(none.color, "");
}

// ------------------------------------------------------------ accountCardNotes

#[test]
fn account_card_note_is_empty_for_an_unknown_account() {
    assert_eq!(
        stores_with(CLIENT_STORE_ACCOUNT_CARD_NOTES, Value::Null).account_card_note("unknown"),
        ""
    );
}

#[test]
fn account_card_note_is_trimmed() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_NOTES, Value::Null);
    stores.set_account_card_note("acc1", "  hello world  ");
    assert_eq!(stores.account_card_note("acc1"), "hello world");
}

#[test]
fn account_card_note_strips_control_characters() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_NOTES, Value::Null);
    stores.set_account_card_note("acc1", "line1\u{0}line2\u{1}line3");
    assert_eq!(stores.account_card_note("acc1"), "line1 line2 line3");
}

#[test]
fn account_card_note_is_cut_at_180_characters() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_NOTES, Value::Null);
    stores.set_account_card_note("acc1", &"a".repeat(300));
    assert_eq!(stores.account_card_note("acc1").len(), 180);
}

#[test]
fn clearing_an_account_card_note_removes_it() {
    let mut stores = stores_with(CLIENT_STORE_ACCOUNT_CARD_NOTES, Value::Null);
    stores.set_account_card_note("acc1", "some note");
    assert_eq!(stores.account_card_note("acc1"), "some note");
    stores.clear_account_card_note("acc1");
    assert_eq!(stores.account_card_note("acc1"), "");
}

// ------------------------------------------------------------ store wiring

#[test]
fn a_refresh_rebuilds_the_typed_copy_and_a_local_write_does_not() {
    let mut stores = stores_with("client.settings", json!({ "animations": "off" }));
    assert_eq!(stores.settings().animations, AnimationsMode::Off);
    assert!(stores.has_stored_settings());
    let mut next = stores.settings().clone();
    next.ui_scale_percent = 500;
    stores.save_settings(&next);
    assert_eq!(stores.settings().ui_scale_percent, 150);
    stores.apply_snapshot(ClientStorageSnapshot {
        manifest: Default::default(),
        stores: BTreeMap::from([("client.settings".to_string(), json!({ "language": "ru" }))]),
    });
    assert_eq!(stores.settings().language.code(), "ru");
    assert_eq!(stores.settings().animations, AnimationsMode::System);
}
