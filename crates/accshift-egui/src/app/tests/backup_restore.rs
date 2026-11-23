//! The backup restore behind its boundary, on the mock backend: it waits for
//! the writes already running, holds new ones, registers the restored
//! platforms before the settings are read, and survives the settings panel
//! closing under it. An export waits behind the same boundary.

use super::super::settings_panel::SettingsAction;
use super::pump;
use super::*;
use accshift_app::i18n::{translate, Locale};
use serde_json::json;

const DESCRIPTOR: &str = include_str!("../../../../accshift-app/tests/data/real/descriptor.json");

/// The mock app, with a restore that brings back a note, a user platform
/// and settings that enable it.
fn restore_app(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    restore_app_with(ctx, |_| {})
}

/// [`restore_app`] with `edit` applied to its mock commands.
fn restore_app_with(ctx: &Context, edit: impl FnOnce(&mut Value)) -> (AppModel, Arc<MockBackend>) {
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
    edit(commands);
    let mock = Arc::new(MockBackend::from_json(&fixture.to_string(), None).unwrap());
    let mut app = AppModel::with_backend(ctx, mock.clone(), Some(mock.clone()), Clock::wall());
    app.wait_boot(ctx).unwrap();
    (app, mock)
}

fn frame(app: &mut AppModel, ctx: &Context) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.frame(ui));
    output.textures_delta.clear();
}

/// Frames until `condition` holds: [`pump::until`].
#[track_caller]
fn pump_until(app: &mut AppModel, ctx: &Context, condition: impl Fn(&AppModel) -> bool) {
    pump::until(app, ctx, frame, condition);
}

#[test]
fn a_slow_frame_is_not_a_hang() {
    // Regression (SLOW_FRAME): the wait's deadline counted the frames
    // themselves. On a loaded runner one debug frame outlasted it, and a
    // condition two frames away "never held" though nothing was stuck.
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    // The panel's mount job cannot land before the first frame polls it.
    assert!(mock.hold("steam_has_api_key"));
    app.toggle_settings_panel();
    let limit = std::time::Duration::from_millis(50);
    pump::within(
        &mut app,
        &ctx,
        limit,
        |app, ctx| {
            frame(app, ctx);
            mock.release("steam_has_api_key");
            // The frame outlasts the limit; the job lands meanwhile.
            std::thread::sleep(limit * 3);
        },
        |a| a.settings_panel.as_ref().is_some_and(|p| p.hydrated),
    );
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

    app.start_backup_restore("C:/b.json".into(), String::new());
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

/// Well inside the next deadline the app asks a frame for on its own (a
/// toast's expiry, seconds away), so a wait that needs a frame nobody asked
/// for does not hold within it.
const NO_TIMER: std::time::Duration = std::time::Duration::from_millis(500);
/// Long enough for a released mocked call to deliver its result and wake
/// the app, and for a note's debounce to run out. Too short only lets the
/// regressions below pass without their fix, never fail with it.
const LANDED: std::time::Duration = std::time::Duration::from_millis(200);

#[test]
fn a_write_ending_under_the_boundary_wakes_the_restore() {
    // Regression (DRAIN_WAKE): the frame that reports a write runs the
    // boundary's drain before it, and asked for no frame after: the restore
    // waited for an unrelated deadline or the next mouse move.
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    assert!(mock.hold(SAVE));
    set_note(&mut app, "typed before");
    pump_until(&mut app, &ctx, |_| mock.waiting(SAVE) == 1);
    app.start_backup_restore("C:/b.json".into(), String::new());
    assert!(mock.hold("backup_restore"));
    mock.release(SAVE);
    // The write's result and its wake are in before the next frame, which
    // then has nothing left to wake it.
    std::thread::sleep(LANDED);
    pump::within(&mut app, &ctx, NO_TIMER, frame, |_| {
        mock.waiting("backup_restore") == 1
    });
    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
}

#[test]
fn a_note_typed_while_its_store_writes_is_written_when_that_write_ends() {
    // Regression (DRAIN_WAKE): a write due while its store still wrote was
    // left out of the timed repaint, and the frame that reported the write
    // asked for none: it waited for an unrelated deadline or the next input.
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    assert!(mock.hold(SAVE));
    set_note(&mut app, "first");
    pump_until(&mut app, &ctx, |_| mock.waiting(SAVE) == 1);
    set_note(&mut app, "second");
    mock.release(SAVE);
    // Past the second note's debounce too: only the frame that reports the
    // first write can start the second.
    std::thread::sleep(LANDED);
    pump::within(&mut app, &ctx, NO_TIMER, frame, |_| mock.seen(SAVE) == 2);
    pump_until(&mut app, &ctx, |a| {
        !a.booted().unwrap().stores.storage().has_unsaved()
    });
    assert_eq!(note(&app), "second");
}

#[test]
fn restored_settings_keep_the_restored_platform_they_enable() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    // The descriptor reload comes back last: the settings must still wait
    // for it before they are sanitized.
    assert!(mock.hold("reload_user_platforms"));
    frame(&mut app, &ctx);
    app.start_backup_restore("C:/b.json".into(), String::new());
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
    app.start_backup_restore("C:/b.json".into(), String::new());
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
    app.start_backup_restore("C:/b.json".into(), String::new());
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
    app.start_backup_restore("C:/b.json".into(), String::new());
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
fn a_save_job_that_dies_cancels_the_restore_and_frees_its_store() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    // The note write is handed out, then its job panics before answering.
    set_note(&mut app, "typed before");
    let stores = &mut app.booted.as_mut().unwrap().stores;
    let mut saves = stores.take_all_saves();
    assert_eq!(saves.len(), 1);
    let save = saves.pop().unwrap();
    let job = app
        .jobs
        .run(|_| -> BackendResult<Option<String>> { panic!("the save job died") });
    app.saves.push((save.store_id, job));

    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 0);
    assert!(app.backup.card.error, "{}", app.backup.card.notice);
    assert!(!app.booted().unwrap().stores.storage().has_unsaved());
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
    let cancelled = accshift_core::error::AppError::Cancelled.to_string();
    assert!(mock.fail("backup_select_export_path", Some(&cancelled)));
    app.export_backup();
    pump_until(&mut app, &ctx, |_| {
        mock.seen("backup_select_export_path") == 1
    });
    // The picker was called; the app takes its answer on a later frame.
    pump_until(&mut app, &ctx, |a| !a.backup.card.busy);
    assert!(!app.backup.holds_writes());
    assert_eq!(mock.seen("backup_export"), 0);
    assert!(app.backup.card.notice.is_empty());
}

#[test]
fn a_picker_that_fails_says_why() {
    // Regression (PICKER_ERROR): any picker error read as a cancel, so a
    // path the backend refused closed the dialog with no word.
    let odd = "C:/a\u{FFFD}.json cannot be used: its name is not valid Unicode";
    for command in ["backup_select_export_path", "backup_select_import_file"] {
        let ctx = Context::default();
        let (mut app, mock) = restore_app(&ctx);
        assert!(mock.fail(command, Some(odd)));
        if command == "backup_select_export_path" {
            app.export_backup();
        } else {
            app.import_backup();
        }
        pump_until(&mut app, &ctx, |_| mock.seen(command) == 1);
        pump_until(&mut app, &ctx, |a| a.backup.card.error);
        assert!(
            app.backup.card.error && app.backup.card.notice.contains(odd),
            "PICKER_ERROR: {command} said {:?}",
            app.backup.card.notice
        );
        assert!(!app.backup.holds_writes());
        assert_eq!(mock.seen("backup_export"), 0);
        assert_eq!(mock.seen("backup_preview"), 0);
    }
}

/// The restore replaced the files but the stores cannot be read back: the
/// old stores in memory are never written over them, and the app asks for
/// the restart that loads them.
fn restored_but_unread(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    let (mut app, mock) = restore_app(ctx);
    frame(&mut app, ctx);
    assert!(mock.hold("backup_restore"));
    assert!(mock.fail("load_client_storage_snapshot", Some("manifest unreadable")));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, ctx, |_| mock.waiting("backup_restore") == 1);
    set_note(&mut app, "typed while restoring");
    mock.release("backup_restore");
    pump_until(&mut app, ctx, |a| a.backup.restart_needed());
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
fn stores_not_read_back_lock_the_app_until_it_restarts() {
    // Regression (LOCKED_EDIT, LOCKED_FOLDER): the app stayed editable on
    // the old stores while it retried the read back; the edits it showed
    // were dropped once the read back went through, or written over the
    // restored files.
    let ctx = Context::default();
    let (mut app, mock) = restored_but_unread(&ctx);
    let locale = app.booted().unwrap().locale;
    assert_eq!(
        app.backup.card.notice,
        translate(locale, "settings.backupRestartNeeded")
    );
    let mode = app.booted().unwrap().stores.view_mode();
    let next = if mode == ViewMode::List {
        ViewMode::Grid
    } else {
        ViewMode::List
    };
    app.apply(&ctx, vec![Action::SetViewMode(next)]);
    assert_eq!(
        app.booted().unwrap().stores.view_mode(),
        mode,
        "LOCKED_EDIT: an edit applied on the old stores"
    );
    // A new folder, through the menu and its dialog.
    let folders = app.booted().unwrap().stores.folders().folders.len();
    app.apply(
        &ctx,
        vec![Action::ContextMenu(
            MenuTarget::Background,
            egui::pos2(200.0, 200.0),
        )],
    );
    app.apply(
        &ctx,
        vec![Action::Menu(
            app.menu_open_id,
            crate::ui::components::context_menu::MenuEvent::Activate(
                accshift_app::actions::dialogs::AppMenuCommand::NewFolder,
            ),
        )],
    );
    if let Some(input) = app.dialog_mounts().0 {
        app.apply(
            &ctx,
            vec![
                Action::InputDialog(input, DialogEvent::Input("Created after restore".into())),
                Action::InputDialog(input, DialogEvent::Submit),
            ],
        );
    }
    assert_eq!(
        app.booted().unwrap().stores.folders().folders.len(),
        folders,
        "LOCKED_FOLDER: a folder created on the old stores"
    );
    // The stores read again: nothing retries, the app stays locked.
    assert!(mock.fail("load_client_storage_snapshot", None));
    let reads = mock.seen("load_client_storage_snapshot");
    for _ in 0..20 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(app.backup.restart_needed());
    assert_eq!(mock.seen("load_client_storage_snapshot"), reads);
    assert_eq!(mock.seen(SAVE), 0, "a restored file was written over");

    // "Restart now": the last flush writes nothing, then the relaunch.
    app.apply(&ctx, vec![Action::RestartAfterRestore]);
    assert_eq!(app.updates.relaunch_requests(), 1);
    assert_eq!(mock.seen(SAVE), 0, "the restart flush wrote the old stores");
    assert!(app.backup.restart_needed());
}

#[test]
fn the_restart_modal_is_drawn_over_the_app_and_its_button_restarts() {
    let ctx = Context::default();
    let (mut app, mock) = restored_but_unread(&ctx);
    let locale = app.booted().unwrap().locale;
    let id = egui::Id::unique(crate::ui::components::dialog::NOTICE_ID)
        .with(0u64)
        .with("ok");
    let rect = ctx
        .read_response(id)
        .map(|r| r.rect)
        .expect("the restart button is drawn");
    assert!(!translate(locale, "settings.backupRestartNow").is_empty());
    // Escape closes nothing: the modal stays.
    let mut input = egui::RawInput::default();
    input.events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    output.textures_delta.clear();
    assert!(app.backup.restart_needed());
    assert_eq!(app.updates.relaunch_requests(), 0);
    // A click on the button.
    let at = rect.center();
    for events in [
        vec![egui::Event::PointerMoved(at)],
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ] {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.frame(ui));
        output.textures_delta.clear();
    }
    assert_eq!(app.updates.relaunch_requests(), 1);
    assert_eq!(mock.seen(SAVE), 0);
}

#[test]
fn closing_with_stores_not_read_back_writes_none_of_the_old_ones() {
    let ctx = Context::default();
    let (mut app, mock) = restored_but_unread(&ctx);
    app.flush_app_state().unwrap();
    assert_eq!(mock.seen(SAVE), 0, "the close flush wrote the old stores");
    // A close that does not end the process leaves the app as locked.
    assert!(app.backup.restart_needed());
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
    app.start_backup_restore("C:/b.json".into(), String::new());
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
fn a_descriptor_install_that_fails_while_the_restore_waits_cancels_it() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app_with(&ctx, |commands| {
        commands["descriptor_install_file"] = json!([{ "args": {}, "error": "disk full" }]);
    });
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    app.apply_settings(&ctx, SettingsAction::PickDescriptor);
    // The panel's jobs only: a drawn frame would show the preview dialog.
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    while app
        .settings_panel
        .as_ref()
        .is_some_and(|p| p.descriptor_preview.is_none())
    {
        assert!(Instant::now() < deadline, "the preview never came");
        app.tick_settings_panel(&ctx, Instant::now());
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(mock.hold("descriptor_install_file"));
    app.apply_settings(&ctx, SettingsAction::InstallDescriptor);
    while mock.waiting("descriptor_install_file") == 0 {
        assert!(Instant::now() < deadline, "the install never started");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    app.start_backup_restore("C:/b.json".into(), String::new());
    // The panel goes; the install still fails under the boundary.
    app.toggle_settings_panel();
    assert!(app.settings_panel.is_none());
    mock.release("descriptor_install_file");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(
        mock.seen("backup_restore"),
        0,
        "the restore ran after a failed descriptor write"
    );
    assert!(app.backup.card.error);
    assert!(
        app.backup.card.notice.contains("disk full"),
        "{}",
        app.backup.card.notice
    );
}

#[test]
fn descriptor_and_theme_writes_are_refused_while_a_backup_runs() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    frame(&mut app, &ctx);
    assert!(mock.hold("backup_restore"));
    app.start_backup_restore("C:/b.json".into(), String::new());
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

#[test]
fn no_update_installs_while_a_restart_is_needed() {
    let ctx = Context::default();
    let (mut app, mock) = restored_but_unread(&ctx);
    app.updates.ready_for_test();
    app.apply_update(&ctx);
    // Refused before its flush: the boundary and the update both stay.
    assert!(app.backup.restart_needed());
    assert!(app.updates.ready());
    assert_eq!(mock.seen(SAVE), 0);
    // Through the title bar: the action is dropped under the modal.
    app.apply(&ctx, vec![Action::ApplyUpdate]);
    assert!(app.updates.ready());
    for _ in 0..10 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        mock.seen(SAVE),
        0,
        "the old notes went over the restored ones"
    );
    assert_ne!(note(&app), "restored note");
}

#[test]
fn no_backup_starts_while_an_update_installs() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    app.updates.ready_for_test();
    app.updates.install_for_test();
    assert!(app.updates.installing());
    let refused = translate(
        app.booted().unwrap().locale,
        "settings.backupUpdateInstalling",
    );

    // A restore confirmed, or an export path picked, while it installs.
    app.start_backup_restore("C:/b.json".into(), String::new());
    assert!(!app.backup.holds_writes());
    assert!(app.backup.card.error);
    assert_eq!(app.backup.card.notice, refused);
    app.export_backup();
    pump_until(&mut app, &ctx, |_| {
        mock.seen("backup_select_export_path") == 1
    });
    for _ in 0..10 {
        frame(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!app.backup.holds_writes());
    assert_eq!(app.backup.card.notice, refused);

    // The install fails and the app stays: a backup runs again.
    app.updates.install_failed_for_test();
    assert!(!app.updates.installing());
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert_eq!(mock.seen("backup_restore"), 0);
    assert_eq!(mock.seen("backup_export"), 0);
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 1);
}

#[test]
fn a_write_failing_right_after_the_boundary_goes_up_cancels_the_restore() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    let gate = app.backup.gate.clone();
    let write = gate
        .admit_write(|_| Err::<(), _>("disk full"))
        .expect("the gate is open");
    let backend = mock.clone();
    // The admitted write fails the moment the boundary is published, before
    // the call that published it returns.
    gate.on_published(move || {
        let _ = write(&*backend);
    });
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(
        mock.seen("backup_restore"),
        0,
        "GATE_FORCED_SCHEDULE: the restore ran after a failed write"
    );
    assert!(app.backup.card.error);
    assert!(
        app.backup.card.notice.contains("disk full"),
        "{}",
        app.backup.card.notice
    );
}

#[test]
fn a_restore_whose_theme_cannot_be_read_keeps_it_selected_until_it_loads() {
    // Regression: a theme file locked during the read back failed it, or
    // the restored settings were sanitized to the default theme and saved.
    let ctx = Context::default();
    let theme = json!({"schemaVersion": 1, "id": "mine", "name": "Mine", "colorScheme": "dark", "tokens": {"bgCard": "#112233"}});
    let (mut app, mock) = restore_app_with(&ctx, |commands| {
        commands["load_client_storage_snapshot"][0]["result"]["stores"]["client.settings"]
            ["themeId"] = json!("mine");
        commands["list_custom_themes"] = json!([{ "args": {}, "result": [theme] }]);
    });
    focus(&mut app, &ctx, true);
    let before = app.booted().unwrap().theme.vars.bg_card;
    assert!(mock.fail("list_custom_themes", Some("theme file locked")));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    // Adopted: the restored stores, the theme they name kept.
    assert!(!app.backup.restart_needed());
    assert_eq!(note(&app), "restored note");
    let b = app.booted().unwrap();
    assert_eq!(b.stores.settings().theme_id, "mine");
    assert!(b.registry.document("mine").is_none());
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert_eq!(mock.seen(SAVE), 0, "the restored settings were rewritten");

    // The file reads again: the next focus loads the theme.
    assert!(mock.fail("list_custom_themes", None));
    focus(&mut app, &ctx, false);
    focus(&mut app, &ctx, true);
    pump::until(
        &mut app,
        &ctx,
        |app, ctx| focus(app, ctx, true),
        |a| a.booted().unwrap().registry.document("mine").is_some(),
    );
    let b = app.booted().unwrap();
    assert_eq!(b.stores.settings().theme_id, "mine");
    assert_ne!(b.theme.vars.bg_card, before, "the theme does not draw");
    assert_eq!(mock.seen(SAVE), 0);
}

/// [`restore_app`] whose restore selects `theme_id` among `themes`, with the
/// restored themes' fingerprint on disk and in the stores read back.
fn theme_restore_app(
    ctx: &Context,
    theme_id: &str,
    themes: Value,
    boot_themes: Value,
) -> (AppModel, Arc<MockBackend>) {
    let theme_id = theme_id.to_owned();
    let (mut app, mock) = restore_app_with(ctx, move |commands| {
        let restored = &mut commands["load_client_storage_snapshot"][0]["result"];
        restored["stores"]["client.settings"]["themeId"] = json!(theme_id);
        restored["manifest"]["stores"]["app.themes"] = json!("restored-themes");
        commands["get_storage_manifest"][0]["result"] = restored["manifest"].clone();
        commands["list_custom_themes"] = json!([{ "args": {}, "result": themes }]);
        commands["get_boot_payload"][0]["result"]["customThemes"] = boot_themes;
    });
    focus(&mut app, ctx, true);
    (app, mock)
}

/// Loses and regains focus until `loaded` holds.
fn refocus_until(app: &mut AppModel, ctx: &Context, loaded: impl Fn(&AppModel) -> bool) {
    focus(app, ctx, false);
    focus(app, ctx, true);
    pump::until(app, ctx, |app, ctx| focus(app, ctx, true), loaded);
}

#[test]
fn a_restore_whose_theme_base_cannot_be_read_reads_the_themes_again_on_the_next_focus() {
    // Regression (RESTORED_BASE_UNLOCK): `child` read without its locked
    // custom base; the restore took the restored themes' fingerprint as
    // seen, and no focus ever read `base`.
    let ctx = Context::default();
    let base = json!({"schemaVersion": 1, "id": "base", "name": "Base", "colorScheme": "dark", "tokens": {"bgCard": "#112233"}});
    let child = json!({"schemaVersion": 1, "id": "child", "name": "Child", "colorScheme": "dark", "extends": "base", "tokens": {}});
    let (mut app, mock) = theme_restore_app(&ctx, "child", json!([base, child]), json!([]));
    assert!(mock.lock_theme("base", true));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    let b = app.booted().unwrap();
    assert_eq!(b.stores.settings().theme_id, "child");
    assert!(b.registry.document("child").is_some());
    assert!(b.registry.document("base").is_none());
    let unbased = b.theme.vars.bg_card;
    let reads = mock.seen("list_custom_themes");

    assert!(mock.lock_theme("base", false));
    refocus_until(&mut app, &ctx, |a| {
        a.booted().unwrap().registry.document("base").is_some()
    });
    let b = app.booted().unwrap();
    assert!(mock.seen("list_custom_themes") > reads);
    assert_eq!(b.stores.settings().theme_id, "child");
    assert_ne!(
        b.theme.vars.bg_card, unbased,
        "child still drawn without its base"
    );
    assert_eq!(mock.seen(SAVE), 0);
}

#[test]
fn a_restore_whose_themes_folder_cannot_be_listed_reads_it_again_on_the_next_focus() {
    // The selected theme stays loaded from before, so a missing theme
    // never asked for the read: the restored version of it never loaded.
    let ctx = Context::default();
    let old = json!({"schemaVersion": 1, "id": "mine", "name": "Mine", "colorScheme": "dark", "tokens": {"bgCard": "#112233"}});
    let restored = json!({"schemaVersion": 1, "id": "mine", "name": "Mine", "colorScheme": "dark", "tokens": {"bgCard": "#445566"}});
    let (mut app, mock) = theme_restore_app(&ctx, "mine", json!([restored]), json!([old]));
    assert!(mock.fail("list_custom_themes", Some("themes folder unreadable")));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    // The version loaded before the restore draws.
    let before = app.booted().unwrap().theme.vars.bg_card;
    assert_eq!(before, crate::theme::Rgba::hex(0x112233, 1.0));

    assert!(mock.fail("list_custom_themes", None));
    refocus_until(&mut app, &ctx, |a| {
        a.booted().unwrap().theme.vars.bg_card != before
    });
    assert_eq!(app.booted().unwrap().stores.settings().theme_id, "mine");
    assert_eq!(mock.seen(SAVE), 0);
}

#[test]
fn a_restore_whose_platforms_cannot_be_reloaded_asks_for_a_restart() {
    // Regression: the restored settings were adopted against the old
    // platform list, which dropped the restored platform they enable and
    // made it no longer the default; the next settings save wrote that.
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    let before = app.booted().unwrap().stores.settings().clone();
    assert!(mock.fail("reload_user_platforms", Some("descriptors folder locked")));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| {
        a.backup.restart_needed() || !a.backup.holds_writes()
    });
    for _ in 0..10 {
        frame(&mut app, &ctx);
    }
    let settings = app.booted().unwrap().stores.settings();
    assert!(
        app.backup.restart_needed(),
        "adopted: enabled {:?}, default {:?}",
        settings.enabled_platforms,
        settings.default_platform_id
    );
    assert_eq!(settings.enabled_platforms, before.enabled_platforms);
    assert_eq!(settings.default_platform_id, before.default_platform_id);
    assert!(app.backup.holds_writes());
    app.flush_app_state().unwrap();
    assert_eq!(
        mock.seen(SAVE),
        0,
        "settings written over the restored ones"
    );
}

/// A frame with the window focused or not.
fn focus(app: &mut AppModel, ctx: &Context, focused: bool) {
    let mut input = egui::RawInput {
        focused,
        ..Default::default()
    };
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .focused = Some(focused);
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    output.textures_delta.clear();
}

/// How a restore that already wrote the files ends without a usable answer.
#[derive(Clone, Copy)]
enum LostRestore {
    /// The job died once the files were written.
    Crashed,
    /// The restore failed and could not put every file back.
    LeftRestored,
}

/// The error of a restore that could not put every file back, with the
/// copy of the files from before it.
const LEFT_RESTORED: &str = "disk full; these files already replaced could not be put back and may hold the restored setup: C:/config/user/client.settings.json. A copy of the files from before the restore is in C:/config/user/backups/pre-restore-1";

/// [`restore_app`] whose restore ends as `how`, with a note typed while it
/// runs.
fn lost_restore(ctx: &Context, how: LostRestore) -> (AppModel, Arc<MockBackend>) {
    let (mut app, mock) = restore_app(ctx);
    frame(&mut app, ctx);
    match how {
        LostRestore::Crashed => assert!(mock.crash("load_client_storage_snapshot", true)),
        LostRestore::LeftRestored => assert!(mock.leave_restored(Some(LEFT_RESTORED))),
    }
    assert!(mock.hold("backup_restore"));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, ctx, |_| mock.waiting("backup_restore") == 1);
    set_note(&mut app, "typed over the restored files");
    mock.release("backup_restore");
    (app, mock)
}

/// A restore lost as `how`, seen in session: the boundary stays up.
fn keeps_the_boundary(how: LostRestore, name: &str) {
    let ctx = Context::default();
    let (mut app, mock) = lost_restore(&ctx, how);
    pump_until(&mut app, &ctx, |a| {
        a.backup.restart_needed() || !a.backup.holds_writes()
    });
    assert!(app.backup.restart_needed(), "{name}: the boundary lifted");
    assert!(app.backup.holds_writes());
    let _ = app.flush_app_state();
    assert_eq!(mock.seen(SAVE), 0, "{name}: the old stores were written");
}

/// A restore lost as `how`, waited for by the close: nothing is written.
fn writes_nothing_on_close(how: LostRestore, name: &str) {
    let ctx = Context::default();
    let (mut app, mock) = lost_restore(&ctx, how);
    let _ = app.flush_app_state();
    assert_eq!(mock.seen(SAVE), 0, "{name}: the old stores were written");
    assert!(app.backup.restart_needed(), "{name}: the boundary lifted");
}

#[test]
fn a_restore_job_lost_after_its_writes_keeps_the_boundary() {
    // Regression (CRASHED): a restore job that died after replacing the
    // files lifted the boundary, and the old stores in memory were written
    // over the restored files.
    keeps_the_boundary(LostRestore::Crashed, "CRASHED");
}

#[test]
fn a_restore_job_lost_after_its_writes_writes_nothing_on_close() {
    // Regression (CRASHED_ON_CLOSE): the close took such a job for a
    // restore that wrote nothing and flushed the old stores.
    writes_nothing_on_close(LostRestore::Crashed, "CRASHED_ON_CLOSE");
}

#[test]
fn a_restore_that_left_restored_files_keeps_the_boundary() {
    // Regression (LEFT_RESTORED): a failed restore that could not put every
    // file back lifted the boundary as if nothing were written.
    keeps_the_boundary(LostRestore::LeftRestored, "LEFT_RESTORED");
}

#[test]
fn a_restore_that_left_restored_files_writes_nothing_on_close() {
    // Regression (LEFT_ON_CLOSE): the same failure waited for by the close.
    writes_nothing_on_close(LostRestore::LeftRestored, "LEFT_ON_CLOSE");
}

#[test]
fn a_restore_that_put_every_file_back_lets_the_app_go_on() {
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    assert!(mock.fail(
        "backup_restore",
        Some("disk full; the files already replaced were put back")
    ));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.restart_needed());
    assert!(app.backup.card.notice.contains("disk full"));
    set_note(&mut app, "typed after");
    let _ = app.flush_app_state();
    assert_eq!(note(&app), "typed after");
    assert!(
        mock.seen(SAVE) >= 1,
        "the edit after a clean failure was lost"
    );
}

/// The account reads the mock answered so far, whichever platform asked.
fn account_reads(mock: &MockBackend) -> usize {
    mock.seen("platform_get_accounts") + mock.seen("platform_get_startup_snapshot")
}

/// Frames until no account load is in flight.
fn settle_accounts(app: &mut AppModel, ctx: &Context) {
    pump_until(app, ctx, |a| {
        !a.booted().unwrap().accounts.loading.loads_in_flight()
    });
}

#[test]
fn a_session_mark_written_during_a_restore_reloads_the_accounts() {
    // Regression (MARK_UNSEEN): the CLI wrote a session mark to the local
    // config while the restore ran. The stores read back took the config's
    // new fingerprint as seen but left it out of the changes, so the
    // accounts never read the mark, nor did the next focus.
    let ctx = Context::default();
    let (mut app, mock) = restore_app_with(&ctx, |commands| {
        // The Riot tab, before and after: no tab change and no store of its
        // own reloads its accounts.
        for settings in [
            "/get_boot_payload/0/result/storageSnapshot/stores/client.settings",
            "/load_client_storage_snapshot/0/result/stores/client.settings",
        ] {
            let settings = commands.pointer_mut(settings).unwrap();
            settings["enabledPlatforms"] = json!(["steam", "riot"]);
            settings["defaultPlatformId"] = json!("riot");
        }
        commands["load_client_storage_snapshot"][0]["result"]["manifest"]["stores"]
            ["app.config.local"] = json!("marked-during-the-restore");
    });
    frame(&mut app, &ctx);
    settle_accounts(&mut app, &ctx);
    let tab = app.booted().unwrap().nav.active_tab.clone();
    assert_eq!(tab, "riot");
    let reads = account_reads(&mock);
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.restart_needed());
    assert_eq!(app.booted().unwrap().nav.active_tab, tab);
    for _ in 0..10 {
        frame(&mut app, &ctx);
    }
    settle_accounts(&mut app, &ctx);
    assert!(
        account_reads(&mock) > reads,
        "MARK_UNSEEN: the accounts were not read again after the mark"
    );
}

#[test]
fn a_restored_descriptor_of_the_active_tab_reloads_its_accounts() {
    // Regression (SOURCE_KEPT): the restored descriptor had the active
    // tab's id and another account source; the service behind the cards
    // changed, their list stayed the old source's.
    let ctx = Context::default();
    let descriptor: Value = serde_json::from_str(DESCRIPTOR).unwrap();
    let mut other = descriptor.clone();
    for os in ["windows", "linux", "macos"] {
        other["os"][os]["state"]["files"][0]["live"] = json!("${LIVE}/other-session.json");
    }
    let (mut app, mock) = restore_app_with(&ctx, move |commands| {
        let boot = &mut commands["get_boot_payload"][0]["result"];
        boot["userPlatforms"]["loaded"] = json!([descriptor]);
        let settings = &mut boot["storageSnapshot"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["steam", "egui-fixture"]);
        settings["defaultPlatformId"] = json!("egui-fixture");
        commands["reload_user_platforms"][0]["result"]["loaded"] = json!([other]);
    });
    assert_eq!(app.booted().unwrap().nav.active_tab, "egui-fixture");
    frame(&mut app, &ctx);
    settle_accounts(&mut app, &ctx);
    let reads = account_reads(&mock);
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.restart_needed());
    assert_eq!(app.booted().unwrap().nav.active_tab, "egui-fixture");
    for _ in 0..10 {
        frame(&mut app, &ctx);
    }
    settle_accounts(&mut app, &ctx);
    assert!(
        account_reads(&mock) > reads,
        "SOURCE_KEPT: the cards kept the accounts of the old source"
    );
}

#[test]
fn a_descriptor_locked_at_boot_keeps_the_platform_the_settings_enable() {
    // Regression (BOOT_LOCKED): on the restart a restore asked for, a
    // descriptor still locked left its platform out of the boot; the
    // settings in memory dropped it from the enabled ones and the default,
    // and the next settings save wrote that over the restored file.
    // Regression (PANEL_SAVE): the settings panel's save of an unrelated
    // change (a zoom with the panel open, a toggle) normalized its draft
    // against the loaded platforms only and wrote Steam as the default.
    let ctx = Context::default();
    let (mut app, _mock) = restore_app_with(&ctx, |commands| {
        let boot = &mut commands["get_boot_payload"][0]["result"];
        boot["userPlatforms"]["incomplete"] = json!(true);
        let settings = &mut boot["storageSnapshot"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["steam", "egui-fixture"]);
        settings["defaultPlatformId"] = json!("egui-fixture");
    });
    frame(&mut app, &ctx);
    let kept = |app: &AppModel| {
        let s = app.booted().unwrap().stores.settings();
        (s.enabled_platforms.clone(), s.default_platform_id.clone())
    };
    let locked = (
        ["steam", "egui-fixture"].map(String::from).to_vec(),
        "egui-fixture".to_string(),
    );
    let b = app.booted().unwrap();
    assert!(b.platforms.iter().all(|p| p.id != "egui-fixture"));
    assert_eq!(
        kept(&app),
        locked,
        "BOOT_LOCKED: the settings in memory lost the locked platform"
    );
    // A settings save outside the panel keeps them.
    let mut next = app.booted().unwrap().stores.settings().clone();
    next.language = Locale::Fr;
    app.booted.as_mut().unwrap().stores.save_settings(&next);
    assert_eq!(kept(&app), locked);

    // The panel's save of a zoom and of a toggle, then its close, too.
    app.toggle_settings_panel();
    pump_until(&mut app, &ctx, |a| {
        a.settings_panel.as_ref().is_some_and(|p| p.hydrated)
    });
    let zoom = app.booted().unwrap().stores.settings().ui_scale_percent;
    app.zoom_key(&ctx, super::super::ui_scale::ZoomKey::In, Instant::now());
    pump_until(&mut app, &ctx, |a| {
        a.booted().unwrap().stores.settings().ui_scale_percent != zoom
    });
    app.apply_settings(
        &ctx,
        SettingsAction::Toggle(super::super::settings_panel::SettingFlag::ShowUsernames),
    );
    frame(&mut app, &ctx);
    app.toggle_settings_panel();
    assert!(app.settings_panel.is_none());
    let s = app.booted().unwrap().stores.settings();
    assert_ne!(s.ui_scale_percent, zoom, "the zoom was not saved");
    assert_eq!(
        kept(&app),
        locked,
        "PANEL_SAVE: the panel's save dropped the locked platform"
    );
    let stored = app
        .booted()
        .unwrap()
        .stores
        .storage()
        .get(accshift_app::storage::CLIENT_STORE_SETTINGS)
        .cloned()
        .unwrap();
    assert_eq!(stored["defaultPlatformId"], json!("egui-fixture"));
    assert_eq!(stored["enabledPlatforms"], json!(["steam", "egui-fixture"]));
}

#[test]
fn the_restart_modal_tells_a_lost_restore_and_a_partial_rollback_apart() {
    // Regression (RESTART_TEXT): after a lost restore job or a rollback
    // that left restored files, the modal said "Backup restored" and hid
    // the rollback's error and where the copy of the old files is.
    for (how, name) in [
        (LostRestore::Crashed, "LOST"),
        (LostRestore::LeftRestored, "PARTIAL"),
    ] {
        let ctx = Context::default();
        let (mut app, _mock) = lost_restore(&ctx, how);
        pump_until(&mut app, &ctx, |a| a.backup.restart_needed());
        let locale = app.booted().unwrap().locale;
        let restored = translate(locale, "settings.backupRestartNeeded");
        let modal = super::super::shell::restart_modal(&app, locale).unwrap();
        assert_ne!(
            modal.message, restored,
            "RESTART_TEXT {name}: the modal says the backup was restored"
        );
        assert_ne!(
            app.backup.card.notice, restored,
            "RESTART_TEXT {name}: the card says the backup was restored"
        );
        if let LostRestore::LeftRestored = how {
            assert!(
                modal.message.contains("disk full")
                    && modal
                        .message
                        .contains("C:/config/user/backups/pre-restore-1"),
                "RESTART_TEXT {name}: the rollback error is hidden: {}",
                modal.message
            );
        }
    }
    // A restore that went through but could not be read back says so.
    let ctx = Context::default();
    let (app, _mock) = restored_but_unread(&ctx);
    let locale = app.booted().unwrap().locale;
    let modal = super::super::shell::restart_modal(&app, locale).unwrap();
    assert_eq!(
        modal.message,
        translate(locale, "settings.backupRestartNeeded")
    );
}

/// [`restore_app`] with the settings panel open and hydrated, and the
/// restore started and held at the disk.
fn restore_held_with_panel(ctx: &Context) -> (AppModel, Arc<MockBackend>) {
    let (mut app, mock) = restore_app(ctx);
    app.toggle_settings_panel();
    pump_until(&mut app, ctx, |a| {
        a.settings_panel.as_ref().is_some_and(|p| p.hydrated)
    });
    assert!(mock.hold("backup_restore"));
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, ctx, |_| mock.waiting("backup_restore") == 1);
    (app, mock)
}

#[test]
fn a_panel_edit_made_during_the_restore_never_lands_over_it() {
    // Regression (PANEL_DRAFT): the panel kept a language picked while the
    // restore ran, merged it over the restored one and saved it.
    let ctx = Context::default();
    let (mut app, mock) = restore_held_with_panel(&ctx);
    app.settings_panel.as_mut().unwrap().draft.language = Locale::Es;
    frame(&mut app, &ctx);
    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.card.error, "{}", app.backup.card.notice);
    // Until the panel has no save left to run: one still queued runs at
    // its deadline, which the app asks a frame for.
    pump_until(&mut app, &ctx, |a| a.settings_panel_deadline().is_none());
    for _ in 0..3 {
        frame(&mut app, &ctx);
    }
    let panel = app.settings_panel.as_ref().unwrap().draft.language;
    let stored = app.booted().unwrap().stores.settings().language;
    assert_eq!(
        (stored, panel),
        (Locale::Fr, Locale::Fr),
        "PANEL_DRAFT: the edit made during the restore replaced the restored language"
    );
}

#[test]
fn a_zoom_made_during_the_restore_is_not_saved_over_it() {
    // Regression (ZOOM_PENDING): the zoom's pending save outlived the
    // adoption and wrote its scale over the restored settings.
    let ctx = Context::default();
    let (mut app, mock) = restore_held_with_panel(&ctx);
    let restored = app.booted().unwrap().stores.settings().ui_scale_percent;
    // Its save is due long after the restore ends.
    let later = Instant::now() + std::time::Duration::from_secs(30);
    app.zoom_key(&ctx, super::super::ui_scale::ZoomKey::In, later);
    assert!(app.ui_scale_live.is_some());
    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.card.error, "{}", app.backup.card.notice);
    let due = app
        .ui_scale
        .take_due(later + std::time::Duration::from_secs(1));
    assert_eq!(
        (due, app.ui_scale_live),
        (None, None),
        "ZOOM_PENDING: the zoom made during the restore is still to be saved"
    );
    assert_eq!(
        app.booted().unwrap().stores.settings().ui_scale_percent,
        restored
    );
}

#[test]
fn the_last_admitted_write_wakes_a_draining_backup() {
    // Regression (DRAIN_WAKE): a descriptor write whose panel had closed
    // returned with no one polling it, so no frame ran and the backup
    // waited in its drain for the next input.
    let ctx = Context::default();
    let (mut app, mock) = restore_app(&ctx);
    frame(&mut app, &ctx);
    let write = app.backup.gate.admit(|_| ()).expect("the gate is open");
    app.start_backup_restore("C:/b.json".into(), String::new());
    for _ in 0..5 {
        frame(&mut app, &ctx);
    }
    assert!(app.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 0);
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    ctx.set_repaint_observer(move |_| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let backend = mock.clone();
    std::thread::spawn(move || write(&*backend)).join().unwrap();
    assert!(
        wakes.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "DRAIN_WAKE: the write returned without waking the UI"
    );
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert_eq!(mock.seen("backup_restore"), 1);
}

#[test]
fn a_restore_that_disables_the_active_tab_falls_back_to_an_enabled_one() {
    // The restored settings leave out the platform shown: the tab moves to
    // one they enable.
    let ctx = Context::default();
    let (mut app, mock) = restore_app_with(&ctx, |commands| {
        let settings =
            &mut commands["load_client_storage_snapshot"][0]["result"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["egui-fixture"]);
    });
    assert_eq!(app.booted().unwrap().nav.active_tab, "steam");
    frame(&mut app, &ctx);
    app.start_backup_restore("C:/b.json".into(), String::new());
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.restart_needed(), "{}", app.backup.card.notice);
    assert_eq!(mock.seen("backup_restore"), 1);
    assert_eq!(app.booted().unwrap().nav.active_tab, "egui-fixture");
}

#[test]
fn a_pin_typed_during_the_restore_is_dropped_with_it() {
    // Regression (PIN_DRAINED): a code typed while the restore ran stayed
    // in the field, marked seen, with no save left to commit it, and the
    // privacy toggle showed a PIN the stored settings did not have.
    let ctx = Context::default();
    let (mut app, mock) = restore_held_with_panel(&ctx);
    let panel = app.settings_panel.as_mut().unwrap();
    panel.pin_setup_pending = true;
    panel.pin_code_input = "1234".into();
    frame(&mut app, &ctx);
    mock.release("backup_restore");
    pump_until(&mut app, &ctx, |a| !a.backup.holds_writes());
    assert!(!app.backup.card.error, "{}", app.backup.card.notice);
    let panel = app.settings_panel.as_ref().unwrap();
    assert_eq!(
        (panel.pin_code_input.as_str(), panel.pin_setup_pending),
        ("", false),
        "PIN_DRAINED: the code typed during the restore is still pending"
    );
    assert!(!app.booted().unwrap().stores.settings().pin_enabled);
}

#[test]
fn a_reload_that_finds_the_locked_descriptor_gone_drops_its_platform() {
    // Regression (GHOST_TAB): the only platform the settings enabled was
    // locked at boot, then deleted; the panel's reload read a complete
    // folder without it, and the settings in memory kept naming it, so no
    // platform tab was offered until the next save.
    let ctx = Context::default();
    let (mut app, _mock) = restore_app_with(&ctx, |commands| {
        let boot = &mut commands["get_boot_payload"][0]["result"];
        boot["userPlatforms"]["incomplete"] = json!(true);
        let settings = &mut boot["storageSnapshot"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["egui-fixture"]);
        settings["defaultPlatformId"] = json!("egui-fixture");
        commands["reload_user_platforms"][0]["result"]["loaded"] = json!([]);
    });
    frame(&mut app, &ctx);
    let s = app.booted().unwrap().stores.settings();
    assert_eq!(s.enabled_platforms, ["egui-fixture"]);
    app.toggle_settings_panel();
    pump_until(&mut app, &ctx, |a| {
        a.settings_panel.as_ref().is_some_and(|p| p.hydrated)
    });
    app.apply_settings(&ctx, SettingsAction::ReloadDescriptors);
    pump_until(&mut app, &ctx, |a| {
        a.booted()
            .unwrap()
            .user_platforms
            .as_ref()
            .is_some_and(|r| !r.incomplete)
    });
    let b = app.booted().unwrap();
    let s = b.stores.settings();
    assert!(
        !s.enabled_platforms.iter().any(|p| p == "egui-fixture"),
        "GHOST_TAB: the settings still enable the deleted platform: {:?}",
        s.enabled_platforms
    );
    assert!(
        s.enabled_platforms.contains(&b.nav.active_tab)
            && b.platforms.iter().any(|p| p.id == b.nav.active_tab),
        "GHOST_TAB: the active tab {} is not an enabled platform",
        b.nav.active_tab
    );
}

#[test]
fn a_reload_that_finds_the_active_tab_descriptor_gone_loads_the_fallback_tab() {
    // Regression (TAB_LOADER): the active tab's descriptor was deleted
    // while another stayed locked; once the reload read every descriptor,
    // the fallback tab showed with the old tab's account loader.
    let ctx = Context::default();
    let descriptor: Value = serde_json::from_str(DESCRIPTOR).unwrap();
    let (mut app, _mock) = restore_app_with(&ctx, move |commands| {
        let boot = &mut commands["get_boot_payload"][0]["result"];
        boot["userPlatforms"]["loaded"] = json!([descriptor]);
        boot["userPlatforms"]["incomplete"] = json!(true);
        let settings = &mut boot["storageSnapshot"]["stores"]["client.settings"];
        settings["enabledPlatforms"] = json!(["steam", "egui-fixture"]);
        settings["defaultPlatformId"] = json!("egui-fixture");
        commands["reload_user_platforms"][0]["result"]["loaded"] = json!([]);
    });
    assert_eq!(app.booted().unwrap().nav.active_tab, "egui-fixture");
    frame(&mut app, &ctx);
    settle_accounts(&mut app, &ctx);
    app.toggle_settings_panel();
    pump_until(&mut app, &ctx, |a| {
        a.settings_panel.as_ref().is_some_and(|p| p.hydrated)
    });
    app.apply_settings(&ctx, SettingsAction::ReloadDescriptors);
    pump_until(&mut app, &ctx, |a| {
        a.booted()
            .unwrap()
            .user_platforms
            .as_ref()
            .is_some_and(|r| !r.incomplete)
    });
    let b = app.booted().unwrap();
    assert_eq!(b.nav.active_tab, "steam");
    assert_eq!(
        b.accounts.loading.loader.adapter_id(),
        Some("steam"),
        "TAB_LOADER: the fallback tab kept the old tab's loader"
    );
}
