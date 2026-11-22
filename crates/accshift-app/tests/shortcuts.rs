//! Global shortcuts: the combo text, the settings field, the binding table,
//! its conflicts, stale bindings and the registration service on a fake
//! registrar.

use accshift_app::settings::sanitize_settings;
use accshift_app::shortcuts::{
    account_menu_id, bindings, parse_account_menu_id, Combo, FakeRegistrar, GlobalShortcuts, Key,
    Outcome, RowStatus, ShortcutService, ShortcutTarget,
};
use accshift_app::stores::StoreContext;
use serde_json::json;

fn ctx() -> StoreContext {
    StoreContext {
        platform_ids: vec!["steam".into(), "riot".into()],
        theme_ids: vec!["dark".into()],
        preferred_languages: vec!["en".into()],
    }
}

fn combo(text: &str) -> Combo {
    Combo::parse(text).unwrap_or_else(|| panic!("{text} parses"))
}

fn account(platform: &str, id: &str) -> ShortcutTarget {
    ShortcutTarget::Account {
        platform_id: platform.into(),
        account_id: id.into(),
    }
}

// ------------------------------------------------------------ combo text

#[test]
fn combo_round_trips_through_its_portable_text() {
    for text in [
        "Ctrl+Alt+K",
        "Ctrl+Shift+F5",
        "Alt+Super+1",
        "F13",
        "Shift+F24",
        "Ctrl+Alt+Shift+Super+PageDown",
        "Ctrl+Minus",
        "Super+Space",
    ] {
        assert_eq!(combo(text).to_string(), text);
    }
}

#[test]
fn combo_parse_is_tolerant_but_formats_canonically() {
    assert_eq!(combo(" alt + ctrl + k ").to_string(), "Ctrl+Alt+K");
    assert_eq!(
        combo("Control+Option+Cmd+a").to_string(),
        "Ctrl+Alt+Super+A"
    );
    assert_eq!(combo("Win+Shift+Up").to_string(), "Shift+Super+Up");
    assert_eq!(combo("ctrl+f1").key, Key::F(1));
}

#[test]
fn combo_parse_refuses_what_cannot_be_a_global_shortcut() {
    for text in [
        "",
        "K",
        "Shift+K",
        "Ctrl",
        "Ctrl+Alt",
        "Ctrl+K+L",
        "Ctrl+Escape",
        "Ctrl+F25",
        "Ctrl+F0",
        "Ctrl+Nope",
        "Ctrl++",
    ] {
        assert_eq!(Combo::parse(text), None, "{text:?}");
    }
}

#[test]
fn combo_label_names_the_super_key_per_os() {
    let combo = combo("Ctrl+Super+K");
    assert_eq!(
        combo.label_for(accshift_app::shortcuts::Os::Windows),
        "Ctrl+Win+K"
    );
    assert_eq!(
        combo.label_for(accshift_app::shortcuts::Os::MacOs),
        "Ctrl+Cmd+K"
    );
    assert_eq!(
        combo.label_for(accshift_app::shortcuts::Os::Linux),
        "Ctrl+Super+K"
    );
    assert_eq!(
        Combo::parse("Alt+Minus")
            .unwrap()
            .label_for(accshift_app::shortcuts::Os::Windows),
        "Alt+-"
    );
}

// ------------------------------------------------------------ settings field

#[test]
fn settings_without_the_field_load_with_no_shortcut() {
    // The shape 1.0.5 and main before this change wrote.
    let old = json!({
        "language": "fr",
        "themeId": "dark",
        "closeToTray": true,
        "enabledPlatforms": ["steam", "riot"],
        "pinEnabled": false
    });
    let settings = sanitize_settings(&old, &ctx());
    assert_eq!(settings.global_shortcuts, GlobalShortcuts::default());
    assert!(settings.close_to_tray);
    assert_eq!(settings.language, accshift_app::i18n::Locale::Fr);
    // Saved again, the file keeps its old keys and gains none.
    let saved = settings.to_value();
    assert!(saved.get("globalShortcuts").is_none());
    assert_eq!(sanitize_settings(&saved, &ctx()), settings);
}

#[test]
fn settings_field_is_sanitized_tolerantly() {
    let raw = json!({
        "globalShortcuts": {
            "openPalette": "alt+ctrl+k",
            "toggleWindow": "K",
            "accounts": [
                {"platformId": "steam", "accountId": "1", "keys": "Ctrl+Alt+1"},
                {"platformId": "steam", "accountId": "1", "keys": "Ctrl+Alt+9"},
                {"platformId": "nope", "accountId": "2", "keys": "Ctrl+Alt+2"},
                {"platformId": "riot", "accountId": "", "keys": "Ctrl+Alt+3"},
                {"platformId": "riot", "accountId": "r", "keys": "garbage"},
                {"platformId": "riot", "accountId": "q", "keys": "Ctrl+Alt+4"},
                "not an object"
            ],
            "personas": [
                {"personaId": "p1", "keys": "Super+F2"},
                {"personaId": 3, "keys": "Super+F3"}
            ]
        }
    });
    let shortcuts = sanitize_settings(&raw, &ctx()).global_shortcuts;
    assert_eq!(shortcuts.open_palette, "Ctrl+Alt+K");
    assert_eq!(shortcuts.toggle_window, "");
    let accounts: Vec<_> = shortcuts
        .accounts
        .iter()
        .map(|a| {
            (
                a.platform_id.as_str(),
                a.account_id.as_str(),
                a.keys.as_str(),
            )
        })
        .collect();
    assert_eq!(
        accounts,
        [("steam", "1", "Ctrl+Alt+1"), ("riot", "q", "Ctrl+Alt+4")]
    );
    assert_eq!(shortcuts.personas.len(), 1);
    assert_eq!(shortcuts.personas[0].keys, "Super+F2");
}

#[test]
fn settings_field_round_trips_and_survives_a_wrong_type() {
    let mut shortcuts = GlobalShortcuts::default();
    shortcuts.set(&ShortcutTarget::OpenPalette, Some(combo("Ctrl+Alt+K")));
    shortcuts.set(&account("steam", "7"), Some(combo("Ctrl+Alt+7")));
    shortcuts.set(
        &ShortcutTarget::Persona {
            persona_id: "p".into(),
        },
        Some(combo("Ctrl+Alt+P")),
    );
    let raw = json!({ "globalShortcuts": shortcuts });
    assert_eq!(sanitize_settings(&raw, &ctx()).global_shortcuts, shortcuts);
    let wrong = json!({ "globalShortcuts": "Ctrl+K" });
    assert_eq!(
        sanitize_settings(&wrong, &ctx()).global_shortcuts,
        GlobalShortcuts::default()
    );
}

#[test]
fn set_replaces_and_clears_one_target() {
    let mut shortcuts = GlobalShortcuts::default();
    let target = account("steam", "7");
    shortcuts.set(&target, Some(combo("Ctrl+Alt+7")));
    shortcuts.set(&target, Some(combo("Ctrl+Alt+8")));
    assert_eq!(shortcuts.accounts.len(), 1);
    assert_eq!(shortcuts.get(&target), Some(combo("Ctrl+Alt+8")));
    shortcuts.set(&target, None);
    assert!(shortcuts.is_empty());
    shortcuts.set(&ShortcutTarget::ToggleWindow, Some(combo("F13")));
    assert_eq!(shortcuts.toggle_window, "F13");
    shortcuts.set(&ShortcutTarget::ToggleWindow, None);
    assert!(shortcuts.is_empty());
}

// ------------------------------------------------------------ stale bindings

#[test]
fn stale_persona_bindings_are_dropped() {
    let mut shortcuts = GlobalShortcuts::default();
    for id in ["kept", "gone"] {
        shortcuts.set(
            &ShortcutTarget::Persona {
                persona_id: id.into(),
            },
            Some(combo(&format!("Ctrl+Alt+{}", id[..1].to_uppercase()))),
        );
    }
    assert!(shortcuts.prune_personas(&["kept", "other"]));
    assert_eq!(shortcuts.personas.len(), 1);
    assert_eq!(shortcuts.personas[0].persona_id, "kept");
    assert!(!shortcuts.prune_personas(&["kept"]));
}

#[test]
fn missing_accounts_are_set_aside_from_a_non_empty_list_only() {
    let mut shortcuts = GlobalShortcuts::default();
    shortcuts.set(&account("steam", "1"), Some(combo("Ctrl+Alt+1")));
    shortcuts.set(&account("steam", "2"), Some(combo("Ctrl+Alt+2")));
    shortcuts.set(&account("riot", "2"), Some(combo("Ctrl+Alt+3")));
    // An empty read is not proof the accounts are gone.
    assert!(shortcuts.absent_accounts("steam", &[]).is_empty());
    // A short list sets the missing account aside without unsaving it.
    assert_eq!(shortcuts.absent_accounts("steam", &["1", "9"]), ["2"]);
    assert!(shortcuts.absent_accounts("riot", &["2"]).is_empty());
    assert_eq!(shortcuts.accounts.len(), 3);
    assert!(shortcuts.remove_account("riot", "2"));
    assert!(!shortcuts.remove_account("riot", "2"));
}

// ------------------------------------------------------------ binding table

#[test]
fn binding_table_lists_actions_then_accounts_then_personas() {
    let mut shortcuts = GlobalShortcuts::default();
    shortcuts.set(
        &ShortcutTarget::Persona {
            persona_id: "p".into(),
        },
        Some(combo("Ctrl+Alt+P")),
    );
    shortcuts.set(&account("steam", "1"), Some(combo("Ctrl+Alt+1")));
    shortcuts.set(&ShortcutTarget::ToggleWindow, Some(combo("Ctrl+Alt+H")));
    shortcuts.set(&ShortcutTarget::OpenPalette, Some(combo("Ctrl+Alt+K")));
    let table = bindings(&shortcuts);
    let targets: Vec<_> = table.iter().map(|(target, _)| target.clone()).collect();
    assert_eq!(
        targets,
        [
            ShortcutTarget::OpenPalette,
            ShortcutTarget::ToggleWindow,
            account("steam", "1"),
            ShortcutTarget::Persona {
                persona_id: "p".into()
            },
        ]
    );
}

// ------------------------------------------------------------ service

fn three_bindings() -> GlobalShortcuts {
    let mut shortcuts = GlobalShortcuts::default();
    shortcuts.set(&ShortcutTarget::OpenPalette, Some(combo("Ctrl+Alt+K")));
    shortcuts.set(&ShortcutTarget::ToggleWindow, Some(combo("Ctrl+Alt+H")));
    shortcuts.set(&account("steam", "1"), Some(combo("Ctrl+Alt+1")));
    shortcuts
}

#[test]
fn service_registers_each_binding_and_maps_its_id_back() {
    let fake = FakeRegistrar::default();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    assert_eq!(fake.registered().len(), 3);
    for (id, combo) in fake.registered() {
        let target = service.target(id).expect("id maps to a target");
        assert_eq!(
            three_bindings().get(target).as_ref(),
            Some(&combo),
            "{target:?}"
        );
    }
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Active)
    );
}

#[test]
fn service_reports_a_combination_another_app_holds() {
    let fake = FakeRegistrar::default();
    fake.take(combo("Ctrl+Alt+H"));
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    assert_eq!(
        service.status(&ShortcutTarget::ToggleWindow),
        Some(RowStatus::Taken)
    );
    assert_eq!(fake.registered().len(), 2);
    // Freed by the other app: the next sync gets it.
    fake.release(combo("Ctrl+Alt+H"));
    service.sync(&bindings(&three_bindings()));
    assert_eq!(
        service.status(&ShortcutTarget::ToggleWindow),
        Some(RowStatus::Active)
    );
}

#[test]
fn service_flags_two_actions_on_one_combination() {
    let mut shortcuts = three_bindings();
    shortcuts.set(&account("riot", "r"), Some(combo("Ctrl+Alt+K")));
    let fake = FakeRegistrar::default();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&shortcuts));
    // The first keeps the OS registration; both rows show the conflict.
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Duplicate(account("riot", "r")))
    );
    assert_eq!(
        service.status(&account("riot", "r")),
        Some(RowStatus::Duplicate(ShortcutTarget::OpenPalette))
    );
    assert_eq!(fake.registered().len(), 3);
    let palette_id = fake
        .registered()
        .into_iter()
        .find(|(_, c)| *c == combo("Ctrl+Alt+K"))
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(
        service.target(palette_id),
        Some(&ShortcutTarget::OpenPalette)
    );
}

#[test]
fn service_moves_a_changed_binding_and_drops_a_cleared_one() {
    let fake = FakeRegistrar::default();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    let mut shortcuts = three_bindings();
    service.sync(&bindings(&shortcuts));
    shortcuts.set(&ShortcutTarget::ToggleWindow, Some(combo("Ctrl+Alt+J")));
    shortcuts.set(&account("steam", "1"), None);
    service.sync(&bindings(&shortcuts));
    let mut combos: Vec<String> = fake
        .registered()
        .into_iter()
        .map(|(_, c)| c.to_string())
        .collect();
    combos.sort();
    assert_eq!(combos, ["Ctrl+Alt+J", "Ctrl+Alt+K"]);
    assert_eq!(service.status(&account("steam", "1")), None);
}

#[test]
fn service_pauses_while_a_combination_is_captured() {
    let fake = FakeRegistrar::default();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    service.set_paused(true);
    assert!(fake.registered().is_empty());
    assert_eq!(service.target(1), None);
    service.set_paused(false);
    assert_eq!(fake.registered().len(), 3);
}

#[test]
fn service_without_os_support_marks_every_row_unavailable() {
    let fake = FakeRegistrar::unavailable();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    assert!(!service.available());
    assert!(fake.registered().is_empty());
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Unavailable)
    );
}

#[test]
fn account_menu_id_names_its_account_and_parses_back() {
    let id = account_menu_id("steam", "76561198100000001");
    assert_eq!(
        parse_account_menu_id(&id),
        Some(account("steam", "76561198100000001"))
    );
    // A user platform id with a colon keeps the account part whole.
    assert_eq!(
        parse_account_menu_id(&account_menu_id("custom", "a:b")),
        Some(account("custom", "a:b"))
    );
    assert_eq!(parse_account_menu_id("account.rename"), None);
    assert_eq!(parse_account_menu_id("account.shortcut:steam"), None);
}

/// A layer that answers later (the Wayland portal): a row is pending until
/// it answers, shows a refusal, shows the trigger it bound, and only a bound
/// id fires.
#[test]
fn service_follows_what_a_late_layer_answers() {
    let fake = FakeRegistrar::default();
    fake.answer(combo("Ctrl+Alt+K"), Outcome::Pending);
    fake.answer(combo("Ctrl+Alt+H"), Outcome::Refused);
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    let palette = fake.id_of(&combo("Ctrl+Alt+K")).unwrap();
    let window = fake.id_of(&combo("Ctrl+Alt+H")).unwrap();
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Pending)
    );
    assert_eq!(service.target(palette), None);
    assert_eq!(
        service.status(&ShortcutTarget::ToggleWindow),
        Some(RowStatus::Failed)
    );
    assert_eq!(service.target(window), None);
    fake.answer(combo("Ctrl+Alt+K"), Outcome::Active(Some("Meta+K".into())));
    service.refresh();
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Active)
    );
    assert_eq!(
        service.trigger(&ShortcutTarget::OpenPalette).as_deref(),
        Some("Meta+K")
    );
    assert_eq!(service.target(palette), Some(&ShortcutTarget::OpenPalette));
}

/// A probe that answers after the first sync (the portal's, in the
/// background): a refresh follows it either way, nothing stays stuck.
#[test]
fn service_follows_a_layer_whose_availability_settles_late() {
    let fake = FakeRegistrar::unavailable();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Unavailable)
    );
    fake.set_available(true);
    service.refresh();
    assert!(service.available());
    assert_eq!(fake.registered().len(), 3);
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Active)
    );
    fake.set_available(false);
    service.refresh();
    assert!(fake.registered().is_empty());
    assert_eq!(
        service.status(&ShortcutTarget::OpenPalette),
        Some(RowStatus::Unavailable)
    );
}

/// A held row (an account a partial list lacks) stays registered, so the
/// portal's table keeps it, but its id never runs, and it yields its
/// combination to a shown row.
#[test]
fn service_keeps_a_held_row_registered_but_never_runs_it() {
    let fake = FakeRegistrar::default();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    let gone = account("steam", "gone");
    let mut table = bindings(&three_bindings());
    table.push((gone.clone(), combo("Ctrl+Alt+3")));
    service.sync_holding(&table, std::slice::from_ref(&gone));
    let held = fake
        .id_of(&combo("Ctrl+Alt+3"))
        .expect("held stays registered");
    assert_eq!(service.target(held), None);
    // A shown row takes the held row's combination: no duplicate, the
    // shown row registers it.
    let mut table = bindings(&three_bindings());
    table.insert(0, (gone.clone(), combo("Ctrl+Alt+H")));
    service.sync_holding(&table, std::slice::from_ref(&gone));
    assert_eq!(
        service.status(&ShortcutTarget::ToggleWindow),
        Some(RowStatus::Active)
    );
    let toggle = fake.id_of(&combo("Ctrl+Alt+H")).unwrap();
    assert_eq!(service.target(toggle), Some(&ShortcutTarget::ToggleWindow));
}

/// The portal keeps a trigger the user set in the system settings: the
/// service opens those settings only where the layer can.
#[test]
fn service_opens_the_os_settings_only_where_the_layer_can() {
    let fake = FakeRegistrar::default();
    let mut service = ShortcutService::new(Box::new(fake.clone()));
    service.sync(&bindings(&three_bindings()));
    assert!(!service.configurable());
    service.configure();
    assert_eq!(fake.configured(), 0);
    fake.set_configurable(true);
    assert!(service.configurable());
    service.configure();
    assert_eq!(fake.configured(), 1);
}
