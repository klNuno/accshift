//! `legacy_web_storage`: the Chromium LevelDB and both WebKit sqlite layouts
//! built in temp folders and read back, the migration into the stores end to
//! end, and a read-only pass over this machine's real WebView2 profile that
//! prints key names and value lengths only.

#[path = "../src/legacy_web_storage/webkit_sqlite.rs"]
#[allow(dead_code)]
mod webkit_sqlite;

use accshift_app::legacy_web_storage::{self, webkit_paths, LegacyRead};
use accshift_app::storage::{
    initialize, LegacyLocalStorage, CLIENT_STORE_FOLDERS, CLIENT_STORE_IDS, CLIENT_STORE_SETTINGS,
    CLIENT_STORE_VIEW_MODE, LEGACY_LOCAL_STORAGE_KEYS,
};
use accshift_app::types::{ClientStorageSnapshot, StorageManifest};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ------------------------------------------------------------------ helpers

/// A scratch folder removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            // Short: the WebKit unified layout under it nears MAX_PATH.
            "alws-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wanted() -> Vec<&'static str> {
    LEGACY_LOCAL_STORAGE_KEYS.iter().map(|(_, k)| *k).collect()
}

fn map(entries: &[(&str, &str)]) -> LegacyLocalStorage {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn empty_snapshot() -> ClientStorageSnapshot {
    ClientStorageSnapshot {
        manifest: StorageManifest {
            schema_version: 1,
            stores: BTreeMap::new(),
        },
        stores: BTreeMap::new(),
    }
}

fn full_snapshot() -> ClientStorageSnapshot {
    let mut snapshot = empty_snapshot();
    for id in CLIENT_STORE_IDS {
        snapshot.stores.insert(id.to_string(), json!({}));
    }
    snapshot
}

const SETTINGS: &str = r#"{"themeId":"dark","language":"fr","note":"Réglages ✓ 😀"}"#;
const FOLDERS: &str = r#"{"version":1,"folders":[{"id":"f1","name":"Main"}]}"#;

// --------------------------------------------------------------- cheap check

#[test]
fn needs_read_only_when_a_store_is_missing_or_null() {
    assert!(legacy_web_storage::needs_read(&empty_snapshot()));
    let full = full_snapshot();
    assert!(!legacy_web_storage::needs_read(&full));

    let mut one_null = full.clone();
    one_null
        .stores
        .insert(CLIENT_STORE_VIEW_MODE.to_string(), Value::Null);
    assert!(legacy_web_storage::needs_read(&one_null));

    let mut one_missing = full.clone();
    one_missing.stores.remove(CLIENT_STORE_FOLDERS);
    assert!(legacy_web_storage::needs_read(&one_missing));
}

#[test]
fn needs_read_cost() {
    let full = full_snapshot();
    let partial = {
        let mut s = full.clone();
        s.stores.remove(CLIENT_STORE_IDS[11]);
        s
    };
    let runs = 100_000u32;
    let start = Instant::now();
    let mut hits = 0u32;
    for _ in 0..runs {
        hits += u32::from(legacy_web_storage::needs_read(std::hint::black_box(&full)));
        hits += u32::from(legacy_web_storage::needs_read(std::hint::black_box(
            &partial,
        )));
    }
    let per_call = start.elapsed() / (runs * 2);
    assert_eq!(hits, runs);
    println!("needs_read: {per_call:?} per call (12 stores, debug build)");
}

// ---------------------------------------------------------- WebKit (sqlite)

/// An `ItemTable` the way WebKit writes it: text keys, UTF-16LE blob values.
fn write_item_table(path: &Path, rows: &[(&str, Vec<u8>)]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, \
             value BLOB NOT NULL ON CONFLICT FAIL)",
        )
        .unwrap();
    for (key, value) in rows {
        connection
            .execute(
                "INSERT INTO ItemTable VALUES (?, ?)",
                rusqlite::params![key, value],
            )
            .unwrap();
    }
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn webkit_rows() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("accshift_settings", utf16(SETTINGS)),
        ("accshift_viewMode", utf16("list")),
        // Not a legacy key: ignored.
        ("accshift_boot_theme", utf16(r#"{"colorScheme":"dark"}"#)),
        // Odd byte count: skipped with a reason.
        ("accshift_avatars", vec![b'{', 0, b'}']),
    ]
}

fn read_candidate(candidates: &[PathBuf]) -> (PathBuf, LegacyLocalStorage, Vec<String>) {
    let path = candidates
        .iter()
        .find(|p| p.is_file())
        .expect("a candidate exists")
        .clone();
    let rows = webkit_sqlite::read_item_table(&path, &wanted()).unwrap();
    (path, rows.items.into_iter().collect(), rows.skipped)
}

fn expected_webkit() -> LegacyLocalStorage {
    map(&[
        ("accshift_settings", SETTINGS),
        ("accshift_viewMode", "list"),
    ])
}

#[test]
fn webkit_linux_old_layout() {
    let scratch = Scratch::new("linux-old");
    let base = scratch.0.join("com.accshift.desktop");
    let db = base
        .join("localstorage")
        .join("tauri_localhost_0.localstorage");
    write_item_table(&db, &webkit_rows());
    let before = std::fs::read(&db).unwrap();

    let candidates = webkit_paths::linux_candidates(&base);
    assert_eq!(candidates, std::slice::from_ref(&db));
    let (path, storage, skipped) = read_candidate(&candidates);
    assert_eq!(path, db);
    assert_eq!(storage, expected_webkit());
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].starts_with("skipped accshift_avatars: odd UTF-16 length"));
    assert_eq!(std::fs::read(&db).unwrap(), before, "original untouched");
}

#[test]
fn webkit_linux_unified_layout_wins_over_the_old_one() {
    let scratch = Scratch::new("linux-unified");
    let base = scratch.0.join("com.accshift.desktop");
    let salt = [7u8, 1, 2, 3, 4, 5, 6, 9];
    std::fs::create_dir_all(base.join("storage")).unwrap();
    std::fs::write(base.join("storage").join("salt"), salt).unwrap();
    let folder = webkit_paths::origin_folder_name("tauri://localhost", &salt);
    let unified = base
        .join("storage")
        .join(&folder)
        .join(&folder)
        .join("LocalStorage")
        .join("localstorage.sqlite3");
    write_item_table(&unified, &webkit_rows());
    let old = base
        .join("localstorage")
        .join("tauri_localhost_0.localstorage");
    write_item_table(&old, &[("accshift_settings", utf16("{}"))]);

    let candidates = webkit_paths::linux_candidates(&base);
    assert_eq!(candidates, [unified.clone(), old]);
    let (path, storage, _) = read_candidate(&candidates);
    assert_eq!(path, unified);
    assert_eq!(storage, expected_webkit());
}

#[test]
fn webkit_macos_both_layouts() {
    let scratch = Scratch::new("macos");
    let home = scratch.0.join("home");
    let website_data = home.join("Library/WebKit/com.accshift.desktop/WebsiteData");

    // Old layout alone.
    let old = website_data
        .join("LocalStorage")
        .join("tauri_localhost_0.localstorage");
    write_item_table(&old, &webkit_rows());
    let candidates = webkit_paths::macos_candidates(&home, "com.accshift.desktop");
    assert_eq!(candidates, std::slice::from_ref(&old));
    assert_eq!(read_candidate(&candidates).1, expected_webkit());

    // Then the unified layout, which WebKit reads first.
    let salt = [0xa5u8; 8];
    std::fs::create_dir_all(website_data.join("Default")).unwrap();
    std::fs::write(website_data.join("Default").join("salt"), salt).unwrap();
    let folder = webkit_paths::origin_folder_name("tauri://localhost", &salt);
    let unified = website_data
        .join("Default")
        .join(&folder)
        .join(&folder)
        .join("LocalStorage/localstorage.sqlite3");
    write_item_table(&unified, &[("accshift_folders", utf16(FOLDERS))]);
    let candidates = webkit_paths::macos_candidates(&home, "com.accshift.desktop");
    assert_eq!(candidates, [unified.clone(), old]);
    let (path, storage, _) = read_candidate(&candidates);
    assert_eq!(path, unified);
    assert_eq!(storage, map(&[("accshift_folders", FOLDERS)]));
}

#[test]
fn webkit_missing_or_broken_files_give_a_reason() {
    let scratch = Scratch::new("webkit-broken");
    let missing = scratch.0.join("nothing.localstorage");
    assert!(webkit_sqlite::read_item_table(&missing, &wanted()).is_err());

    let garbage = scratch.0.join("garbage.localstorage");
    std::fs::write(&garbage, b"not a database at all, just bytes").unwrap();
    let error = webkit_sqlite::read_item_table(&garbage, &wanted()).unwrap_err();
    assert!(!error.is_empty());

    let no_table = scratch.0.join("empty.localstorage");
    Connection::open(&no_table)
        .unwrap()
        .execute_batch("CREATE TABLE Other (x)")
        .unwrap();
    let error = webkit_sqlite::read_item_table(&no_table, &wanted()).unwrap_err();
    assert!(error.starts_with("no ItemTable"), "{error}");
}

/// The merge `read_webkit_candidates` does, through the library on Linux and
/// macOS.
#[cfg(not(windows))]
#[test]
fn webkit_candidates_merge_newest_first() {
    let scratch = Scratch::new("webkit-merge");
    let new = scratch.0.join("new.sqlite3");
    let old = scratch.0.join("old.localstorage");
    write_item_table(&new, &[("accshift_settings", utf16(SETTINGS))]);
    write_item_table(
        &old,
        &[
            ("accshift_settings", utf16("{}")),
            ("accshift_folders", utf16(FOLDERS)),
        ],
    );
    let read = legacy_web_storage::read_webkit_candidates(&[scratch.0.join("absent"), new, old]);
    assert_eq!(
        read.storage,
        map(&[
            ("accshift_settings", SETTINGS),
            ("accshift_folders", FOLDERS)
        ])
    );
    assert!(read.notes[0].starts_with("no file at"));
}

// ------------------------------------------------------ migration end to end

fn assert_migrates(legacy: &LegacyLocalStorage) {
    let backend = accshift_app::mock::MockBackend::load(
        &accshift_app::mock::MockBackend::data_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap(),
        ),
        "empty",
        None,
    )
    .unwrap();
    let (storage, report) = initialize(&backend, Some(empty_snapshot()), Some(legacy), false);
    assert!(report.failed_migrations.is_empty(), "{report:?}");
    let mut expected: Vec<&str> = Vec::new();
    for (id, key) in LEGACY_LOCAL_STORAGE_KEYS {
        if legacy.contains_key(key) {
            expected.push(id);
        }
    }
    assert_eq!(report.migrated, expected);
    if let Some(raw) = legacy.get("accshift_settings") {
        let value: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(storage.get(CLIENT_STORE_SETTINGS), Some(&value));
    }
    if let Some(raw) = legacy.get("accshift_viewMode") {
        assert_eq!(storage.get(CLIENT_STORE_VIEW_MODE), Some(&json!(raw)));
    }
    assert!(!storage.has_unsaved());
}

#[test]
fn webkit_read_migrates_into_the_mock_backend() {
    let scratch = Scratch::new("webkit-e2e");
    let base = scratch.0.join("com.accshift.desktop");
    let db = base
        .join("localstorage")
        .join("tauri_localhost_0.localstorage");
    write_item_table(&db, &webkit_rows());
    let (_, storage, _) = read_candidate(&webkit_paths::linux_candidates(&base));
    assert_migrates(&storage);
}

// ------------------------------------------------------- Chromium (LevelDB)

#[cfg(windows)]
mod chromium {
    use super::*;
    use accshift_app::legacy_web_storage::chromium::{
        decode_string, encode_string, item_key, read_leveldb, split_item_key,
    };
    use accshift_app::real::{RealBackend, BUNDLE_IDENTIFIER};
    use rusty_leveldb::compressor::{CompressorId, SnappyCompressor};
    use rusty_leveldb::env::Env;
    use rusty_leveldb::{LdbIterator, MemEnv, Options, DB};
    use std::io::Read;
    use std::rc::Rc;
    use std::sync::Arc;

    const HTTP: &str = "http://tauri.localhost";
    const HTTPS: &str = "https://tauri.localhost";

    #[test]
    fn decodes_both_formats() {
        assert_eq!(decode_string(&[]).unwrap(), (String::new(), false));
        assert_eq!(
            decode_string(&encode_string("café", false)).unwrap(),
            ("café".to_string(), false)
        );
        assert_eq!(encode_string("café", false)[0], 1, "Latin-1 fits");
        assert_eq!(encode_string("✓", false)[0], 0, "needs UTF-16");
        assert_eq!(
            decode_string(&encode_string("Réglages ✓ 😀", false)).unwrap(),
            ("Réglages ✓ 😀".to_string(), false)
        );
        assert!(decode_string(&[0, 1]).is_err(), "odd UTF-16 length");
        assert!(decode_string(&[2, b'x']).is_err(), "unknown format");
        // A lone high surrogate: kept as U+FFFD, flagged.
        assert_eq!(
            decode_string(&[0, 0x3d, 0xd8]).unwrap(),
            ("\u{fffd}".to_string(), true)
        );
        let key = item_key(HTTP, "accshift_settings", false);
        let (origin, script) = split_item_key(&key).unwrap();
        assert_eq!(origin, HTTP.as_bytes());
        assert_eq!(decode_string(script).unwrap().0, "accshift_settings");
        assert_eq!(split_item_key(b"VERSION"), None);
        assert_eq!(split_item_key(b"META:http://tauri.localhost"), None);
    }

    /// Writes `db` files from the in-memory env into `dir`.
    fn dump(env: &Rc<Box<dyn Env>>, dir: &Path) -> Vec<String> {
        std::fs::create_dir_all(dir).unwrap();
        let mut names = Vec::new();
        for child in env.children(Path::new("db")).unwrap() {
            let mut bytes = Vec::new();
            env.open_sequential_file(&Path::new("db").join(&child))
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            std::fs::write(dir.join(&child), bytes).unwrap();
            names.push(child.to_string_lossy().into_owned());
        }
        names.sort();
        names
    }

    /// A Chromium-shaped localStorage LevelDB: Snappy tables, a compacted
    /// `.ldb` and a live `.log`, two app origins, another origin, the
    /// bookkeeping keys, both encodings, a deletion and an overwrite in the
    /// log, one corrupt value.
    fn build_fixture(dir: &Path) -> Vec<String> {
        let env: Rc<Box<dyn Env>> = Rc::new(Box::new(MemEnv::new()));
        let options = Options {
            env: env.clone(),
            compressor: SnappyCompressor::ID,
            ..Options::default()
        };
        let mut db = DB::open("db", options).unwrap();
        db.put(b"VERSION", b"1").unwrap();
        db.put(format!("META:{HTTP}").as_bytes(), b"\x08\x01")
            .unwrap();
        let put = |db: &mut DB, origin: &str, key: &str, value: &str, utf16: bool| {
            db.put(&item_key(origin, key, utf16), &encode_string(value, utf16))
                .unwrap();
        };
        // Table phase.
        put(&mut db, HTTP, "accshift_settings", SETTINGS, false);
        put(&mut db, HTTP, "accshift_folders", "{}", false);
        put(&mut db, HTTP, "accshift_viewMode", "grid", false);
        put(&mut db, HTTP, "accshift_boot_theme", "{}", false);
        put(&mut db, HTTPS, "accshift_settings", "{}", true);
        put(&mut db, HTTPS, "accshift_personas", "[]", true);
        put(
            &mut db,
            "http://localhost:1420",
            "accshift_account_card_notes",
            "{}",
            false,
        );
        let corrupt = item_key(HTTP, "accshift_avatars", false);
        db.put(&corrupt, &[9, b'{', b'}']).unwrap();
        // A long value so Snappy has something to compress.
        let long = format!("{{\"blob\":\"{}\"}}", "a".repeat(4000));
        put(&mut db, HTTP, "accshift_ban_info_cache_v1", &long, true);
        db.compact_range(b"\x00", b"\xff").unwrap();
        // Log phase: an overwrite, a deletion and a new key.
        put(&mut db, HTTP, "accshift_folders", FOLDERS, true);
        db.delete(&item_key(HTTP, "accshift_viewMode", false))
            .unwrap();
        put(
            &mut db,
            HTTP,
            "accshift_account_card_colors",
            r#"{"a":"red"}"#,
            false,
        );
        db.flush().unwrap();
        // Dump before the drop: `Drop for DB` only flushes the log, but the
        // files must show the log entries still unmerged.
        let names = dump(&env, dir);
        drop(db);
        names
    }

    fn expected_chromium() -> LegacyLocalStorage {
        map(&[
            ("accshift_settings", SETTINGS),
            ("accshift_folders", FOLDERS),
            ("accshift_personas", "[]"),
            ("accshift_account_card_colors", r#"{"a":"red"}"#),
            (
                "accshift_ban_info_cache_v1",
                &format!("{{\"blob\":\"{}\"}}", "a".repeat(4000)),
            ),
        ])
    }

    #[test]
    fn reads_a_chromium_leveldb_copy() {
        let scratch = Scratch::new("chromium");
        let dir = scratch.0.join("leveldb");
        let names = build_fixture(&dir);
        assert!(names.iter().any(|n| n.ends_with(".ldb")), "{names:?}");
        assert!(names.iter().any(|n| n.ends_with(".log")), "{names:?}");
        assert!(
            names.iter().any(|n| n.starts_with("MANIFEST-")),
            "{names:?}"
        );
        // The log really holds the late keys: the table alone does not.
        let log = names.iter().find(|n| n.ends_with(".log")).unwrap();
        let log_bytes = std::fs::read(dir.join(log)).unwrap();
        assert!(log_bytes
            .windows(28)
            .any(|w| w == b"accshift_account_card_colors"));
        let before: Vec<(String, Vec<u8>)> = names
            .iter()
            .map(|n| (n.clone(), std::fs::read(dir.join(n)).unwrap()))
            .collect();

        let read = read_leveldb(&dir);
        assert_eq!(read.storage, expected_chromium(), "{:?}", read.notes);
        assert!(
            read.notes
                .iter()
                .any(|n| n.contains("skipped accshift_avatars: unknown format byte 9")),
            "{:?}",
            read.notes
        );

        let mut after: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        after.sort();
        assert_eq!(after, names, "no file added to the folder");
        for (name, bytes) in before {
            assert_eq!(std::fs::read(dir.join(&name)).unwrap(), bytes, "{name}");
        }
    }

    #[test]
    fn missing_empty_and_corrupt_folders_give_a_reason() {
        let scratch = Scratch::new("chromium-broken");
        let read = read_leveldb(&scratch.0.join("absent"));
        assert!(read.storage.is_empty());
        assert!(read.notes[0].starts_with("no WebView2 localStorage folder"));

        let empty = scratch.0.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let read = read_leveldb(&empty);
        assert!(read.storage.is_empty());
        assert!(read.notes[0].ends_with("empty folder"), "{:?}", read.notes);

        let corrupt = scratch.0.join("corrupt");
        std::fs::create_dir_all(&corrupt).unwrap();
        std::fs::write(corrupt.join("CURRENT"), b"MANIFEST-000009\n").unwrap();
        std::fs::write(corrupt.join("MANIFEST-000009"), b"garbage garbage").unwrap();
        std::fs::write(corrupt.join("000010.ldb"), [0u8; 64]).unwrap();
        let read = read_leveldb(&corrupt);
        assert!(read.storage.is_empty());
        assert_eq!(read.notes.len(), 1, "{:?}", read.notes);

        // A truncated table under a valid manifest: no panic escapes.
        let truncated = scratch.0.join("truncated");
        let names = build_fixture(&truncated);
        let ldb = names.iter().find(|n| n.ends_with(".ldb")).unwrap();
        let bytes = std::fs::read(truncated.join(ldb)).unwrap();
        std::fs::write(truncated.join(ldb), &bytes[..bytes.len() / 2]).unwrap();
        // rusty-leveldb asserts on the bad block checksum; the panic is caught.
        let read = read_leveldb(&truncated);
        assert!(read.storage.is_empty());
        assert!(
            read.notes[0].ends_with("the LevelDB reader panicked"),
            "{:?}",
            read.notes
        );
    }

    #[test]
    fn read_in_finds_the_webview2_folder() {
        let scratch = Scratch::new("chromium-read-in");
        let dir = legacy_web_storage::webview2_leveldb_dir(&scratch.0, BUNDLE_IDENTIFIER);
        assert!(dir.ends_with("com.accshift.desktop/EBWebView/Default/Local Storage/leveldb"));
        build_fixture(&dir);
        let read = legacy_web_storage::read_in(&scratch.0, BUNDLE_IDENTIFIER);
        assert_eq!(read.storage, expected_chromium());
    }

    #[test]
    fn chromium_read_migrates_into_the_mock_backend() {
        let scratch = Scratch::new("chromium-e2e-mock");
        let dir = scratch.0.join("leveldb");
        build_fixture(&dir);
        assert_migrates(&read_leveldb(&dir).storage);
    }

    /// The same, written to disk by the real backend on a scratch folder and
    /// read back from the files.
    #[test]
    fn chromium_read_migrates_into_real_store_files() {
        struct Ctx(PathBuf);
        impl accshift_core::AppContext for Ctx {
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
        let scratch = Scratch::new("chromium-e2e-real");
        let dir = scratch.0.join("leveldb");
        build_fixture(&dir);
        let legacy = read_leveldb(&dir).storage;
        let backend = RealBackend::with_context(Arc::new(Ctx(scratch.0.join("app"))));
        use accshift_app::Backend;
        let boot = backend.load_client_storage_snapshot().unwrap();
        assert!(legacy_web_storage::needs_read(&boot));
        let (_, report) = initialize(&backend, Some(boot), Some(&legacy), false);
        assert!(report.failed_migrations.is_empty(), "{report:?}");
        assert_eq!(report.migrated.len(), legacy.len());

        let reloaded = backend.load_client_storage_snapshot().unwrap();
        let settings: Value = serde_json::from_str(SETTINGS).unwrap();
        assert_eq!(reloaded.stores.get(CLIENT_STORE_SETTINGS), Some(&settings));
        let folders: Value = serde_json::from_str(FOLDERS).unwrap();
        assert_eq!(reloaded.stores.get(CLIENT_STORE_FOLDERS), Some(&folders));
        // A second boot over the same files migrates nothing more.
        let (_, again) = initialize(&backend, Some(reloaded), Some(&legacy), false);
        assert!(again.migrated.is_empty());
    }

    /// Read-only pass over this machine's WebView2 profile. Prints key names,
    /// value lengths and origins, never a value. Skipped when the folder is
    /// missing.
    #[test]
    fn real_webview2_profile_read_only() {
        let Some(local) = dirs::data_local_dir() else {
            return;
        };
        let dir = legacy_web_storage::webview2_leveldb_dir(&local, BUNDLE_IDENTIFIER);
        if !dir.is_dir() {
            println!("skipped: no {}", dir.display());
            return;
        }
        let listing = |dir: &Path| {
            let mut names: Vec<(String, u64)> = std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| {
                    let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                    (e.file_name().to_string_lossy().into_owned(), len)
                })
                .collect();
            names.sort();
            names
        };
        let before = listing(&dir);

        let start = Instant::now();
        let read: LegacyRead = legacy_web_storage::read_in(&local, BUNDLE_IDENTIFIER);
        let elapsed = start.elapsed();

        println!("profile files: {before:?}");
        println!("full read: {elapsed:?} (debug build)");
        println!("notes: {:?}", read.notes);
        let mut found = 0;
        for (_, key) in LEGACY_LOCAL_STORAGE_KEYS {
            match read.storage.get(key) {
                Some(value) => {
                    found += 1;
                    println!("  {key}: {} chars", value.chars().count());
                }
                None => println!("  {key}: absent"),
            }
        }
        println!("legacy keys found: {found}/12");

        // Origins and script key names of the copy (no values).
        let env: Rc<Box<dyn Env>> = Rc::new(Box::new(MemEnv::new()));
        for (name, _) in &before {
            if matches!(name.as_str(), "LOCK" | "LOG" | "LOG.old") {
                continue;
            }
            let bytes = std::fs::read(dir.join(name)).unwrap();
            use std::io::Write;
            env.open_writable_file(&Path::new("db").join(name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
        let mut db = DB::open(
            "db",
            Options {
                env,
                create_if_missing: false,
                ..Options::default()
            },
        )
        .unwrap();
        let mut origins: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut iter = db.new_iter().unwrap();
        while let Some((key, value)) = iter.next() {
            match split_item_key(&key) {
                // Key name, format byte and decoded length: proves the real
                // Chromium encoding decodes, without showing a value.
                Some((origin, script)) => origins
                    .entry(String::from_utf8_lossy(origin).into_owned())
                    .or_default()
                    .push(format!(
                        "{} (format {:?}, {})",
                        decode_string(script)
                            .map(|(k, _)| k)
                            .unwrap_or_else(|e| format!("<{e}>")),
                        value.first(),
                        decode_string(&value)
                            .map(|(v, lossy)| format!("{} chars, lossy {lossy}", v.chars().count()))
                            .unwrap_or_else(|e| format!("<{e}>"))
                    )),
                None => origins
                    .entry("<bookkeeping>".into())
                    .or_default()
                    .push(String::from_utf8_lossy(&key).chars().take(40).collect()),
            }
        }
        for (origin, keys) in &origins {
            println!("origin {origin}: {keys:?}");
        }

        let after = listing(&dir);
        let names = |l: &[(String, u64)]| l.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
        assert_eq!(names(&after), names(&before), "file set unchanged");
    }
}
