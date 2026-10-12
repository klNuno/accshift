//! `registry.test.ts`, `descriptors.test.ts`, `genericAdapter.test.ts`,
//! `battle-net/adapter.test.ts` and `ubisoft/adapter.test.ts` ported.

mod support;

use accshift_app::platforms::adapter::AdapterKind;
use accshift_app::platforms::descriptors::{
    install_descriptor_file, preview_descriptor_file, reload_user_platforms, remove_descriptor,
    render_template, select_descriptor_file, PlatformDescriptor,
};
use accshift_app::platforms::generic::{
    default_to_account, mask_email, to_battle_net_account, to_ubisoft_account, GenericConfig,
};
use accshift_app::platforms::registry::{accent_for, PlatformRegistry, CHROME_ORDER};
use accshift_app::platforms::{resolve_path_placeholder, PathPlaceholder, RuntimeOs};
use serde_json::{json, Value};

fn acme() -> PlatformDescriptor {
    PlatformDescriptor::from_value(&json!({
        "id": "acme",
        "name": "Acme Launcher",
        "os": {
            "windows": {
                "executable": {
                    "candidates": [
                        { "kind": "registry" },
                        { "kind": "path", "template": "${ProgramFiles}/Acme/Acme.exe" }
                    ]
                }
            },
            "linux": {}
        }
    }))
    .unwrap()
}

// ------------------------------------------------ registry: shipped platforms

#[test]
fn shipped_takes_description_from_descriptor() {
    let registry = PlatformRegistry::shipped();
    let gog = registry.definition("gog").unwrap();
    assert_eq!(gog.name, "GOG Galaxy");
    assert_eq!(gog.supported_os, vec![RuntimeOs::Windows]);
    assert!(
        resolve_path_placeholder(gog.path_placeholder.as_ref(), RuntimeOs::Windows)
            .contains("GalaxyClient.exe")
    );
}

#[test]
fn shipped_keeps_platforms_no_descriptor_covers() {
    let registry = PlatformRegistry::shipped();
    let steam = registry.definition("steam").unwrap();
    assert_eq!(steam.name, "Steam");
    assert_eq!(
        steam.supported_os,
        vec![RuntimeOs::Windows, RuntimeOs::Linux, RuntimeOs::Macos]
    );
}

#[test]
fn shipped_none_user_provided() {
    let registry = PlatformRegistry::shipped();
    assert!(registry.defs().iter().all(|def| !def.user_provided));
    // Display order is the chrome order.
    let ids: Vec<&str> = registry.defs().iter().map(|def| def.id.as_str()).collect();
    assert_eq!(ids, CHROME_ORDER);
}

// ------------------------------------------------ registry: user platforms

#[test]
fn user_platform_with_no_entry() {
    let mut registry = PlatformRegistry::shipped();
    registry.register_user_platforms(&[acme()]);
    let def = registry.definition("acme").unwrap();
    assert_eq!(def.name, "Acme Launcher");
    assert!(def.user_provided);
    assert!(def.implemented);
    assert_eq!(def.supported_os, vec![RuntimeOs::Windows, RuntimeOs::Linux]);
    assert!(def.accent.starts_with("hsl("));
    let adapter = registry.adapter("acme").unwrap();
    assert_eq!(
        adapter.kind(),
        &AdapterKind::Generic(GenericConfig::user("Acme Launcher"))
    );
}

#[test]
fn user_platform_spells_path_like_the_system() {
    let mut registry = PlatformRegistry::shipped();
    registry.register_user_platforms(&[acme()]);
    let def = registry.definition("acme").unwrap();
    assert_eq!(
        resolve_path_placeholder(def.path_placeholder.as_ref(), RuntimeOs::Windows),
        "%ProgramFiles%\\Acme\\Acme.exe"
    );
    assert_eq!(
        def.path_placeholder,
        Some(PathPlaceholder::PerOs(vec![(
            RuntimeOs::Windows,
            "%ProgramFiles%\\Acme\\Acme.exe".to_string()
        )]))
    );
}

#[test]
fn user_platform_disappears_with_its_file() {
    let mut registry = PlatformRegistry::shipped();
    registry.register_user_platforms(&[acme()]);
    registry.register_user_platforms(&[]);
    assert!(registry.definition("acme").is_none());
    assert!(registry.adapter("acme").is_none());
    assert_eq!(registry.definition("gog").unwrap().name, "GOG Galaxy");
}

#[test]
fn user_platform_cannot_take_over_shipped() {
    let mut registry = PlatformRegistry::shipped();
    let mut fake = acme();
    fake.id = "steam".to_string();
    fake.name = "Not Steam".to_string();
    registry.register_user_platforms(&[fake]);
    assert_eq!(registry.definition("steam").unwrap().name, "Steam");
    assert_eq!(
        registry
            .defs()
            .iter()
            .filter(|def| def.id == "steam")
            .count(),
        1
    );
    assert_eq!(
        registry.adapter("steam").unwrap().kind(),
        &AdapterKind::Steam
    );
}

#[test]
fn user_platform_keeps_its_colour() {
    let mut registry = PlatformRegistry::shipped();
    registry.register_user_platforms(&[acme()]);
    let first = registry.definition("acme").unwrap().accent.clone();
    registry.register_user_platforms(&[]);
    registry.register_user_platforms(&[acme()]);
    assert_eq!(registry.definition("acme").unwrap().accent, first);
    assert_eq!(first, accent_for("acme"));
}

// ------------------------------------------------ descriptors: folder calls

/// A full engine descriptor for `acme`: the embedded GOG one renamed, so it
/// decodes as the engine's own type.
fn acme_core() -> Value {
    let (loaded, _) = accshift_core::platforms::descriptor::load_embedded();
    let gog = loaded
        .iter()
        .find(|descriptor| descriptor.id == "gog")
        .unwrap();
    let mut value = serde_json::to_value(gog).unwrap();
    value["id"] = json!("acme");
    value["name"] = json!("Acme Launcher");
    value
}

fn report(loaded: Vec<Value>) -> Value {
    json!({ "dir": "C:/data/platforms", "loaded": loaded, "skipped": [], "rejected": [] })
}

#[test]
fn descriptors_reload_registers_folder() {
    let backend = support::scripted("demo");
    backend.push("reload_user_platforms", Ok(report(vec![acme_core()])));
    let mut registry = PlatformRegistry::shipped();
    let result = reload_user_platforms(&mut registry, &backend).unwrap();
    assert_eq!(backend.calls("reload_user_platforms").len(), 1);
    assert_eq!(result.dir, "C:/data/platforms");
    assert!(registry.definition("acme").unwrap().user_provided);
}

#[test]
fn descriptors_remove_drops_platform() {
    let backend = support::scripted("demo");
    backend.push("reload_user_platforms", Ok(report(vec![acme_core()])));
    backend.push("descriptor_remove", Ok(report(vec![])));
    let mut registry = PlatformRegistry::shipped();
    reload_user_platforms(&mut registry, &backend).unwrap();
    remove_descriptor(&mut registry, &backend, "acme").unwrap();
    assert_eq!(
        backend.calls("descriptor_remove"),
        vec!["(\"acme\",)".to_string()]
    );
    assert!(registry.definition("acme").is_none());
}

#[test]
fn descriptors_install_registers_without_second_call() {
    let backend = support::scripted("demo");
    backend.push("descriptor_install_file", Ok(report(vec![acme_core()])));
    let mut registry = PlatformRegistry::shipped();
    install_descriptor_file(&mut registry, &backend, "C:/downloads/acme.json").unwrap();
    assert_eq!(backend.calls("descriptor_install_file").len(), 1);
    assert!(backend.calls("reload_user_platforms").is_empty());
    assert_eq!(registry.definition("acme").unwrap().name, "Acme Launcher");
}

#[test]
fn descriptors_cancelled_picker_is_nothing() {
    let backend = support::scripted("demo");
    backend.push("descriptor_select_file", Err("cancelled"));
    assert_eq!(select_descriptor_file(&backend), None);
}

#[test]
fn descriptors_rejection_reaches_caller() {
    let backend = support::scripted("demo");
    backend.push(
        "descriptor_preview_file",
        Err("acme.json: schemaVersion: expected 1"),
    );
    let error = preview_descriptor_file(&backend, "C:/downloads/acme.json").unwrap_err();
    assert_eq!(error.message, "acme.json: schemaVersion: expected 1");
}

// ------------------------------------------------ genericAdapter

#[test]
fn generic_converts_ms_to_seconds() {
    let account = default_to_account(&json!({
        "accountId": "acct-1", "label": "main", "lastUsedAt": 1_760_000_000_999u64
    }));
    assert_eq!(account.last_login_at_sec, Some(1_760_000_000.0));
}

#[test]
fn generic_missing_stamp_is_none() {
    assert_eq!(
        default_to_account(&json!({ "accountId": "acct-1", "label": "main" })).last_login_at_sec,
        None
    );
    assert_eq!(
        default_to_account(&json!({ "accountId": "acct-1", "label": "main", "lastUsedAt": null }))
            .last_login_at_sec,
        None
    );
}

#[test]
fn generic_empty_label_falls_back_to_id() {
    let account = default_to_account(&json!({ "accountId": "acct-1", "label": "" }));
    assert_eq!(account.display_name, "acct-1");
}

// ------------------------------------------------ battle-net

#[test]
fn battle_net_converts_ms_to_seconds() {
    let account = to_battle_net_account(&json!({
        "email": "player@example.com", "battleTag": "Player#1234", "lastLoginAt": 1_760_000_000_999u64
    }));
    assert_eq!(account.last_login_at_sec, Some(1_760_000_000.0));
}

#[test]
fn battle_net_missing_stamp_is_none() {
    assert_eq!(
        to_battle_net_account(&json!({ "email": "player@example.com" })).last_login_at_sec,
        None
    );
    assert_eq!(
        to_battle_net_account(&json!({ "email": "player@example.com", "lastLoginAt": null }))
            .last_login_at_sec,
        None
    );
}

#[test]
fn battle_net_discovered_account_without_metadata() {
    let account = to_battle_net_account(&json!({
        "email": "fresh@example.com", "battleTag": "", "lastLoginAt": null
    }));
    assert_eq!(account.display_name, "fresh");
    assert_eq!(account.last_login_at_sec, None);
}

#[test]
fn battle_net_prefers_battle_tag_name() {
    let account =
        to_battle_net_account(&json!({ "email": "player@example.com", "battleTag": "Nova#1234" }));
    assert_eq!(account.display_name, "Nova");
    assert_eq!(account.id, "player@example.com");
}

// ------------------------------------------------ ubisoft

#[test]
fn ubisoft_converts_ms_to_seconds() {
    let account = to_ubisoft_account(&json!({
        "uuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "label": "main",
        "lastUsedAt": 1_760_000_000_999u64, "snapshotSaved": true
    }));
    assert_eq!(account.last_login_at_sec, Some(1_760_000_000.0));
}

#[test]
fn ubisoft_missing_stamp_is_none() {
    let account = to_ubisoft_account(&json!({
        "uuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "label": "main", "snapshotSaved": false
    }));
    assert_eq!(account.last_login_at_sec, None);
}

#[test]
fn ubisoft_shortens_uuid_without_label() {
    let account = to_ubisoft_account(&json!({
        "uuid": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", "label": "", "snapshotSaved": false
    }));
    assert_eq!(account.display_name, "aaaaaaaa");
}

// ------------------------------------------------ Rust-only checks

#[test]
fn render_template_per_system() {
    let template = "${LOCALAPPDATA}/Acme\\${ProgramFiles(x86)}/x";
    assert_eq!(
        render_template(template, RuntimeOs::Windows),
        "%LOCALAPPDATA%\\Acme\\%ProgramFiles(x86)%\\x"
    );
    assert_eq!(
        render_template(template, RuntimeOs::Linux),
        "$LOCALAPPDATA/Acme/$ProgramFiles(x86)/x"
    );
    // Not a variable: left as written.
    assert_eq!(render_template("${}/a", RuntimeOs::Linux), "${}/a");
}

#[test]
fn battle_net_masks_emails_in_switch_logs() {
    assert_eq!(mask_email("player@example.com"), "pla\u{2026}");
    let config = GenericConfig::shipped("battle-net").unwrap();
    assert_eq!(
        config.mask_switch_log_id("ab@x.y"),
        Some("ab\u{2026}".to_string())
    );
    assert!(!config.supports_account_labels());
    assert_eq!(
        GenericConfig::shipped("gog")
            .unwrap()
            .mask_switch_log_id("a"),
        None
    );
}

#[test]
fn every_shipped_platform_has_an_adapter() {
    let registry = PlatformRegistry::shipped();
    for def in registry.defs() {
        let adapter = registry.adapter(&def.id).unwrap();
        assert_eq!(adapter.id(), def.id);
        assert!(adapter.no_accounts_toast().is_some(), "{}", def.id);
    }
}
