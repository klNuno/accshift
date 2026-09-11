//! The app's own view of the OS secret store: the calls encrypted snapshots go
//! through, an index of every entry they created, and the collector that frees
//! the ones no snapshot file points at any more.
//!
//! Why an index exists. On Linux and macOS `os::encrypt_bytes` returns a UUID
//! naming a keyring entry that holds the real bytes, so an entry outlives the
//! file pointing at it whenever that file disappears without going through
//! `delete_bytes`: a crash between the two writes, a snapshot directory removed
//! from outside the app, or a bug like the one that removed a captured
//! directory without freeing its entries first. Finding those orphans would
//! mean listing the store, and nothing here can. The `keyring` crate has no
//! list call at all; Secret Service and the macOS Keychain can only search by
//! attributes this code never sets. So every id is written down as it is
//! created, and the collector compares that list against the tokens still on
//! disk.
//!
//! Windows keeps the ciphertext inline through DPAPI and owns no entry, so
//! recording and collecting are both no-ops there.
//!
//! Only the bytes API is indexed. `os::encrypt_secret` (the Roblox cookies, the
//! Steam API key) stores its token in the config file, which the collector
//! never reads, so those ids are deliberately absent from the index and can
//! never be taken for orphans.

use crate::context::AppContext;
use crate::error::AppError;
use crate::snapshot_crypto::{ENCRYPTED_HEADER, SNAPSHOT_PLATFORM_IDS};
use crate::storage;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

/// The real backend: `crate::os`, whose bytes API is DPAPI on Windows and the
/// keyring elsewhere.
#[cfg(not(test))]
mod backend {
    /// True when a stored secret lives in an OS keyring entry that has to be
    /// freed by hand. False under Windows DPAPI, where the ciphertext is the
    /// token and owns nothing.
    pub const KEYRING_BACKED: bool = cfg!(any(target_os = "linux", target_os = "macos"));

    pub use crate::os::{decrypt_bytes, delete_bytes, encrypt_bytes};
}

/// In-memory stand-in for the keyring, so a test can count what a capture
/// leaves behind on a machine whose real backend is DPAPI (which stores
/// nothing) or a headless keyring (which cannot be reached).
///
/// It mirrors the Linux/macOS backend exactly: storing returns a UUID naming
/// the entry, and the entry lives until something deletes it. The store is per
/// thread, because the test harness gives each test its own thread and a shared
/// store would make every entry count a race between them.
#[cfg(test)]
pub(crate) mod backend {
    use crate::error::AppError;
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub const KEYRING_BACKED: bool = true;

    thread_local! {
        static ENTRIES: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    }

    /// How many entries this thread's keyring holds right now.
    pub fn entry_count() -> usize {
        ENTRIES.with(|entries| entries.borrow().len())
    }

    pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let id = uuid::Uuid::new_v4().to_string();
        ENTRIES.with(|entries| entries.borrow_mut().insert(id.clone(), data.to_vec()));
        Ok(id.into_bytes())
    }

    pub fn decrypt_bytes(token: &[u8]) -> Result<Vec<u8>, AppError> {
        if token.is_empty() {
            return Ok(Vec::new());
        }
        let id = String::from_utf8_lossy(token).into_owned();
        ENTRIES
            .with(|entries| entries.borrow().get(&id).cloned())
            .ok_or_else(|| AppError::ProcessStart(format!("keyring: no entry named {id}")))
    }

    pub fn delete_bytes(token: &[u8]) -> Result<(), AppError> {
        if token.is_empty() {
            return Ok(());
        }
        let id = String::from_utf8_lossy(token).into_owned();
        ENTRIES.with(|entries| entries.borrow_mut().remove(&id));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The calls snapshots go through
// ---------------------------------------------------------------------------

/// Store bytes and hand back the token that reads them again. The token is the
/// ciphertext on Windows and a keyring entry id elsewhere; either way it is
/// what ends up in the snapshot file after the header.
pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let token = backend::encrypt_bytes(data)?;
    record(&token);
    Ok(token)
}

/// Read back what [`encrypt_bytes`] stored.
pub fn decrypt_bytes(token: &[u8]) -> Result<Vec<u8>, AppError> {
    backend::decrypt_bytes(token)
}

/// Free what [`encrypt_bytes`] stored. A missing entry is a success, so forget
/// flows stay idempotent.
///
/// The id is not struck from the index here: a single directory snapshot can
/// hold thousands of files, and rewriting the whole index per file would turn a
/// forget into minutes of IO. The next [`gc`] compacts it instead, off the
/// tokens actually on disk, which is the same answer with one rewrite.
pub fn delete_bytes(token: &[u8]) -> Result<(), AppError> {
    backend::delete_bytes(token)
}

// ---------------------------------------------------------------------------
// The index
// ---------------------------------------------------------------------------

/// Where the index lives, once the app has said which state directory is its
/// own. A process that never calls [`init`] still stores and reads secrets; its
/// entries are simply not collectable.
///
/// Per thread in a test build, so tests running side by side never share one
/// index file. Process-global everywhere else.
#[cfg(not(test))]
mod index_path {
    use std::path::PathBuf;
    use std::sync::Mutex;

    static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

    pub fn set(path: PathBuf) {
        *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    }

    pub fn get() -> Option<PathBuf> {
        PATH.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// See the module above.
#[cfg(test)]
mod index_path {
    use std::cell::RefCell;
    use std::path::PathBuf;

    thread_local! {
        static PATH: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    pub fn set(path: PathBuf) {
        PATH.with(|slot| *slot.borrow_mut() = Some(path));
    }

    pub fn get() -> Option<PathBuf> {
        PATH.with(|slot| slot.borrow().clone())
    }
}

/// Ids the index already held when this process started. Only these are ever
/// swept: an entry created by this process may well have been written before
/// its file was, and collecting it because the walk ran in between would leave
/// a snapshot that can no longer be decrypted.
#[cfg(not(test))]
static SWEEP_CANDIDATES: Mutex<Option<Vec<String>>> = Mutex::new(None);

// See above. Per thread in a test build for the same reason as the index path.
#[cfg(test)]
thread_local! {
    static SWEEP_CANDIDATES_TL: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(not(test))]
fn set_sweep_candidates(ids: Vec<String>) {
    *SWEEP_CANDIDATES.lock().unwrap_or_else(|e| e.into_inner()) = Some(ids);
}

#[cfg(not(test))]
fn take_sweep_candidates() -> Option<Vec<String>> {
    SWEEP_CANDIDATES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

#[cfg(test)]
fn set_sweep_candidates(ids: Vec<String>) {
    SWEEP_CANDIDATES_TL.with(|slot| *slot.borrow_mut() = Some(ids));
}

#[cfg(test)]
fn take_sweep_candidates() -> Option<Vec<String>> {
    SWEEP_CANDIDATES_TL.with(|slot| slot.borrow_mut().take())
}

/// Serialises the appends within this process. Two processes appending one
/// short line each is the remaining race, and an interleaved line is dropped by
/// the reader below rather than mistaken for an id.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// Point the index at the app's state directory, and take the snapshot of ids
/// [`gc`] is allowed to sweep.
///
/// Call it once, before anything can capture a snapshot. A no-op on Windows.
pub fn init(app: &dyn AppContext) {
    if !backend::KEYRING_BACKED {
        return;
    }
    let Ok(path) = storage::secrets_index_path(app) else {
        return;
    };
    let existing = read_index(&path);
    index_path::set(path);
    set_sweep_candidates(existing);
}

fn record(token: &[u8]) {
    if !backend::KEYRING_BACKED || token.is_empty() {
        return;
    }
    let Ok(id) = std::str::from_utf8(token) else {
        return;
    };
    let Some(path) = index_path::get() else {
        return;
    };
    let _guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{id}");
    }
}

/// The ids the index holds, in order, without duplicates. A line that is not a
/// plausible entry id is skipped: the file is append-only from possibly two
/// processes, so a torn line is a thing that can happen.
fn read_index(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    text.lines()
        .map(str::trim)
        .filter(|line| looks_like_entry_id(line))
        .filter(|line| seen.insert(line.to_string()))
        .map(str::to_string)
        .collect()
}

/// A UUID as `encrypt_bytes` writes it: 36 characters of hex and dashes.
fn looks_like_entry_id(line: &str) -> bool {
    line.len() == 36 && line.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn write_index(path: &Path, ids: &[String]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("dir={} error={e}", parent.display()))?;
    }
    let mut body = String::with_capacity(ids.len() * 37);
    for id in ids {
        body.push_str(id);
        body.push('\n');
    }
    fs::write(path, body).map_err(|e| format!("file={} error={e}", path.display()))
}

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

/// Outcome of one collection pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcStats {
    /// Entries no snapshot file pointed at any more, now freed.
    pub freed: usize,
    /// Entries that could not be freed. They stay indexed for the next run.
    pub failed: usize,
}

impl GcStats {
    /// True when the pass had anything to report. A clean store returns false,
    /// which is the normal case on every launch.
    pub fn touched_anything(&self) -> bool {
        self.freed > 0 || self.failed > 0
    }
}

/// Free every indexed keyring entry no snapshot file points at, then compact
/// the index down to what is actually live.
///
/// A no-op on Windows, and a no-op in a process that never called [`init`].
/// The sweep is abandoned rather than guessed at when the snapshot store cannot
/// be read end to end: an incomplete live set would look exactly like a store
/// full of orphans, and freeing those would leave every snapshot undecryptable.
pub fn gc(app: &dyn AppContext, report: &mut dyn FnMut(&str, String)) -> GcStats {
    let mut stats = GcStats::default();
    if !backend::KEYRING_BACKED {
        return stats;
    }
    let Some(path) = index_path::get() else {
        return stats;
    };
    let Some(candidates) = take_sweep_candidates() else {
        // Already swept once in this process. Running again would only risk the
        // entries created since.
        return stats;
    };
    if candidates.is_empty() {
        return stats;
    }

    let live = match live_tokens(app) {
        Ok(live) => live,
        Err(detail) => {
            report(
                "Could not read the snapshot store, skipped the keyring sweep",
                detail,
            );
            return stats;
        }
    };

    let mut freed: HashSet<&str> = HashSet::new();
    for id in &candidates {
        if live.contains(id) {
            continue;
        }
        match backend::delete_bytes(id.as_bytes()) {
            Ok(()) => {
                stats.freed += 1;
                freed.insert(id.as_str());
            }
            Err(e) => {
                stats.failed += 1;
                report(
                    "Could not free an orphaned keyring entry",
                    format!("error={e}"),
                );
            }
        }
    }

    // Compact off what the index holds now, so ids recorded while the walk ran
    // survive, then adopt any live token the index never knew about (a snapshot
    // captured by a build that predates this file).
    let mut keep: Vec<String> = read_index(&path)
        .into_iter()
        .filter(|id| !freed.contains(id.as_str()))
        .collect();
    let known: HashSet<String> = keep.iter().cloned().collect();
    let mut adopted: Vec<String> = live.into_iter().filter(|id| !known.contains(id)).collect();
    adopted.sort();
    keep.append(&mut adopted);
    if let Err(detail) = write_index(&path, &keep) {
        report("Could not rewrite the keyring entry index", detail);
    }
    stats
}

/// Every token the snapshot store still points at, across all platforms.
fn live_tokens(app: &dyn AppContext) -> Result<HashSet<String>, String> {
    let mut out = HashSet::new();
    for platform_id in SNAPSHOT_PLATFORM_IDS {
        let dir = storage::platform_snapshots_dir(app, platform_id)
            .map_err(|detail| format!("platform={platform_id} error={detail}"))?;
        collect_tokens(&dir, &mut out)?;
    }
    Ok(out)
}

fn collect_tokens(dir: &Path, out: &mut HashSet<String>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A platform that never captured anything has no directory.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("dir={} error={e}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir={} error={e}", dir.display()))?;
        if crate::fs_utils::is_reparse_point(&entry) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_tokens(&path, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let data = fs::read(&path).map_err(|e| format!("file={} error={e}", path.display()))?;
        let Some(token) = data.strip_prefix(ENCRYPTED_HEADER) else {
            // Legacy plaintext, it owns no entry.
            continue;
        };
        if let Ok(id) = std::str::from_utf8(token) {
            out.insert(id.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_crypto::{encrypted_copy_file, ENCRYPTED_HEADER};
    use std::path::PathBuf;

    struct TempCtx {
        root: PathBuf,
    }

    impl AppContext for TempCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-secrets-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// Captures one file into the platform's snapshot directory the way the
    /// engine does, and returns the entry id it now owns.
    fn capture(ctx: &TempCtx, name: &str, body: &[u8]) -> String {
        let source = ctx.root.join("live").join(name);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, body).unwrap();
        let dest = storage::platform_snapshots_dir(ctx, "gog")
            .unwrap()
            .join(name);
        encrypted_copy_file(&source, &dest).unwrap();
        let stored = fs::read(&dest).unwrap();
        String::from_utf8(stored[ENCRYPTED_HEADER.len()..].to_vec()).unwrap()
    }

    #[test]
    fn an_id_is_indexed_when_the_entry_is_created() {
        let root = scratch("index-record");
        let ctx = TempCtx { root: root.clone() };
        init(&ctx);

        let id = capture(&ctx, "session.json", b"one");

        let index = read_index(&storage::secrets_index_path(&ctx).unwrap());
        assert_eq!(index, vec![id]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_frees_the_entry_of_a_snapshot_file_that_is_gone() {
        let root = scratch("gc-orphan");
        let ctx = TempCtx { root: root.clone() };
        init(&ctx);

        let kept = capture(&ctx, "kept.json", b"kept");
        let orphaned = capture(&ctx, "orphaned.json", b"orphaned");
        assert_eq!(backend::entry_count(), 2);

        // The file disappears without going through delete_bytes, which is
        // exactly the shape of the leak this collector exists for.
        let snapshots = storage::platform_snapshots_dir(&ctx, "gog").unwrap();
        fs::remove_file(snapshots.join("orphaned.json")).unwrap();

        // The sweep candidates are taken at init, so the entries this test just
        // created are only eligible after a second init.
        init(&ctx);
        let mut failures: Vec<String> = Vec::new();
        let stats = gc(&ctx, &mut |message, detail| {
            failures.push(format!("{message} ({detail})"))
        });

        assert_eq!(failures, Vec::<String>::new());
        assert_eq!(
            stats,
            GcStats {
                freed: 1,
                failed: 0
            }
        );
        assert_eq!(backend::entry_count(), 1);
        assert!(decrypt_bytes(kept.as_bytes()).is_ok());
        assert!(decrypt_bytes(orphaned.as_bytes()).is_err());
        assert_eq!(
            read_index(&storage::secrets_index_path(&ctx).unwrap()),
            vec![kept]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_keeps_every_entry_a_snapshot_still_points_at() {
        let root = scratch("gc-live");
        let ctx = TempCtx { root: root.clone() };
        init(&ctx);

        capture(&ctx, "one.json", b"one");
        capture(&ctx, "two.json", b"two");

        init(&ctx);
        let stats = gc(&ctx, &mut |_, _| {});

        assert_eq!(stats, GcStats::default());
        assert_eq!(backend::entry_count(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_does_nothing_when_the_snapshot_store_cannot_be_read() {
        // An unreadable store looks exactly like a store full of orphans, and
        // acting on that would free the entries of every snapshot the user has.
        let root = scratch("gc-unreadable");
        let ctx = TempCtx { root: root.clone() };
        init(&ctx);
        capture(&ctx, "one.json", b"one");

        // A file where the snapshot directory of another platform belongs makes
        // the walk fail with something other than "not found".
        let jagex = storage::platform_snapshots_dir(&ctx, "jagex").unwrap();
        fs::create_dir_all(jagex.parent().unwrap()).unwrap();
        fs::write(&jagex, b"not a directory").unwrap();

        init(&ctx);
        let mut messages: Vec<String> = Vec::new();
        let stats = gc(&ctx, &mut |message, _| messages.push(message.to_string()));

        assert_eq!(stats, GcStats::default());
        assert_eq!(backend::entry_count(), 1);
        assert_eq!(
            messages,
            vec!["Could not read the snapshot store, skipped the keyring sweep"]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_torn_index_line_is_not_taken_for_an_entry_id() {
        let root = scratch("index-torn");
        let path = root.join("secret-entries.txt");
        fs::write(
            &path,
            "0f9d4a2b-1c3e-4d5f-8a7b-6c5d4e3f2a1b\nhalf-a-line\n\n",
        )
        .unwrap();
        assert_eq!(
            read_index(&path),
            vec!["0f9d4a2b-1c3e-4d5f-8a7b-6c5d4e3f2a1b"]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
