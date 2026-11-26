use super::*;
use serde_json::json;

struct Ctx(PathBuf);

impl AppContext for Ctx {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("config"))
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("data"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("cache"))
    }
}

fn ctx(tag: &str) -> Ctx {
    let root = std::env::temp_dir().join(format!(
        "accshift-backup-test-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    Ctx(root)
}

const HASH: &str = "0123456789abcdef0123456789abcdef:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn seed(app: &Ctx) {
    let stores = [
        (
            STORE_SETTINGS,
            json!({"language": "fr", "themeId": "midnight", "pinEnabled": true, "pinHash": HASH, "startWithSystem": true, "cliEnabled": true}),
        ),
        (
            STORE_FOLDERS,
            json!({"version": 1, "folders": [{"id": "f1", "name": "Main", "parentId": null, "platform": "steam"}],
                   "itemOrder": {"root:steam": [{"type": "folder", "id": "f1"}], "f1": [{"type": "account", "id": "765"}]}}),
        ),
        (
            STORE_PERSONAS,
            json!([{"id": "p1", "name": "Evening", "color": "", "image": null, "assignments": []}]),
        ),
        (STORE_ACCOUNT_CARD_NOTES, json!({"765": "main account"})),
        (STORE_ACCOUNT_CARD_COLORS, json!({"765": "#ff0000"})),
        (STORE_FOLDER_CARD_COLORS, json!({"f1": "#00ff00"})),
        (STORE_VIEW_MODE, json!("list")),
    ];
    for (id, value) in stores {
        save_client_store(app, id, &value).unwrap();
    }
    let themes = themes_dir(app).unwrap();
    fs::create_dir_all(&themes).unwrap();
    fs::write(
        themes.join("midnight.json"),
        r#"{"id":"midnight","name":"Midnight"}"#,
    )
    .unwrap();
    let descriptors = crate::platforms::descriptor::user_dir(app).unwrap();
    fs::create_dir_all(&descriptors).unwrap();
    fs::write(
        descriptors.join("acme.json"),
        r#"{"id":"acme","schemaVersion":1}"#,
    )
    .unwrap();
    // What must never travel: a saved session and the portable config
    // with its analytics id and a launcher path.
    let snapshots = platform_snapshots_dir(app, "epic").unwrap();
    fs::create_dir_all(&snapshots).unwrap();
    fs::write(snapshots.join("acct.bin"), b"SESSION-SECRET").unwrap();
    let portable = portable_config_path(app).unwrap();
    fs::create_dir_all(portable.parent().unwrap()).unwrap();
    fs::write(
        &portable,
        r#"{"telemetry":{"installId":"INSTALL-ID-123"},"epic":{"pathOverride":"C:/Launchers/Epic"}}"#,
    )
    .unwrap();
}

fn store(app: &Ctx, id: &str) -> Option<Value> {
    read_json_if_exists(&client_store_path(app, id).unwrap()).unwrap()
}

/// Every file under `dir`, with its bytes, for "nothing changed" checks.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    out
}

#[test]
fn a_backup_restores_every_store_theme_and_descriptor_on_a_fresh_install() {
    let from = ctx("round-from");
    seed(&from);
    let file = from.0.join("accshift-backup.json");
    let exported = export_backup(&from, &file).unwrap();
    assert_eq!(exported.preview.folders, 1);
    assert_eq!(exported.preview.notes, 1);
    assert_eq!(exported.preview.colors, 2);
    assert_eq!(exported.preview.personas, 1);
    assert_eq!(exported.preview.themes, 1);
    assert_eq!(exported.preview.descriptors, 1);
    assert!(exported.preview.has_settings);

    let to = ctx("round-to");
    let report = restore_backup(&to, &file).unwrap();
    assert_eq!(report.preview, preview_backup(&file).unwrap());
    for id in BACKUP_STORES {
        if id == STORE_SETTINGS {
            continue;
        }
        assert_eq!(store(&to, id), store(&from, id), "{id}");
    }
    let settings = store(&to, STORE_SETTINGS).unwrap();
    assert_eq!(settings["language"], "fr");
    assert_eq!(settings["themeId"], "midnight");
    assert!(themes_dir(&to).unwrap().join("midnight.json").exists());
    assert!(crate::platforms::descriptor::user_dir(&to)
        .unwrap()
        .join("acme.json")
        .exists());
}

#[test]
fn the_file_never_holds_the_pin_sessions_analytics_id_or_launcher_paths() {
    let app = ctx("secrets");
    seed(&app);
    let file = app.0.join("out.json");
    export_backup(&app, &file).unwrap();
    let text = fs::read_to_string(&file).unwrap();
    for forbidden in [
        "pinHash",
        "pinEnabled",
        "startWithSystem",
        "cliEnabled",
        "0123456789abcdef",
        "SESSION-SECRET",
        "INSTALL-ID-123",
        "C:/Launchers/Epic",
    ] {
        assert!(!text.contains(forbidden), "{forbidden} leaked");
    }
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["kind"], BACKUP_KIND);
    assert_eq!(parsed["format"], BACKUP_FORMAT);
}

#[test]
fn a_restore_keeps_this_pcs_pin_and_startup_flag() {
    let from = ctx("pin-from");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();

    let to = ctx("pin-to");
    let other = "fedcba9876543210fedcba9876543210:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    save_client_store(
        &to,
        STORE_SETTINGS,
        &json!({"language": "en", "pinEnabled": true, "pinHash": other, "startWithSystem": false}),
    )
    .unwrap();
    restore_backup(&to, &file).unwrap();
    let settings = store(&to, STORE_SETTINGS).unwrap();
    assert_eq!(settings["language"], "fr");
    assert_eq!(settings["pinHash"], other);
    assert_eq!(settings["pinEnabled"], true);
    assert_eq!(settings["startWithSystem"], false);
}

#[test]
fn a_newer_format_is_refused_with_no_change_on_disk() {
    let app = ctx("newer");
    seed(&app);
    let file = app.0.join("newer.json");
    let mut backup = build_backup(&app, 1).unwrap();
    backup.format = BACKUP_FORMAT + 1;
    backup.stores.insert(STORE_VIEW_MODE.into(), json!("grid"));
    fs::write(&file, serde_json::to_vec(&backup).unwrap()).unwrap();
    let before = tree(&app.0.join("config"));

    let error = restore_backup(&app, &file).unwrap_err().message;
    assert!(error.contains("newer"), "{error}");
    assert!(preview_backup(&file).is_err());
    assert_eq!(tree(&app.0.join("config")), before);
}

#[test]
fn a_corrupt_or_foreign_file_leaves_the_disk_untouched() {
    let app = ctx("corrupt");
    seed(&app);
    let before = tree(&app.0.join("config"));
    let cases: [&[u8]; 4] = [
        b"{\"kind\":\"accshift-backup\",\"format\":1,\"stores\":",
        b"\xff\xfe not json",
        br#"{"kind":"something-else","format":1}"#,
        br#"{"kind":"accshift-backup","format":1,"createdAt":1,"appVersion":"2.0.0","themes":{"../evil":{}}}"#,
    ];
    for (n, body) in cases.iter().enumerate() {
        let file = app.0.join(format!("bad-{n}.json"));
        fs::write(&file, body).unwrap();
        assert!(restore_backup(&app, &file).is_err(), "case {n}");
        assert_eq!(tree(&app.0.join("config")), before, "case {n}");
    }
    assert!(restore_backup(&app, &app.0.join("missing.json")).is_err());
    assert_eq!(tree(&app.0.join("config")), before);
}

#[test]
fn a_restore_copies_the_current_files_to_a_pre_restore_folder_first() {
    let from = ctx("copy-from");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();

    let to = ctx("copy-to");
    save_client_store(&to, STORE_ACCOUNT_CARD_NOTES, &json!({"42": "old note"})).unwrap();
    let themes = themes_dir(&to).unwrap();
    fs::create_dir_all(&themes).unwrap();
    fs::write(
        themes.join("midnight.json"),
        r#"{"id":"midnight","name":"Old"}"#,
    )
    .unwrap();

    let report = restore_backup_at(&to, &file, 1_791_000_000).unwrap();
    let dir = PathBuf::from(&report.pre_restore_dir);
    assert!(
        dir.ends_with("pre-restore-20261003-040000"),
        "{}",
        dir.display()
    );
    assert!(dir.starts_with(app_config_root(&to).unwrap().join("user").join("backups")));
    let notes: Value = serde_json::from_slice(
        &fs::read(dir.join("user").join("account-card-notes.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(notes, json!({"42": "old note"}));
    let theme = fs::read_to_string(dir.join("themes").join("midnight.json")).unwrap();
    assert!(theme.contains("Old"));
    assert_eq!(
        store(&to, STORE_ACCOUNT_CARD_NOTES),
        Some(json!({"765": "main account"}))
    );
    // A second restore in the same second gets its own folder.
    let again = restore_backup_at(&to, &file, 1_791_000_000).unwrap();
    assert_ne!(again.pre_restore_dir, report.pre_restore_dir);
}

#[test]
fn folder_entries_and_notes_of_accounts_missing_here_are_kept() {
    let from = ctx("unknown-from");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();
    // The target PC has no account at all.
    let to = ctx("unknown-to");
    restore_backup(&to, &file).unwrap();
    let folders = store(&to, STORE_FOLDERS).unwrap();
    assert_eq!(folders["itemOrder"]["f1"][0]["id"], "765");
    assert_eq!(
        store(&to, STORE_ACCOUNT_CARD_NOTES).unwrap()["765"],
        "main account"
    );
    assert_eq!(
        store(&to, STORE_ACCOUNT_CARD_COLORS).unwrap()["765"],
        "#ff0000"
    );
}

/// A destination with a real PIN, its hash, and the CLI turned off.
fn locked_destination(tag: &str) -> (Ctx, String) {
    let to = ctx(tag);
    let hash = crate::pin::hash_pin_code("4321").unwrap();
    save_client_store(
        &to,
        STORE_SETTINGS,
        &json!({"language": "en", "pinEnabled": true, "pinHash": hash, "startWithSystem": true, "cliEnabled": false}),
    )
    .unwrap();
    (to, hash)
}

/// A backup file whose settings store is `settings`.
fn file_with_settings(dir: &Path, settings: Value) -> PathBuf {
    let file = dir.join(format!("settings-{}.json", settings.to_string().len()));
    let body = json!({
        "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
        "stores": {(STORE_SETTINGS): settings, (STORE_VIEW_MODE): "list"}
    });
    fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    file
}

#[test]
fn null_or_non_object_settings_are_refused_before_any_write_and_the_pin_stays() {
    let (to, hash) = locked_destination("bad-settings");
    let bak = backup_path(&client_store_path(&to, STORE_SETTINGS).unwrap());
    fs::write(&bak, br#"{"pinEnabled":true}"#).unwrap();
    let before = tree(&to.0.join("config"));
    for settings in [Value::Null, json!([]), json!("x"), json!(3)] {
        let file = file_with_settings(&to.0, settings.clone());
        assert!(preview_backup(&file).is_err(), "{settings}");
        let error = restore_backup(&to, &file).unwrap_err().message;
        assert!(error.contains("not an object"), "{settings}: {error}");
        assert_eq!(tree(&to.0.join("config")), before, "{settings}");
        assert_eq!(
            crate::pin::read_pin_lock(&to),
            crate::pin::PinLock::On(hash.clone())
        );
    }
}

#[test]
fn empty_settings_restore_the_defaults_and_keep_every_local_field() {
    let (to, hash) = locked_destination("empty-settings");
    let file = file_with_settings(&to.0, json!({}));
    restore_backup(&to, &file).unwrap();
    let settings = store(&to, STORE_SETTINGS).unwrap();
    assert_eq!(
        settings,
        json!({"pinEnabled": true, "pinHash": hash.clone(), "startWithSystem": true, "cliEnabled": false})
    );
    assert_eq!(
        crate::pin::read_pin_lock(&to),
        crate::pin::PinLock::On(hash)
    );
}

#[test]
fn a_restore_never_turns_on_the_cli_this_pc_turned_off() {
    for incoming in [
        json!({"language": "fr", "cliEnabled": true}),
        json!({"language": "fr"}),
    ] {
        let (to, _) = locked_destination("cli");
        let file = file_with_settings(&to.0, incoming.clone());
        restore_backup(&to, &file).unwrap();
        let settings = store(&to, STORE_SETTINGS).unwrap();
        assert_eq!(settings["language"], "fr");
        assert_eq!(settings["cliEnabled"], false, "{incoming}");
    }
    // A PC that never wrote the flag keeps its default (allowed), even
    // when the file says off: the flag is this PC's alone.
    let to = ctx("cli-unset");
    let file = file_with_settings(&to.0, json!({"cliEnabled": false, "language": "fr"}));
    restore_backup(&to, &file).unwrap();
    let settings = store(&to, STORE_SETTINGS).unwrap();
    assert!(settings.get("cliEnabled").is_none(), "{settings}");
}

#[test]
fn a_fresh_install_exports_settings_another_pc_can_restore() {
    let from = ctx("fresh-from");
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();
    assert!(preview_backup(&file).unwrap().has_settings);
    let (to, hash) = locked_destination("fresh-to");
    restore_backup(&to, &file).unwrap();
    assert_eq!(
        crate::pin::read_pin_lock(&to),
        crate::pin::PinLock::On(hash)
    );
}

#[test]
fn the_pre_restore_copy_holds_the_bak_recovery_files() {
    let from = ctx("bak-from");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();

    let to = ctx("bak-to");
    let notes = client_store_path(&to, STORE_ACCOUNT_CARD_NOTES).unwrap();
    let colors = client_store_path(&to, STORE_ACCOUNT_CARD_COLORS).unwrap();
    fs::create_dir_all(notes.parent().unwrap()).unwrap();
    // Notes: the primary is missing, the app reads the .bak.
    fs::write(backup_path(&notes), br#"{"42":"recovered note"}"#).unwrap();
    // Colors: the primary is corrupt, the app reads the .bak.
    fs::write(&colors, b"{\"42\":").unwrap();
    fs::write(backup_path(&colors), br##"{"42":"#123456"}"##).unwrap();
    assert_eq!(
        store(&to, STORE_ACCOUNT_CARD_NOTES),
        Some(json!({"42": "recovered note"}))
    );

    let report = restore_backup_at(&to, &file, 1_791_000_000).unwrap();
    let dir = PathBuf::from(&report.pre_restore_dir).join("user");
    assert_eq!(
        fs::read(dir.join("account-card-notes.bak")).unwrap(),
        br#"{"42":"recovered note"}"#
    );
    assert!(!dir.join("account-card-notes.json").exists());
    assert_eq!(
        fs::read(dir.join("account-card-colors.json")).unwrap(),
        b"{\"42\":"
    );
    assert_eq!(
        fs::read(dir.join("account-card-colors.bak")).unwrap(),
        br##"{"42":"#123456"}"##
    );
    assert_eq!(
        store(&to, STORE_ACCOUNT_CARD_NOTES),
        Some(json!({"765": "main account"}))
    );
}

/// The user platforms the real registry loads on this OS, as
/// `(id, name)` sorted by id. The registry is emptied again after.
fn loaded_platforms(app: &Ctx) -> Vec<(String, String)> {
    let _registry = crate::platforms::descriptor::test_support::registry_lock();
    let mut loaded: Vec<(String, String)> = crate::platforms::reload_user_platforms(app)
        .loaded
        .into_iter()
        .map(|d| (d.id, d.name))
        .collect();
    crate::platforms::reload_user_platforms(&ctx("empty-registry"));
    loaded.sort();
    loaded
}

/// The descriptor files the loader would read, by name.
fn descriptor_files(app: &Ctx) -> Vec<String> {
    let dir = crate::platforms::descriptor::user_dir(app).unwrap();
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".json"))
        .collect();
    names.sort();
    names
}

const EVERY_OS: [&str; 3] = ["windows", "linux", "macos"];

/// A fixture descriptor for `id` named `name`, with a profile for each
/// OS of `oses` only.
fn descriptor(app: &Ctx, id: &str, name: &str, oses: &[&str]) -> String {
    use crate::platforms::descriptor::test_support::fixture;
    let mut value: Value = serde_json::from_str(&fixture(id, &app.0)).unwrap();
    value["name"] = json!(name);
    let all = value["os"].as_object().unwrap().clone();
    value["os"] = Value::Object(
        all.into_iter()
            .filter(|(os, _)| oses.contains(&os.as_str()))
            .collect(),
    );
    value.to_string()
}

/// Exports `from`, restores the file on a fresh install and answers the
/// platforms both load; both hold the same descriptor files.
fn platforms_across_a_backup(
    tag: &str,
    files: &[(&str, &str, &str)],
) -> [Vec<(String, String)>; 2] {
    use crate::platforms::descriptor::test_support::drop_in;
    let from = ctx(&format!("{tag}-from"));
    for (file, id, name) in files {
        drop_in(&from, file, &descriptor(&from, id, name, &EVERY_OS));
    }
    let backup = from.0.join("backup.json");
    export_backup(&from, &backup).unwrap();
    let to = ctx(&format!("{tag}-to"));
    restore_backup(&to, &backup).unwrap();
    assert_eq!(descriptor_files(&to), descriptor_files(&from));
    [loaded_platforms(&from), loaded_platforms(&to)]
}

#[test]
fn a_descriptor_with_an_upper_case_extension_is_backed_up() {
    let [from, to] = platforms_across_a_backup("ext-case", &[("acme.JSON", "acme", "Acme")]);
    assert_eq!(from, [("acme".to_string(), "Acme".to_string())]);
    assert_eq!(to, from);
}

#[test]
fn a_descriptor_is_backed_up_whatever_its_file_name() {
    let [from, to] = platforms_across_a_backup(
        "stem",
        &[
            ("My Platform.json", "mine", "Mine"),
            // Two files declare one id: the loader keeps the first.
            ("a copy.json", "dup", "First"),
            ("b.json", "dup", "Second"),
        ],
    );
    assert_eq!(from.len(), 2, "{from:?}");
    assert_eq!(to, from);
}

#[test]
fn two_files_of_one_id_for_different_oses_load_the_same_after_a_restore() {
    use crate::platforms::descriptor::test_support::drop_in;
    let here = crate::platforms::descriptor::schema::Os::current()
        .unwrap()
        .as_str();
    let other = EVERY_OS.into_iter().find(|os| *os != here).unwrap();
    let from = ctx("os-collision-from");
    // The first file in path order is for another OS: the loader skips
    // it here and loads the second.
    drop_in(
        &from,
        "a.json",
        &descriptor(&from, "dup", "Elsewhere", &[other]),
    );
    drop_in(&from, "b.json", &descriptor(&from, "dup", "Here", &[here]));
    let file = from.0.join("backup.json");
    export_backup(&from, &file).unwrap();
    let to = ctx("os-collision-to");
    restore_backup(&to, &file).unwrap();

    let before = loaded_platforms(&from);
    let after = loaded_platforms(&to);
    assert_eq!(before, [("dup".to_string(), "Here".to_string())]);
    assert_eq!(
        after,
        before,
        "before={} after={}",
        before.len(),
        after.len()
    );
    assert_eq!(descriptor_files(&to), ["a.json", "b.json"]);
}

#[test]
fn a_restore_on_the_same_pc_overwrites_the_descriptor_file_it_came_from() {
    use crate::platforms::descriptor::test_support::drop_in;
    let app = ctx("alias");
    let original = descriptor(&app, "mine", "Original", &EVERY_OS);
    drop_in(&app, "My Platform.json", &original);
    let file = app.0.join("backup.json");
    export_backup(&app, &file).unwrap();
    drop_in(
        &app,
        "My Platform.json",
        &descriptor(&app, "mine", "Edited", &EVERY_OS),
    );

    let report = restore_backup(&app, &file).unwrap();
    let after = loaded_platforms(&app);
    let files = descriptor_files(&app);
    assert_eq!(
        after,
        [("mine".to_string(), "Original".to_string())],
        "descriptor_files={}",
        files.len()
    );
    assert_eq!(files, ["My Platform.json"]);
    // The edit it replaced is in the pre-restore copy.
    let copy = fs::read_to_string(
        PathBuf::from(&report.pre_restore_dir)
            .join("descriptors")
            .join("My Platform.json"),
    )
    .unwrap();
    assert!(copy.contains("Edited"), "{copy}");
}

/// A backup file whose only descriptor is under `name`.
fn file_with_descriptor(dir: &Path, n: usize, name: &str) -> PathBuf {
    let file = dir.join(format!("descriptor-{n}.json"));
    let body = json!({
        "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
        "descriptors": {(name): r#"{"id":"x"}"#}
    });
    fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    file
}

#[test]
fn a_descriptor_name_that_is_not_a_plain_json_file_refuses_the_backup() {
    let app = ctx("names");
    seed(&app);
    let files = ctx("names-files");
    let before = tree(&app.0);
    // Refused on every OS; the names only Windows refuses are in
    // a_name_this_os_cannot_write_refuses_the_whole_restore.
    let refused = [
        "../evil.json",
        "..\\evil.json",
        "sub/x.json",
        "sub\\x.json",
        "C:\\x.json",
        "/abs.json",
        "\\\\server\\share\\x.json",
        "x.txt",
        "x.json.txt",
        ".json",
        "..",
        ".",
        "a\u{0}.json",
        "",
        "acme",
    ];
    for (n, name) in refused.iter().enumerate() {
        let file = file_with_descriptor(&files.0, n, name);
        assert!(preview_backup(&file).is_err(), "{name:?}");
        assert!(restore_backup(&app, &file).is_err(), "{name:?}");
        assert_eq!(tree(&app.0), before, "{name:?}");
    }
    // Plain names in any case, and any dot inside one.
    for (n, name) in [
        "My Platform.json",
        "acme.JSON",
        "a-b_c.d.json",
        "a..json",
        "..a.json",
    ]
    .iter()
    .enumerate()
    {
        let file = file_with_descriptor(&files.0, 100 + n, name);
        assert!(preview_backup(&file).is_ok(), "{name:?}");
    }
}

#[test]
fn a_descriptor_whose_name_holds_two_dots_is_backed_up() {
    let [from, to] = platforms_across_a_backup("two-dots", &[("a..json", "dotted", "Dotted")]);
    assert_eq!(from, [("dotted".to_string(), "Dotted".to_string())]);
    assert_eq!(
        to,
        from,
        "SILENT_SKIP: before={} after={}",
        from.len(),
        to.len()
    );
}

/// A backup file holding `descriptors` as they are.
fn file_with_descriptors(dir: &Path, tag: &str, descriptors: Value) -> PathBuf {
    file_with_entries(dir, tag, "descriptors", descriptors)
}

/// A backup file holding `entries` under `field` (`descriptors` or
/// `themes`), each as its source text.
fn file_with_entries(dir: &Path, tag: &str, field: &str, entries: Value) -> PathBuf {
    let file = dir.join(format!("{field}-{tag}.json"));
    let entries: Map<String, Value> = entries
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| match value {
            Value::String(_) => (name.clone(), value.clone()),
            other => (name.clone(), Value::String(other.to_string())),
        })
        .collect();
    let body = json!({
        "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
        (field): entries
    });
    fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    file
}

/// The ids of the custom themes the real theme list loads.
fn theme_ids(app: &Ctx) -> Vec<String> {
    crate::themes::list_custom_themes(app)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect()
}

/// The theme files the theme list would read, by name.
fn theme_files(app: &Ctx) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(themes_dir(app).unwrap())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    names
}

const THEME: &str =
    r#"{"schemaVersion":1,"id":"mine","name":"Mine","colorScheme":"dark","tokens":{}}"#;

#[test]
fn two_names_of_one_file_refuse_the_whole_restore_on_every_os() {
    // Windows and macOS ignore case, and so can the filesystem a Linux
    // folder sits on.
    assert_eq!(destination_key("A.json"), destination_key("a.json"));
    let theme: Value = serde_json::from_str(THEME).unwrap();
    for field in ["descriptors", "themes"] {
        for (n, (one, two, one_file, two_file)) in [
            ("Acme.json", "acme.json", "Acme.json", "acme.json"),
            ("A.json", "a.json", "A.json", "a.json"),
        ]
        .into_iter()
        .enumerate()
        {
            let to = ctx(&format!("case-collision-{field}-{n}"));
            let value: Value = if field == "themes" {
                theme.clone()
            } else {
                serde_json::from_str(&descriptor(&to, "first", "First", &EVERY_OS)).unwrap()
            };
            let file = file_with_entries(
                &to.0,
                "pair",
                field,
                json!({(one): value.clone(), (two): value}),
            );
            let before = tree(&to.0);
            assert!(
                preview_backup(&file).is_err(),
                "CASE_COLLISION: {field} {one} and {two} pass the preview"
            );
            let error = restore_backup(&to, &file).unwrap_err().message;
            assert!(error.contains(one_file), "{error}");
            assert!(error.contains(two_file), "{error}");
            assert_eq!(tree(&to.0), before, "{field} {one}");
        }
    }
}

#[test]
fn a_theme_under_a_file_name_of_its_own_is_backed_up_and_restored_under_it() {
    let from = ctx("theme-name-from");
    let dir = themes_dir(&from).unwrap();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("My Theme.json"), THEME).unwrap();
    assert_eq!(theme_ids(&from), ["mine"]);
    let file = from.0.join("b.json");
    let exported = export_backup(&from, &file).unwrap();

    let to = ctx("theme-name-to");
    restore_backup(&to, &file).unwrap();
    assert_eq!(
        theme_ids(&to),
        ["mine"],
        "THEME_ALIAS_SKIP: exported={}",
        exported.preview.themes
    );
    assert_eq!(theme_files(&to), ["My Theme.json"]);
    // Restored on the PC it came from, it overwrites its own file.
    restore_backup(&from, &file).unwrap();
    assert_eq!(theme_files(&from), ["My Theme.json"]);
    assert_eq!(theme_ids(&from), ["mine"]);
}

#[test]
fn an_entry_whose_metadata_cannot_be_read_fails_the_export() {
    for which in ["descriptors", "themes"] {
        let app = ctx(&format!("unstatable-{which}"));
        seed(&app);
        let dir = if which == "themes" {
            themes_dir(&app).unwrap()
        } else {
            crate::platforms::descriptor::user_dir(&app).unwrap()
        };
        // Listed by its folder; its metadata fails with a loop error.
        if !crate::test_fs::link_to_itself(&dir.join("loop.json")) {
            return;
        }
        let target = app.0.join("out.json");
        let result = export_backup(&app, &target);
        let exists = target.exists();
        let _ = fs::remove_dir_all(&app.0);
        let error = result.expect_err(&format!("{which}: the export left loop.json out"));
        assert!(error.contains("loop.json"), "{which}: {error}");
        assert!(!exists, "{which}");
    }
}

#[test]
fn a_name_too_long_for_a_file_refuses_the_whole_restore() {
    let to = ctx("too-long");
    seed(&to);
    let files = ctx("too-long-files");
    let before = tree(&to.0);
    let long = format!("{}.json", "a".repeat(300));
    for field in ["descriptors", "themes"] {
        let file = file_with_entries(
            &files.0,
            "long",
            field,
            json!({"ok.json": {"id": "x"}, (long.as_str()): {"id": "x"}}),
        );
        assert!(
            preview_backup(&file).is_err(),
            "LONG_NAME: a {field} name of {} bytes passes the preview",
            long.len()
        );
        let error = restore_backup(&to, &file).unwrap_err().message;
        assert!(error.contains(&long), "{field}: {error}");
        assert_eq!(tree(&to.0), before, "{field}");
    }
    // The limit as each OS counts it: bytes, or UTF-16 units on Windows.
    let at_limit = format!("{}.json", "a".repeat(250));
    let over = format!("{}.json", "a".repeat(251));
    for windows in [false, true] {
        assert_eq!(unwritable_reason(&at_limit, windows), None);
        assert!(unwritable_reason(&over, windows).is_some());
    }
    // 600 bytes, 205 UTF-16 units.
    let wide = format!("{}.json", "\u{65e5}".repeat(200));
    assert_eq!(unwritable_reason(&wide, true), None);
    assert!(unwritable_reason(&wide, false).is_some());
}

#[test]
fn a_name_as_long_as_a_file_name_can_be_round_trips() {
    // Regression: the temp file of each write took the target's name
    // plus `.<pid>.tmp`, longer than a file name can be, and the restore
    // of a theme the loader read and the export carried failed.
    let from = ctx("long-valid-from");
    let theme_name = format!("{}.json", "x".repeat(250));
    let descriptor_name = format!("{}.JSON", "y".repeat(250));
    let themes = themes_dir(&from).unwrap();
    fs::create_dir_all(&themes).unwrap();
    fs::write(themes.join(&theme_name), THEME).unwrap();
    let descriptors = crate::platforms::descriptor::user_dir(&from).unwrap();
    fs::create_dir_all(&descriptors).unwrap();
    fs::write(
        descriptors.join(&descriptor_name),
        descriptor(&from, "long", "Long", &EVERY_OS),
    )
    .unwrap();
    assert_eq!(theme_ids(&from), ["mine"]);
    let file = from.0.join("b.json");
    let exported = export_backup(&from, &file).unwrap();
    assert_eq!(
        (exported.preview.themes, exported.preview.descriptors),
        (1, 1)
    );
    preview_backup(&file).unwrap();

    let to = ctx("long-valid-to");
    let restored = restore_backup(&to, &file);
    assert!(restored.is_ok(), "LONG_VALID_NAME: {restored:?}");
    assert_eq!(theme_files(&to), std::slice::from_ref(&theme_name));
    assert_eq!(theme_ids(&to), ["mine"]);
    assert_eq!(
        descriptor_files(&to),
        std::slice::from_ref(&descriptor_name)
    );
    // Over the files it restored: the pre-restore copy keeps the long
    // names too.
    let again = restore_backup(&to, &file).unwrap();
    let copy = PathBuf::from(&again.pre_restore_dir);
    assert!(copy.join("themes").join(&theme_name).is_file());
    assert!(copy.join("descriptors").join(&descriptor_name).is_file());
    // No temp file is left behind.
    let left: Vec<PathBuf> = tree(&to.0)
        .into_keys()
        .filter(|p| p.extension().is_some_and(|e| e == "tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_theme_name_this_os_cannot_write_refuses_the_whole_restore() {
    let to = ctx("theme-unwritable");
    seed(&to);
    let files = ctx("theme-unwritable-files");
    let theme: Value = serde_json::from_str(THEME).unwrap();
    let before = tree(&to.0);
    for (n, (key, name)) in [("con.json", "con.json"), ("a:b.json", "a:b.json")]
        .into_iter()
        .enumerate()
    {
        let file = file_with_entries(
            &files.0,
            &n.to_string(),
            "themes",
            json!({"ok.json": theme.clone(), (key): theme.clone()}),
        );
        if cfg!(windows) {
            assert!(
                preview_backup(&file).is_err(),
                "the theme {key:?} passes the preview"
            );
            let error = restore_backup(&to, &file).unwrap_err().message;
            assert!(error.contains(name), "{key:?}: {error}");
            assert_eq!(tree(&to.0), before, "{key:?}");
        } else {
            assert!(preview_backup(&file).is_ok(), "{key:?}");
        }
    }
}

#[test]
fn a_name_this_os_cannot_write_refuses_the_whole_restore() {
    let to = ctx("unwritable");
    seed(&to);
    let files = ctx("unwritable-files");
    let named: Value = serde_json::from_str(&descriptor(&to, "named", "Named", &EVERY_OS)).unwrap();
    let before = tree(&to.0);
    let windows_only = [
        "a:b.json",
        "C:x.json",
        "a<b.json",
        "a>b.json",
        "a\"b.json",
        "a|b.json",
        "a?.json",
        "a*.json",
        "a\u{1}.json",
        "con.json",
        "NUL.txt.json",
        "lpt1 .JSON",
        "COM\u{b9}.json",
    ];
    for (n, name) in windows_only.iter().enumerate() {
        let file = file_with_descriptors(
            &files.0,
            &n.to_string(),
            json!({"ok.json": named.clone(), (*name): named.clone()}),
        );
        if cfg!(windows) {
            let error = restore_backup(&to, &file).unwrap_err().message;
            assert!(error.contains(name), "{name:?}: {error}");
            assert!(preview_backup(&file).is_err(), "{name:?}");
            assert_eq!(tree(&to.0), before, "{name:?}");
        } else {
            assert!(preview_backup(&file).is_ok(), "{name:?}");
        }
    }
    #[cfg(unix)]
    {
        let file = file_with_descriptors(&files.0, "colon", json!({"a:b.json": named}));
        restore_backup(&to, &file).unwrap();
        assert!(descriptor_files(&to).contains(&"a:b.json".to_string()));
    }
}

#[test]
fn what_windows_cannot_name_is_told_apart() {
    for name in [
        "a:b.json",
        "a\u{1f}.json",
        "AUX.json",
        "conin$.json",
        "lpt\u{b3}.json",
        "x.",
        "x ",
        "a\u{0}.json",
    ] {
        assert!(unwritable_reason(name, true).is_some(), "{name:?}");
    }
    assert!(unwritable_reason("a\u{0}.json", false).is_some());
    for name in [
        "a..json",
        "My Platform.json",
        "console.json",
        "com10.json",
        "a.con.json",
        "a\u{7f}.json",
    ] {
        assert_eq!(unwritable_reason(name, true), None, "{name:?}");
    }
    for name in ["a:b.json", "con.json", "a\u{1}.json", "x."] {
        assert_eq!(unwritable_reason(name, false), None, "{name:?}");
    }
}

#[test]
fn a_folder_the_export_cannot_read_fails_it() {
    for which in ["descriptors", "themes"] {
        let app = ctx(&format!("unreadable-{which}"));
        seed(&app);
        let dir = if which == "themes" {
            themes_dir(&app).unwrap()
        } else {
            crate::platforms::descriptor::user_dir(&app).unwrap()
        };
        fs::remove_dir_all(&dir).unwrap();
        fs::write(&dir, b"not a folder").unwrap();
        let target = app.0.join("out.json");
        let result = export_backup(&app, &target);
        assert!(result.is_err(), "{which}: {result:?}");
        assert!(!target.exists(), "{which}");
    }
    // A folder never created holds nothing.
    let app = ctx("no-folders");
    let report = export_backup(&app, &app.0.join("out.json")).unwrap();
    assert_eq!((report.preview.themes, report.preview.descriptors), (0, 0));
}

#[test]
fn an_export_too_big_to_restore_is_refused_before_it_is_written() {
    let app = ctx("oversized");
    let dir = crate::platforms::descriptor::user_dir(&app).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let pad = "x".repeat(12 * 1024 * 1024);
    for n in 0..3 {
        fs::write(
            dir.join(format!("big-{n}.json")),
            format!(r#"{{"pad":"{pad}"}}"#),
        )
        .unwrap();
    }
    let target = app.0.join("out.json");
    let result = export_backup(&app, &target);
    let exists = target.exists();
    let _ = fs::remove_dir_all(&app.0);
    let error = result.unwrap_err();
    assert!(error.contains("too big"), "{error}");
    assert!(!exists);
}

#[test]
fn an_export_refuses_two_files_a_restore_would_take_for_one() {
    // Regression: a case-sensitive folder keeps `A.json` and `a.json`
    // apart; the export wrote both and every restore refused the file,
    // even on this PC.
    let app = ctx("export-case");
    let theme: Value = serde_json::from_str(THEME).unwrap();
    for field in ["themes", "descriptors"] {
        let mut backup = build_backup(&app, 1).unwrap();
        let entries = if field == "themes" {
            &mut backup.themes
        } else {
            &mut backup.descriptors
        };
        entries.insert("A.json".into(), theme.clone());
        entries.insert("a.json".into(), theme.clone());
        let target = app.0.join(format!("{field}.json"));
        let error = write_backup(&backup, &target).unwrap_err();
        assert!(error.contains("`A.json`"), "{error}");
        assert!(error.contains("`a.json`"), "{error}");
        assert!(!target.exists(), "{field}: a backup no restore reads");
    }
    // The same through the folder, where it keeps both names.
    let dir = themes_dir(&app).unwrap();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("A.json"), THEME).unwrap();
    fs::write(dir.join("a.json"), THEME).unwrap();
    let target = app.0.join("out.json");
    let result = export_backup(&app, &target);
    if fs::read_dir(&dir).unwrap().count() == 2 {
        let error = result.unwrap_err();
        assert!(error.contains("`A.json`") && error.contains("`a.json`"));
        assert!(!target.exists());
    } else {
        // One file under either name: the backup restores.
        result.unwrap();
        preview_backup(&target).unwrap();
    }
    let _ = fs::remove_dir_all(&app.0);
}

#[test]
fn a_theme_at_the_loaders_size_limit_still_loads_once_restored() {
    // Regression: the restore wrote the theme pretty-printed, over the
    // 64 KiB the loader reads, so a theme that loaded was left out.
    let from = ctx("theme-limit-from");
    let dir = themes_dir(&from).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let tokens: Map<String, Value> = (0..4000)
        .map(|n| (format!("k{n:04}"), json!("x")))
        .collect();
    let mut theme = json!({
        "schemaVersion": 1, "id": "big", "name": "Big", "colorScheme": "dark",
        "tokens": tokens, "css": ""
    });
    let limit = crate::themes::MAX_THEME_FILE_BYTES as usize;
    let short = limit - serde_json::to_vec(&theme).unwrap().len();
    theme["css"] = json!("x".repeat(short));
    let body = serde_json::to_vec(&theme).unwrap();
    assert_eq!(body.len(), limit);
    fs::write(dir.join("big.json"), &body).unwrap();
    let ids = |app: &Ctx| -> Vec<String> {
        crate::themes::list_custom_themes(app)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect()
    };
    assert_eq!(ids(&from), ["big"]);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();

    let to = ctx("theme-limit-to");
    restore_backup(&to, &file).unwrap();
    let restored = fs::read(themes_dir(&to).unwrap().join("big.json")).unwrap();
    assert!(restored.len() <= limit, "{} bytes", restored.len());
    assert_eq!(
        ids(&to),
        ["big"],
        "THEME_LIMIT: the restored theme no longer loads"
    );
    let _ = fs::remove_dir_all(&from.0);
    let _ = fs::remove_dir_all(&to.0);
}

#[test]
fn a_write_that_fails_leaves_its_file_and_recovery_copy_as_they_were() {
    // Regression: a failed rename fell back to copying over the file,
    // which first copied the file over its `.bak`; the failed entry was
    // then left out of the rollback, so that `.bak` stayed clobbered.
    let to = ctx("write-fails");
    let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
    fs::create_dir_all(view.parent().unwrap()).unwrap();
    fs::write(&view, br#""grid""#).unwrap();
    fs::write(backup_path(&view), br#""older""#).unwrap();
    let readonly = |on: bool| {
        let mut perms = fs::metadata(&view).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(on);
        fs::set_permissions(&view, perms).unwrap();
    };
    let file = to.0.join("view.json");
    let body = json!({
        "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
        "stores": {(STORE_VIEW_MODE): "list"}
    });
    fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    // No rename lands, and the file refuses a copy over it.
    readonly(true);
    force_rename_failures(true);
    let result = restore_backup(&to, &file);
    force_rename_failures(false);
    readonly(false);
    let primary = fs::read(&view).unwrap();
    let bak = fs::read(backup_path(&view)).ok();
    let _ = fs::remove_dir_all(&to.0);
    let error = result.unwrap_err().message;
    assert!(error.contains("were put back"), "{error}");
    assert_eq!(primary, br#""grid""#);
    assert_eq!(
        bak.as_deref(),
        Some(&br#""older""#[..]),
        "the recovery copy was written over"
    );
}

/// Runs `body` with `faults` injected on this thread.
fn with_faults<T>(faults: Vec<(Step, PathBuf)>, body: impl FnOnce() -> T) -> T {
    FAULTS.with(|f| *f.borrow_mut() = faults);
    let out = body();
    FAULTS.with(|f| f.borrow_mut().clear());
    out
}

#[test]
fn a_put_back_that_fails_is_reported_not_claimed() {
    let (to, _) = locked_destination("put-back-fails");
    let file = file_with_settings(&to.0, json!({"language": "fr"}));
    let settings = client_store_path(&to, STORE_SETTINGS).unwrap();
    let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
    let error = with_faults(
        vec![(Step::Write, view), (Step::PutBack, settings.clone())],
        || restore_backup(&to, &file),
    )
    .unwrap_err()
    .message;
    assert!(!error.contains("were put back"), "{error}");
    assert!(error.contains(&settings.display().to_string()), "{error}");
    assert!(error.contains("pre-restore"), "{error}");
}

#[test]
fn a_recovery_copy_that_cannot_be_removed_fails_the_restore() {
    let to = ctx("bak-stays");
    save_client_store(&to, STORE_VIEW_MODE, &json!("grid")).unwrap();
    let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
    fs::write(backup_path(&view), br#""grid""#).unwrap();
    let file = to.0.join("null-view.json");
    let body = json!({
        "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
        "stores": {(STORE_VIEW_MODE): null}
    });
    fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    let result = with_faults(vec![(Step::RemoveBak, backup_path(&view))], || {
        restore_backup(&to, &file)
    });
    assert!(result.is_err(), "{result:?}");
    assert_eq!(store(&to, STORE_VIEW_MODE), Some(json!("grid")));
    assert!(view.exists(), "the store was left removed");
}

/// Exports `name` holding `body` from `kind`'s folder (`themes` or
/// `descriptors`), restores it on a fresh install, and answers the
/// destination with the bytes the restore wrote.
fn round_trip_file(tag: &str, kind: &str, name: &str, body: &[u8]) -> (Ctx, Vec<u8>) {
    let dir_of = |app: &Ctx| {
        if kind == "themes" {
            themes_dir(app).unwrap()
        } else {
            crate::platforms::descriptor::user_dir(app).unwrap()
        }
    };
    let from = ctx(&format!("{tag}-from"));
    fs::create_dir_all(dir_of(&from)).unwrap();
    fs::write(dir_of(&from).join(name), body).unwrap();
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();
    let to = ctx(&format!("{tag}-to"));
    restore_backup(&to, &file).unwrap();
    let restored = fs::read(dir_of(&to).join(name)).unwrap();
    let _ = fs::remove_dir_all(&from.0);
    (to, restored)
}

#[test]
fn a_descriptor_that_declares_a_field_twice_is_restored_as_it_was() {
    // Regression: the backup kept the file as a parsed value, which
    // keeps one of the two fields: the restored descriptor loaded where
    // the one it came from was refused.
    let body = br#"{"id":"acme","id":"acme","schemaVersion":1}"#;
    let (to, restored) = round_trip_file("dup-descriptor", "descriptors", "acme.json", body);
    let (loaded, errors) = crate::platforms::descriptor::load_user(&to);
    let _ = fs::remove_dir_all(&to.0);
    assert_eq!(
        String::from_utf8_lossy(&restored),
        String::from_utf8_lossy(body),
        "SOURCE_BYTES: the restored descriptor is not the file it came from"
    );
    assert!(loaded.is_empty());
    assert!(errors.iter().any(|e| e.source == "acme.json"), "{errors:?}");
}

#[test]
fn a_theme_over_the_loaders_size_limit_stays_left_out_once_restored() {
    // Regression: the restore wrote the theme without its whitespace,
    // under the 64 KiB the loader reads, so a theme it skipped loaded.
    let limit = crate::themes::MAX_THEME_FILE_BYTES as usize;
    let mut body = THEME.replace(',', ",\n  ").into_bytes();
    body.resize(limit + 1, b' ');
    let (to, restored) = round_trip_file("theme-over", "themes", "mine.json", &body);
    let ids = theme_ids(&to);
    let _ = fs::remove_dir_all(&to.0);
    assert_eq!(restored.len(), body.len(), "SOURCE_BYTES: rewritten");
    assert_eq!(restored, body);
    assert!(ids.is_empty(), "a theme the loader skipped loads: {ids:?}");
}

#[test]
fn a_theme_with_exponent_numbers_at_the_size_limit_still_loads_once_restored() {
    // Regression: numbers went through f64 and came back longer
    // (`1e2` as `100.0`), over the 64 KiB the loader reads.
    let limit = crate::themes::MAX_THEME_FILE_BYTES as usize;
    let tokens: Vec<String> = (0..2000)
        .map(|n| format!(r#""n{n:04}":{}"#, if n % 2 == 0 { "1e140" } else { "1e2" }))
        .collect();
    let head = format!(
        r#"{{"schemaVersion":1,"id":"big","name":"Big","colorScheme":"dark","tokens":{{{}}},"css":""#,
        tokens.join(",")
    );
    let mut body = head.into_bytes();
    body.resize(limit - 2, b'x');
    body.extend_from_slice(br#""}"#);
    assert_eq!(body.len(), limit);
    let (to, restored) = round_trip_file("theme-exponent", "themes", "big.json", &body);
    let ids = theme_ids(&to);
    let _ = fs::remove_dir_all(&to.0);
    assert!(
        restored == body,
        "SOURCE_BYTES: {} bytes restored from {}",
        restored.len(),
        body.len()
    );
    assert_eq!(ids, ["big"]);
}

#[test]
fn settings_that_are_not_an_object_fail_the_export() {
    // Regression: the export wrote them as they were, and every preview
    // and restore of that backup then refused it.
    for settings in ["null", "[1]", "\"fr\""] {
        let app = ctx("export-bad-settings");
        let path = client_store_path(&app, STORE_SETTINGS).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, settings).unwrap();
        let target = app.0.join("out.json");
        let result = export_backup(&app, &target);
        let exists = target.exists();
        let _ = fs::remove_dir_all(&app.0);
        let error = result.expect_err(settings);
        assert!(error.contains("not an object"), "{settings}: {error}");
        assert!(!exists, "{settings}: a backup no restore reads");
    }
}

#[test]
fn a_failed_restore_says_whether_restored_files_remain() {
    // Regression: the error was a sentence, so the app could not tell a
    // restore put back whole from one that left restored files.
    let (to, _) = locked_destination("left-restored");
    let file = file_with_settings(&to.0, json!({"language": "fr"}));
    let settings = client_store_path(&to, STORE_SETTINGS).unwrap();
    let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
    let left = with_faults(
        vec![(Step::Write, view.clone()), (Step::PutBack, settings)],
        || restore_backup(&to, &file),
    )
    .unwrap_err();
    let whole = with_faults(vec![(Step::Write, view)], || restore_backup(&to, &file)).unwrap_err();
    let _ = fs::remove_dir_all(&to.0);
    assert!(left.left_restored, "{left}");
    assert!(!whole.left_restored, "{whole}");
}

#[test]
fn an_export_never_writes_over_a_file_the_app_reads() {
    // Regression (EXPORT_OVER_STORE): the export read the settings,
    // then wrote the backup envelope over them as the target.
    let app = ctx("export-managed");
    seed(&app);
    let settings = client_store_path(&app, STORE_SETTINGS).unwrap();
    let notes = client_store_path(&app, STORE_ACCOUNT_CARD_NOTES).unwrap();
    fs::write(backup_path(&notes), br#"{"765":"older"}"#).unwrap();
    let mut managed = vec![
        settings.clone(),
        backup_path(&notes),
        portable_config_path(&app).unwrap(),
        themes_dir(&app).unwrap().join("midnight.json"),
        crate::platforms::descriptor::user_dir(&app)
            .unwrap()
            .join("new.json"),
    ];
    if cfg!(windows) {
        let name = settings
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_uppercase();
        managed.push(settings.with_file_name(name));
    }
    let before = tree(&app.0);
    for target in &managed {
        let result = export_backup(&app, target);
        assert!(
            result.is_err(),
            "EXPORT_OVER_STORE: exported over {}",
            target.display()
        );
    }
    let after = tree(&app.0);
    // A backup file, new or there already, is still a destination.
    let file = app.0.join("accshift-backup.json");
    export_backup(&app, &file).unwrap();
    export_backup(&app, &file).unwrap();
    let _ = fs::remove_dir_all(&app.0);
    assert_eq!(after, before, "a managed file changed");
}

#[test]
fn an_export_through_a_link_never_writes_over_a_theme() {
    // Regression (EXPORT_LINK): only the folder the link sat in was
    // checked, so a link to a theme passed and a failed rename copied
    // the backup through it, over the theme.
    let app = ctx("export-link");
    let themes = themes_dir(&app).unwrap();
    fs::create_dir_all(&themes).unwrap();
    let theme = themes.join("mine.json");
    fs::write(&theme, THEME).unwrap();
    let link = app.0.join("backup.json");
    let dangling = app.0.join("later.json");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&theme, &link)
        .and_then(|()| std::os::unix::fs::symlink(themes.join("new.json"), &dangling));
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&theme, &link)
        .and_then(|()| std::os::windows::fs::symlink_file(themes.join("new.json"), &dangling));
    if made.is_err() {
        // Windows without Developer Mode or admin rights makes no links.
        let _ = fs::remove_dir_all(&app.0);
        return;
    }
    let through = export_backup(&app, &link);
    let into = export_backup(&app, &dangling);
    let kept = fs::read_to_string(&theme).unwrap();
    let created = themes.join("new.json").exists();
    let _ = fs::remove_dir_all(&app.0);
    assert!(through.is_err(), "EXPORT_LINK: exported through the link");
    assert!(into.is_err(), "EXPORT_LINK: exported into the theme folder");
    assert_eq!(kept, THEME, "EXPORT_LINK: the theme changed");
    assert!(!created, "EXPORT_LINK: a file appeared in the theme folder");
}

#[test]
fn a_recovery_copy_past_the_size_limit_fails_the_export_before_it_is_read() {
    // Regression (BAK_BUDGET): only the store was charged, so a corrupt
    // store sent the read to a recovery copy of any size.
    let app = ctx("bak-budget");
    seed(&app);
    let notes = client_store_path(&app, STORE_ACCOUNT_CARD_NOTES).unwrap();
    fs::write(&notes, b"{not json").unwrap();
    let pad = "x".repeat(MAX_JSON_STORE_BYTES as usize);
    fs::write(backup_path(&notes), format!(r#"{{"1":"{pad}"}}"#)).unwrap();
    let result = export_backup(&app, &app.0.join("out.json"));
    let _ = fs::remove_dir_all(&app.0);
    let error = result.unwrap_err();
    let bak = backup_path(&notes)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        error.contains(&bak),
        "BAK_BUDGET: the recovery copy was read before the size check: {error}"
    );
}

#[test]
fn a_store_read_never_passes_the_budget_left() {
    // Regression (BAK_GROWS): the sizes were checked, then each file
    // read whole, so one that grew in between passed the limit.
    let app = ctx("store-budget");
    let dir = app.0.join("user");
    fs::create_dir_all(&dir).unwrap();
    let store = dir.join("notes.json");
    fs::write(&store, b"{not json").unwrap();
    fs::write(backup_path(&store), br#"{"1":"recovered"}"#).unwrap();
    // The corrupt store's bytes are given back for its copy.
    let mut fits = ReadBudget(20);
    let served = read_store_in_budget(&store, &mut fits);
    // A copy past what is left fails, whatever its size was before.
    let mut short = ReadBudget(12);
    let refused = read_store_in_budget(&store, &mut short);
    let _ = fs::remove_dir_all(&app.0);
    assert_eq!(served.unwrap(), Some(json!({"1": "recovered"})));
    assert_eq!(fits.0, 20 - 17);
    let error = refused.unwrap_err();
    assert!(error.contains("notes.bak"), "BAK_GROWS: {error}");
}

#[test]
fn a_restore_after_a_preview_refuses_a_file_changed_since() {
    // Regression (PREVIEW_SWAP): the restore applied whatever file was
    // at the path once confirmed, not the one the preview showed.
    let from = ctx("swap-from");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();
    let preview = preview_backup(&file).unwrap();
    assert_eq!(preview.digest.len(), 64);
    // A sync client replaces it with another valid backup.
    let other = file_with_settings(&from.0, json!({"language": "es"}));
    fs::copy(&other, &file).unwrap();

    let to = ctx("swap-to");
    let before = tree(&to.0);
    let result = restore_previewed(&to, &file, &preview.digest);
    let after = tree(&to.0);
    let again = restore_previewed(&to, &file, &preview_backup(&file).unwrap().digest);
    let _ = fs::remove_dir_all(&from.0);
    let _ = fs::remove_dir_all(&to.0);
    let error = result.expect_err("PREVIEW_SWAP: restored a file the preview never showed");
    assert!(!error.left_restored);
    assert!(
        error.message.contains("changed since its preview"),
        "{error}"
    );
    assert_eq!(after, before);
    // Previewed again, the new file restores.
    again.unwrap();
}

#[test]
fn a_source_past_the_size_limit_fails_the_export_before_it_is_read() {
    // Regression (READ_BUDGET): every file was read whole, then the
    // encoded backup measured, so a huge file was read into memory
    // before the size error.
    let app = ctx("read-budget");
    let dir = crate::platforms::descriptor::user_dir(&app).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let pad = "x".repeat(MAX_JSON_STORE_BYTES as usize);
    fs::write(dir.join("huge.json"), format!(r#"{{"pad":"{pad}"}}"#)).unwrap();
    let result = export_backup(&app, &app.0.join("out.json"));
    let _ = fs::remove_dir_all(&app.0);
    let error = result.unwrap_err();
    assert!(
        error.contains("huge.json"),
        "READ_BUDGET: the file was read before the size check: {error}"
    );
}

#[test]
fn a_theme_name_the_theme_loader_skips_refuses_the_restore() {
    // Regression (THEME_EXT): `Mine.JSON` passed the checks and was
    // restored, though the theme loader reads lower case `.json` only.
    let to = ctx("theme-ext");
    let file = file_with_entries(&to.0, "upper", "themes", json!({"Mine.JSON": THEME}));
    let before = tree(&to.0);
    let preview = preview_backup(&file);
    let restore = restore_backup(&to, &file);
    let after = tree(&to.0);
    // A descriptor keeps its upper case extension.
    let acme = descriptor(&to, "acme", "Acme", &EVERY_OS);
    let descriptors = file_with_descriptors(&to.0, "upper", json!({"Acme.JSON": acme}));
    let descriptor_restore = restore_backup(&to, &descriptors);
    let _ = fs::remove_dir_all(&to.0);
    assert!(preview.is_err(), "THEME_EXT: previewed {preview:?}");
    assert!(restore.is_err(), "THEME_EXT: restored");
    assert_eq!(after, before);
    descriptor_restore.unwrap();
}

#[test]
fn a_recovery_copy_of_a_replaced_store_that_cannot_be_removed_fails_the_restore() {
    // Regression (BAK_KEPT): the old `.bak` stayed beside the restored
    // notes and the restore said it went through.
    let to = ctx("bak-kept");
    save_client_store(&to, STORE_ACCOUNT_CARD_NOTES, &json!({"765": "old"})).unwrap();
    let notes = client_store_path(&to, STORE_ACCOUNT_CARD_NOTES).unwrap();
    fs::write(backup_path(&notes), br#"{"765":"older"}"#).unwrap();
    let primary = fs::read(&notes).unwrap();
    let file = to.0.join("notes.json");
    let body = json!({
        "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
        "stores": {(STORE_ACCOUNT_CARD_NOTES): {"765": "restored"}}
    });
    fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
    let result = with_faults(vec![(Step::RemoveBak, backup_path(&notes))], || {
        restore_backup(&to, &file)
    });
    let now = fs::read(&notes).unwrap();
    let bak = fs::read(backup_path(&notes)).ok();
    let _ = fs::remove_dir_all(&to.0);
    let error = result.expect_err("BAK_KEPT: the restore went through");
    assert!(!error.left_restored, "{error}");
    assert_eq!(now, primary, "the notes were not put back");
    assert_eq!(bak.as_deref(), Some(&br#"{"765":"older"}"#[..]));
}

#[test]
fn a_store_too_big_once_written_refuses_the_restore_before_any_write() {
    // Regression (STORE_SIZE): a compact backup held a store that the
    // pretty-printed write took past the read limit, so the restored
    // store never loaded.
    let to = ctx("store-size");
    save_client_store(&to, STORE_ACCOUNT_CARD_NOTES, &json!({"765": "old"})).unwrap();
    // Deep arrays: each element is indented by two spaces a level.
    let depth = 100;
    let inner = vec!["0"; 170_000].join(",");
    let nested = format!("{}{inner}{}", "[".repeat(depth), "]".repeat(depth));
    let body = format!(
        r#"{{"kind":"{BACKUP_KIND}","format":1,"createdAt":1,"appVersion":"2.0.0","stores":{{"{STORE_ACCOUNT_CARD_NOTES}":{{"765":{nested}}}}}}}"#
    );
    let file = to.0.join("deep.json");
    fs::write(&file, body).unwrap();
    assert!(fs::metadata(&file).unwrap().len() < MAX_JSON_STORE_BYTES);
    let before = tree(&to.0);
    let result = restore_backup(&to, &file);
    let after = tree(&to.0);
    let _ = fs::remove_dir_all(&to.0);
    let error = result.expect_err("STORE_SIZE: restored a store no read loads");
    assert!(error.message.contains(STORE_ACCOUNT_CARD_NOTES), "{error}");
    assert!(after == before, "a file was written");
}

#[test]
fn an_unreadable_recovery_copy_of_a_missing_store_fails_the_export() {
    // Regression (BAK_LOCKED): any error on the `.bak` of a missing
    // store read as no store, so the export said `null` and a restore
    // removed the notes the copy held.
    let app = ctx("bak-locked");
    seed(&app);
    let notes = client_store_path(&app, STORE_ACCOUNT_CARD_NOTES).unwrap();
    let body = fs::read(&notes).unwrap();
    fs::remove_file(&notes).unwrap();
    let held = crate::test_fs::Unreadable::new(&backup_path(&notes), &body);
    let target = app.0.join("out.json");
    let result = export_backup(&app, &target);
    let exists = target.exists();
    drop(held);
    // Once it reads, the copy stands in for the missing store.
    let again = build_backup(&app, 1);
    let _ = fs::remove_dir_all(&app.0);
    let error = result.expect_err("BAK_LOCKED: exported the notes as empty");
    assert!(error.contains("account-card-notes.bak"), "{error}");
    assert!(!exists, "a backup was written");
    assert_eq!(
        again.unwrap().stores[STORE_ACCOUNT_CARD_NOTES],
        json!({"765": "main account"})
    );
}

#[test]
fn a_linked_theme_or_descriptor_fails_the_export_and_names_it() {
    // Regression (LINKED): the folder read followed links, so `x.json`
    // linked to the settings put their text, PIN hash included, in the
    // backup.
    for which in ["themes", "descriptors"] {
        let app = ctx(&format!("linked-{which}"));
        seed(&app);
        let dir = if which == "themes" {
            themes_dir(&app).unwrap()
        } else {
            crate::platforms::descriptor::user_dir(&app).unwrap()
        };
        let settings = client_store_path(&app, STORE_SETTINGS).unwrap();
        let link = dir.join("x.json");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&settings, &link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&settings, &link);
        if made.is_err() {
            // Windows without Developer Mode or admin rights makes no links.
            let _ = fs::remove_dir_all(&app.0);
            return;
        }
        let target = app.0.join("out.json");
        let result = export_backup(&app, &target);
        let text = fs::read_to_string(&target).unwrap_or_default();
        let _ = fs::remove_dir_all(&app.0);
        assert!(
            !text.contains("pinHash"),
            "LINKED: {which}: the settings went out through x.json"
        );
        let error = result.expect_err(&format!("LINKED: {which}: exported through the link"));
        assert!(error.contains("x.json"), "{which}: {error}");
    }
}

#[test]
fn an_export_leaves_the_file_beside_its_target_alone() {
    // Regression (SIBLING): the export wrote through the store writer,
    // which takes `setup.bak` for its own recovery copy of `setup.json`
    // and removes it.
    let app = ctx("export-sibling");
    seed(&app);
    let target = app.0.join("setup.json");
    let sibling = app.0.join("setup.bak");
    fs::write(&sibling, b"the user's own file").unwrap();
    let result = export_backup(&app, &target);
    let kept = fs::read(&sibling).ok();
    let _ = fs::remove_dir_all(&app.0);
    result.unwrap();
    assert_eq!(
        kept.as_deref(),
        Some(&b"the user's own file"[..]),
        "SIBLING: the export removed setup.bak"
    );
}

/// `n` zeros as a JSON array, counting each one encoded.
struct Counted<'a>(&'a std::cell::Cell<usize>, usize);

impl serde::Serialize for Counted<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(self.1))?;
        for _ in 0..self.1 {
            self.0.set(self.0.get() + 1);
            seq.serialize_element(&0u8)?;
        }
        seq.end()
    }
}

#[test]
fn an_encoding_stops_once_it_passes_the_limit() {
    // Regression (ENCODE_BOUND): the store was encoded whole, then
    // measured, so a deep store could take gigabytes before the size
    // error.
    let count = std::cell::Cell::new(0);
    let encoded = encode_within(&Counted(&count, 100_000), 1024).unwrap();
    assert!(encoded.is_none());
    assert!(
        count.get() < 1_000,
        "ENCODE_BOUND: encoded {} elements past a limit of 1024 bytes",
        count.get()
    );
    // Within the limit, the bytes are the ones of `to_vec_pretty`.
    let value = json!({"a": [1, {"b": "c\u{e9}"}], "d": null, "e": {}});
    let pretty = serde_json::to_vec_pretty(&value).unwrap();
    let limit = pretty.len() as u64;
    assert_eq!(
        encode_within(&value, MAX_JSON_STORE_BYTES).unwrap(),
        Some(pretty.clone())
    );
    assert_eq!(encode_within(&value, limit).unwrap(), Some(pretty));
    assert_eq!(encode_within(&value, limit - 1).unwrap(), None);
}

#[test]
fn a_backup_that_grows_after_its_size_check_is_refused() {
    // Regression (IMPORT_GROWS): the size was checked, then the file
    // read whole, so one a sync client replaced in between was read at
    // any size.
    let from = ctx("import-grows");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();
    let mut grown = fs::read(&file).unwrap();
    grown.resize(MAX_JSON_STORE_BYTES as usize + 10, b' ');
    let path = file.clone();
    BEFORE_READ.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || fs::write(&path, &grown).unwrap()));
    });
    let result = preview_backup(&file);
    BEFORE_READ.with(|hook| hook.borrow_mut().take());
    let _ = fs::remove_dir_all(&from.0);
    let error = result.expect_err("IMPORT_GROWS: read a backup past the limit");
    assert!(error.contains("too big"), "{error}");
}

#[cfg(unix)]
#[test]
fn the_pre_restore_copies_are_the_owners_alone() {
    // Regression (PRIVATE_COPY): the copies took the umask's mode, so
    // settings kept owner-only were copied readable by every user.
    use std::os::unix::fs::PermissionsExt;
    let from = ctx("private-from");
    seed(&from);
    let file = from.0.join("b.json");
    export_backup(&from, &file).unwrap();
    let to = ctx("private-to");
    seed(&to);
    let notes = client_store_path(&to, STORE_ACCOUNT_CARD_NOTES).unwrap();
    fs::write(backup_path(&notes), br#"{"1":"older"}"#).unwrap();
    let report = restore_backup(&to, &file).unwrap();
    let dir = PathBuf::from(&report.pre_restore_dir);
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir_mode = mode(&dir);
    let files: Vec<(PathBuf, u32)> = tree(&dir)
        .into_keys()
        .map(|p| {
            let bits = mode(&p);
            (p, bits)
        })
        .collect();
    let _ = fs::remove_dir_all(&from.0);
    let _ = fs::remove_dir_all(&to.0);
    assert_eq!(dir_mode, 0o700, "PRIVATE_COPY: the folder is {dir_mode:o}");
    assert!(files
        .iter()
        .any(|(p, _)| p.ends_with("user/account-card-notes.bak")));
    for (path, mode) in files {
        assert_eq!(mode, 0o600, "PRIVATE_COPY: {} is {mode:o}", path.display());
    }
}

#[test]
fn the_file_name_carries_the_date() {
    assert_eq!(
        backup_file_name(1_791_000_000),
        "accshift-backup-2026-10-03.json"
    );
}
