//! The WebView `localStorage` of installs older than v1.0.0, read from disk.
//!
//! Before v1.0.0 the Tauri app kept settings, folders, notes, colours and
//! caches only in the WebView's `localStorage`; the Svelte app copies them
//! into the backend stores in `initializeClientStorage`
//! (`src/lib/storage/clientStorage.ts`). The egui app has no WebView, so it
//! reads that storage itself and hands the 12 keys of
//! [`LEGACY_LOCAL_STORAGE_KEYS`] to [`crate::storage::initialize`].
//!
//! Boot use: [`needs_read`] on the boot snapshot first (no I/O), [`read`] only
//! when it says so. Nothing here panics or fails the boot: a missing folder,
//! a locked or corrupt database or an undecodable value gives an empty or
//! partial map, with the reason in [`LegacyRead::notes`].
//!
//! # Where the storage lives
//!
//! Every path starts at Tauri's data folder for the identifier, which the
//! egui app shares (`crate::real::context`): Tauri 2.11 forces the webview
//! data directory to `BaseDirectory::LocalData` + identifier on Windows and
//! Linux (`tauri-2.11.5/src/manager/webview.rs:534-545`), that is
//! `dirs::data_local_dir()` (`dirs-6.0.0/src/win.rs:11`, `lin.rs:11-12`).
//!
//! - Windows, WebView2 (Chromium): `<data>\EBWebView\Default\Local Storage\leveldb`
//!   (WebView2's user data folder layout, given the data directory by
//!   `vendor/wry/src/webview2/mod.rs:312-330`). Origin
//!   `http://tauri.localhost`: `tauri-2.11.5/src/manager/mod.rs:339-343`,
//!   `use_https_scheme` false by default and unset in every released
//!   `tauri.conf.json`; `https://tauri.localhost` is read as a fallback.
//!   Format in [`chromium`].
//! - Linux, WebKitGTK: wry builds a `WebsiteDataManager` with
//!   `base-data-directory` = the data folder
//!   (`vendor/wry/src/webkitgtk/web_context.rs:32-39`). Origin
//!   `tauri://localhost` (`tauri-2.11.5/src/manager/mod.rs:339-343`).
//!   Paths in [`webkit_paths::linux_candidates`].
//! - macOS, WKWebView: wry uses `WKWebsiteDataStore.defaultDataStore` unless
//!   a data store identifier is set (`vendor/wry/src/wkwebview/mod.rs:228-243`;
//!   the app sets none). Same origin. Paths in
//!   [`webkit_paths::macos_candidates`].
//!
//! Old names: the identifier was `com.zazaswitcher.desktop` until d90d838
//! (2026-02-14), but every release tag, v0.1.0 included, ships
//! `com.accshift.desktop` and the `accshift_*` keys, so no other profile is
//! read. Every v0.x release was Windows only.
//!
//! [`LEGACY_LOCAL_STORAGE_KEYS`]: crate::storage::LEGACY_LOCAL_STORAGE_KEYS

#[cfg(windows)]
pub mod chromium;
pub mod webkit_paths;
#[cfg(not(windows))]
mod webkit_sqlite;

use crate::storage::{LegacyLocalStorage, CLIENT_STORE_IDS, LEGACY_LOCAL_STORAGE_KEYS};
use crate::types::ClientStorageSnapshot;
use std::path::{Path, PathBuf};

/// What [`read`] found. `storage` only ever holds the 12 legacy keys.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LegacyRead {
    pub storage: LegacyLocalStorage,
    /// One line per place looked at or value skipped: paths, key names and
    /// reasons, never a value.
    pub notes: Vec<String>,
}

impl LegacyRead {
    /// Adds the keys of `other` that are not there yet, and its notes.
    pub fn merge_missing(&mut self, other: LegacyRead) {
        for (key, value) in other.storage {
            self.storage.entry(key).or_insert(value);
        }
        self.notes.extend(other.notes);
    }
}

/// True when `key` is one of the 12 legacy keys.
pub fn is_legacy_key(key: &str) -> bool {
    LEGACY_LOCAL_STORAGE_KEYS.iter().any(|(_, k)| *k == key)
}

/// The cheap check, no I/O: true when a client store is missing or `null` in
/// the boot snapshot, the only case where [`crate::storage::initialize`]
/// would migrate anything (`ClientStorage::migrate_legacy` skips every store
/// that has a value).
pub fn needs_read(snapshot: &ClientStorageSnapshot) -> bool {
    CLIENT_STORE_IDS
        .iter()
        .any(|id| snapshot.stores.get(*id).is_none_or(|value| value.is_null()))
}

/// Reads the legacy `localStorage` of the app `identifier` (the Tauri bundle
/// identifier, [`crate::real::BUNDLE_IDENTIFIER`]) for this OS.
pub fn read(identifier: &str) -> LegacyRead {
    let Some(data_local) = dirs::data_local_dir() else {
        return note_only("no local data folder for this user");
    };
    read_in(&data_local, identifier)
}

/// [`read`] with the per-user root given: `%LOCALAPPDATA%` on Windows,
/// `$XDG_DATA_HOME` (or `~/.local/share`) on Linux, `~/Library/Application
/// Support` on macOS. Only Windows and Linux use it; macOS reads under
/// `~/Library/WebKit`, found from the home folder.
#[cfg(windows)]
pub fn read_in(data_local: &Path, identifier: &str) -> LegacyRead {
    chromium::read_leveldb(&webview2_leveldb_dir(data_local, identifier))
}

/// See the Windows version.
#[cfg(not(windows))]
pub fn read_in(data_local: &Path, identifier: &str) -> LegacyRead {
    let candidates = if cfg!(target_os = "macos") {
        match dirs::home_dir() {
            Some(home) => webkit_paths::macos_candidates(&home, identifier),
            None => return note_only("no home folder for this user"),
        }
    } else {
        webkit_paths::linux_candidates(&data_local.join(identifier))
    };
    read_webkit_candidates(&candidates)
}

/// The WebView2 localStorage folder under `data_local`.
pub fn webview2_leveldb_dir(data_local: &Path, identifier: &str) -> PathBuf {
    data_local
        .join(identifier)
        .join("EBWebView")
        .join("Default")
        .join("Local Storage")
        .join("leveldb")
}

/// Reads each WebKit database that exists, newest layout first; a key found
/// in an earlier one wins.
#[cfg(not(windows))]
pub fn read_webkit_candidates(candidates: &[PathBuf]) -> LegacyRead {
    let mut result = LegacyRead::default();
    let wanted: Vec<&str> = LEGACY_LOCAL_STORAGE_KEYS.iter().map(|(_, k)| *k).collect();
    for path in candidates {
        if !path.is_file() {
            result.notes.push(format!("no file at {}", path.display()));
            continue;
        }
        let read = match webkit_sqlite::read_item_table(path, &wanted) {
            Ok(rows) => LegacyRead {
                notes: std::iter::once(format!(
                    "{}: {} legacy keys",
                    path.display(),
                    rows.items.len()
                ))
                .chain(rows.skipped)
                .collect(),
                storage: rows.items.into_iter().collect(),
            },
            Err(reason) => note_only(&format!("{}: {reason}", path.display())),
        };
        result.merge_missing(read);
    }
    result
}

fn note_only(note: &str) -> LegacyRead {
    LegacyRead {
        storage: LegacyLocalStorage::new(),
        notes: vec![note.to_string()],
    }
}
