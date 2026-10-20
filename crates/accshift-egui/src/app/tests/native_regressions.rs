use super::*;
use egui::{pos2, vec2, Id, RawInput, Rect, Sense, ViewportId};
use serde_json::json;

fn external_app(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    external_app_with(ctx, |_| {})
}

fn external_app_with(
    ctx: &Context,
    configure: impl FnOnce(&mut Value),
) -> (AppModel, Arc<MockBackend>) {
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/empty.json")).unwrap();
    fixture["spec"]["switchDelayMs"] = json!(0);
    let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
    boot["storageSnapshot"]["stores"]["client.settings"]["themeId"] = json!("dark");
    boot["storageSnapshot"]["stores"]["client.settings"]["animations"] = json!("off");
    let mut snapshot = boot["storageSnapshot"].clone();
    snapshot["stores"]["client.settings"]["language"] = json!("fr");
    snapshot["stores"]["client.view-mode"] = json!("list");
    snapshot["stores"]["client.account-card-notes"] = json!({"external": "external note"});
    snapshot["manifest"]["stores"] = json!({
        "client.settings": "settings-2",
        "client.view-mode": "view-2",
        "client.account-card-notes": "notes-2",
        "app.config.local": "config-2"
    });
    fixture["commands"]["get_storage_manifest"][0]["result"] = snapshot["manifest"].clone();
    fixture["commands"]["load_client_storage_snapshot"][0]["result"] = snapshot;
    configure(&mut fixture);
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    let mut app = AppModel::with_backend(ctx, mock.clone(), Some(mock.clone()), Clock::wall());
    app.wait_boot(ctx).unwrap();
    (app, mock)
}

#[test]
fn native_external_settings_disable_the_active_platform() {
    let ctx = Context::default();
    let (mut app, _) = external_app_with(&ctx, |fixture| {
        let settings = &mut fixture["commands"]["load_client_storage_snapshot"][0]["result"]
            ["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["riot"]);
        settings["defaultPlatformId"] = json!("riot");
    });
    frame(&mut app, &ctx, true, false);
    app.booted
        .as_mut()
        .unwrap()
        .accounts
        .navigation
        .set_current_folder_id(Some("old-steam-folder".into()));
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().nav.active_tab == "riot"
    });
    let b = app.booted().unwrap();
    assert_eq!(b.nav.active_tab, "riot");
    assert_eq!(b.accounts.loading.loader.adapter_id(), Some("riot"));
    assert_eq!(b.accounts.navigation.current_folder_id(), None);
}

#[test]
fn native_external_custom_theme_edits_reapply_with_the_same_theme_count() {
    let ctx = Context::default();
    let (mut app, _) = external_app_with(&ctx, |fixture| {
        let theme = json!({"schemaVersion": 1, "id": "custom-test", "name": "Test", "colorScheme": "dark", "tokens": {"bgCard": "#112233"}});
        let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
        boot["customThemes"] = json!([theme.clone()]);
        boot["storageSnapshot"]["stores"]["client.settings"]["themeId"] = json!("custom-test");
        let snapshot = &mut fixture["commands"]["load_client_storage_snapshot"][0]["result"];
        snapshot["stores"]["client.settings"]["themeId"] = json!("custom-test");
        snapshot["manifest"]["stores"]["app.themes"] = json!("theme-2");
        fixture["commands"]["get_storage_manifest"][0]["result"] = snapshot["manifest"].clone();
        let mut next = theme;
        next["tokens"]["bgCard"] = json!("#445566");
        fixture["commands"]["list_custom_themes"][0]["result"] = json!([next]);
    });
    let before = app.booted().unwrap().theme.vars.bg_card;
    assert_eq!(
        app.booted().unwrap().stores.settings().theme_id,
        "custom-test"
    );
    frame(&mut app, &ctx, true, false);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().theme.vars.bg_card != before
    });
    assert_ne!(app.booted().unwrap().theme.vars.bg_card, before);
    assert_eq!(app.booted().unwrap().registry.all_themes().len(), 7);
}

fn input(focused: bool, minimized: bool) -> RawInput {
    let mut input = RawInput {
        focused,
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 680.0))),
        ..Default::default()
    };
    let vp = input.viewports.get_mut(&ViewportId::ROOT).unwrap();
    vp.focused = Some(focused);
    vp.minimized = Some(minimized);
    input
}

fn frame(app: &mut AppModel, ctx: &Context, focused: bool, minimized: bool) {
    let mut output = ctx.run_ui(input(focused, minimized), |ui| app.frame(ui));
    output.textures_delta.clear();
}

fn pump_until(app: &mut AppModel, ctx: &Context, condition: impl Fn(&AppModel) -> bool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(2);
    while !condition(app) && Instant::now() < deadline {
        frame(app, ctx, true, false);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn native_external_storage_on_focus_refreshes_stores_and_accounts() {
    let ctx = Context::default();
    let (mut app, mock) = external_app(&ctx);
    // Finish the initial account load before the external CLI switch.
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());
    mock.platform_switch_account("steam", "external", &json!({}))
        .unwrap();
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        let b = a.booted().unwrap();
        b.stores.account_card_note("external") == "external note"
            && b.accounts.loading.loader.current_account() == "external"
    });
    let b = app.booted().unwrap();
    assert_eq!(b.stores.account_card_note("external"), "external note");
    assert_eq!(b.stores.view_mode(), ViewMode::List);
    assert_eq!(b.locale.code(), "fr");
    assert_eq!(b.accounts.loading.loader.current_account(), "external");
}

#[test]
fn native_external_storage_on_restore_preserves_dirty_local_values() {
    let ctx = Context::default();
    let (mut app, _) = external_app(&ctx);
    frame(&mut app, &ctx, true, false);
    app.booted
        .as_mut()
        .unwrap()
        .stores
        .set_account_card_note("external", "local draft");
    // Restoration must work even if the viewport still reports focus.
    frame(&mut app, &ctx, true, true);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.view_mode() == ViewMode::List
    });
    let b = app.booted().unwrap();
    assert_eq!(b.stores.view_mode(), ViewMode::List);
    assert_eq!(b.stores.account_card_note("external"), "local draft");
}

#[test]
fn native_external_snapshot_keeps_edits_made_after_the_manifest_read() {
    let ctx = Context::default();
    let (mut app, mock) = external_app(&ctx);
    frame(&mut app, &ctx, true, false);
    mock.latency(30);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    // Other targets are recorded by begin_refresh before the snapshot job
    // starts. This observes that boundary without relying on a sleep race.
    pump_until(&mut app, &ctx, |a| {
        a.booted()
            .unwrap()
            .stores
            .storage()
            .manifest()
            .stores
            .get("app.config.local")
            .is_some_and(|v| v == "config-2")
    });
    assert_eq!(app.booted().unwrap().stores.view_mode(), ViewMode::Grid);
    app.booted
        .as_mut()
        .unwrap()
        .stores
        .set_account_card_note("external", "typed during load");
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.view_mode() == ViewMode::List
    });
    let b = app.booted().unwrap();
    assert_eq!(b.stores.view_mode(), ViewMode::List);
    assert_eq!(b.stores.account_card_note("external"), "typed during load");
}

#[test]
fn native_external_theme_read_failure_does_not_discard_other_store_changes() {
    let ctx = Context::default();
    let (mut app, mock) = external_app_with(&ctx, |fixture| {
        fixture["commands"]["get_storage_manifest"][0]["result"]["stores"]["app.themes"] =
            json!("theme-2");
    });
    mock.fail("list_custom_themes", Some("missing theme folder"));
    frame(&mut app, &ctx, true, false);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.view_mode() == ViewMode::List
    });
    assert_eq!(app.booted().unwrap().stores.view_mode(), ViewMode::List);
    assert_eq!(
        app.booted().unwrap().stores.account_card_note("external"),
        "external note"
    );
}

#[test]
fn native_external_account_change_without_client_stores_keeps_the_manifest_schema() {
    let ctx = Context::default();
    let (mut app, mock) = external_app_with(&ctx, |fixture| {
        fixture["commands"]["get_storage_manifest"][0]["result"] = json!({
            "schemaVersion": 1, "stores": {"app.config.local": "config-2"}
        });
    });
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());
    // A config-only refresh must not need a client snapshot at all.
    mock.fail(
        "load_client_storage_snapshot",
        Some("unexpected snapshot read"),
    );
    mock.platform_switch_account("steam", "external", &json!({}))
        .unwrap();
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted()
            .unwrap()
            .accounts
            .loading
            .loader
            .current_account()
            == "external"
    });
    let b = app.booted().unwrap();
    assert_eq!(b.accounts.loading.loader.current_account(), "external");
    assert_eq!(b.stores.storage().manifest().schema_version, 1);
    assert_eq!(b.stores.view_mode(), ViewMode::Grid);
}

#[test]
fn native_text_input_keeps_focus_for_vertical_arrows() {
    use crate::ui::components::controls::text_input::{self, InputKind, TextInputProps};
    for key in [egui::Key::ArrowDown, egui::Key::ArrowUp] {
        let ctx = Context::default();
        let (app, _) = external_app(&ctx);
        let vars = &app.booted().unwrap().theme.vars;
        let style = text_input::settings_style(vars);
        let field = Id::unique("settings-input");
        let mut value = String::from("editable");
        for pass in 0..4 {
            let mut raw = input(true, false);
            if pass == 3 {
                raw.events.push(Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(raw, |ui| {
                for (name, y) in [("above", 20.0), ("below", 220.0)] {
                    ui.interact(
                        Rect::from_min_size(pos2(50.0, y), vec2(200.0, 40.0)),
                        Id::unique(name),
                        Sense::click(),
                    );
                }
                if pass == 0 {
                    ui.memory_mut(|m| m.request_focus(field));
                }
                text_input::text_input(
                    ui,
                    Rect::from_min_size(pos2(50.0, 120.0), vec2(200.0, 40.0)),
                    field,
                    &mut value,
                    &TextInputProps {
                        style: &style,
                        kind: InputKind::Text,
                        placeholder: "",
                        label: "Settings",
                        max_len: None,
                        enabled: true,
                        vars,
                        dark: true,
                    },
                );
            });
            output.textures_delta.clear();
        }
        assert!(
            ctx.memory(|m| m.has_focus(field)),
            "{key:?} moved focus out of the text input"
        );
        assert_eq!(value, "editable");
    }
}
