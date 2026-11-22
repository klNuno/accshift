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
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while !condition(app) {
        assert!(
            Instant::now() < deadline,
            "condition did not become true within 10 s"
        );
        frame(app, ctx, true, false);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Frames until no external refresh runs, accounts it reloads included.
fn wait_refresh_over(app: &mut AppModel, ctx: &Context) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while app.external_refresh.running() {
        assert!(Instant::now() < deadline, "the refresh never ended");
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

/// Commands held at the mock for one test, released when it drops. A failed
/// assertion then unwinds through the release: otherwise a worker stays parked
/// at the hold and `Jobs::drop`, joining it, hangs the suite.
struct HeldCommands {
    mock: Arc<MockBackend>,
    commands: Vec<&'static str>,
}

impl HeldCommands {
    /// Declare it after the app so it drops first.
    fn new(mock: &Arc<MockBackend>) -> Self {
        Self {
            mock: mock.clone(),
            commands: Vec::new(),
        }
    }

    fn hold(&mut self, command: &'static str) {
        self.commands.push(command);
        assert!(self.mock.hold(command), "{command} could not be held");
    }

    fn release(&mut self, command: &str) {
        self.commands.retain(|held| *held != command);
        self.mock.release(command);
    }
}

impl Drop for HeldCommands {
    fn drop(&mut self) {
        for command in self.commands.drain(..) {
            self.mock.release(command);
        }
    }
}

/// Waits until a call of `command` sits at its hold in the mock.
fn wait_held(mock: &MockBackend, command: &str) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while mock.waiting(command) == 0 {
        assert!(
            Instant::now() < deadline,
            "{command} never reached the mock"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn native_external_storage_on_restore_preserves_dirty_local_values() {
    let ctx = Context::default();
    let (mut app, mock) = external_app(&ctx);
    // Drain initial boot jobs so the 2-worker pool is completely free before
    // holding commands at the mock.
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());
    frame(&mut app, &ctx, true, false);
    // Hold the manifest read, the snapshot read and the store write at the
    // mock so every phase transition is synchronized explicitly, with no
    // dependence on wall-clock timing.
    let mut held = HeldCommands::new(&mock);
    held.hold("get_storage_manifest");
    held.hold("load_client_storage_snapshot");
    held.hold("save_client_storage_store");
    // Restoration must work even if the viewport still reports focus.
    frame(&mut app, &ctx, true, true);
    frame(&mut app, &ctx, true, false);
    // Typed while the manifest read is under way, so before begin_refresh.
    wait_held(&mock, "get_storage_manifest");
    app.booted
        .as_mut()
        .unwrap()
        .stores
        .set_account_card_note("external", "local draft");
    held.release("get_storage_manifest");
    // begin_refresh records the other targets and starts the snapshot job,
    // which waits at its hold until we release it below.
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
    // The note's write, debounced or held at the mock, is still owed.
    assert!(app
        .booted()
        .unwrap()
        .stores
        .storage()
        .has_pending_save("client.account-card-notes"));
    // Let the snapshot finish first while the store write stays held, so
    // finish_refresh deterministically runs with the dirty local value still
    // pending instead of racing the write completion.
    held.release("load_client_storage_snapshot");
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.view_mode() == ViewMode::List
    });
    held.release("save_client_storage_store");
    wait_refresh_over(&mut app, &ctx);
    let b = app.booted().unwrap();
    assert_eq!(b.stores.view_mode(), ViewMode::List);
    assert_eq!(b.stores.account_card_note("external"), "local draft");
}

#[test]
fn a_failed_assertion_while_a_command_is_held_fails_instead_of_hanging() {
    // Jobs::drop joins its workers, so a worker parked at a hold the test
    // never released would keep the failing test, and the suite, waiting.
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| {
            let ctx = Context::default();
            let (mut app, mock) = external_app(&ctx);
            pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());
            frame(&mut app, &ctx, true, false);
            let mut held = HeldCommands::new(&mock);
            held.hold("get_storage_manifest");
            frame(&mut app, &ctx, true, true);
            frame(&mut app, &ctx, true, false);
            wait_held(&mock, "get_storage_manifest");
            panic!("assertion failed while get_storage_manifest is held");
        });
        let _ = done_tx.send(outcome.is_err());
    });
    assert_eq!(
        done_rx.recv_timeout(std::time::Duration::from_secs(20)),
        Ok(true),
        "the failing test hung on its held command instead of failing"
    );
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
fn native_external_themes_read_in_part_are_read_again_on_the_next_focus() {
    // Regression: the selected theme read without its locked custom base
    // took the new themes fingerprint as seen, and with the theme loaded
    // no focus read `base` again.
    let ctx = Context::default();
    let (mut app, mock) = external_app_with(&ctx, |fixture| {
        let snapshot = &mut fixture["commands"]["load_client_storage_snapshot"][0]["result"];
        snapshot["stores"]["client.settings"]["themeId"] = json!("child");
        snapshot["manifest"]["stores"]["app.themes"] = json!("theme-2");
        fixture["commands"]["get_storage_manifest"][0]["result"] = snapshot["manifest"].clone();
        fixture["commands"]["list_custom_themes"][0]["result"] = json!([
            {"schemaVersion": 1, "id": "base", "name": "Base", "colorScheme": "dark", "tokens": {"bgCard": "#112233"}},
            {"schemaVersion": 1, "id": "child", "name": "Child", "colorScheme": "dark", "extends": "base", "tokens": {}}
        ]);
    });
    assert!(mock.lock_theme("base", true));
    frame(&mut app, &ctx, true, false);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().registry.document("child").is_some()
    });
    wait_refresh_over(&mut app, &ctx);
    let b = app.booted().unwrap();
    assert_eq!(b.stores.settings().theme_id, "child");
    assert!(b.registry.document("base").is_none());
    let unbased = b.theme.vars.bg_card;

    assert!(mock.lock_theme("base", false));
    let reads = mock.seen("list_custom_themes");
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().registry.document("base").is_some()
    });
    let b = app.booted().unwrap();
    assert!(
        mock.seen("list_custom_themes") > reads,
        "a partial read taken for the whole folder"
    );
    assert!(b.registry.document("base").is_some());
    assert_ne!(b.theme.vars.bg_card, unbased);
}

#[test]
fn native_external_themes_are_read_again_after_the_stores_fail_to_read() {
    // Regression (EXTERNAL_SNAPSHOT_UNLOCK): the stores read failed before
    // the themes were read, with the new themes fingerprint already taken
    // as seen: the edited theme stayed stale on every later focus.
    let ctx = Context::default();
    let old = json!({"schemaVersion": 1, "id": "external", "name": "External", "colorScheme": "dark", "tokens": {"bgCard": "#112233"}});
    let (mut app, mock) = external_app_with(&ctx, |fixture| {
        let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
        boot["customThemes"] = json!([old.clone()]);
        boot["storageSnapshot"]["stores"]["client.settings"]["themeId"] = json!("external");
        let snapshot = &mut fixture["commands"]["load_client_storage_snapshot"][0]["result"];
        snapshot["stores"]["client.settings"]["themeId"] = json!("external");
        snapshot["manifest"]["stores"]["app.themes"] = json!("theme-2");
        fixture["commands"]["get_storage_manifest"][0]["result"] = snapshot["manifest"].clone();
        let mut next = old;
        next["tokens"]["bgCard"] = json!("#445566");
        fixture["commands"]["list_custom_themes"][0]["result"] = json!([next]);
    });
    let before = app.booted().unwrap().theme.vars.bg_card;
    mock.fail("load_client_storage_snapshot", Some("settings locked"));
    frame(&mut app, &ctx, true, false);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |_| {
        mock.seen("load_client_storage_snapshot") > 0
    });
    wait_refresh_over(&mut app, &ctx);
    assert_eq!(app.booted().unwrap().theme.vars.bg_card, before);

    mock.fail("load_client_storage_snapshot", None);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().theme.vars.bg_card != before
    });
    wait_refresh_over(&mut app, &ctx);
    let b = app.booted().unwrap();
    assert!(
        mock.seen("list_custom_themes") > 0,
        "the themes were never read"
    );
    assert_ne!(b.theme.vars.bg_card, before);
    assert_eq!(b.stores.view_mode(), ViewMode::List);
}

#[test]
fn native_external_themes_that_fail_to_read_are_read_again_on_the_next_focus() {
    // Regression (EXTERNAL_THEME_UNLOCK): the refresh took the new themes
    // fingerprint before the list failed, so no later focus read them, and
    // the settings naming the theme were sanitized to the default theme.
    let ctx = Context::default();
    let (mut app, mock) = external_app_with(&ctx, |fixture| {
        let snapshot = &mut fixture["commands"]["load_client_storage_snapshot"][0]["result"];
        snapshot["stores"]["client.settings"]["themeId"] = json!("external");
        snapshot["manifest"]["stores"]["app.themes"] = json!("theme-2");
        fixture["commands"]["get_storage_manifest"][0]["result"] = snapshot["manifest"].clone();
        fixture["commands"]["list_custom_themes"][0]["result"] = json!([{
            "schemaVersion": 1, "id": "external", "name": "External",
            "colorScheme": "dark", "tokens": {"bgCard": "#112233"}
        }]);
    });
    let before = app.booted().unwrap().theme.vars.bg_card;
    mock.fail("list_custom_themes", Some("one file locked"));
    frame(&mut app, &ctx, true, false);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.view_mode() == ViewMode::List
    });
    let b = app.booted().unwrap();
    assert_eq!(b.stores.view_mode(), ViewMode::List);
    assert_eq!(b.stores.settings().theme_id, "external");
    assert!(b.registry.document("external").is_none());
    assert_ne!(
        b.stores
            .storage()
            .manifest()
            .stores
            .get("app.themes")
            .map(String::as_str),
        Some("theme-2"),
        "themes never read taken as seen"
    );

    // The refresh whose read failed is over before the failure is lifted:
    // a focus while it runs starts nothing, and a read still in flight
    // would answer with the failure lifted.
    wait_refresh_over(&mut app, &ctx);
    mock.fail("list_custom_themes", None);
    let reads = mock.seen("list_custom_themes");
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().registry.document("external").is_some()
    });
    let b = app.booted().unwrap();
    assert!(mock.seen("list_custom_themes") > reads);
    assert!(b.registry.document("external").is_some());
    assert_eq!(b.stores.settings().theme_id, "external");
    assert_ne!(b.theme.vars.bg_card, before);
    assert_eq!(
        b.stores
            .storage()
            .manifest()
            .stores
            .get("app.themes")
            .map(String::as_str),
        Some("theme-2")
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

#[test]
fn a_boot_that_reset_a_store_or_failed_a_migration_says_so() {
    let ctx = Context::default();
    let (app, _) = external_app_with(&ctx, |fixture| {
        let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
        boot["migration"] = json!("error:disk full");
        boot["recoveredStores"] = json!(["client.folders"]);
    });
    let texts: Vec<Text> = app
        .toasts()
        .toasts()
        .iter()
        .map(|t| t.text.clone())
        .collect();
    assert_eq!(
        texts,
        [
            Text::key("toast.legacyConfigMigrationFailed"),
            Text::key("toast.storesReset"),
        ]
    );
}

#[test]
fn a_failed_boot_draws_its_error_instead_of_a_blank_window() {
    let ctx = Context::default();
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/empty.json")).unwrap();
    fixture["commands"]["get_boot_payload"] =
        json!([{ "args": {}, "error": "Could not read file settings.json: access denied" }]);
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    let mut app = AppModel::with_backend(&ctx, mock.clone(), Some(mock), Clock::wall());
    let error = app.wait_boot(&ctx).unwrap_err();
    assert!(error.contains("access denied"), "{error}");

    // The first frames bind the fonts; the screen is there once they are.
    let mut shapes = 0;
    for _ in 0..4 {
        let mut output = ctx.run_ui(input(true, false), |ui| app.frame(ui));
        output.textures_delta.clear();
        shapes = output.shapes.len();
    }
    // Background, three texts, the button and its label.
    assert!(shapes >= 6, "boot error screen drew {shapes} shapes");
}

#[test]
fn a_debounced_save_wakes_the_ui_at_its_deadline() {
    // Regression: nothing asked for a frame when a debounced write fell due,
    // so a change waited for the next input and was lost if the app ended.
    let ctx = Context::default();
    let (mut app, _) = external_app(&ctx);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.next_save_due().is_none()
    });
    // The idle app sleeps: nothing else would draw the frame of the write.
    // A frame, and how long the app then sleeps without input.
    let sleep_after_frame = |app: &mut AppModel| {
        let mut output = ctx.run_ui(input(true, false), |ui| app.frame(ui));
        output.textures_delta.clear();
        output.viewport_output[&ViewportId::ROOT].repaint_delay
    };
    let settled = Instant::now() + std::time::Duration::from_secs(5);
    while sleep_after_frame(&mut app) <= std::time::Duration::from_secs(1) {
        assert!(Instant::now() < settled, "the idle app never settles");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.booted
        .as_mut()
        .unwrap()
        .stores
        .set_account_card_note("76561198000000001", "wake me");
    let due = app.booted().unwrap().stores.next_save_due();
    assert!(due.is_some(), "the note is a debounced write");

    let delay = sleep_after_frame(&mut app);
    assert!(
        delay <= std::time::Duration::from_secs(1),
        "no frame is scheduled for the write: {delay:?}"
    );
}

#[test]
fn a_boot_sends_the_settings_snapshot_once() {
    // Regression: the event was ported and never sent.
    let ctx = Context::default();
    let (mut app, mock) = external_app(&ctx);
    let sent = |mock: &MockBackend| mock.seen("telemetry_track_settings_snapshot");
    pump_until(&mut app, &ctx, |_| sent(&mock) > 0);
    frame(&mut app, &ctx, true, false);
    assert_eq!(sent(&mock), 1);
}

fn mouse_back(app: &mut AppModel, ctx: &Context) {
    let mut raw = input(true, false);
    raw.events.push(egui::Event::PointerButton {
        pos: pos2(500.0, 400.0),
        button: egui::PointerButton::Extra1,
        pressed: true,
        modifiers: Default::default(),
    });
    let mut output = ctx.run_ui(raw, |ui| app.frame(ui));
    output.textures_delta.clear();
}

#[test]
fn the_mouse_back_button_leaves_settings_then_the_folder() {
    let ctx = Context::default();
    let (mut app, _mock) = external_app(&ctx);
    frame(&mut app, &ctx, true, false);
    let folder = {
        let b = app.booted.as_mut().unwrap();
        let tab = b.nav.active_tab.clone();
        let folder = b.stores.create_folder("Smurfs", None, &tab);
        b.accounts
            .navigate_to(&b.stores, &tab, Some(folder.id.clone()));
        folder.id
    };
    let current = |app: &AppModel| {
        let b = app.booted.as_ref().unwrap();
        b.accounts
            .navigation
            .current_folder_id()
            .map(str::to_string)
    };

    // Settings is open over the folder: back closes it and stays in the folder.
    app.toggle_settings_panel();
    mouse_back(&mut app, &ctx);
    assert!(app.settings_panel.is_none(), "back closes Settings");
    assert_eq!(current(&app), Some(folder.clone()));

    // A dialog owns the pointer: back does nothing behind it.
    app.dialogs.open_new_folder_dialog();
    mouse_back(&mut app, &ctx);
    assert_eq!(current(&app), Some(folder));
    app.dialogs.close_input_dialog();

    mouse_back(&mut app, &ctx);
    assert_eq!(current(&app), None, "back leaves the folder");
}

#[test]
fn backspace_after_escape_in_one_frame_leaves_the_folder() {
    let ctx = Context::default();
    let (mut app, _mock) = external_app(&ctx);
    frame(&mut app, &ctx, true, false);
    {
        let b = app.booted.as_mut().unwrap();
        let tab = b.nav.active_tab.clone();
        let folder = b.stores.create_folder("Smurfs", None, &tab);
        b.accounts.navigate_to(&b.stores, &tab, Some(folder.id));
    }
    let search = Id::unique(crate::app::keys::SEARCH_ID);
    ctx.memory_mut(|m| m.request_focus(search));
    frame(&mut app, &ctx, true, false);
    frame(&mut app, &ctx, true, false);
    assert!(
        crate::ui::components::focus::text_field_focused(&ctx),
        "the search has focus"
    );
    // Escape leaves the search; the Backspace after it, in the same
    // frame, is the window's again: back out of the folder.
    let mut raw = input(true, false);
    for key in [egui::Key::Escape, egui::Key::Backspace] {
        raw.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
    }
    let mut output = ctx.run_ui(raw, |ui| app.frame(ui));
    output.textures_delta.clear();
    let b = app.booted.as_ref().unwrap();
    assert_eq!(b.accounts.navigation.current_folder_id(), None);
}

fn press(app: &mut AppModel, ctx: &Context, key: egui::Key) {
    let mut raw = input(true, false);
    raw.events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    });
    let mut output = ctx.run_ui(raw, |ui| app.frame(ui));
    output.textures_delta.clear();
}

#[test]
fn escape_in_the_theme_editor_keeps_settings_open() {
    let ctx = Context::default();
    let (mut app, _mock) = external_app(&ctx);
    frame(&mut app, &ctx, true, false);
    app.toggle_settings_panel();
    frame(&mut app, &ctx, true, false);
    app.apply_settings(&ctx, settings_panel::SettingsAction::CustomizeTheme);
    frame(&mut app, &ctx, true, false);
    let editor_open = |app: &AppModel| {
        app.settings_panel
            .as_ref()
            .is_some_and(|p| p.theme_editor.is_some())
    };
    assert!(editor_open(&app), "the theme editor opened");

    press(&mut app, &ctx, egui::Key::Escape);
    frame(&mut app, &ctx, true, false);
    assert!(app.settings_panel.is_some(), "Escape keeps Settings open");
    assert!(!editor_open(&app), "Escape closes the theme editor");

    // With nothing over it, Escape closes the panel itself.
    press(&mut app, &ctx, egui::Key::Escape);
    frame(&mut app, &ctx, true, false);
    assert!(app.settings_panel.is_none(), "Escape closes Settings");
}

#[test]
fn a_path_picker_that_cannot_open_says_why() {
    let ctx = Context::default();
    let (mut app, mock) = external_app(&ctx);
    frame(&mut app, &ctx, true, false);
    app.toggle_settings_panel();
    frame(&mut app, &ctx, true, false);
    let choose = || settings_panel::SettingsAction::ChoosePlatformPath("steam".into());
    // The picker's own toasts: the backend's message, not a translation key.
    let raised = |app: &AppModel| -> Vec<(Text, accshift_app::actions::toasts::ToastType)> {
        let toasts = app.toasts().toasts().iter();
        toasts
            .filter(|toast| toast.text.key_str().is_none())
            .map(|toast| (toast.text.clone(), toast.kind))
            .collect()
    };

    // A cancel stays silent.
    mock.fail("platform_select_path", Some("Dialog cancelled"));
    app.apply_settings(&ctx, choose());
    pump_until(&mut app, &ctx, |_| mock.seen("platform_select_path") == 1);
    for _ in 0..20 {
        frame(&mut app, &ctx, true, false);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(raised(&app), [], "a cancel raises nothing");

    // A dialog that could not open reports the backend's reason.
    let why = "No file picker is available: install xdg-desktop-portal or zenity";
    mock.fail("platform_select_path", Some(why));
    app.apply_settings(&ctx, choose());
    pump_until(&mut app, &ctx, |app| !raised(app).is_empty());
    assert_eq!(
        raised(&app),
        [(
            Text::raw(why),
            accshift_app::actions::toasts::ToastType::Error
        )]
    );
}

/// The app on the Riot tab with a Riot setup waiting for the sign-in. The
/// next focus reads the poll's config writes as external changes.
fn riot_setup_app(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    let (mut app, mock) = external_app_with(ctx, |fixture| {
        let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
        let settings = &mut boot["storageSnapshot"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["riot"]);
        settings["defaultPlatformId"] = json!("riot");
        boot["storageSnapshot"]["manifest"]["stores"] = json!({
            "app.config.portable": "portable-1",
            "platform.riot.snapshots": "snapshots-1"
        });
        let mut snapshot = boot["storageSnapshot"].clone();
        snapshot["manifest"]["stores"] = json!({
            "app.config.portable": "portable-2",
            "platform.riot.snapshots": "snapshots-2"
        });
        fixture["commands"]["get_storage_manifest"][0]["result"] = snapshot["manifest"].clone();
        fixture["commands"]["load_client_storage_snapshot"][0]["result"] = snapshot;
    });
    mock.set_setup(Some(vec![json!({
        "setupId": "riot-profile-new",
        "state": "waiting_for_login",
        "accountId": "",
        "accountDisplayName": "",
        "errorMessage": ""
    })]));
    frame(&mut app, ctx, true, false);
    pump_until(&mut app, ctx, |a| a.booted().unwrap().accounts.settled());
    assert_eq!(app.booted().unwrap().nav.active_tab, "riot");
    app.apply_one(ctx, Action::AddAccount);
    pump_until(&mut app, ctx, |a| {
        a.booted().unwrap().accounts.add_flow.is_active()
    });
    (app, mock)
}

/// A Riot setup waits for the sign-in in the Riot Client, so the user comes
/// back to accshift with a click: the poll's own config writes and the new
/// profile's snapshot folder then read as external changes on that focus.
/// They must not end the setup.
#[test]
fn native_focus_refresh_after_a_riot_setup_write_keeps_the_setup() {
    let ctx = Context::default();
    let (mut app, mock) = riot_setup_app(&ctx);
    frame(&mut app, &ctx, false, false);
    frame(&mut app, &ctx, true, false);
    wait_refresh_over(&mut app, &ctx);
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());

    assert_eq!(mock.seen("get_storage_manifest"), 1);
    assert_eq!(mock.seen("platform_cancel_setup"), 0);
    assert!(app.booted().unwrap().accounts.add_flow.is_active());
}

/// The tab's title ("Riot Games") is the breadcrumb's current crumb: a click
/// on it went to the folder already open and dropped the setup.
#[test]
fn native_opening_the_current_folder_keeps_the_setup() {
    let ctx = Context::default();
    let (mut app, mock) = riot_setup_app(&ctx);
    app.apply_one(&ctx, Action::NavigateToFolder(None));
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());

    assert_eq!(mock.seen("platform_cancel_setup"), 0);
    assert!(app.booted().unwrap().accounts.add_flow.is_active());
}

/// A right click in the empty grid dropped the Riot setup the user was
/// signing in to (Dev Build `ae609bf`): the background and folder menus hold
/// nothing that conflicts with it (refresh, new folder, rename, colour).
#[test]
fn native_a_background_or_folder_menu_keeps_the_setup() {
    let ctx = Context::default();
    let (mut app, mock) = riot_setup_app(&ctx);
    app.apply_one(
        &ctx,
        Action::ContextMenu(MenuTarget::Background, pos2(600.0, 360.0)),
    );
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());

    assert!(app.dialogs.context_menu().is_some(), "the menu opened");
    assert_eq!(mock.seen("platform_cancel_setup"), 0);
    assert!(app.booted().unwrap().accounts.add_flow.is_active());
}
