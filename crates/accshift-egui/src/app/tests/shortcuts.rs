//! Global shortcuts in the app: a delivered id runs the same path as the
//! click or the key it stands for, on the mock with its fake registrar.

use super::*;
use accshift_app::shortcuts::{Combo, ShortcutTarget};
use egui::{pos2, vec2, Event, Modifiers, RawInput, Rect, ViewportId};
use serde_json::json;

use crate::app::shortcuts::{read_press, Press, Shortcuts, WindowOp, MOCK_COMMAND};

const PIN_HASH: &str =
    "000102030405060708090a0b0c0d0e0f:e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";
const STEAM_OTHER: &str = "76561198100000002";
/// Marked expired by the dev scenario's session marks.
const STEAM_EXPIRED: &str = "76561198100000007";

/// The dev scenario with `settings` merged over its `client.settings`, the
/// personas seeded, and the fake registrar refusing `taken`.
fn app_with(ctx: &Context, settings: Value, taken: &str) -> (AppModel, Arc<MockBackend>) {
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/dev.json")).unwrap();
    fixture["spec"]["switchDelayMs"] = json!(0);
    let personas = json!([{
        "id": "persona-main",
        "name": "Main",
        "color": "",
        "assignments": [{"platformId": "steam", "accountId": STEAM_OTHER}],
    }]);
    let commands = &mut fixture["commands"];
    for (command, path) in [
        (
            "get_boot_payload",
            &["result", "storageSnapshot", "stores"][..],
        ),
        ("load_client_storage_snapshot", &["result", "stores"][..]),
    ] {
        let mut stores = &mut commands[command][0];
        for part in path {
            stores = &mut stores[*part];
        }
        let store = &mut stores["client.settings"];
        store["animations"] = json!("off");
        store["personasEnabled"] = json!(true);
        for (key, value) in settings.as_object().unwrap() {
            store[key] = value.clone();
        }
        stores["client.personas"] = personas.clone();
    }
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    let mut app = AppModel::with_backend(ctx, mock.clone(), Some(mock.clone()), Clock::wall());
    app.wait_boot(ctx).unwrap();
    let failures = if taken.is_empty() {
        Vec::new()
    } else {
        vec![(MOCK_COMMAND.to_string(), taken.to_string())]
    };
    app.shortcuts = Shortcuts::mock(&failures);
    (app, mock)
}

fn bound(settings: Value) -> Value {
    json!({ "globalShortcuts": settings })
}

fn raw(events: Vec<Event>) -> RawInput {
    let mut input = RawInput {
        focused: true,
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 680.0))),
        events,
        ..Default::default()
    };
    let vp = input.viewports.get_mut(&ViewportId::ROOT).unwrap();
    vp.focused = Some(true);
    vp.minimized = Some(false);
    input
}

fn frame_with(app: &mut AppModel, ctx: &Context, events: Vec<Event>) {
    let mut output = ctx.run_ui(raw(events), |ui| app.frame(ui));
    output.textures_delta.clear();
}

fn frame(app: &mut AppModel, ctx: &Context) {
    frame_with(app, ctx, Vec::new());
}

fn pump_until(app: &mut AppModel, ctx: &Context, condition: impl Fn(&AppModel) -> bool) -> bool {
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    while !condition(app) && Instant::now() < deadline {
        frame(app, ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    condition(app)
}

/// Boot frames until the accounts and the service settled.
fn settle(app: &mut AppModel, ctx: &Context) {
    assert!(pump_until(app, ctx, |a| a
        .booted()
        .unwrap()
        .accounts
        .settled()));
    frame(app, ctx);
    frame(app, ctx);
}

/// The fake registrar's id for `combo`, as the OS would deliver it.
fn id_of(app: &AppModel, combo: &str) -> u32 {
    app.fake_registrar()
        .unwrap()
        .id_of(&Combo::parse(combo).unwrap())
        .unwrap_or_else(|| panic!("{combo} is not registered"))
}

fn key(key: egui::Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn has_toast(app: &AppModel, text_key: &str) -> bool {
    app.toasts()
        .toasts()
        .iter()
        .any(|t| t.text == accshift_app::text::Text::key(text_key))
}

#[test]
fn no_binding_registers_nothing_and_builds_no_service() {
    let ctx = Context::default();
    let (mut app, _) = app_with(&ctx, json!({}), "");
    settle(&mut app, &ctx);
    assert!(app.shortcuts.service.is_none());
    assert!(app.fake_registrar().unwrap().registered().is_empty());
}

#[test]
fn the_palette_shortcut_opens_the_palette_like_its_key() {
    let ctx = Context::default();
    let (mut app, _) = app_with(&ctx, bound(json!({ "openPalette": "Ctrl+Alt+K" })), "");
    settle(&mut app, &ctx);
    assert!(app.palette.is_none());
    let id = id_of(&app, "Ctrl+Alt+K");
    assert_eq!(app.hotkey(&ctx, id), WindowOp::Reveal);
    frame(&mut app, &ctx);
    assert!(app.palette.is_some());
}

#[test]
fn the_toggle_shortcut_hands_the_window_a_toggle() {
    let ctx = Context::default();
    let (mut app, _) = app_with(&ctx, bound(json!({ "toggleWindow": "Ctrl+Alt+H" })), "");
    settle(&mut app, &ctx);
    let id = id_of(&app, "Ctrl+Alt+H");
    assert_eq!(app.hotkey(&ctx, id), WindowOp::Toggle);
    assert_eq!(app.hotkey(&ctx, id + 1000), WindowOp::None);
}

#[test]
fn an_account_shortcut_switches_through_the_card_path_with_its_session_notice() {
    let ctx = Context::default();
    let (mut app, mock) = app_with(
        &ctx,
        bound(
            json!({ "accounts": [{"platformId": "steam", "accountId": STEAM_EXPIRED, "keys": "Ctrl+Alt+1"}] }),
        ),
        "",
    );
    settle(&mut app, &ctx);
    let before = mock.seen("platform_switch_account");
    let id = id_of(&app, "Ctrl+Alt+1");
    assert_eq!(app.hotkey(&ctx, id), WindowOp::Reveal);
    // No confirm: the user pressed it, like a click.
    assert!(pump_until(&mut app, &ctx, |_| mock
        .seen("platform_switch_account")
        > before));
    assert!(app.dialogs.confirm_dialog().is_none());
    // The #127 notice of an expired session, as a click raises it.
    let notice = accshift_app::platforms::riot::SESSION_EXPIRED_NOTICE_KEY;
    assert!(pump_until(&mut app, &ctx, |a| has_toast(a, notice)));
    assert!(pump_until(&mut app, &ctx, |a| {
        a.booted()
            .unwrap()
            .accounts
            .loading
            .loader
            .current_account()
            == STEAM_EXPIRED
    }));
}

#[test]
fn the_pin_lock_refuses_every_shortcut_but_the_toggle() {
    let ctx = Context::default();
    let (mut app, mock) = app_with(
        &ctx,
        json!({
            "pinEnabled": true,
            "pinHash": PIN_HASH,
            "globalShortcuts": {
                "openPalette": "Ctrl+Alt+K",
                "toggleWindow": "Ctrl+Alt+H",
                "accounts": [{"platformId": "steam", "accountId": STEAM_OTHER, "keys": "Ctrl+Alt+1"}],
                "personas": [{"personaId": "persona-main", "keys": "Ctrl+Alt+2"}],
            },
        }),
        "",
    );
    // Locked from boot, the accounts may wait: only the service is needed.
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert!(app.booted().unwrap().secure.is_pin_locked());
    let before = mock.seen("platform_switch_account");
    for combo in ["Ctrl+Alt+K", "Ctrl+Alt+1", "Ctrl+Alt+2"] {
        let id = id_of(&app, combo);
        app.hotkey(&ctx, id);
    }
    for _ in 0..30 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.palette.is_none());
    assert!(app.dialogs.confirm_dialog().is_none());
    assert_eq!(mock.seen("platform_switch_account"), before);
    assert!(app.booted().unwrap().secure.is_pin_locked());
    let toggle = id_of(&app, "Ctrl+Alt+H");
    assert_eq!(app.hotkey(&ctx, toggle), WindowOp::Toggle);
}

#[test]
fn a_persona_shortcut_takes_the_persona_click_path() {
    let ctx = Context::default();
    let (mut app, _) = app_with(
        &ctx,
        bound(json!({ "personas": [{"personaId": "persona-main", "keys": "Ctrl+Alt+2"}] })),
        "",
    );
    settle(&mut app, &ctx);
    let id = id_of(&app, "Ctrl+Alt+2");
    assert_eq!(app.hotkey(&ctx, id), WindowOp::Reveal);
    frame(&mut app, &ctx);
    let b = app.booted().unwrap();
    assert!(
        app.dialogs.confirm_dialog().is_some() || b.personas.switching(),
        "the persona switch neither asked nor started"
    );
}

/// A persona the store lacks is dropped. A bound account its loaded list
/// lacks is set aside, neither registered nor shown, but stays saved: a
/// launcher file read short must not erase it for good.
#[test]
fn stale_bindings_are_set_aside_once_their_lists_load() {
    let ctx = Context::default();
    let (mut app, _) = app_with(
        &ctx,
        bound(json!({
            "accounts": [
                {"platformId": "steam", "accountId": STEAM_OTHER, "keys": "Ctrl+Alt+1"},
                {"platformId": "steam", "accountId": "gone", "keys": "Ctrl+Alt+3"},
            ],
            "personas": [
                {"personaId": "persona-main", "keys": "Ctrl+Alt+2"},
                {"personaId": "persona-gone", "keys": "Ctrl+Alt+4"},
            ],
        })),
        "",
    );
    settle(&mut app, &ctx);
    let saved = &app.booted().unwrap().stores.settings().global_shortcuts;
    assert_eq!(saved.accounts.len(), 2, "a loaded list never unsaves one");
    assert_eq!(saved.personas.len(), 1);
    assert_eq!(saved.personas[0].persona_id, "persona-main");
    let registered: Vec<Combo> = app
        .fake_registrar()
        .unwrap()
        .registered()
        .into_iter()
        .map(|(_, combo)| combo)
        .collect();
    assert!(registered.contains(&Combo::parse("Ctrl+Alt+1").unwrap()));
    assert!(!registered.contains(&Combo::parse("Ctrl+Alt+3").unwrap()));
    let view = app.shortcuts_view();
    assert!(!view.rows.iter().any(|row| matches!(
        &row.target,
        ShortcutTarget::Account { account_id, .. } if account_id == "gone"
    )));
}

/// From the moment a row listens, a delivered shortcut is the combination
/// being typed: refused, and one delivered just before is dropped.
#[test]
fn a_listening_row_refuses_delivered_shortcuts() {
    let ctx = Context::default();
    let (mut app, _) = app_with(&ctx, bound(json!({ "openPalette": "Ctrl+Alt+K" })), "");
    settle(&mut app, &ctx);
    let id = id_of(&app, "Ctrl+Alt+K");
    assert_eq!(app.hotkey(&ctx, id), WindowOp::Reveal);
    app.open_shortcut_capture(ShortcutTarget::ToggleWindow);
    assert_eq!(app.hotkey(&ctx, id), WindowOp::None);
    frame(&mut app, &ctx);
    frame(&mut app, &ctx);
    assert!(app.palette.is_none());
    assert!(app.shortcuts.capture.is_some());
}

/// The PIN lock engaging while a row listens ends the capture before the
/// row reads a key: the digits go to the PIN field.
#[test]
fn a_pin_lock_ends_the_capture_before_it_reads_a_key() {
    let ctx = Context::default();
    let (mut app, _) = app_with(&ctx, json!({}), "");
    settle(&mut app, &ctx);
    app.open_shortcut_capture(ShortcutTarget::OpenPalette);
    frame(&mut app, &ctx);
    assert!(app.shortcuts.capture.is_some());
    app.booted.as_mut().unwrap().secure.handle_backend_locked();
    frame_with(
        &mut app,
        &ctx,
        vec![
            key(egui::Key::Num1, Modifiers::NONE),
            Event::Text("1".into()),
        ],
    );
    assert!(app.shortcuts.capture.is_none());
    assert!(app.booted().unwrap().secure.is_pin_locked());
}

/// Windows, X11 and the portal bind the symbol the layout types, so the
/// capture keeps it: on AZERTY the key at the US Q position is A. macOS
/// binds positions and keeps the physical key.
#[test]
fn the_capture_keeps_the_typed_key_where_the_os_binds_symbols() {
    let ctrl_alt = Modifiers {
        ctrl: true,
        alt: true,
        command: true,
        ..Modifiers::NONE
    };
    let events = [Event::Key {
        key: egui::Key::A,
        physical_key: Some(egui::Key::Q),
        pressed: true,
        repeat: false,
        modifiers: ctrl_alt,
    }];
    let expected = if cfg!(target_os = "macos") {
        "Ctrl+Alt+Q"
    } else {
        "Ctrl+Alt+A"
    };
    assert_eq!(
        read_press(&events, false),
        Some(Press::Set(Combo::parse(expected).unwrap()))
    );
}

/// Super comes from the window's modifiers, which egui drops outside
/// macOS: Super+K is usable and Ctrl+Super+K keeps its Super.
#[test]
fn the_capture_takes_super_from_the_window_modifiers() {
    let ctrl = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };
    assert_eq!(
        read_press(&[key(egui::Key::K, ctrl)], true),
        Some(Press::Set(Combo::parse("Ctrl+Super+K").unwrap()))
    );
    assert_eq!(
        read_press(&[key(egui::Key::K, Modifiers::NONE)], true),
        Some(Press::Set(Combo::parse("Super+K").unwrap()))
    );
    assert_eq!(
        read_press(&[key(egui::Key::K, Modifiers::NONE)], false),
        Some(Press::Unusable)
    );
}

#[test]
fn a_listening_row_saves_the_pressed_combination_and_escape_cancels() {
    let ctx = Context::default();
    let (mut app, _) = app_with(&ctx, json!({}), "");
    settle(&mut app, &ctx);
    app.open_shortcut_capture(ShortcutTarget::OpenPalette);
    frame(&mut app, &ctx);
    assert!(app.shortcuts.capture.is_some());
    // A plain letter is refused and keeps listening.
    frame_with(&mut app, &ctx, vec![key(egui::Key::K, Modifiers::NONE)]);
    assert!(app.shortcuts.capture.as_ref().is_some_and(|c| c.unusable));
    let mods = Modifiers {
        ctrl: true,
        alt: true,
        command: true,
        ..Modifiers::NONE
    };
    frame_with(&mut app, &ctx, vec![key(egui::Key::K, mods)]);
    assert!(app.shortcuts.capture.is_none());
    let saved = &app.booted().unwrap().stores.settings().global_shortcuts;
    assert_eq!(saved.open_palette, "Ctrl+Alt+K");
    frame(&mut app, &ctx);
    assert!(id_of(&app, "Ctrl+Alt+K") > 0);

    app.listen_shortcut(ShortcutTarget::ToggleWindow);
    frame_with(
        &mut app,
        &ctx,
        vec![key(egui::Key::Escape, Modifiers::NONE)],
    );
    assert!(app.shortcuts.capture.is_none());
    assert!(
        app.settings_panel.is_some(),
        "Escape stops the capture, not the settings"
    );
    let saved = &app.booted().unwrap().stores.settings().global_shortcuts;
    assert!(saved.toggle_window.is_empty());
}

#[test]
fn a_combination_another_app_holds_shows_taken() {
    let ctx = Context::default();
    let (mut app, _) = app_with(
        &ctx,
        bound(json!({ "openPalette": "Ctrl+Alt+K", "toggleWindow": "Ctrl+Alt+H" })),
        "Ctrl+Alt+K",
    );
    settle(&mut app, &ctx);
    app.open_shortcut_capture(ShortcutTarget::OpenPalette);
    app.shortcuts.capture = None;
    frame(&mut app, &ctx);
    let view = app.shortcuts_view();
    let status = |target: ShortcutTarget| {
        view.rows
            .iter()
            .find(|r| r.target == target)
            .and_then(|r| r.status.clone())
    };
    use accshift_app::shortcuts::RowStatus;
    assert_eq!(status(ShortcutTarget::OpenPalette), Some(RowStatus::Taken));
    assert_eq!(
        status(ShortcutTarget::ToggleWindow),
        Some(RowStatus::Active)
    );
}
