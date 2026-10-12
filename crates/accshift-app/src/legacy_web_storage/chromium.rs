//! The WebView2 (Chromium) `localStorage` LevelDB.
//!
//! Layout, from Chromium `main` at f990ecd0892e
//! (`components/services/storage/dom_storage/leveldb/local_storage_leveldb.h:39-56`,
//! `local_storage_leveldb.cc:135-156`):
//!
//! - `VERSION`, `META:<storage key>`, `METAACCESS:<storage key>`: bookkeeping,
//!   skipped.
//! - `_` + storage key + `0x00` + script key: one item. A first-party storage
//!   key serializes as the bare origin, `http://tauri.localhost`, with no
//!   trailing slash (`third_party/blink/common/storage_key/storage_key.cc:725-736`,
//!   `SerializeForLocalStorage`).
//!
//! Script keys and values both start with a format byte
//! (`third_party/blink/renderer/modules/storage/cached_storage_area.cc:37`,
//! decoding at `:748-768`): `0x00` UTF-16LE, `0x01` Latin-1, anything else
//! corrupt. An empty byte string is the empty string.
//!
//! The folder is never opened in place: opening a LevelDB can write recovery
//! files. Every file except `LOCK` and the text `LOG`s is read into
//! `rusty_leveldb`'s in-memory env and the database is opened there, so the
//! `.log` replay and any compaction happen in memory only.

use super::{is_legacy_key, LegacyRead};
use rusty_leveldb::env::Env;
use rusty_leveldb::{LdbIterator, MemEnv, Options, DB};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::rc::Rc;

/// The origins the app was served from, preferred first.
pub const ORIGINS: [&str; 2] = ["http://tauri.localhost", "https://tauri.localhost"];

/// Splits an item key into its storage key and encoded script key; `None`
/// for the bookkeeping keys.
pub fn split_item_key(key: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = key.strip_prefix(b"_")?;
    let separator = rest.iter().position(|byte| *byte == 0)?;
    Some((&rest[..separator], &rest[separator + 1..]))
}

/// Decodes a script key or a value: format byte, then UTF-16LE or Latin-1.
/// Unpaired surrogates (which a Rust string cannot hold) become U+FFFD and
/// the second field says so.
pub fn decode_string(bytes: &[u8]) -> Result<(String, bool), String> {
    let Some((format, payload)) = bytes.split_first() else {
        return Ok((String::new(), false));
    };
    match format {
        0 => {
            if payload.len() % 2 != 0 {
                return Err(format!("odd UTF-16 length {}", payload.len()));
            }
            let units: Vec<u16> = payload
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            match String::from_utf16(&units) {
                Ok(text) => Ok((text, false)),
                Err(_) => Ok((String::from_utf16_lossy(&units), true)),
            }
        }
        1 => Ok((
            payload.iter().map(|byte| char::from(*byte)).collect(),
            false,
        )),
        other => Err(format!("unknown format byte {other}")),
    }
}

/// Encodes a string the way Chromium stores it: Latin-1 when every char fits,
/// UTF-16LE otherwise. For tests and fixtures.
pub fn encode_string(text: &str, force_utf16: bool) -> Vec<u8> {
    if !force_utf16 && text.chars().all(|c| u32::from(c) < 0x100) {
        let mut out = vec![1];
        out.extend(text.chars().map(|c| u32::from(c) as u8));
        return out;
    }
    let mut out = vec![0];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// The item key of `script_key` under `origin`, encoded as Chromium writes it.
pub fn item_key(origin: &str, script_key: &str, force_utf16: bool) -> Vec<u8> {
    let mut key = vec![b'_'];
    key.extend_from_slice(origin.as_bytes());
    key.push(0);
    key.extend(encode_string(script_key, force_utf16));
    key
}

/// Reads the legacy keys from a copy of the LevelDB folder `dir`.
pub fn read_leveldb(dir: &Path) -> LegacyRead {
    read_leveldb_origins(dir, &ORIGINS)
}

/// [`read_leveldb`] for other origins, preferred first: the parity bench
/// serves the page from `http://localhost:1420`.
pub fn read_leveldb_origins(dir: &Path, origins: &[&str]) -> LegacyRead {
    let mut result = LegacyRead::default();
    if !dir.is_dir() {
        result.notes.push(format!(
            "no WebView2 localStorage folder at {}",
            dir.display()
        ));
        return result;
    }
    match catch_unwind(AssertUnwindSafe(|| read_copy(dir, origins, &mut result))) {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => result.notes.push(format!("{}: {reason}", dir.display())),
        Err(_) => result
            .notes
            .push(format!("{}: the LevelDB reader panicked", dir.display())),
    }
    result
}

fn read_copy(dir: &Path, origins: &[&str], result: &mut LegacyRead) -> Result<(), String> {
    let env: Rc<Box<dyn Env>> = Rc::new(Box::new(MemEnv::new()));
    let db_path = Path::new("db");
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot list: {e}"))?;
    let mut copied = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if matches!(name, "LOCK" | "LOG" | "LOG.old") || !entry.path().is_file() {
            continue;
        }
        // std opens with FILE_SHARE_READ | WRITE | DELETE on Windows, so a
        // running WebView2 does not block the read.
        let bytes = match std::fs::read(entry.path()) {
            Ok(bytes) => bytes,
            Err(error) => {
                result.notes.push(format!("cannot read {name}: {error}"));
                continue;
            }
        };
        let mut file = env
            .open_writable_file(&db_path.join(name))
            .map_err(|e| format!("in-memory copy of {name}: {e}"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("in-memory copy of {name}: {e}"))?;
        copied += 1;
    }
    if copied == 0 {
        return Err("empty folder".to_string());
    }

    let options = Options {
        env,
        create_if_missing: false,
        ..Options::default()
    };
    let mut db = DB::open(db_path, options).map_err(|e| format!("cannot open: {e}"))?;
    let mut iter = db.new_iter().map_err(|e| format!("cannot iterate: {e}"))?;
    let mut by_origin: Vec<Vec<(String, String)>> = vec![Vec::new(); origins.len()];
    while let Some((key, value)) = iter.next() {
        let Some((origin, script_key)) = split_item_key(&key) else {
            continue;
        };
        let Some(slot) = origins.iter().position(|o| o.as_bytes() == origin) else {
            continue;
        };
        let Ok((script_key, _)) = decode_string(script_key) else {
            continue;
        };
        if !is_legacy_key(&script_key) {
            continue;
        }
        match decode_string(&value) {
            Ok((text, lossy)) => {
                if lossy {
                    result.notes.push(format!(
                        "{}: unpaired surrogates in {script_key} replaced",
                        origins[slot]
                    ));
                }
                by_origin[slot].push((script_key, text));
            }
            Err(reason) => result
                .notes
                .push(format!("{}: skipped {script_key}: {reason}", origins[slot])),
        }
    }
    for (slot, items) in by_origin.into_iter().enumerate() {
        if items.is_empty() {
            continue;
        }
        result
            .notes
            .push(format!("{}: {} legacy keys", origins[slot], items.len()));
        for (key, value) in items {
            result.storage.entry(key).or_insert(value);
        }
    }
    Ok(())
}
