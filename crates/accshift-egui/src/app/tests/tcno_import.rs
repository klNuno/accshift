//! The TcNo import behind the backup's boundary, on the mock backend: it
//! waits for the writes already running, holds new ones, and the open app
//! adopts the imported stores instead of writing its own over them.

use super::super::settings_panel::SettingsAction;
use super::*;
use accshift_app::types::{TcNoImportReport, TcNoPreview};
use serde_json::json;

const SAVE: &str = "save_client_storage_store";
const IMPORT: &str = "tcno_import";
const ROBLOX_ADD: &str = "roblox_add_account_by_cookie";

struct HeldCommand<'a>(&'a MockBackend, &'static str);

impl<'a> HeldCommand<'a> {
    fn new(mock: &'a MockBackend, command: &'static str) -> Self {
        assert!(mock.hold(command));
        Self(mock, command)
    }
}

impl Drop for HeldCommand<'_> {
    fn drop(&mut self) {
        self.0.release(self.1);
    }
}

/// The mock app, with a TcNo folder whose import adds a note to account
/// `a` and puts it first in the Steam root.
fn tcno_app(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    tcno_app_with(ctx, |_| {})
}

/// [`tcno_app`] on a fixture `configure` changes first.
fn tcno_app_with(
    ctx: &Context,
    configure: impl FnOnce(&mut Value),
) -> (AppModel, Arc<MockBackend>) {
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/empty.json")).unwrap();
    fixture["spec"]["switchDelayMs"] = json!(0);
    configure(&mut fixture);
    let boot = &fixture["commands"]["get_boot_payload"][0]["result"];
    let mut imported = boot["storageSnapshot"].clone();
    imported["stores"]["client.account-card-notes"] = json!({"a": "imported note"});
    imported["manifest"]["stores"] = json!({
        "client.account-card-notes": "imported-notes"
    });
    let preview = TcNoPreview {
        path: "C:/TcNo".into(),
        accounts_new: 2,
        notes_new: 1,
        snapshots_new: 1,
        to_capture: 1,
        ..TcNoPreview::default()
    };
    let report = TcNoImportReport {
        path: "C:/TcNo".into(),
        accounts_imported: 2,
        notes_imported: 1,
        snapshots_imported: 1,
        to_capture: 1,
        ..TcNoImportReport::default()
    };
    let commands = &mut fixture["commands"];
    commands["load_client_storage_snapshot"] = json!([{ "args": {}, "result": imported }]);
    commands["tcno_select_folder"] = json!([{ "args": {}, "result": "C:/TcNo" }]);
    commands["tcno_preview"] = json!([{ "args": {"path": "C:/TcNo"}, "result": preview }]);
    commands[IMPORT] = json!([{ "args": {"path": "C:/TcNo"}, "result": report }]);
    commands[ROBLOX_ADD] = json!([{ "args": {}, "result": {
        "userId": "77", "username": "builder", "displayName": "Builder", "lastLoginAt": null
    } }]);
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    let mut app = AppModel::with_backend(ctx, mock.clone(), Some(mock.clone()), Clock::wall());
    app.wait_boot(ctx).unwrap();
    (app, mock)
}

fn frame(app: &mut AppModel, ctx: &Context) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.frame(ui));
    output.textures_delta.clear();
}

fn pump_until(app: &mut AppModel, ctx: &Context, condition: impl Fn(&AppModel) -> bool) {
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    while !condition(app) {
        assert!(Instant::now() < deadline, "the condition never held");
        frame(app, ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn frames(app: &mut AppModel, ctx: &Context, count: usize) {
    for _ in 0..count {
        frame(app, ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn note(app: &AppModel) -> String {
    app.booted()
        .unwrap()
        .stores
        .account_card_note("a")
        .to_string()
}

fn set_note(app: &mut AppModel, text: &str) {
    app.booted
        .as_mut()
        .unwrap()
        .stores
        .set_account_card_note("a", text);
}

#[test]
fn the_open_app_never_writes_its_stores_over_a_tcno_import() {
    let ctx = Context::default();
    let (mut app, mock) = tcno_app(&ctx);
    frame(&mut app, &ctx);
    // A note write is under way and held at the disk.
    let held_save = HeldCommand::new(&mock, SAVE);
    set_note(&mut app, "typed before");
    pump_until(&mut app, &ctx, |_| mock.waiting(SAVE) == 1);

    app.start_tcno_import("C:/TcNo".into());
    // Typed while the import waits: no new write starts, nor the import.
    set_note(&mut app, "typed during the import");
    frames(&mut app, &ctx, 20);
    assert!(app.backup.holds_writes());
    assert_eq!(mock.seen(IMPORT), 0, "the import ran before the save ended");
    assert_eq!(mock.seen(SAVE), 1, "a second write raced the first");

    // The held write ends, the one typed meanwhile is written, then the
    // import runs.
    let held_import = HeldCommand::new(&mock, IMPORT);
    drop(held_save);
    pump_until(&mut app, &ctx, |_| mock.waiting(IMPORT) == 1);
    let written = mock.seen(SAVE);
    assert_eq!(written, 2);
    // Typed while the import writes the stores: never written over them.
    set_note(&mut app, "typed while importing");
    frames(&mut app, &ctx, 20);
    assert_eq!(mock.seen(SAVE), written, "a write ran during the import");
    drop(held_import);
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    frames(&mut app, &ctx, 5);
    assert_eq!(mock.seen(IMPORT), 1);
    assert_eq!(
        mock.seen(SAVE),
        written,
        "a stale write followed the import"
    );
    assert_eq!(note(&app), "imported note");
    assert!(!app.booted().unwrap().stores.storage().has_unsaved());
    let card = &app.backup.tcno.card;
    assert!(!card.error, "{}", card.notice);
    assert!(card.notice.contains('2'), "{}", card.notice);
}

#[test]
fn a_picked_tcno_folder_is_previewed_then_imported_once_confirmed() {
    let ctx = Context::default();
    let (mut app, mock) = tcno_app(&ctx);
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    assert!(app.settings_panel.as_ref().unwrap().tcno.available);

    app.import_tcno();
    pump_until(&mut app, &ctx, |a| a.backup.tcno.pending.is_some());
    assert_eq!(mock.seen("tcno_preview"), 1);
    assert_eq!(mock.seen(IMPORT), 0, "the import ran before the confirm");
    assert!(app.settings_panel.as_ref().unwrap().tcno.busy);
    // A second pick waits for the confirm.
    app.import_tcno();
    frames(&mut app, &ctx, 3);
    assert_eq!(mock.seen("tcno_select_folder"), 1);

    let (_, ticket) = app.backup.tcno.pending.clone().unwrap();
    app.settle_tcno_import(ticket, true);
    pump_until(&mut app, &ctx, |a| {
        !a.backup.holds_writes() && !a.backup.tcno.card.notice.is_empty()
    });
    assert_eq!(mock.seen(IMPORT), 1);
    assert_eq!(note(&app), "imported note");
    frame(&mut app, &ctx);
    let card = &app.settings_panel.as_ref().unwrap().tcno;
    assert!(!card.busy);
    assert!(!card.error, "{}", card.notice);
}

#[test]
fn a_refused_tcno_confirm_imports_nothing() {
    let ctx = Context::default();
    let (mut app, mock) = tcno_app(&ctx);
    frame(&mut app, &ctx);
    app.import_tcno();
    pump_until(&mut app, &ctx, |a| a.backup.tcno.pending.is_some());
    let (_, ticket) = app.backup.tcno.pending.clone().unwrap();
    app.settle_tcno_import(ticket, false);
    frames(&mut app, &ctx, 5);
    assert!(!app.backup.holds_writes());
    assert_eq!(mock.seen(IMPORT), 0);
    assert!(!app.backup.busy());
}

/// Pastes a Roblox cookie in the panel and adds it.
fn add_roblox(app: &mut AppModel, ctx: &Context, cookie: &str) {
    app.apply_settings(ctx, SettingsAction::RobloxCookie(cookie.into()));
    app.apply_settings(ctx, SettingsAction::AddRobloxCookie);
}

#[test]
fn an_account_being_saved_holds_the_tcno_import_and_none_starts_during_it() {
    let ctx = Context::default();
    let (mut app, mock) = tcno_app(&ctx);
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    // A Roblox account is being written to its list.
    let held_add = HeldCommand::new(&mock, ROBLOX_ADD);
    add_roblox(&mut app, &ctx, "cookie-a");
    pump_until(&mut app, &ctx, |_| mock.waiting(ROBLOX_ADD) == 1);

    let held_import = HeldCommand::new(&mock, IMPORT);
    app.start_tcno_import("C:/TcNo".into());
    frames(&mut app, &ctx, 20);
    let imported_early = mock.seen(IMPORT);
    // Nor does an account write start while the import waits or runs.
    drop(held_add);
    pump_until(&mut app, &ctx, |_| mock.waiting(IMPORT) == 1);
    add_roblox(&mut app, &ctx, "cookie-b");
    frames(&mut app, &ctx, 10);
    let added_during = mock.seen(ROBLOX_ADD);
    let refusal = app.settings_panel.as_ref().unwrap().roblox_error.clone();
    // Every held call ends before asserting: a worker left blocked would
    // hang the test's teardown.
    drop(held_import);
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(
        imported_early, 0,
        "the import ran while an account was saved"
    );
    assert_eq!(added_during, 1, "an account write ran during the import");
    assert_eq!(
        refusal,
        accshift_app::i18n::translate(accshift_app::i18n::Locale::En, "settings.tcnoHoldsWrites")
    );
    assert_eq!(mock.seen(IMPORT), 1);
}

const POLL: &str = "platform_get_setup_status";

#[test]
fn a_setup_poll_holds_the_tcno_import_and_none_is_sent_during_it() {
    let ctx = Context::default();
    let (mut app, mock) = tcno_app_with(&ctx, |fixture| {
        let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
        let settings = &mut boot["storageSnapshot"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["riot"]);
        settings["defaultPlatformId"] = json!("riot");
    });
    mock.set_setup(Some(vec![json!({
        "setupId": "riot-profile-new",
        "state": "waiting_for_login",
        "accountId": "",
        "accountDisplayName": "",
        "errorMessage": ""
    })]));
    pump_until(&mut app, &ctx, |a| a.booted().unwrap().accounts.settled());
    app.apply_one(&ctx, Action::AddAccount);
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().accounts.add_flow.is_active()
    });
    // A poll is on its way: its answer may save the new account.
    let held_poll = HeldCommand::new(&mock, POLL);
    pump_until(&mut app, &ctx, |_| mock.waiting(POLL) == 1);

    let held_import = HeldCommand::new(&mock, IMPORT);
    app.start_tcno_import("C:/TcNo".into());
    frames(&mut app, &ctx, 20);
    let imported_early = mock.seen(IMPORT);
    drop(held_poll);
    pump_until(&mut app, &ctx, |_| mock.waiting(IMPORT) == 1);
    let polls = mock.seen(POLL);
    // Past the poll delay: the setup sends no poll while the import runs.
    let until = Instant::now() + accshift_app::platform_add_flow::POLL_DELAY * 2;
    while Instant::now() < until {
        frames(&mut app, &ctx, 1);
    }
    let polled_during = mock.seen(POLL) - polls;
    // Every held call ends before asserting (see above).
    drop(held_import);
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(imported_early, 0, "the import ran while a setup poll did");
    assert_eq!(polled_during, 0, "a setup poll ran during the import");
    assert!(app.booted().unwrap().accounts.add_flow.is_active());
}
