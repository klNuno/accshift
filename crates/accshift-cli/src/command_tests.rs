//! Tests of the folder, current, search, persona and completion commands.

use crate::commands::{self, CompletionShell, StepFailure};
use crate::folders::{self, FolderLookupError, FolderStore};
use crate::i18n::{self, Tr};
use crate::output::Format;
use crate::personas::{self, Assignment, PersonaLookupError};
use crate::{exit, Command};
use accshift_core::app_dirs::{AppDirs, BaseDirs};
use accshift_core::storage::{client_store_path, STORE_FOLDERS, STORE_PERSONAS, STORE_SETTINGS};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "accshift-cli-cmd-test-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn dirs(&self) -> AppDirs {
        let root = Some(self.0.clone());
        AppDirs::new(BaseDirs {
            config: root.clone(),
            data: root.clone(),
            local_data: root.clone(),
            cache: root,
        })
    }

    fn write(&self, store: &str, value: &Value) -> AppDirs {
        let ctx = self.dirs();
        let path = client_store_path(&ctx, store).expect("store path");
        fs::create_dir_all(path.parent().expect("store parent")).expect("store dir");
        fs::write(&path, value.to_string()).expect("write store");
        ctx
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Two "Main" folders on Steam, one nested in "Smurfs", plus a Riot folder
/// of the same name that must not count.
fn duplicate_names() -> Value {
    json!({
        "version": 1,
        "folders": [
            { "id": "f-main", "name": "Main", "parentId": null, "platform": "steam" },
            { "id": "f-smurfs", "name": "Smurfs", "parentId": null, "platform": "steam" },
            { "id": "f-main-2", "name": "main", "parentId": "f-smurfs", "platform": "steam" },
            { "id": "f-riot", "name": "Main", "parentId": null, "platform": "riot" }
        ],
        "itemOrder": {
            "root:steam": [
                { "type": "folder", "id": "f-smurfs" },
                { "type": "folder", "id": "f-main" },
                { "type": "account", "id": "765" }
            ],
            "f-main": [{ "type": "account", "id": "761" }, { "type": "account", "id": "762" }],
            "f-smurfs": [{ "type": "account", "id": "763" }, { "type": "folder", "id": "f-main-2" }],
            "f-main-2": [{ "type": "account", "id": "764" }]
        }
    })
}

#[test]
fn a_name_two_folders_share_is_refused_with_their_ids_and_paths() {
    let store = FolderStore::from_value(&duplicate_names());
    let error = folders::resolve(&store, "steam", "MAIN").expect_err("ambiguous");
    assert_eq!(
        error,
        FolderLookupError::Ambiguous(vec![
            ("f-main".into(), "Main".into()),
            ("f-main-2".into(), "Smurfs / main".into()),
        ])
    );
}

#[test]
fn a_folder_id_picks_one_of_the_duplicates() {
    let store = FolderStore::from_value(&duplicate_names());
    let folder = folders::resolve(&store, "steam", "f-main-2").expect("by id");
    assert_eq!(folder.id, "f-main-2");
    let ids = folders::accounts_in_folder(&store, &folder.id);
    assert_eq!(ids, ["764".to_string()].into_iter().collect());
    // The id of another platform's folder is not this platform's.
    assert!(matches!(
        folders::resolve(&store, "steam", "f-riot"),
        Err(FolderLookupError::NotFound(_))
    ));
}

#[test]
fn a_unique_name_still_resolves_and_includes_subfolders() {
    let store = FolderStore::from_value(&duplicate_names());
    let folder = folders::resolve(&store, "steam", " smurfs ").expect("by name");
    let ids = folders::accounts_in_folder(&store, &folder.id);
    assert_eq!(
        ids,
        ["763".to_string(), "764".to_string()].into_iter().collect()
    );
    assert_eq!(
        folders::resolve(&store, "epic", "x"),
        Err(FolderLookupError::NoneConfigured)
    );
}

#[test]
fn the_folder_tree_follows_the_app_order_with_depth_and_counts() {
    let store = FolderStore::from_value(&duplicate_names());
    let tree = folders::tree(&store, "steam");
    let summary: Vec<(&str, usize, usize, usize)> = tree
        .iter()
        .map(|n| (n.id.as_str(), n.depth, n.accounts, n.total_accounts))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("f-smurfs", 0, 1, 2),
            ("f-main-2", 1, 1, 1),
            ("f-main", 0, 2, 2),
        ]
    );
    assert_eq!(tree[1].path, "Smurfs / main");
    assert_eq!(tree[1].parent_id.as_deref(), Some("f-smurfs"));
}

#[test]
fn a_parent_cycle_still_lists_every_folder() {
    let store = FolderStore::from_value(&json!({
        "folders": [
            { "id": "a", "name": "A", "parentId": "b", "platform": "steam" },
            { "id": "b", "name": "B", "parentId": "a", "platform": "steam" }
        ],
        "itemOrder": {}
    }));
    let ids: BTreeSet<String> = folders::tree(&store, "steam")
        .into_iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(
        ids,
        ["a".to_string(), "b".to_string()].into_iter().collect()
    );
}

#[test]
fn the_folder_reader_keeps_what_the_app_keeps() {
    let raw = json!({
        "folders": [
            { "id": " a ", "name": " A ", "parentId": " ", "platform": "steam" },
            { "id": "", "name": "no id", "platform": "steam" },
            { "id": "b", "name": "B", "parentId": "a", "platform": "steam" },
            { "id": "c", "name": "", "platform": "steam" },
            "not a folder"
        ],
        "itemOrder": {
            "root:steam": [{ "type": "folder", "id": "a" }, { "type": "folder", "id": "gone" }],
            "a": [{ "type": "account", "id": " 1 " }, { "type": "weird", "id": "2" }],
            "dead": [{ "type": "account", "id": "3" }],
            "root:": [{ "type": "account", "id": "4" }]
        }
    });
    let app = accshift_app::folders::sanitize_folder_store(&raw);
    let cli = FolderStore::from_value(&raw);
    let app_folders: Vec<(String, String, Option<String>, String)> = app
        .folders
        .iter()
        .map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                f.parent_id.clone(),
                f.platform.clone(),
            )
        })
        .collect();
    let cli_folders: Vec<(String, String, Option<String>, String)> = cli
        .folders
        .iter()
        .map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                f.parent_id.clone(),
                f.platform.clone(),
            )
        })
        .collect();
    assert_eq!(cli_folders, app_folders);
    let app_order: BTreeSet<(String, Vec<String>)> = app
        .item_order
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().map(|i| i.id.clone()).collect()))
        .collect();
    let cli_order: BTreeSet<(String, Vec<String>)> = cli
        .item_order
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().map(|i| i.id.clone()).collect()))
        .collect();
    assert_eq!(cli_order, app_order);
}

#[test]
fn list_with_an_ambiguous_folder_exits_before_reading_accounts() {
    let scratch = Scratch::new("ambiguous");
    let ctx = scratch.write(STORE_FOLDERS, &duplicate_names());
    assert_eq!(
        commands::folder_filter(&ctx, Format::Json, "steam", "main").map(|_| ()),
        Err(exit::GENERIC)
    );
    let (ids, path) =
        commands::folder_filter(&ctx, Format::Json, "steam", "f-main").expect("by id");
    assert_eq!(path, "Main");
    assert_eq!(ids.len(), 2);
}

#[test]
fn a_corrupt_folder_store_is_an_io_error_not_a_missing_folder() {
    let scratch = Scratch::new("corrupt-folders");
    let ctx = scratch.dirs();
    let path = client_store_path(&ctx, STORE_FOLDERS).expect("path");
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    fs::write(&path, "{\"folders\": [").expect("write");
    assert_eq!(
        commands::folder_filter(&ctx, Format::Json, "steam", "main").map(|_| ()),
        Err(exit::IO)
    );
    assert_eq!(commands::folders_in(&ctx, Format::Json, "steam"), exit::IO);
}

#[test]
fn folders_answers_for_a_known_platform_and_refuses_an_unknown_one() {
    let scratch = Scratch::new("folders-cmd");
    let ctx = scratch.write(STORE_FOLDERS, &duplicate_names());
    assert_eq!(commands::folders_in(&ctx, Format::Json, "steam"), exit::OK);
    assert_eq!(commands::folders_in(&ctx, Format::Human, "steam"), exit::OK);
    assert_eq!(
        commands::folders_in(&ctx, Format::Json, "no-such-platform"),
        exit::PLATFORM_UNAVAILABLE
    );
    // No store at all is an empty tree, not an error.
    let empty = Scratch::new("folders-none");
    assert_eq!(
        commands::folders_in(&empty.dirs(), Format::Json, "steam"),
        exit::OK
    );
}

// ---------------------------------------------------------------- current

#[test]
fn current_is_unknown_when_the_platform_cannot_tell() {
    for detected in [None, Some(""), Some("   ")] {
        let entry = commands::current_entry("steam", detected, None);
        assert_eq!(entry.status, "unknown");
        assert_eq!(entry.account_id, None);
    }
}

#[test]
fn current_names_the_account_from_its_list() {
    let accounts = json!([
        { "account_name": "alice", "persona_name": "Alice", "steam_id": "761" },
        { "account_name": "bob", "persona_name": "Bob", "steam_id": "762" }
    ]);
    let entry = commands::current_entry("steam", Some("Bob"), Some(&accounts));
    assert_eq!(entry.status, "known");
    assert_eq!(entry.account_id.as_deref(), Some("bob"));
    assert_eq!(entry.name.as_deref(), Some("Bob"));
    // Detected but no longer listed: still known, without a name.
    let entry = commands::current_entry("steam", Some("carol"), Some(&accounts));
    assert_eq!(entry.account_id.as_deref(), Some("carol"));
    assert_eq!(entry.name, None);
}

// ---------------------------------------------------------------- search

#[test]
fn search_matches_name_display_name_and_note_without_case() {
    let accounts = json!([
        { "account_name": "alice", "persona_name": "Sniper Queen", "steam_id": "761" },
        { "account_name": "bob", "persona_name": "Bob", "steam_id": "762" },
        { "account_name": "carol", "persona_name": "Carol", "steam_id": "763" }
    ]);
    let notes: HashMap<String, String> = [("763".to_string(), "Faceit SMURF".to_string())]
        .into_iter()
        .collect();
    let ids = |query: &str| -> Vec<String> {
        commands::search_platform("steam", &accounts, &notes, query)
            .into_iter()
            .map(|h| h.account_id)
            .collect()
    };
    assert_eq!(ids("ALI"), vec!["alice"]);
    assert_eq!(ids("sniper"), vec!["alice"]);
    assert_eq!(ids("smurf"), vec!["carol"]);
    assert!(ids("nobody").is_empty());
    let hit = &commands::search_platform("steam", &accounts, &notes, "faceit")[0];
    assert_eq!(hit.platform, "steam");
    assert_eq!(hit.note.as_deref(), Some("Faceit SMURF"));
}

#[test]
fn search_reads_a_user_platform_label_not_its_raw_json() {
    let accounts = json!([{ "id": "u1", "label": "Work" }]);
    let notes = HashMap::new();
    assert_eq!(
        commands::search_platform("my-launcher", &accounts, &notes, "work").len(),
        1
    );
    // "label" is a key of the raw JSON, not a value a person typed.
    assert!(commands::search_platform("my-launcher", &accounts, &notes, "label").is_empty());
}

// ---------------------------------------------------------------- personas

fn two_personas() -> Value {
    json!([
        {
            "id": "p1", "name": "Main", "color": "",
            "assignments": [
                { "platformId": "steam", "accountId": "761" },
                { "platformId": "riot", "accountId": "r1" },
                { "platformId": "gone", "accountId": "x" }
            ]
        },
        { "id": "p2", "name": "main", "color": "", "assignments": [] },
        { "id": "p3", "name": "Alt", "color": "", "assignments": [] }
    ])
}

#[test]
fn the_persona_reader_keeps_what_the_app_keeps() {
    let raw = json!([
        { "id": "a", "name": "  ", "assignments": [
            { "platformId": "steam", "accountId": "1" },
            { "platformId": "steam", "accountId": "2" },
            { "platformId": "", "accountId": "3" },
            { "platformId": "riot", "accountId": "" },
            { "platformId": "riot", "accountId": "4" },
            [1, 2]
        ]},
        { "id": "a", "name": "duplicate id" },
        { "id": "", "name": "no id" },
        { "id": "b", "name": "A name well past the forty character limit of the app" },
        "not a persona"
    ]);
    let app: Vec<(String, String, Vec<(String, String)>)> =
        accshift_app::personas::sanitize_personas(&raw)
            .into_iter()
            .map(|p| {
                let a = p
                    .assignments
                    .into_iter()
                    .map(|a| (a.platform_id, a.account_id))
                    .collect();
                (p.id, p.name, a)
            })
            .collect();
    let cli: Vec<(String, String, Vec<(String, String)>)> = personas::from_value(&raw)
        .into_iter()
        .map(|p| {
            let a = p
                .assignments
                .into_iter()
                .map(|a| (a.platform_id, a.account_id))
                .collect();
            (p.id, p.name, a)
        })
        .collect();
    assert_eq!(cli, app);
}

#[test]
fn a_persona_resolves_by_id_then_by_unique_name() {
    let list = personas::from_value(&two_personas());
    assert_eq!(personas::resolve(&list, "p2").expect("id").id, "p2");
    assert_eq!(personas::resolve(&list, "ALT").expect("name").id, "p3");
    assert_eq!(
        personas::resolve(&list, "main"),
        Err(PersonaLookupError::Ambiguous(vec![
            ("p1".into(), "Main".into()),
            ("p2".into(), "main".into()),
        ]))
    );
    assert!(matches!(
        personas::resolve(&list, "nobody"),
        Err(PersonaLookupError::NotFound(_))
    ));
    assert_eq!(
        personas::resolve(&[], "x"),
        Err(PersonaLookupError::NoneConfigured)
    );
}

fn step(platform: &str, account: &str) -> Assignment {
    Assignment {
        platform_id: platform.into(),
        account_id: account.into(),
    }
}

#[test]
fn a_persona_switch_stops_at_the_first_failure() {
    let steps = vec![step("steam", "761"), step("riot", "r1"), step("epic", "e1")];
    let mut tried = Vec::new();
    let report = commands::run_steps(&steps, &mut |s: &Assignment| {
        tried.push(s.platform_id.clone());
        if s.platform_id == "riot" {
            Err(StepFailure {
                platform_id: s.platform_id.clone(),
                account_id: s.account_id.clone(),
                code: "platform_error".into(),
                message: "boom".into(),
                exit: exit::GENERIC,
            })
        } else {
            Ok(())
        }
    });
    assert_eq!(tried, vec!["steam", "riot"]);
    assert_eq!(report.succeeded, vec![step("steam", "761")]);
    assert_eq!(
        report.failed.as_ref().map(|f| f.platform_id.as_str()),
        Some("riot")
    );
    assert_eq!(report.not_tried, vec![step("epic", "e1")]);
}

#[test]
fn a_persona_skips_platforms_this_build_does_not_have() {
    let list = personas::from_value(&two_personas());
    let (steps, skipped) = commands::plan(&list[0], &|id: &str| id != "gone");
    assert_eq!(steps, vec![step("steam", "761"), step("riot", "r1")]);
    assert_eq!(skipped, vec![step("gone", "x")]);
}

fn persona_ctx(scratch: &Scratch, settings: Value) -> AppDirs {
    scratch.write(STORE_SETTINGS, &settings);
    scratch.write(STORE_PERSONAS, &two_personas())
}

#[test]
fn persona_runs_every_step_and_reports_the_failure_exit() {
    let scratch = Scratch::new("persona-run");
    let ctx = persona_ctx(&scratch, json!({}));
    let mut switched = Vec::new();
    let code = commands::persona_in(&ctx, Format::Json, "p1", &|id| id != "gone", &mut |s| {
        switched.push(s.clone());
        Ok(())
    });
    assert_eq!(code, exit::OK);
    assert_eq!(switched, vec![step("steam", "761"), step("riot", "r1")]);

    let code = commands::persona_in(&ctx, Format::Human, "p1", &|_| true, &mut |s| {
        if s.platform_id == "riot" {
            Err(StepFailure {
                platform_id: s.platform_id.clone(),
                account_id: s.account_id.clone(),
                code: "unknown_account".into(),
                message: "gone".into(),
                exit: exit::UNKNOWN_ACCOUNT,
            })
        } else {
            Ok(())
        }
    });
    assert_eq!(code, exit::UNKNOWN_ACCOUNT);
}

#[test]
fn persona_honours_the_pin_lock_before_any_switch() {
    let scratch = Scratch::new("persona-pin");
    // PIN enabled with no usable hash: refused, never prompted.
    let ctx = persona_ctx(&scratch, json!({ "pinEnabled": true }));
    let mut called = false;
    let code = commands::persona_in(&ctx, Format::Json, "p1", &|_| true, &mut |_| {
        called = true;
        Ok(())
    });
    assert_eq!(code, exit::PIN_DENIED);
    assert!(!called, "a switch ran past the PIN lock");
}

#[test]
fn persona_refuses_ambiguous_unknown_and_empty_personas_without_switching() {
    let scratch = Scratch::new("persona-refuse");
    let ctx = persona_ctx(&scratch, json!({}));
    for (query, available) in [
        ("main", true),
        ("nobody", true),
        ("p1", false),
        ("Alt", true),
    ] {
        let mut called = false;
        let code = commands::persona_in(&ctx, Format::Json, query, &|_| available, &mut |_| {
            called = true;
            Ok(())
        });
        assert_eq!(code, exit::GENERIC, "{query}");
        assert!(!called, "{query} switched");
    }
    let empty = Scratch::new("persona-none");
    assert_eq!(commands::personas_in(&empty.dirs(), Format::Json), exit::OK);
    assert_eq!(commands::personas_in(&ctx, Format::Human), exit::OK);
}

// ---------------------------------------------------------------- completions

#[test]
fn completions_cover_every_shell_and_the_new_subcommands() {
    for shell in [
        CompletionShell::Bash,
        CompletionShell::Zsh,
        CompletionShell::Fish,
        CompletionShell::Powershell,
    ] {
        let script = String::from_utf8(commands::completion_script(shell)).expect("utf-8");
        for word in [
            "accshift",
            "folders",
            "current",
            "search",
            "personas",
            "persona",
            "completions",
        ] {
            assert!(script.contains(word), "{shell:?} script lacks {word}");
        }
    }
}

// ---------------------------------------------------------------- telemetry

#[test]
fn every_new_subcommand_reaches_telemetry_by_name_only() {
    let commands = [
        Command::Folders {
            platform: "steam".into(),
        },
        Command::Current {
            platform: Some("steam".into()),
        },
        Command::Search {
            query: "secret".into(),
        },
        Command::Personas,
        Command::Persona {
            persona: "secret".into(),
        },
        Command::Completions {
            shell: CompletionShell::Bash,
        },
    ];
    for command in commands {
        let name = command.name();
        assert!(!name.contains("secret"));
        assert_eq!(
            accshift_core::telemetry::code_from(name, accshift_core::telemetry::CLI_COMMANDS),
            name,
            "{name} would reach telemetry as `other`"
        );
    }
}

// ---------------------------------------------------------------- i18n

fn placeholders(text: &str) -> BTreeSet<String> {
    text.split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string()))
        .collect()
}

#[test]
fn every_table_has_the_english_keys_and_placeholders() {
    let en = i18n::table("en");
    assert!(!en.is_empty(), "en.json failed to parse");
    for locale in i18n::LOCALES {
        let table = i18n::table(locale);
        let keys: BTreeSet<&String> = table.keys().collect();
        let en_keys: BTreeSet<&String> = en.keys().collect();
        assert_eq!(keys, en_keys, "{locale} keys differ from en");
        for (key, value) in en {
            let text = table[key].as_str().expect("string");
            assert!(!text.is_empty(), "{locale} {key} is empty");
            assert_eq!(
                placeholders(text),
                placeholders(value.as_str().expect("string")),
                "{locale} {key} placeholders"
            );
            assert!(
                !text.contains('\u{2014}') && !text.contains('\u{2026}'),
                "{locale} {key}"
            );
        }
    }
}

#[test]
fn every_key_the_code_asks_for_exists_in_english() {
    let en = i18n::table("en");
    for source in [include_str!("commands.rs"), include_str!("main.rs")] {
        for chunk in source.split("tr.t(").skip(1) {
            let Some(rest) = chunk.trim_start().strip_prefix('"') else {
                continue;
            };
            let key = rest.split('"').next().unwrap_or_default();
            assert!(en.contains_key(key), "missing i18n key {key}");
        }
    }
}

#[test]
fn the_language_follows_the_app_then_the_os() {
    let os = vec!["pt_BR.UTF-8".to_string()];
    assert_eq!(i18n::pick_locale(None, &os), "pt-br");
    assert_eq!(i18n::pick_locale(Some("fr"), &os), "fr");
    // A stored language the app does not know is English, as in the app.
    assert_eq!(i18n::pick_locale(Some("klingon"), &os), "en");
    assert_eq!(i18n::pick_locale(None, &["zh-CN".to_string()]), "zh");
    assert_eq!(i18n::pick_locale(None, &["de-DE".to_string()]), "en");
    assert_eq!(
        Tr::locale("fr").t("folders.empty", &[("platform", "steam")]),
        "Aucun dossier sur steam."
    );
    // JSON output stays English whatever the app is set to.
    let scratch = Scratch::new("tr-json");
    let ctx = scratch.write(STORE_SETTINGS, &json!({ "language": "ru" }));
    assert_eq!(
        Tr::for_run(Format::Json, &ctx).t("current.unknown", &[]),
        "unknown"
    );
    assert_eq!(
        Tr::for_run(Format::Human, &ctx).t("current.unknown", &[]),
        "неизвестно"
    );
}
