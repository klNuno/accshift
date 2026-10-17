//! Reads WebKit's localStorage `ItemTable` from a copy of the sqlite file.
//!
//! Schema (WebKit `main` at 1557a275c0cf,
//! `Source/WebKit/NetworkProcess/storage/SQLiteStorageArea.cpp:47-48`):
//! `ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL)`,
//! the old `.localstorage` files with the same table. Keys are bound as text,
//! values as the UTF-16 code units of the string (`bindBlob`, `:399`; read
//! back with `columnBlobAsString`, `:356-357`), little-endian on every
//! machine WebKit runs on here. A key stored as a blob or a value stored as
//! text is accepted too.
//!
//! Self-contained (std and rusqlite only): `tests/legacy_web_storage.rs`
//! compiles this file by path so the WebKit fixtures run on Windows, where
//! the app itself never links sqlite.

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// What one database held.
#[derive(Debug, Default)]
pub struct ItemRows {
    /// `(key, value)` for each wanted key.
    pub items: Vec<(String, String)>,
    /// One line per wanted key whose value could not be decoded or was
    /// repaired.
    pub skipped: Vec<String>,
}

/// Copies `db` (and its `-wal`, if any) to a fresh temp folder, opens the
/// copy and returns the rows whose key is in `wanted`. The original is only
/// read.
pub fn read_item_table(db: &Path, wanted: &[&str]) -> Result<ItemRows, String> {
    let temp = TempDir::new().map_err(|e| format!("temp folder: {e}"))?;
    let copy = temp.0.join("localstorage.sqlite3");
    std::fs::copy(db, &copy).map_err(|e| format!("cannot copy: {e}"))?;
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    if wal.is_file() {
        std::fs::copy(&wal, temp.0.join("localstorage.sqlite3-wal"))
            .map_err(|e| format!("cannot copy the -wal: {e}"))?;
    }
    let connection = Connection::open_with_flags(
        &copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("cannot open: {e}"))?;
    let mut statement = connection
        .prepare("SELECT key, value FROM ItemTable")
        .map_err(|e| format!("no ItemTable: {e}"))?;
    let mut rows = statement
        .query([])
        .map_err(|e| format!("cannot query: {e}"))?;
    let mut out = ItemRows::default();
    loop {
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(error) => {
                out.skipped.push(format!("stopped reading: {error}"));
                break;
            }
        };
        let Ok(Ok((key, _))) = row.get_ref(0).map(decode) else {
            continue;
        };
        if !wanted.contains(&key.as_str()) {
            continue;
        }
        match row.get_ref(1).map(decode) {
            Ok(Ok((value, lossy))) => {
                if lossy {
                    out.skipped
                        .push(format!("unpaired surrogates in {key} replaced"));
                }
                out.items.push((key, value));
            }
            Ok(Err(reason)) => out.skipped.push(format!("skipped {key}: {reason}")),
            Err(error) => out.skipped.push(format!("skipped {key}: {error}")),
        }
    }
    Ok(out)
}

/// Text as UTF-8, a blob as UTF-16LE. Unpaired surrogates, which a Rust
/// string cannot hold, become U+FFFD and the flag says so.
fn decode(value: ValueRef<'_>) -> Result<(String, bool), String> {
    match value {
        ValueRef::Text(bytes) => String::from_utf8(bytes.to_vec())
            .map(|text| (text, false))
            .map_err(|_| "bad UTF-8".into()),
        ValueRef::Blob(bytes) => {
            if bytes.len() % 2 != 0 {
                return Err(format!("odd UTF-16 length {}", bytes.len()));
            }
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            Ok(match String::from_utf16(&units) {
                Ok(text) => (text, false),
                Err(_) => (String::from_utf16_lossy(&units), true),
            })
        }
        ValueRef::Null => Err("null".into()),
        ValueRef::Integer(_) | ValueRef::Real(_) => Err("not a string".into()),
    }
}

/// A temp folder removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "accshift-legacy-webkit-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
