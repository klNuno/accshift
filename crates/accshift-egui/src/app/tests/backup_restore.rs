//! The backup restore behind its boundary, on the mock backend: it waits for
//! the writes already running, holds new ones, registers the restored
//! platforms before the settings are read, and survives the settings panel
//! closing under it. An export waits behind the same boundary.

use super::super::settings_panel::SettingsAction;
use super::*;
use accshift_app::i18n::translate;
use serde_json::json;

const DESCRIPTOR: &str = include_str!("../../../../accshift-app/tests/data/real/descriptor.json");

/// The mock app, with a restore that brings back a note, a user platform
/// and settings that enable it.
fn restore_app(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    let mut fixture: Value =
        serde_json::from_str(include_str!("../../../parity/data/empty.json")).unwrap();
    fixture["spec"]["switchDelayMs"] = json!(0);
    let boot = &mut fixture["commands"]["get_boot_payload"][0]["result"];
    boot["storageSnapshot"]["stores"]["client.settings"]["animations"] = json!("off");
    let mut restored = boot["storageSnapshot"].clone();
    let settings = &mut restored["stores"]["client.settings"];
    settings["enabledPlatforms"] = json!(["steam", "egui-fixture"]);
    settings["defaultPlatformId"] = json!("egui-fixture");
    settings["language"] = json!("fr");
    restored["stores"]["client.account-card-notes"] = json!({"a": "restored note"});
    restored["manifest"]["stores"] = json!({
        "client.settings": "restored-settings",
        "client.account-card-notes": "restored-notes"
    });
    let descriptor: Value = serde_json::from_str(DESCRIPTOR).unwrap();
    let commands = &mut fixture["commands"];
    commands["load_client_storage_snapshot"] = json!([{ "args": {}, "result": restored }]);
    commands["reload_user_platforms"] = json!([{ "args": {}, "result": {
        "dir": "C:/platforms", "loaded": [descriptor], "skipped": [], "rejected": []
    }}]);
    commands["backup_restore"] = json!([{ "args": {"path": "C:/b.json"}, "result": {
        "preview": {
            "path": "C:/b.json", "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "folders": 0, "notes": 1, "colors": 0, "personas": 0, "themes": 0,
            "descriptors": 1, "hasSettings": true
        },
        "preRestoreDir": "C:/config/user/backups/pre-restore-1"
    }}]);
    commands["backup_select_export_path"] = json!([{ "args": {}, "result": "C:/out.json" }]);
    commands["backup_export"] = json!([{ "args": {"path": "C:/out.json"}, "result": {
        "preview": {
            "path": "C:/out.json", "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "folders": 0, "notes": 1, "colors": 0, "personas": 0, "themes": 0,
            "descriptors": 0, "hasSettings": true
        },
        "preRestoreDir": ""
    }}]);
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

const SAVE: &str = "save_client_storage_store";

#[test]
fn a_restore_waits_for_a_running_save_and_holds_new_writes() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    // A note write is under way and held at the disk.
    assert!(mock.hold(SAVE));
    set_note(&mut app, "typed before");
    pump_until(&mut app, &ctx, |_| mock.waiting(SAVE) == 1);

    app.start_backup_restore("C:/b.json".into());
    // Typed while the restore waits: no new write starts.
    set_note(&mut app, "typed during the restore");
    for _ in 0..20 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.backup.holds_writes());
    assert_eq!(
        mock.seen("backup_restore"),
        0,
        "the restore ran before the save ended"
    );
    assert_eq!(
        mock.seen(SAVE),
        1,
        "a second write of the store raced the first"
    );

    // The held write ends, the one typed meanwhile is written before the
    // restore so the pre-restore copy holds it; then the restore runs.
    assert!(mock.hold("backup_restore"));
    mock.release(SAVE);
    pump_until(&mut app, &ctx, |_| mock.waiting("backup_restore") == 1);
    let written = mock.seen(SAVE);
    assert_eq!(written, 2);
    // Typed while the restore job runs: held until it is over, then
    // replaced by the restored store.
    set_note(&mut app, "typed while restoring");
    for _ in 0..20 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(mock.seen(SAVE), written, "a write ran during the restore");
    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 1);
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert_eq!(
        mock.seen(SAVE),
        written,
        "a stale write followed the restore"
    );
    assert_eq!(note(&app), "restored note");
    assert!(!app.booted().unwrap().stores.storage().has_unsaved());
    assert!(!app.backup.card.error, "{}", app.backup.card.notice);
    assert!(app.backup.card.notice.contains("pre-restore-1"));
}

#[test]
fn restored_settings_keep_the_restored_platform_they_enable() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    // The descriptor reload comes back last: the settings must still wait
    // for it before they are sanitized.
    assert!(mock.hold("reload_user_platforms"));
    frame(&mut app, &ctx);
    app.start_backup_restore("C:/b.json".into());
    pump_until(&mut app, &ctx, |_| {
        mock.waiting("reload_user_platforms") == 1
    });
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert_ne!(app.booted().unwrap().stores.settings().language, Locale::Fr);
    mock.release("reload_user_platforms");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    let settings = app.booted().unwrap().stores.settings();
    assert_eq!(settings.language, Locale::Fr);
    assert_eq!(settings.enabled_platforms, ["steam", "egui-fixture"]);
    assert_eq!(settings.default_platform_id, "egui-fixture");
    assert!(app
        .booted()
        .unwrap()
        .platforms
        .iter()
        .any(|p| p.id == "egui-fixture"));
}

#[test]
fn closing_settings_during_a_restore_keeps_its_outcome() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    assert!(mock.hold("backup_restore"));
    app.start_backup_restore("C:/b.json".into());
    pump_until(&mut app, &ctx, |_| mock.waiting("backup_restore") == 1);
    assert!(app.settings_panel.as_ref().unwrap().backup.busy);

    app.toggle_settings_panel();
    assert!(app.settings_panel.is_none());
    frame(&mut app, &ctx);
    // Reopened while it runs: the card is still busy and a second restore
    // cannot start.
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    assert!(app.settings_panel.as_ref().unwrap().backup.busy);
    app.import_backup();
    app.start_backup_restore("C:/b.json".into());
    app.toggle_settings_panel();

    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 1);
    assert_eq!(mock.seen("backup_select_import_file"), 0);
    assert_eq!(note(&app), "restored note");
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    let card = &app.settings_panel.as_ref().unwrap().backup;
    assert!(!card.busy);
    assert!(card.notice.contains("pre-restore-1"), "{}", card.notice);
}

#[test]
fn a_write_that_fails_before_the_restore_cancels_it() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    assert!(mock.fail(SAVE, Some("disk full")));
    set_note(&mut app, "typed before");
    app.start_backup_restore("C:/b.json".into());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 0);
    assert!(app.backup.card.error);
    assert!(
        app.backup.card.notice.contains("disk full"),
        "{}",
        app.backup.card.notice
    );
}

#[test]
fn the_backup_dialogs_are_titled_in_the_app_language() {
    use super::super::backup_flow::dialog_labels;
    let (title, all_files) = dialog_labels(Locale::Fr, "settings.backupImportDialog");
    assert_eq!(title, "Restaurer une sauvegarde d'accshift");
    assert_eq!(all_files, "Tous les fichiers");
    // Every language has its own text, not the English fallback.
    for key in ["settings.backupExportDialog", "settings.backupImportDialog"] {
        let english = dialog_labels(Locale::En, key);
        assert!(!english.0.starts_with("settings."), "{key}");
        for locale in accshift_app::i18n::LOCALES.into_iter().skip(1) {
            let (title, all_files) = dialog_labels(locale, key);
            assert_ne!(title, english.0, "{locale:?} {key}");
            assert_ne!(all_files, english.1, "{locale:?}");
        }
    }
}

#[test]
fn an_export_waits_for_a_running_save_and_holds_new_writes() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    assert!(mock.hold(SAVE));
    set_note(&mut app, "typed before");
    pump_until(&mut app, &ctx, |_| mock.waiting(SAVE) == 1);

    assert!(mock.hold("backup_export"));
    app.export_backup();
    pump_until(&mut app, &ctx, |a| a.backup.holds_writes());
    for _ in 0..20 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        mock.seen("backup_export"),
        0,
        "the export read before the save ended"
    );

    // The save lands, then the export reads; a note typed while it reads
    // waits for it.
    mock.release(SAVE);
    pump_until(&mut app, &ctx, |_| mock.waiting("backup_export") == 1);
    assert_eq!(mock.seen(SAVE), 1);
    set_note(&mut app, "typed while exporting");
    for _ in 0..20 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(mock.seen(SAVE), 1, "a write ran while the export read");
    mock.release("backup_export");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.card.error, "{}", app.backup.card.notice);
    assert!(app.backup.card.notice.contains("C:/out.json"));
    pump_until(&mut app, &ctx, |_| mock.seen(SAVE) == 2);
}

#[test]
fn an_export_cancelled_in_the_dialog_holds_nothing() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    assert!(mock.fail("backup_select_export_path", Some("cancelled")));
    app.export_backup();
    pump_until(&mut app, &ctx, |_| {
        mock.seen("backup_select_export_path") == 1
    });
    for _ in 0..10 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!app.backup.card.busy);
    assert!(!app.backup.holds_writes());
    assert_eq!(mock.seen("backup_export"), 0);
    assert!(app.backup.card.notice.is_empty());
}

/// The restore replaced the files but the stores cannot be read back: the
/// old stores in memory are never written over them.
fn restored_but_unread(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    let (mut app, mock) = restore_app(ctx);
    frame(&mut app, ctx);
    assert!(mock.hold("backup_restore"));
    assert!(mock.fail("load_client_storage_snapshot", Some("manifest unreadable")));
    app.start_backup_restore("C:/b.json".into());
    pump_until(&mut app, ctx, |_| mock.waiting("backup_restore") == 1);
    set_note(&mut app, "typed while restoring");
    mock.release("backup_restore");
    pump_until(&mut app, ctx, |_| {
        mock.seen("load_client_storage_snapshot") >= 2
    });
    for _ in 0..10 {
        frame(&mut app, ctx);
    }
    assert!(app.backup.holds_writes());
    assert_eq!(
        mock.seen(SAVE),
        0,
        "the old notes went over the restored ones"
    );
    (app, mock)
}

#[test]
fn stores_not_read_back_are_held_then_adopted_once_read() {
    let ctx = Context::default();
    let (mut app, mock) = restored_but_unread(&ctx);
    let restart = translate(app.booted().unwrap().locale, "settings.backupRestartNeeded");
    assert_eq!(app.backup.card.notice, restart);
    assert!(mock.fail("load_client_storage_snapshot", None));
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(note(&app), "restored note");
    assert!(
        app.backup.card.notice.contains("pre-restore-1"),
        "{}",
        app.backup.card.notice
    );
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert_eq!(mock.seen(SAVE), 0);
}

#[test]
fn closing_with_stores_not_read_back_writes_none_of_the_old_ones() {
    let ctx = Context::default();
    let (mut app, mock) = restored_but_unread(&ctx);
    app.flush_app_state().unwrap();
    assert_eq!(mock.seen(SAVE), 0, "the close flush wrote the old stores");
    assert!(!app.backup.holds_writes());
}

#[test]
fn a_descriptor_task_keeps_the_boundary_up_after_settings_close() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    assert!(mock.hold("reload_user_platforms"));
    app.apply_settings(&ctx, SettingsAction::ReloadDescriptors);
    pump_until(&mut app, &ctx, |_| {
        mock.waiting("reload_user_platforms") == 1
    });
    app.start_backup_restore("C:/b.json".into());
    // The panel and its handle on the task go; the task still runs.
    app.toggle_settings_panel();
    assert!(app.settings_panel.is_none());
    for _ in 0..20 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        mock.seen("backup_restore"),
        0,
        "the restore ran beside a descriptor task"
    );
    mock.release("reload_user_platforms");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 1);
}

#[test]
fn descriptor_and_theme_writes_are_refused_while_a_backup_runs() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    assert!(mock.hold("backup_restore"));
    app.start_backup_restore("C:/b.json".into());
    pump_until(&mut app, &ctx, |_| mock.waiting("backup_restore") == 1);

    let reloads = mock.seen("reload_user_platforms");
    app.apply_settings(&ctx, SettingsAction::ReloadDescriptors);
    let panel = app.settings_panel.as_ref().unwrap();
    assert!(!panel.descriptor_busy);
    assert_eq!(
        panel.descriptor_error,
        translate(panel.draft.language, "settings.backupHoldsWrites")
    );
    assert!(
        app.backup.gate.admit(|_| ()).is_none(),
        "a theme save got through"
    );
    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("reload_user_platforms"), reloads + 1);

    // Once over, both run again.
    assert!(app.backup.gate.admit(|_| ()).is_some());
    app.apply_settings(&ctx, SettingsAction::ReloadDescriptors);
    assert!(app.settings_panel.as_ref().unwrap().descriptor_busy);
}
