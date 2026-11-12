//! The lifecycle state machines: the boot and close timings, the update
//! flow, the external refresh, the boot helpers, the UI scale and the
//! process control (a fake never relaunches or exits).

mod support;

use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::lifecycle::app_shell::{
    first_run_platforms, initial_load, migration_toast, plan_external_refresh, settings_snapshot,
    ExternalRefreshGate, InitialLoad, RefreshStep,
};
use accshift_app::lifecycle::boot::{
    log_app_event, mask_value, redact_object, serialize_log_value, track_update, truncate_log,
    BOOT_FAILSAFE,
};
use accshift_app::lifecycle::process::{self, ProcessControl};
use accshift_app::lifecycle::ui_scale::{
    backdrop_available, clamp_ui_scale_percent, liquid_backdrop_active, UiScale, WheelDeltaMode,
    PERSIST_DELAY,
};
use accshift_app::lifecycle::update_flow::{
    classify_update_check_error, AppUpdater, LogLevel, PendingUpdate, UpdateCommand,
    UpdateErrorCode, UpdateOutput, UpdateStage, UpdateState,
};
use accshift_app::lifecycle::{GEOMETRY_SAVE_WAIT, UPDATE_CHECK_DELAY};
use accshift_app::platforms::{PlatformRegistry, RuntimeOs};
use accshift_app::settings::{sanitize_settings, AppSettings};
use accshift_app::storage::{
    CLIENT_STORE_PERSONAS, CLIENT_STORE_SETTINGS, STORAGE_TARGET_APP_CONFIG_LOCAL,
    STORAGE_TARGET_CUSTOM_THEMES,
};
use accshift_app::stores::StoreContext;
use accshift_app::text::Text;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Boot and shutdown timings

#[test]
fn the_boot_and_close_timings_hold() {
    assert_eq!(UPDATE_CHECK_DELAY, Duration::from_millis(3500));
    assert_eq!(BOOT_FAILSAFE, Duration::from_millis(5000));
    assert_eq!(GEOMETRY_SAVE_WAIT, Duration::from_secs(5));
}

// ---------------------------------------------------------------------------
// Update flow

#[derive(Debug, Clone, PartialEq)]
struct FakeUpdate(&'static str);

impl PendingUpdate for FakeUpdate {
    fn version(&self) -> String {
        self.0.to_string()
    }
}

const TARGET_NOT_FOUND: &str =
    "the platform `linux-x86_64` was not found in the response `platforms` object";
const TARGETS_NOT_FOUND: &str = "None of the fallback platforms `[\"darwin-aarch64-app\", \"darwin-aarch64\"]` were found in the response `platforms` object";

fn failed_check(error: &str) -> Vec<UpdateOutput<FakeUpdate>> {
    let mut updater = AppUpdater::<FakeUpdate>::new(false);
    assert_eq!(updater.start(), [UpdateOutput::Run(UpdateCommand::Check)]);
    let outputs = updater.on_check_result(Err(error.into()));
    assert_eq!(updater.state(), UpdateState::Idle);
    outputs
}

fn tracked(
    outputs: &[UpdateOutput<FakeUpdate>],
) -> Vec<(UpdateStage, Option<String>, Option<&'static str>)> {
    outputs
        .iter()
        .filter_map(|output| match output {
            UpdateOutput::Track {
                stage,
                target_version,
                error_code,
            } => Some((
                *stage,
                target_version.clone(),
                error_code.map(UpdateErrorCode::as_str),
            )),
            _ => None,
        })
        .collect()
}

fn logged(outputs: &[UpdateOutput<FakeUpdate>], level: LogLevel) -> usize {
    outputs
        .iter()
        .filter(|output| matches!(output, UpdateOutput::Log { level: l, .. } if *l == level))
        .count()
}

#[test]
fn a_missing_platform_entry_has_its_own_code_and_no_error_log() {
    let outputs = failed_check(TARGET_NOT_FOUND);
    assert_eq!(
        tracked(&outputs),
        [(UpdateStage::Failed, None, Some("update_target_missing"))]
    );
    assert_eq!(logged(&outputs, LogLevel::Error), 0);
    assert_eq!(logged(&outputs, LogLevel::Info), 1);
}

#[test]
fn an_unreadable_manifest_is_told_apart_from_a_dead_endpoint() {
    let outputs = failed_check("Could not fetch a valid release JSON from the remote");
    assert_eq!(
        tracked(&outputs),
        [(UpdateStage::Failed, None, Some("update_manifest_invalid"))]
    );
}

#[test]
fn a_network_failure_is_still_check_failed() {
    let outputs = failed_check("error sending request for url (https://github.com)");
    assert_eq!(
        tracked(&outputs),
        [(UpdateStage::Failed, None, Some("check_failed"))]
    );
    assert_eq!(logged(&outputs, LogLevel::Error), 1);
}

#[test]
fn no_update_stays_quiet() {
    let mut updater = AppUpdater::<FakeUpdate>::new(false);
    updater.start();
    assert!(updater.on_check_result(Ok(None)).is_empty());
    assert_eq!(updater.state(), UpdateState::Idle);
}

#[test]
fn the_check_errors_classify_as_the_ts_tests() {
    let code = |text: Option<&str>| classify_update_check_error(text).as_str();
    assert_eq!(code(Some(TARGET_NOT_FOUND)), "update_target_missing");
    assert_eq!(code(Some(TARGETS_NOT_FOUND)), "update_target_missing");
    assert_eq!(
        code(Some("missing field `version` at line 1 column 42")),
        "update_manifest_invalid"
    );
    assert_eq!(
        code(Some(
            "The signature ...= could not be decoded, please check if it is a valid base64 string."
        )),
        "update_manifest_invalid"
    );
    assert_eq!(code(Some("operation timed out")), "check_failed");
    assert_eq!(code(None), "check_failed");
    // The rest of MANIFEST_INVALID_PATTERNS, case-insensitive.
    for text in [
        "INVALID UPDATER BINARY FORMAT",
        "expected value at line 1 column 1",
        "invalid type: integer `1`, expected a string",
        "trailing characters at line 1 column 9",
        "unexpected character 'x' while parsing Minor version number",
    ] {
        assert_eq!(code(Some(text)), "update_manifest_invalid", "{text}");
    }
}

#[test]
fn the_whole_flow_downloads_then_applies() {
    let mut updater = AppUpdater::<FakeUpdate>::new(false);
    assert_eq!(updater.cta_label(), None);
    assert_eq!(updater.cta_title(), Text::key("update.restartToApply"));
    updater.start();
    // A second start while checking does nothing.
    assert!(updater.start().is_empty());
    let outputs = updater.on_check_result(Ok(Some(FakeUpdate("1.2.0"))));
    assert_eq!(
        outputs,
        [
            UpdateOutput::Track {
                stage: UpdateStage::Available,
                target_version: Some("1.2.0".into()),
                error_code: None,
            },
            UpdateOutput::Run(UpdateCommand::Download(FakeUpdate("1.2.0"))),
        ]
    );
    assert_eq!(updater.state(), UpdateState::Downloading);
    // Not ready yet: apply does nothing.
    assert!(updater.apply().is_empty());

    let outputs = updater.on_download_result(Ok(vec![1, 2, 3]));
    assert_eq!(
        outputs[1],
        UpdateOutput::Toast(Text::with(
            "update.readyToastVersion",
            [("version", "1.2.0".to_string())]
        ))
    );
    assert_eq!(updater.state(), UpdateState::Ready);
    assert_eq!(updater.cta_label(), Some(Text::key("update.ctaAvailable")));
    assert_eq!(
        updater.cta_title(),
        Text::with(
            "update.restartToApplyVersion",
            [("version", "1.2.0".to_string())]
        )
    );
    assert!(!updater.cta_disabled());

    let outputs = updater.apply();
    assert_eq!(
        outputs,
        [UpdateOutput::Run(UpdateCommand::Install {
            update: FakeUpdate("1.2.0"),
            bytes: vec![1, 2, 3],
        })]
    );
    assert!(updater.cta_disabled());
    assert_eq!(updater.cta_label(), Some(Text::key("update.ctaInstalling")));
    assert!(updater.apply().is_empty());

    let outputs = updater.on_install_result(Ok(()));
    assert_eq!(
        tracked(&outputs),
        [(UpdateStage::Applied, Some("1.2.0".into()), None)]
    );
    assert_eq!(outputs[1], UpdateOutput::Run(UpdateCommand::Relaunch));
}

#[test]
fn a_failed_download_resets_and_tracks_the_version() {
    let mut updater = AppUpdater::<FakeUpdate>::new(false);
    updater.start();
    updater.on_check_result(Ok(Some(FakeUpdate("1.2.0"))));
    let outputs = updater.on_download_result(Err("`Download request failed`".into()));
    assert_eq!(
        tracked(&outputs),
        [(
            UpdateStage::Failed,
            Some("1.2.0".into()),
            Some("download_failed")
        )]
    );
    assert_eq!(updater.state(), UpdateState::Idle);
    assert_eq!(updater.version(), "");
    // The flow can start again.
    assert_eq!(updater.start(), [UpdateOutput::Run(UpdateCommand::Check)]);
}

#[test]
fn a_failed_install_toasts_and_checks_again() {
    let mut updater = AppUpdater::<FakeUpdate>::new(false);
    updater.start();
    updater.on_check_result(Ok(Some(FakeUpdate("1.2.0"))));
    updater.on_download_result(Ok(Vec::new()));
    updater.apply();
    let outputs = updater.on_install_result(Err("flush failed".into()));
    assert_eq!(
        tracked(&outputs),
        [(
            UpdateStage::Failed,
            Some("1.2.0".into()),
            Some("install_failed")
        )]
    );
    assert!(
        matches!(&outputs[0], UpdateOutput::Log { level: LogLevel::Error, message } if message.starts_with("Failed to restart for update: "))
    );
    assert_eq!(
        &outputs[2..],
        [
            UpdateOutput::Toast(Text::key("update.restartFailed")),
            UpdateOutput::Run(UpdateCommand::Check),
        ]
    );
    assert_eq!(updater.state(), UpdateState::Checking);
}

/// The update is already installed when the relaunch fails: the app stays
/// open, asks for a manual restart and does not download it again.
#[test]
fn a_failed_relaunch_asks_for_a_manual_restart_without_checking_again() {
    let mut updater = AppUpdater::<FakeUpdate>::new(false);
    updater.start();
    updater.on_check_result(Ok(Some(FakeUpdate("1.2.0"))));
    updater.on_download_result(Ok(Vec::new()));
    updater.apply();
    updater.on_install_result(Ok(()));
    let outputs = updater.on_relaunch_failed("spawn failed".into());
    assert_eq!(
        tracked(&outputs),
        [(
            UpdateStage::Failed,
            Some("1.2.0".into()),
            Some("relaunch_failed")
        )]
    );
    assert!(
        matches!(&outputs[0], UpdateOutput::Log { level: LogLevel::Error, message } if message == "Failed to restart for update: spawn failed")
    );
    assert_eq!(
        &outputs[2..],
        [UpdateOutput::Toast(Text::key("update.restartManually"))]
    );
    assert_eq!(updater.state(), UpdateState::Idle);
    assert_eq!(updater.cta_label(), None);
    assert!(updater.start().is_empty());
    assert!(updater.apply().is_empty());
}

#[test]
fn a_dev_build_never_checks() {
    let mut updater = AppUpdater::<FakeUpdate>::new(true);
    assert!(updater.start().is_empty());
    assert_eq!(updater.state(), UpdateState::Idle);
}

#[test]
fn update_telemetry_reaches_the_backend() {
    let backend = support::scripted("demo");
    track_update(&backend, "failed", None, Some("check_failed"));
    assert_eq!(backend.calls("telemetry_track_update").len(), 1);
    let call = &backend.calls("telemetry_track_update")[0];
    assert!(
        call.contains("failed") && call.contains("check_failed"),
        "{call}"
    );
}

// ---------------------------------------------------------------------------
// App shell

fn settings(patch: Value) -> AppSettings {
    sanitize_settings(&patch, &StoreContext::shipped())
}

fn refresh(changed: &[&str], enabled: &[&str], active_tab: &str) -> Vec<RefreshStep> {
    let changed: Vec<String> = changed.iter().map(|c| c.to_string()).collect();
    let settings = settings(json!({
        "enabledPlatforms": enabled,
        "defaultPlatformId": enabled[0],
    }));
    plan_external_refresh(
        &changed,
        &settings,
        active_tab,
        &PlatformRegistry::shipped(),
        RuntimeOs::Windows,
        false,
    )
}

#[test]
fn the_personas_list_reloads_when_another_process_rewrote_it() {
    let steps = refresh(&[CLIENT_STORE_PERSONAS], &["steam"], "steam");
    assert_eq!(
        steps
            .iter()
            .filter(|step| **step == RefreshStep::RefreshPersonas)
            .count(),
        1
    );
}

#[test]
fn unrelated_stores_leave_the_personas_alone() {
    let steps = refresh(&["something-else"], &["steam"], "steam");
    assert!(!steps.contains(&RefreshStep::RefreshPersonas));
    // Only the log line.
    assert_eq!(steps.len(), 1);
    assert_eq!(
        steps[0],
        RefreshStep::Log {
            details: r#"{"changed":["something-else"],"activeTab":"steam"}"#.into()
        }
    );
}

/// The calls the refresh steps stand for, as plain names.
fn recorded_calls(steps: &[RefreshStep]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|step| match step {
            RefreshStep::ClearLoaderForPlatformChange => Some("clear".to_string()),
            RefreshStep::SetActiveTab(tab) => Some(format!("tab:{tab}")),
            RefreshStep::LoadAccounts { .. } => Some("load".to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_external_change_that_disabled_the_tab_moves_off_it_and_reloads() {
    let steps = refresh(&[CLIENT_STORE_SETTINGS], &["riot"], "steam");
    assert_eq!(recorded_calls(&steps), ["clear", "tab:riot", "load"]);
    assert!(steps.contains(&RefreshStep::LoadAccounts {
        force: true,
        warnings: false
    }));
    assert!(steps.contains(&RefreshStep::ReplaceHistory("riot".into())));
}

#[test]
fn the_grid_is_left_alone_while_the_tab_stays_enabled() {
    let steps = refresh(&[CLIENT_STORE_SETTINGS], &["steam", "riot"], "steam");
    assert!(recorded_calls(&steps).is_empty());
}

#[test]
fn app_config_and_platform_stores_reload_the_accounts() {
    let steps = refresh(&[STORAGE_TARGET_APP_CONFIG_LOCAL], &["steam"], "steam");
    assert!(matches!(
        steps.last(),
        Some(RefreshStep::LoadAccounts { force: true, .. })
    ));
    let steps = refresh(&[STORAGE_TARGET_CUSTOM_THEMES], &["steam"], "steam");
    assert_eq!(steps[1], RefreshStep::LoadCustomThemes);
    assert_eq!(steps[2], RefreshStep::RefreshSettings);
    assert!(refresh(&[], &["steam"], "steam").is_empty());
}

#[test]
fn the_shell_init_helpers_match_the_ts() {
    assert_eq!(
        migration_toast("migrated"),
        Some(Toast::new(
            Text::key("toast.legacyConfigMigrated"),
            ToastKind::Default
        ))
    );
    assert_eq!(
        migration_toast("error:denied").map(|toast| toast.text),
        Some(Text::key("toast.legacyConfigMigrationFailed"))
    );
    assert_eq!(migration_toast("none"), None);

    let registry = PlatformRegistry::shipped();
    let detected = first_run_platforms(
        &["riot".to_string(), "nope".to_string(), "steam".to_string()],
        &registry,
        RuntimeOs::Windows,
    );
    assert_eq!(detected.detected, ["riot", "steam"]);
    assert_eq!(
        detected.enable,
        Some((
            vec!["riot".to_string(), "steam".to_string()],
            "riot".to_string()
        ))
    );
    assert_eq!(
        first_run_platforms(&[], &registry, RuntimeOs::Windows).enable,
        None
    );

    assert_eq!(
        initial_load(&registry, "steam", RuntimeOs::Windows),
        InitialLoad::LoadAccounts
    );
    assert_eq!(
        initial_load(&registry, "steam", RuntimeOs::Unknown),
        InitialLoad::RefreshItems
    );

    let snapshot = settings_snapshot(&settings(json!({"enabledPlatforms": ["steam"]})));
    assert_eq!(snapshot.enabled_platforms, ["steam"]);
    assert_eq!(snapshot.ui_language, "en");
    assert!(!snapshot.streamer_mode.is_empty());
    assert!(!snapshot.animations.is_empty());

    let mut gate = ExternalRefreshGate::default();
    assert!(gate.begin());
    assert!(!gate.begin());
    gate.end();
    assert!(gate.begin());
}

// ---------------------------------------------------------------------------
// Boot helpers

#[test]
fn log_details_are_redacted_as_app_logger() {
    assert_eq!(mask_value(&json!("ab")), json!("***"));
    assert_eq!(mask_value(&json!("alice")), json!("al***"));
    assert_eq!(mask_value(&json!(["a", "b"])), json!("[2 items]"));
    assert_eq!(mask_value(&json!(76561198000000000u64)), json!("***"));

    let details = json!({
        "Username": "alice",
        "steamId": "76561198000000000",
        "nested": {"email": "a@b.c", "keep": 1},
        "list": [{"username": "kept in arrays"}],
        "platform": "steam",
    });
    let redacted = redact_object(details.as_object().unwrap());
    assert_eq!(
        Value::Object(redacted),
        json!({
            "Username": "al***",
            "steamId": "76***",
            "nested": {"email": "a@***", "keep": 1},
            "list": [{"username": "kept in arrays"}],
            "platform": "steam",
        })
    );
    assert_eq!(serialize_log_value(&json!("plain text")), "plain text");
    assert_eq!(serialize_log_value(&json!(42)), "42");
    // `typeof [] === "object"`: a top-level array goes through redactObject.
    assert_eq!(
        serialize_log_value(&json!([{"uuid": "abcdef"}, "x"])),
        r#"{"0":{"uuid":"ab***"},"1":"x"}"#
    );

    let backend = support::scripted("demo");
    log_app_event(
        &backend,
        "info",
        "frontend.test",
        "hello",
        Some(&json!({"userId": "1234"})),
    );
    log_app_event(
        &backend,
        "info",
        "frontend.test",
        "no details",
        Some(&Value::Null),
    );
    let calls = backend.calls("log_app_event");
    assert_eq!(calls.len(), 2);
    assert!(calls[0].contains("12***"), "{}", calls[0]);
    assert!(calls[1].contains("None"), "{}", calls[1]);
}

#[test]
fn log_lines_are_capped_in_utf16_units() {
    let long = "é".repeat(600);
    let (message, details) = truncate_log(&long, Some(&"x".repeat(20_000)));
    assert_eq!(message.chars().count(), 512);
    assert_eq!(details.unwrap().len(), 16_384);
    let (short, none) = truncate_log("ok", None);
    assert_eq!((short.as_str(), none), ("ok", None));
}

// ---------------------------------------------------------------------------
// UI scale and backdrop

#[test]
fn the_ui_scale_rounds_to_the_step_and_clamps() {
    assert_eq!(clamp_ui_scale_percent(100.0), 100);
    assert_eq!(clamp_ui_scale_percent(102.4), 100);
    assert_eq!(clamp_ui_scale_percent(102.5), 105);
    assert_eq!(clamp_ui_scale_percent(10.0), 75);
    assert_eq!(clamp_ui_scale_percent(400.0), 150);
}

#[test]
fn the_ui_scale_follows_shortcuts_and_the_ctrl_wheel_then_saves_late() {
    let now = Instant::now();
    let mut scale = UiScale::new();
    assert_eq!(scale.zoom_in(100, now), Some(105));
    assert_eq!(scale.zoom_out(100, now), Some(95));
    assert_eq!(scale.reset(100, now), None);
    assert_eq!(scale.zoom_in(150, now), None);

    // Pixels accumulate up to 80 before a step; up is zoom in.
    assert_eq!(
        scale.wheel(true, -50.0, WheelDeltaMode::Pixel, 800.0, 100, now),
        None
    );
    assert_eq!(
        scale.wheel(true, -50.0, WheelDeltaMode::Pixel, 800.0, 100, now),
        Some(105)
    );
    // Lines count 16 px, pages the page height.
    assert_eq!(
        scale.wheel(true, 5.0, WheelDeltaMode::Line, 800.0, 100, now),
        Some(95)
    );
    assert_eq!(
        scale.wheel(true, -0.1, WheelDeltaMode::Page, 800.0, 100, now),
        Some(105)
    );
    // Without ctrl the wheel scrolls and the sum resets.
    assert_eq!(
        scale.wheel(true, -50.0, WheelDeltaMode::Pixel, 800.0, 100, now),
        None
    );
    assert_eq!(
        scale.wheel(false, -50.0, WheelDeltaMode::Pixel, 800.0, 100, now),
        None
    );
    assert_eq!(
        scale.wheel(true, -50.0, WheelDeltaMode::Pixel, 800.0, 100, now),
        None
    );

    let start = Instant::now();
    let mut scale = UiScale::new();
    scale.set(100, 120.0, start);
    assert_eq!(scale.next_persist(), Some(start + PERSIST_DELAY));
    assert_eq!(scale.take_due(start + Duration::from_millis(100)), None);
    assert_eq!(scale.take_due(start + PERSIST_DELAY), Some(120));
    assert_eq!(scale.take_due(start + PERSIST_DELAY), None);

    scale.set(120, 130.0, start);
    assert_eq!(scale.destroy(125), Some(125));
    assert_eq!(scale.destroy(125), None);
}

#[test]
fn the_backdrop_decisions_match_use_theme_application() {
    assert!(liquid_backdrop_active(true, RuntimeOs::Windows));
    assert!(!liquid_backdrop_active(true, RuntimeOs::Macos));
    assert!(!liquid_backdrop_active(false, RuntimeOs::Windows));
    assert!(!backdrop_available(RuntimeOs::Linux, false, true));
    // No blur is applied on macOS either: the theme must draw opaque.
    assert!(!backdrop_available(RuntimeOs::Macos, false, true));
    assert!(backdrop_available(RuntimeOs::Windows, false, false));
    assert!(!backdrop_available(RuntimeOs::Windows, true, false));
    assert!(backdrop_available(RuntimeOs::Windows, true, true));
}

// ---------------------------------------------------------------------------
// Process control

#[derive(Default)]
struct FakeProcess {
    calls: RefCell<Vec<String>>,
    fail_spawn: bool,
}

impl ProcessControl for FakeProcess {
    fn spawn(&self, program: &Path, args: &[OsString]) -> std::io::Result<()> {
        self.calls.borrow_mut().push(format!(
            "spawn:{}:{}",
            program.display(),
            args.iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        if self.fail_spawn {
            Err(std::io::Error::other("denied"))
        } else {
            Ok(())
        }
    }

    fn exit(&self, code: i32) {
        self.calls.borrow_mut().push(format!("exit:{code}"));
    }
}

/// The single-instance guard goes before the spawn (the new process must
/// not forward its launch to this one), and nothing exits unless a new
/// process started: a failed relaunch leaves the app open.
#[test]
fn relaunch_releases_the_instance_spawns_then_exits_and_never_exits_alone() {
    let control = FakeProcess::default();
    let args: Vec<OsString> = vec!["--flag".into(), "x y".into()];
    let calls = &control.calls;
    let record = |step: &'static str| move || calls.borrow_mut().push(step.to_string());
    process::restart(
        &control,
        Ok(PathBuf::from("app-binary")),
        &args,
        record("release"),
        record("before_exit"),
    )
    .unwrap();
    assert_eq!(
        *control.calls.borrow(),
        [
            "release",
            "spawn:app-binary:--flag,x y",
            "before_exit",
            "exit:0"
        ]
    );

    // A failed spawn is reported and the process stays.
    let failing = FakeProcess {
        fail_spawn: true,
        ..Default::default()
    };
    let error = process::restart(
        &failing,
        Ok(PathBuf::from("app-binary")),
        &args,
        || {},
        || failing.calls.borrow_mut().push("before_exit".into()),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "denied");
    assert_eq!(*failing.calls.borrow(), ["spawn:app-binary:--flag,x y"]);

    // No binary: nothing is released, spawned or exited.
    let control = FakeProcess::default();
    let released = RefCell::new(false);
    let error = process::restart(
        &control,
        Err(std::io::Error::other("gone")),
        &args,
        || *released.borrow_mut() = true,
        || {},
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "gone");
    assert!(control.calls.borrow().is_empty());
    assert!(!*released.borrow());

    let control = FakeProcess::default();
    process::exit(&control, 3, || {});
    assert_eq!(*control.calls.borrow(), ["exit:3"]);

    // The real binary resolves to this test executable.
    let binary = process::current_binary().unwrap();
    assert!(binary.is_file());
    assert_eq!(process::relaunch_program(&binary), binary);
}
