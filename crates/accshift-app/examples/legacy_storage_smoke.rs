//! Run with `cargo run --release -p accshift-app --example legacy_storage_smoke`.
//! Unlike the test harness, this uses the application's release panic strategy.

use accshift_app::legacy_web_storage::{is_legacy_key, LegacyRead};

#[path = "../src/legacy_web_storage/chromium.rs"]
#[allow(dead_code)]
mod chromium;

fn main() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/legacy_web_storage/clean");
    let scratch = tempfile::tempdir().expect("create migration fixture directory");
    let mut originals = Vec::new();
    for entry in std::fs::read_dir(fixture).unwrap() {
        let entry = entry.unwrap();
        let bytes = std::fs::read(entry.path()).unwrap();
        let path = scratch.path().join(entry.file_name());
        std::fs::write(&path, &bytes).unwrap();
        originals.push((path, bytes));
    }
    let read = chromium::read_leveldb_origins(scratch.path(), &["http://localhost:1420"]);
    assert!(!read.storage.is_empty(), "clean fixture: {:?}", read.notes);

    let (table, bytes) = originals
        .iter_mut()
        .find(|(path, _)| path.extension().is_some_and(|ext| ext == "ldb"))
        .expect("fixture has a LevelDB table");
    bytes.truncate(bytes.len() / 2);
    std::fs::write(table, bytes).unwrap();

    let read = chromium::read_leveldb(scratch.path());
    assert!(
        read.storage.is_empty(),
        "truncated table must not migrate data"
    );
    assert!(
        !read.notes.is_empty(),
        "corruption must return a diagnostic: {:?}",
        read.notes
    );
    assert_eq!(
        std::fs::read_dir(scratch.path()).unwrap().count(),
        originals.len()
    );
    for (path, bytes) in originals {
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "{} changed",
            path.display()
        );
    }
    println!("legacy migration: clean fixture read, corrupt table recovered, source unchanged");
}
