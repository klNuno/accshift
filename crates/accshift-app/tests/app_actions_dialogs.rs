//! The app dialogs against the mock backend's demo scenario: every input
//! and confirm dialog kind is opened, edited, validated, confirmed and
//! cancelled, plus `app/useAppDialogs.test.ts` (confirm replacement) and the
//! context menus.

mod support;

use accshift_app::accounts::ToastKind;
use accshift_app::actions::dialogs::{
    bulk_edit_result_toast, finish_rename_account, folder_path_label, rename_account_blocking,
    AppDialogs, AppMenuCommand, AppMenuEntry, ConfirmConfig, ConfirmPurpose, ContextMenuTarget,
    DialogContext, InputPurpose, MenuInput,
};
use accshift_app::actions::{load_options, AppEffect};
use accshift_app::folders::{FolderInfo, ItemRef};
use accshift_app::menu::{MenuCommand, PlatformTask};
use accshift_app::mock::MockBackend;
use accshift_app::platforms::{
    PlatformAccount, PlatformAdapter, PlatformBulkEditResult, PlatformRegistry,
};
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext, MAX_NOTE_LENGTH};
use accshift_app::text::Text;
use accshift_app::BackendError;
use std::sync::Arc;

const SMURFS: &str = "demo-folder-smurfs";
const STEAM: &str = "steam";

struct Demo {
    backend: MockBackend,
    stores: ClientStores,
    dialogs: AppDialogs,
}

impl Demo {
    fn load() -> Self {
        let backend = support::mock("demo");
        let (storage, report) = storage::initialize(&backend, None, None, true);
        assert!(report.load_error.is_none(), "{:?}", report.load_error);
        Self {
            backend,
            stores: ClientStores::new(storage, StoreContext::shipped()),
            dialogs: AppDialogs::new(),
        }
    }

    fn ctx<'a>(
        &'a mut self,
        tab: &'a str,
        folder: Option<&'a str>,
    ) -> (&'a mut AppDialogs, DialogContext<'a>) {
        (
            &mut self.dialogs,
            DialogContext {
                stores: &mut self.stores,
                active_tab: tab,
                current_folder_id: folder,
            },
        )
    }

    fn submit(&mut self, tab: &str, folder: Option<&str>) -> Vec<AppEffect> {
        let (dialogs, mut ctx) = self.ctx(tab, folder);
        dialogs.submit_input(&mut ctx)
    }

    fn confirm(&mut self) -> Vec<AppEffect> {
        let (dialogs, mut ctx) = self.ctx(STEAM, None);
        dialogs.confirm_current(&mut ctx)
    }

    fn activate(&mut self, tab: &str, command: &AppMenuCommand) -> Vec<AppEffect> {
        let (dialogs, mut ctx) = self.ctx(tab, None);
        dialogs.activate(command, &mut ctx)
    }

    fn folder(&self, id: &str) -> FolderInfo {
        self.stores.folders().folder(id).expect(id).clone()
    }

    fn folder_names(&self, tab: &str) -> Vec<String> {
        self.stores
            .folders()
            .list_folders(tab)
            .into_iter()
            .map(|folder| folder.name.clone())
            .collect()
    }

    fn menu(&self, adapter: Option<&PlatformAdapter>, tab: &str) -> Vec<AppMenuEntry> {
        self.dialogs.context_menu_items(&MenuInput {
            stores: &self.stores,
            adapter,
            active_tab: tab,
            active_tab_usable: true,
            current_account_id: None,
        })
    }
}

fn find<'a>(entries: &'a [AppMenuEntry], id: &str) -> &'a AppMenuEntry {
    entries
        .iter()
        .find(|entry| entry.id() == Some(id))
        .unwrap_or_else(|| panic!("no entry {id} in {:?}", ids(entries)))
}

fn ids(entries: &[AppMenuEntry]) -> Vec<&str> {
    entries
        .iter()
        .map(|entry| entry.id().unwrap_or("|"))
        .collect()
}

fn gog_account() -> PlatformAccount {
    PlatformAccount {
        id: "g1".into(),
        display_name: "Gamer".into(),
        username: "g1".into(),
        last_login_at_sec: None,
    }
}

// Input dialog: new folder.

#[test]
fn new_folder_dialog_opens_validates_confirms_and_cancels() {
    let mut demo = Demo::load();
    demo.dialogs.open_background_context_menu(10.0, 20.0);
    let menu = demo.menu(None, STEAM);
    assert_eq!(ids(&menu), ["background.newFolder"]);
    let steam = PlatformAdapter::shipped(STEAM).unwrap();
    assert_eq!(
        ids(&demo.menu(Some(&steam), STEAM)),
        ["background.refresh", "background.newFolder"]
    );
    let command = find(&menu, "background.newFolder")
        .command()
        .unwrap()
        .clone();
    assert!(demo.activate(STEAM, &command).is_empty());
    assert!(demo.dialogs.context_menu().is_none());

    // Open: empty, not submittable.
    let dialog = demo.dialogs.input_dialog().unwrap();
    assert_eq!(dialog.title, Text::key("dialog.newFolderTitle"));
    assert_eq!(dialog.purpose, InputPurpose::NewFolder);
    assert!(!dialog.can_submit());

    // Validate: blank stays refused, the dialog stays up.
    demo.dialogs.input_dialog_mut().unwrap().set_value("   ");
    assert!(demo.submit(STEAM, None).is_empty());
    assert!(demo.dialogs.input_dialog().is_some());

    // Edit and confirm: trimmed, created in the current folder.
    demo.dialogs
        .input_dialog_mut()
        .unwrap()
        .set_value("  Mains  ");
    assert_eq!(
        demo.submit(STEAM, Some(SMURFS)),
        vec![AppEffect::RefreshCurrentItems]
    );
    assert!(demo.dialogs.input_dialog().is_none());
    let created = demo
        .stores
        .folders()
        .list_folders(STEAM)
        .into_iter()
        .find(|folder| folder.name == "Mains")
        .expect("created")
        .clone();
    assert_eq!(created.parent_id.as_deref(), Some(SMURFS));
    assert_eq!(
        folder_path_label(&demo.stores, &created.id),
        format!("{} / Mains", demo.folder(SMURFS).name)
    );

    // Cancel: nothing written.
    let before = demo.folder_names(STEAM);
    demo.dialogs.open_new_folder_dialog();
    demo.dialogs.input_dialog_mut().unwrap().set_value("Nope");
    demo.dialogs.close_input_dialog();
    assert!(demo.dialogs.input_dialog().is_none());
    assert_eq!(demo.folder_names(STEAM), before);
}

// Input dialog: rename folder.

#[test]
fn rename_folder_dialog_opens_with_the_name_and_renames_on_confirm() {
    let mut demo = Demo::load();
    let folder = demo.folder(SMURFS);
    demo.dialogs
        .open_folder_context_menu(5.0, 5.0, folder.clone());
    let menu = demo.menu(None, STEAM);
    assert_eq!(
        ids(&menu),
        ["folder.rename", "folder.color", "folder.delete"]
    );
    let rename = find(&menu, "folder.rename").command().unwrap().clone();
    demo.activate(STEAM, &rename);

    let dialog = demo.dialogs.input_dialog().unwrap();
    assert_eq!(dialog.value(), folder.name);
    assert_eq!(
        dialog.purpose,
        InputPurpose::RenameFolder {
            folder_id: SMURFS.into()
        }
    );
    demo.dialogs.input_dialog_mut().unwrap().set_value("");
    assert!(demo.submit(STEAM, None).is_empty());

    demo.dialogs.input_dialog_mut().unwrap().set_value(" Alts ");
    assert_eq!(
        demo.submit(STEAM, None),
        vec![AppEffect::RefreshCurrentItems]
    );
    assert_eq!(demo.folder(SMURFS).name, "Alts");

    // Cancel keeps the name.
    demo.dialogs.open_rename_folder_dialog(&demo.folder(SMURFS));
    demo.dialogs.input_dialog_mut().unwrap().set_value("Other");
    demo.dialogs.close_input_dialog();
    assert_eq!(demo.folder(SMURFS).name, "Alts");
}

// Input dialog: card note.

#[test]
fn card_note_dialog_counts_clamps_sets_and_clears() {
    let mut demo = Demo::load();
    let command = AppMenuCommand::Platform(MenuCommand::EditNote {
        account_id: "a1".into(),
        initial_note: String::new(),
    });
    demo.activate(STEAM, &command);
    let dialog = demo.dialogs.input_dialog().unwrap();
    assert!(dialog.allow_empty);
    assert_eq!(dialog.maxlength, Some(MAX_NOTE_LENGTH));
    assert!(dialog.can_submit());
    assert_eq!(dialog.char_count(), None);

    // Typing past the limit is refused; the counter shows near it.
    let long = "x".repeat(MAX_NOTE_LENGTH + 20);
    demo.dialogs.input_dialog_mut().unwrap().set_value(long);
    let dialog = demo.dialogs.input_dialog().unwrap();
    assert_eq!(dialog.value().len(), MAX_NOTE_LENGTH);
    assert_eq!(
        dialog.char_count(),
        Some((MAX_NOTE_LENGTH, MAX_NOTE_LENGTH))
    );

    demo.dialogs
        .input_dialog_mut()
        .unwrap()
        .set_value("main account");
    assert_eq!(demo.submit(STEAM, None), vec![AppEffect::CardNotesChanged]);
    assert_eq!(demo.stores.account_card_note("a1"), "main account");

    // A blank note clears it.
    demo.activate(
        STEAM,
        &AppMenuCommand::Platform(MenuCommand::EditNote {
            account_id: "a1".into(),
            initial_note: "main account".into(),
        }),
    );
    assert_eq!(demo.dialogs.input_dialog().unwrap().value(), "main account");
    demo.dialogs.input_dialog_mut().unwrap().set_value("  ");
    assert_eq!(demo.submit(STEAM, None), vec![AppEffect::CardNotesChanged]);
    assert_eq!(demo.stores.account_card_note("a1"), "");

    // Cancel writes nothing.
    demo.activate(STEAM, &command);
    demo.dialogs.input_dialog_mut().unwrap().set_value("draft");
    demo.dialogs.close_input_dialog();
    assert_eq!(demo.stores.account_card_note("a1"), "");
}

// Input dialog: a platform menu's rename, run on the mock.

#[test]
fn platform_input_dialog_runs_the_adapter_task_on_confirm() {
    let mut demo = Demo::load();
    let gog = PlatformAdapter::shipped("gog").unwrap();
    demo.dialogs
        .open_account_context_menu(1.0, 1.0, gog_account());
    let menu = demo.menu(Some(&gog), "gog");
    let rename = find(&menu, "platform.rename.g1").command().unwrap().clone();
    demo.activate("gog", &rename);
    let dialog = demo.dialogs.input_dialog().unwrap();
    assert!(matches!(dialog.purpose, InputPurpose::PlatformInput(_)));
    assert_eq!(dialog.value(), "Gamer");

    demo.dialogs
        .input_dialog_mut()
        .unwrap()
        .set_value("Renamed");
    let effects = demo.submit("gog", None);
    let task = PlatformTask::SetAccountLabel {
        platform_id: "gog".into(),
        account_id: "g1".into(),
        label: "Renamed".into(),
    };
    assert_eq!(effects, vec![AppEffect::RunPlatformTask(task.clone())]);
    let settings = demo.stores.settings().clone();
    let outcome = gog.run_task(&task, &demo.backend, &settings);
    assert!(!outcome.effects.is_empty());

    // Cancel runs nothing.
    demo.activate("gog", &rename);
    demo.dialogs.close_input_dialog();
    assert!(demo.dialogs.input_dialog().is_none());
}

// Input dialog: F2 rename and the rename after an add.

#[test]
fn rename_account_dialogs_label_through_the_adapter() {
    let mut demo = Demo::load();
    let gog = Arc::new(PlatformAdapter::shipped("gog").unwrap());
    let steam = PlatformAdapter::shipped(STEAM).unwrap();
    // Steam cannot label accounts: no dialog.
    demo.dialogs
        .open_rename_account_dialog(Some(&steam), &gog_account());
    assert!(demo.dialogs.input_dialog().is_none());

    demo.dialogs
        .open_rename_account_dialog(Some(&gog), &gog_account());
    assert!(demo.dialogs.input_dialog().unwrap().can_submit());
    demo.dialogs.input_dialog_mut().unwrap().set_value("Main");
    let effects = demo.submit("gog", None);
    assert_eq!(
        effects,
        vec![AppEffect::RenameAccount {
            platform_id: "gog".into(),
            account_id: "g1".into(),
            label: "Main".into()
        }]
    );
    let result = rename_account_blocking(&demo.backend, &gog, "g1", "Main");
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        finish_rename_account(result),
        vec![AppEffect::LoadAccounts(load_options(
            true, false, false, false
        ))]
    );
    let failed = finish_rename_account(Err(BackendError::new("nope")));
    let [AppEffect::Toast(toast)] = failed.as_slice() else {
        panic!("{failed:?}");
    };
    assert_eq!(
        toast.text,
        Text::with("toast.renameAccountFailed", [("error", "nope")])
    );
    assert_eq!(toast.kind, ToastKind::Error);

    // After an add: prefilled with the detected name, blank keeps it.
    let registry = PlatformRegistry::shipped();
    demo.dialogs
        .prompt_rename_new_account(&registry, STEAM, "s1", Some("x"));
    assert!(demo.dialogs.input_dialog().is_none());
    demo.dialogs
        .prompt_rename_new_account(&registry, "gog", "g2", Some("  Detected  "));
    assert_eq!(demo.dialogs.input_dialog().unwrap().value(), "Detected");
    demo.dialogs.input_dialog_mut().unwrap().set_value("");
    assert!(demo.submit("gog", None).is_empty());
    assert!(demo.dialogs.input_dialog().is_none());

    demo.dialogs
        .prompt_rename_new_account(&registry, "gog", "g2", None);
    demo.dialogs.close_input_dialog();
    assert!(demo.dialogs.input_dialog().is_none());
}

// Confirm dialog: delete folder.

#[test]
fn delete_folder_confirm_deletes_on_confirm_and_keeps_on_cancel() {
    let mut demo = Demo::load();
    let folder = demo.folder(SMURFS);
    demo.activate(STEAM, &AppMenuCommand::DeleteFolder(folder.clone()));
    let dialog = demo.dialogs.confirm_dialog().unwrap();
    assert_eq!(
        dialog.title,
        Text::with("dialog.deleteFolderTitle", [("name", folder.name.as_str())])
    );
    assert_eq!(
        dialog.confirm_label(),
        Text::key("context.menu.deleteFolder")
    );

    // Cancel: the folder stays.
    assert!(demo.dialogs.close_confirm_dialog().is_empty());
    assert!(demo.stores.folders().folder(SMURFS).is_some());

    demo.activate(STEAM, &AppMenuCommand::DeleteFolder(folder));
    assert_eq!(demo.confirm(), vec![AppEffect::RefreshCurrentItems]);
    assert!(demo.stores.folders().folder(SMURFS).is_none());
    assert!(demo.dialogs.confirm_dialog().is_none());
}

// Confirm dialog: a platform menu's confirm.

#[test]
fn platform_confirm_runs_its_task_on_confirm_only() {
    let mut demo = Demo::load();
    let gog = PlatformAdapter::shipped("gog").unwrap();
    demo.dialogs
        .open_account_context_menu(1.0, 1.0, gog_account());
    let menu = demo.menu(Some(&gog), "gog");
    let forget = find(&menu, "gog.forget.g1").command().unwrap().clone();
    demo.activate("gog", &forget);
    let dialog = demo.dialogs.confirm_dialog().unwrap().clone();
    let ConfirmPurpose::PlatformTask(task) = dialog.purpose else {
        panic!("{:?}", dialog.purpose);
    };
    assert!(demo.dialogs.close_confirm_dialog().is_empty());

    demo.activate("gog", &forget);
    assert_eq!(demo.confirm(), vec![AppEffect::RunPlatformTask(task)]);
}

// requestConfirm (useAppDialogs.test.ts).

#[test]
fn request_confirm_settles_false_when_another_confirm_replaces_it() {
    let mut demo = Demo::load();
    let mut effects = Vec::new();
    let ticket = demo.dialogs.request_confirm(
        ConfirmConfig {
            title: Text::raw("switch"),
            message: Text::raw("sure?"),
            confirm_label: None,
            confirm_color: None,
        },
        &mut effects,
    );
    assert!(effects.is_empty());
    assert_eq!(
        demo.dialogs.confirm_dialog().unwrap().confirm_label(),
        Text::key("common.confirm")
    );
    let replaced = demo.activate(STEAM, &AppMenuCommand::DeleteFolder(demo.folder(SMURFS)));
    assert_eq!(
        replaced,
        vec![AppEffect::ConfirmSettled {
            ticket,
            allowed: false
        }]
    );
}

#[test]
fn confirming_the_replacing_dialog_runs_it_and_never_accepts_the_old_request() {
    let mut demo = Demo::load();
    let mut effects = Vec::new();
    demo.dialogs.request_confirm(
        ConfirmConfig {
            title: Text::raw("switch"),
            message: Text::raw("sure?"),
            confirm_label: None,
            confirm_color: None,
        },
        &mut effects,
    );
    let mut settled = demo.activate(STEAM, &AppMenuCommand::DeleteFolder(demo.folder(SMURFS)));
    settled.extend(demo.confirm());
    assert!(demo.stores.folders().folder(SMURFS).is_none());
    let answers: Vec<bool> = settled
        .iter()
        .filter_map(|effect| match effect {
            AppEffect::ConfirmSettled { allowed, .. } => Some(*allowed),
            _ => None,
        })
        .collect();
    assert_eq!(answers, vec![false]);
}

#[test]
fn request_confirm_resolves_true_on_its_own_dialog_and_false_on_cancel() {
    let mut demo = Demo::load();
    let config = ConfirmConfig {
        title: Text::raw("switch"),
        message: Text::raw("sure?"),
        confirm_label: Some(Text::key("personas.switchConfirmAction")),
        confirm_color: None,
    };
    let mut effects = Vec::new();
    let ticket = demo.dialogs.request_confirm(config.clone(), &mut effects);
    assert_eq!(demo.dialogs.pending_confirm(), Some(ticket));
    assert_eq!(
        demo.confirm(),
        vec![AppEffect::ConfirmSettled {
            ticket,
            allowed: true
        }]
    );
    let second = demo.dialogs.request_confirm(config, &mut effects);
    assert_ne!(second, ticket);
    assert_eq!(
        demo.dialogs.close_confirm_dialog(),
        vec![AppEffect::ConfirmSettled {
            ticket: second,
            allowed: false
        }]
    );
    assert_eq!(demo.dialogs.pending_confirm(), None);
}

// Context menus.

#[test]
fn context_menu_commands_write_the_stores() {
    let mut demo = Demo::load();
    let folder = demo.folder(SMURFS);
    demo.dialogs.open_folder_context_menu(0.0, 0.0, folder);
    let menu = demo.menu(None, STEAM);
    let AppMenuEntry::Swatches { swatches, .. } = find(&menu, "folder.color") else {
        panic!("swatches");
    };
    let swatch = swatches.last().unwrap().clone();
    assert_eq!(
        demo.activate(STEAM, &swatch.command),
        vec![AppEffect::CardColorsChanged]
    );
    assert_eq!(demo.stores.folder_card_color(SMURFS), swatch.color);

    let account = demo
        .stores
        .folders()
        .list_folders(STEAM)
        .iter()
        .find(|f| f.id == SMURFS)
        .map(|f| f.id.clone())
        .unwrap();
    let mv = AppMenuCommand::Platform(MenuCommand::MoveToFolder {
        account_id: "a-moved".into(),
        folder_id: Some(account.clone()),
    });
    assert_eq!(
        demo.activate(STEAM, &mv),
        vec![AppEffect::RefreshCurrentItems]
    );
    assert_eq!(
        demo.stores
            .folders()
            .find_item_folder_id(&ItemRef::account("a-moved"), STEAM),
        Some(SMURFS)
    );

    let color = AppMenuCommand::Platform(MenuCommand::SetCardColor {
        account_id: "a1".into(),
        color: swatch.color.clone(),
    });
    assert_eq!(
        demo.activate(STEAM, &color),
        vec![AppEffect::CardColorsChanged]
    );
    assert_eq!(demo.stores.account_card_color("a1"), swatch.color);

    let copy = AppMenuCommand::Platform(MenuCommand::Copy {
        value: "76561198000000001".into(),
        label: Text::key("platform.steamId"),
    });
    assert_eq!(
        demo.activate(STEAM, &copy),
        vec![AppEffect::CopyToClipboard {
            text: "76561198000000001".into(),
            label: Text::key("platform.steamId")
        }]
    );
    assert_eq!(
        demo.activate(STEAM, &AppMenuCommand::RefreshAccounts),
        vec![AppEffect::LoadAccounts(load_options(
            false, true, false, true
        ))]
    );

    demo.dialogs.open_custom_context_menu(
        0.0,
        0.0,
        vec![AppMenuEntry::item(
            "persona.edit",
            Text::key("personas.edit"),
            AppMenuCommand::Custom("persona.edit".into()),
        )],
    );
    assert!(matches!(
        demo.dialogs.context_menu().unwrap().target,
        ContextMenuTarget::Custom(_)
    ));
    let menu = demo.menu(None, STEAM);
    let custom = find(&menu, "persona.edit").command().unwrap().clone();
    assert_eq!(
        demo.activate(STEAM, &custom),
        vec![AppEffect::CustomMenu("persona.edit".into())]
    );
}

#[test]
fn bulk_edit_result_picks_success_partial_or_failure() {
    let ok = bulk_edit_result_toast(&PlatformBulkEditResult {
        succeeded: 3,
        failed: vec![],
    });
    assert_eq!(
        ok.text,
        Text::with("bulkEdit.toastSuccess", [("count", 3.0)])
    );
    let partial = bulk_edit_result_toast(&PlatformBulkEditResult {
        succeeded: 2,
        failed: vec!["x".into()],
    });
    assert_eq!(
        partial.text,
        Text::with(
            "bulkEdit.toastPartial",
            [("succeeded", 2.0), ("failed", 1.0)]
        )
    );
    let failed = bulk_edit_result_toast(&PlatformBulkEditResult {
        succeeded: 0,
        failed: vec!["x".into()],
    });
    assert_eq!(failed.kind, ToastKind::Error);
}
