//! The settings' about card on the mock: the version, the hand check and
//! where the update flow stands. The mock run keeps the updater off; a test
//! turns the flow on over an updater that cannot be built, so a check fails
//! at once and nothing is fetched.

use super::super::settings_panel::{update_row, SettingsAction, UpdateStatus};
use super::super::updates::Updates;
use super::*;
use accshift_app::i18n::{translate, Locale};
use accshift_app::lifecycle::update_flow::CommandResult;
use serde_json::json;

fn about_app(ctx: &Context) -> AppModel {
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/empty.json")).unwrap();
    let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
    boot["storageSnapshot"]["stores"]["client.settings"]["animations"] = json!("off");
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    let mut app = AppModel::with_backend(ctx, mock.clone(), Some(mock), Clock::wall());
    app.wait_boot(ctx).unwrap();
    app
}

fn frame(app: &mut AppModel, ctx: &Context) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.frame(ui));
    output.textures_delta.clear();
}

/// The card as the open panel holds it after a frame.
fn card(app: &mut AppModel, ctx: &Context) -> UpdateStatus {
    frame(app, ctx);
    app.settings_panel.as_ref().unwrap().update.clone()
}

fn check(app: &mut AppModel, ctx: &Context) {
    app.apply_settings(ctx, SettingsAction::CheckUpdates);
}

#[test]
fn a_hand_check_starts_at_rest_waits_while_one_runs_and_shows_its_outcome() {
    let ctx = Context::default();
    let mut app = about_app(&ctx);
    app.updates = Updates::on_for_test("the updater is offline");
    app.toggle_settings_panel();
    assert_eq!(card(&mut app, &ctx), UpdateStatus::Idle);

    // A check runs: the button does nothing more.
    let result = app.updates.hold_check_for_test();
    assert_eq!(card(&mut app, &ctx), UpdateStatus::Checking);
    check(&mut app, &ctx);
    assert!(app.updates.running_for_test());
    assert_eq!(card(&mut app, &ctx), UpdateStatus::Checking);

    // It finds nothing newer.
    result
        .send(Some(CommandResult::Checked(Box::new(Ok(None)))))
        .unwrap();
    let status = card(&mut app, &ctx);
    assert_eq!(status, UpdateStatus::UpToDate);
    let locale = app.booted().unwrap().locale;
    let row = update_row(&status, locale);
    assert_eq!(row.notice, translate(locale, "settings.updateUpToDate"));
    assert!(row.enabled && !row.error);

    // Idle again: a click checks, and the updater's error shows.
    check(&mut app, &ctx);
    let status = card(&mut app, &ctx);
    assert_eq!(
        status,
        UpdateStatus::Failed("the updater is offline".into())
    );
    let row = update_row(&status, locale);
    assert!(row.error && row.enabled);
    assert!(
        row.notice.contains("the updater is offline"),
        "{}",
        row.notice
    );
}

#[test]
fn the_mock_run_shows_the_updater_off_and_a_click_does_nothing() {
    let ctx = Context::default();
    let mut app = about_app(&ctx);
    app.toggle_settings_panel();
    assert_eq!(card(&mut app, &ctx), UpdateStatus::Unavailable);
    check(&mut app, &ctx);
    assert!(!app.updates.running_for_test());
    assert_eq!(card(&mut app, &ctx), UpdateStatus::Unavailable);
    assert!(!update_row(&UpdateStatus::Unavailable, Locale::En).enabled);
}

/// A downloaded update: no second check, and the card's restart is the
/// title bar's install.
#[test]
fn a_ready_update_takes_no_check_and_restarts_through_the_title_bar_path() {
    let ctx = Context::default();
    let mut app = about_app(&ctx);
    app.updates = Updates::on_for_test("the updater is offline");
    app.updates.ready_for_test();
    app.toggle_settings_panel();
    assert_eq!(card(&mut app, &ctx), UpdateStatus::Ready("2.0.0".into()));
    check(&mut app, &ctx);
    assert!(app.updates.ready());
    assert!(!app.updates.running_for_test());

    app.apply_settings(&ctx, SettingsAction::ApplyUpdate);
    assert!(!app.updates.ready(), "the install did not start");
}

#[test]
fn the_about_card_draws_in_every_state() {
    let ctx = Context::default();
    let mut app = about_app(&ctx);
    app.toggle_settings_panel();
    for state in ["idle", "checking", "up-to-date", "failed", "ready"] {
        app.updates = Updates::scene(state).unwrap();
        let status = card(&mut app, &ctx);
        assert_ne!(status, UpdateStatus::Unavailable, "{state}");
        frame(&mut app, &ctx);
    }
    assert!(Updates::scene("soon").is_err());
}
