//! Personas: `app/usePersonas.test.ts` and `app/usePersonaSwitch.test.ts`,
//! run on the mock backend through a scripted wrapper.

mod support;

use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::actions::dialogs::AppDialogs;
use accshift_app::actions::personas::{
    persona_platforms, switch_steps, switch_to_persona_blocking, PersonaController,
    PersonaPlatform, PersonaSwitch, PersonaSwitchFailure, PersonaSwitchResult,
};
use accshift_app::actions::AppEffect;
use accshift_app::jobs::Jobs;
use accshift_app::personas::{Persona, PersonaAssignment, PersonaInput, PersonaPatch};
use accshift_app::platforms::{PlatformAccount, PlatformRegistry, RuntimeOs};
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use accshift_app::Backend;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::scripted::Scripted;

fn persona(assignments: &[(&str, &str)]) -> Persona {
    Persona {
        id: "persona-1".into(),
        name: "Main".into(),
        color: String::new(),
        image: None,
        assignments: assignments
            .iter()
            .map(|(platform_id, account_id)| PersonaAssignment {
                platform_id: (*platform_id).into(),
                account_id: (*account_id).into(),
            })
            .collect(),
    }
}

fn steam_accounts(backend: &dyn Backend, registry: &PlatformRegistry) -> Vec<PlatformAccount> {
    registry
        .adapter("steam")
        .unwrap()
        .load_accounts(backend)
        .unwrap()
}

fn stores_of(backend: &dyn Backend) -> ClientStores {
    let (storage, _) = storage::initialize(backend, None, None, true);
    ClientStores::new(storage, StoreContext::shipped())
}

// usePersonas.test.ts

#[test]
fn switches_with_the_adapters_resolved_account() {
    // The dev scenario's Steam accounts have login names; demo's are blank.
    let backend = support::scripted("dev");
    let registry = PlatformRegistry::shipped();
    let account = steam_accounts(&backend, &registry)
        .into_iter()
        .find(|account| !account.username.is_empty() && account.username != account.id)
        .expect("a Steam account with a login name");
    let settings = stores_of(&backend).settings().clone();
    let steps = switch_steps(&registry, &persona(&[("steam", &account.id)]));
    backend.clear_log();

    let result = switch_to_persona_blocking(&backend, &steps, &settings);

    assert_eq!(
        result,
        Some(PersonaSwitchResult {
            succeeded: vec!["steam".into()],
            failed: vec![]
        })
    );
    // Steam switches by login name, never by the steam id.
    let calls = backend.calls("platform_switch_account");
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].contains(&format!("{:?}", account.username)),
        "{calls:?}"
    );
}

#[test]
fn fails_the_assignment_instead_of_switching_an_unresolved_id() {
    let backend = support::scripted("demo");
    let registry = PlatformRegistry::shipped();
    let settings = stores_of(&backend).settings().clone();
    let steps = switch_steps(&registry, &persona(&[("steam", "steam-id")]));
    backend.clear_log();

    let result = switch_to_persona_blocking(&backend, &steps, &settings).unwrap();

    assert!(backend.calls("platform_switch_account").is_empty());
    assert!(result.succeeded.is_empty());
    assert_eq!(result.failed[0].platform_id, "steam");
    assert!(result.failed[0].error.contains("steam/steam-id"));
}

#[test]
fn reports_the_switch_in_flight_until_every_platform_is_done() {
    let backend = Arc::new(support::scripted("demo"));
    let registry = PlatformRegistry::shipped();
    let account = steam_accounts(backend.as_ref(), &registry).remove(0);
    let settings = stores_of(backend.as_ref()).settings().clone();
    let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
    let mut controller = PersonaController::new(&stores_of(backend.as_ref()));
    let target = persona(&[("steam", &account.id)]);

    assert!(controller.start_switch(&jobs, &registry, &settings, target.clone(), false));
    assert!(controller.switching());
    assert_eq!(controller.switching_persona_id(), Some("persona-1"));
    // A second start while one runs is refused.
    assert!(!controller.start_switch(&jobs, &registry, &settings, target, false));

    let deadline = Instant::now() + Duration::from_secs(10);
    let (done, result) = loop {
        if let Some(done) = controller.poll_switch() {
            break done;
        }
        assert!(Instant::now() < deadline, "switch never finished");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(done.id, "persona-1");
    assert_eq!(result.unwrap().succeeded, vec!["steam".to_string()]);
    assert!(!controller.switching());
    assert_eq!(controller.poll_switch(), None);
}

#[test]
fn stops_quietly_when_the_backend_is_locked_by_the_pin() {
    let backend = support::scripted("demo");
    let registry = PlatformRegistry::shipped();
    let account = steam_accounts(&backend, &registry).remove(0);
    let settings = stores_of(&backend).settings().clone();
    backend.push(
        "platform_switch_account",
        Err("pin_locked: Accshift is locked. Enter the PIN to switch accounts."),
    );
    let steps = switch_steps(
        &registry,
        &persona(&[("steam", &account.id), ("riot", "riot-1")]),
    );
    backend.clear_log();

    assert_eq!(
        switch_to_persona_blocking(&backend, &steps, &settings),
        None
    );
    assert_eq!(backend.calls("platform_switch_account").len(), 1);
}

#[test]
fn a_pin_lock_after_a_landed_platform_reports_and_stops() {
    let backend = support::scripted("demo");
    let registry = PlatformRegistry::shipped();
    let account = steam_accounts(&backend, &registry).remove(0);
    let settings = stores_of(&backend).settings().clone();
    let steps = switch_steps(
        &registry,
        &persona(&[
            ("steam", &account.id),
            ("riot", "riot-1"),
            ("riot", "riot-2"),
        ]),
    );
    backend.push("platform_switch_account", Ok(Value::Null));
    backend.push("platform_switch_account", Err("pin_locked: locked"));

    let result = switch_to_persona_blocking(&backend, &steps, &settings).unwrap();
    assert_eq!(result.succeeded, vec!["steam".to_string()]);
    assert_eq!(
        result.failed,
        vec![PersonaSwitchFailure {
            platform_id: "riot".into(),
            error: "pin_locked: locked".into()
        }]
    );
}

#[test]
fn refuses_to_start_while_an_account_switch_is_running() {
    let backend = Arc::new(support::scripted("demo"));
    let registry = PlatformRegistry::shipped();
    let settings = stores_of(backend.as_ref()).settings().clone();
    let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
    let mut controller = PersonaController::new(&stores_of(backend.as_ref()));
    backend.clear_log();

    assert!(!controller.start_switch(&jobs, &registry, &settings, persona(&[]), true));
    assert!(!controller.switching());
    assert_eq!(controller.poll_switch(), None);
}

#[test]
fn controller_keeps_its_list_in_step_with_the_store() {
    let backend = support::mock("demo");
    let mut stores = stores_of(&backend);
    let mut controller = PersonaController::new(&stores);
    let before = controller.personas().len();
    let created = controller.create(
        &mut stores,
        PersonaInput {
            name: "Work".into(),
            color: String::new(),
            image: None,
            assignments: vec![],
        },
    );
    assert_eq!(controller.personas().len(), before + 1);
    controller.update(
        &mut stores,
        &created.id,
        &PersonaPatch {
            name: Some("Office".into()),
            ..PersonaPatch::default()
        },
    );
    assert!(controller.personas().iter().any(|p| p.name == "Office"));
    controller.remove(&mut stores, &created.id);
    assert_eq!(controller.personas().len(), before);
}

// usePersonaSwitch.test.ts

#[test]
fn offers_the_implemented_enabled_platforms_of_this_os_as_slots() {
    let backend = support::mock("demo");
    let registry = PlatformRegistry::shipped();
    let mut settings = stores_of(&backend).settings().clone();
    settings.enabled_platforms = registry.defs().iter().map(|def| def.id.clone()).collect();
    let all = persona_platforms(&registry, RuntimeOs::Windows, &settings);
    let expected: Vec<PersonaPlatform> = registry
        .defs()
        .iter()
        .filter(|def| def.implemented && def.supported_os.contains(&RuntimeOs::Windows))
        .map(|def| PersonaPlatform {
            id: def.id.clone(),
            name: def.name.clone(),
            accent: def.accent.clone(),
        })
        .collect();
    assert_eq!(all, expected);
    assert!(all.iter().any(|platform| platform.id == "steam"));

    settings.enabled_platforms = vec!["riot".into(), "steam".into()];
    let ids: Vec<String> = persona_platforms(&registry, RuntimeOs::Windows, &settings)
        .into_iter()
        .map(|platform| platform.id)
        .collect();
    // Registry order, not settings order.
    assert_eq!(ids, ["steam", "riot"]);
}

#[test]
fn opens_the_panel_after_closing_settings_and_bulk_edit() {
    let mut switch = PersonaSwitch::new();
    assert_eq!(
        switch.open_personas(true, true),
        vec![AppEffect::CloseSettingsPanel, AppEffect::CloseBulkEdit]
    );
    assert!(switch.show_personas);
    switch.sync_enabled(false);
    assert!(!switch.show_personas);
    assert!(switch.open_personas(false, false).is_empty());
    assert!(!switch.show_personas);
}

#[test]
fn does_nothing_without_a_confirmation() {
    let mut switch = PersonaSwitch::new();
    let mut dialogs = AppDialogs::new();
    let target = persona(&[("steam", "s1"), ("riot", "r1")]);
    assert!(switch
        .handle_switch_persona(&mut dialogs, &target, false, false)
        .is_empty());
    let dialog = dialogs.confirm_dialog().unwrap();
    assert_eq!(
        dialog.message,
        Text::with(
            "personas.switchConfirmMessage",
            [
                ("name", accshift_app::i18n::TranslationValue::from("Main")),
                ("count", accshift_app::i18n::TranslationValue::from(2.0))
            ]
        )
    );
    let settled = dialogs.close_confirm_dialog();
    let [AppEffect::ConfirmSettled { ticket, allowed }] = settled.as_slice() else {
        panic!("{settled:?}");
    };
    assert_eq!(switch.confirm_settled(*ticket, *allowed), None);

    // Busy: no dialog at all.
    let mut dialogs = AppDialogs::new();
    switch.handle_switch_persona(&mut dialogs, &target, true, false);
    switch.handle_switch_persona(&mut dialogs, &target, false, true);
    assert!(dialogs.confirm_dialog().is_none());

    // Accepted: the persona comes back for the controller.
    let mut dialogs = AppDialogs::new();
    switch.handle_switch_persona(&mut dialogs, &target, false, false);
    let ticket = dialogs.pending_confirm().unwrap();
    assert_eq!(switch.confirm_settled(ticket, true), Some(target));
}

#[test]
fn toasts_success_total_failure_and_a_partial_switch_by_platform_name() {
    let platforms = vec![
        PersonaPlatform {
            id: "steam".into(),
            name: "Steam".into(),
            accent: "#1".into(),
        },
        PersonaPlatform {
            id: "riot".into(),
            name: "Riot".into(),
            accent: "#2".into(),
        },
    ];
    let target = persona(&[("steam", "s1"), ("riot", "r1")]);
    let fail = |id: &str| PersonaSwitchFailure {
        platform_id: id.into(),
        error: "x".into(),
    };

    let ok = PersonaSwitch::finish_switch(
        &target,
        Some(&PersonaSwitchResult {
            succeeded: vec!["steam".into(), "riot".into()],
            failed: vec![],
        }),
        &platforms,
    );
    assert_eq!(
        ok,
        vec![
            AppEffect::TrackPersonaSwitch {
                platforms: 2,
                succeeded: 2
            },
            AppEffect::Toast(Toast::new(
                Text::with("personas.switched", [("name", "Main")]),
                ToastKind::Success
            ))
        ]
    );

    let failed = PersonaSwitch::finish_switch(
        &target,
        Some(&PersonaSwitchResult {
            succeeded: vec![],
            failed: vec![fail("steam"), fail("riot")],
        }),
        &platforms,
    );
    assert_eq!(
        failed[1],
        AppEffect::Toast(Toast::new(
            Text::with("personas.switchFailed", [("name", "Main")]),
            ToastKind::Error
        ))
    );

    let partial = PersonaSwitch::finish_switch(
        &target,
        Some(&PersonaSwitchResult {
            succeeded: vec!["steam".into()],
            failed: vec![fail("riot"), fail("gog")],
        }),
        &platforms,
    );
    assert_eq!(
        partial[1],
        AppEffect::Toast(Toast::new(
            Text::with(
                "personas.switchPartial",
                [("name", "Main"), ("failed", "Riot, gog")]
            ),
            ToastKind::Default
        ))
    );

    assert!(PersonaSwitch::finish_switch(&target, None, &platforms).is_empty());
}

#[test]
fn telemetry_counter_reaches_the_backend() {
    let backend = support::scripted("demo");
    let _ = backend.telemetry_track_persona_switch(2, 1);
    assert_eq!(backend.calls("telemetry_track_persona_switch"), ["(2, 1)"]);
    let _ = Scripted::new(support::mock("demo"));
}
