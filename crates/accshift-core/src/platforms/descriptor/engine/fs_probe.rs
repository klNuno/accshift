//! File system probes: log tails, freshness and content checks.

#[allow(unused_imports)]
use super::*;

/// Reads the last `tail_bytes` of a file the launcher keeps open.
///
/// Shared access is the point on Windows: the launcher holds its log open, and
/// an ordinary open would simply fail. The tail is decoded lossily so a cut
/// through a multi-byte character cannot fail the read, and only the end is
/// read because the log runs to megabytes and the sign-in sits at the bottom.
pub(super) fn read_log_tail(path: &Path, tail_bytes: u64) -> Option<String> {
    read_log_window(path, 0, tail_bytes).map(|(_start, text)| text)
}

/// The last `tail_bytes` of `path`, never starting before `not_before`.
///
/// The returned offset is where the string begins. A switch records the log's
/// length and only lines past it are new; a tail that would begin earlier is
/// pulled forward to that offset so an old identity line cannot sneak in.
pub(super) fn read_log_window(
    path: &Path,
    not_before: u64,
    tail_bytes: u64,
) -> Option<(u64, String)> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = open_log(path)?;
    let len = file.metadata().ok()?.len();
    if not_before > len {
        return Some((len, String::new()));
    }
    let start = len.saturating_sub(tail_bytes).max(not_before);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buffer = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut buffer).ok()?;
    Some((start, String::from_utf8_lossy(&buffer).into_owned()))
}

/// One byte, used to see whether an offset landed on a line boundary.
pub(super) fn read_log_byte(path: &Path, at: u64) -> Option<u8> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = open_log(path)?;
    file.seek(SeekFrom::Start(at)).ok()?;
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte).ok()?;
    Some(byte[0])
}

/// Bytes hashed by [`log_mark`], ending at the recorded offset.
const LOG_MARK_BYTES: u64 = 256;

/// Fingerprint of the bytes just before `end`. An append-only log keeps them,
/// so a different value at the same offset means the file was rewritten.
/// FNV-1a, written out because the value is stored in the config and must
/// not change with the toolchain.
pub(super) fn log_mark(path: &Path, end: u64) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = open_log(path)?;
    if file.metadata().ok()?.len() < end {
        return None;
    }
    let start = end.saturating_sub(LOG_MARK_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buffer = vec![0_u8; (end - start) as usize];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    }))
}

fn open_log(path: &Path) -> Option<fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
            .open(path)
            .ok()
    }
    #[cfg(not(windows))]
    {
        fs::File::open(path).ok()
    }
}

/// True when a path holds material written within `window_ms`. A stale
/// timestamp means the launcher never flushed the new session, so capturing it
/// would store the previous account's material.
///
/// A file qualifies when it is non-empty and its own mtime is recent. A
/// directory is judged on the newest mtime anywhere below it: a launcher that
/// keeps its session in a store of many files rewrites some of them and leaves
/// the directory's own mtime untouched.
pub(super) fn file_is_fresh(path: &Path, window_ms: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.is_dir() {
        return dir_newest_modified(path)
            .is_some_and(|modified| written_within(modified, window_ms));
    }
    if meta.len() == 0 {
        return false;
    }
    let Ok(modified) = meta.modified() else {
        return true;
    };
    written_within(modified, window_ms)
}

/// A timestamp the clock cannot make sense of is not evidence of staleness, so
/// it counts as fresh rather than blocking a capture that is probably right.
pub(super) fn written_within(modified: SystemTime, window_ms: u64) -> bool {
    let Ok(elapsed) = modified.elapsed() else {
        return true;
    };
    (elapsed.as_millis() as u64) <= window_ms
}

/// The newest modification time of any file under `dir`, at any depth.
pub(super) fn dir_newest_modified(dir: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let candidate = if path.is_dir() {
            dir_newest_modified(&path)
        } else {
            fs::metadata(&path)
                .ok()
                .and_then(|meta| meta.modified().ok())
        };
        if let Some(time) = candidate {
            newest = Some(newest.map_or(time, |current| current.max(time)));
        }
    }
    newest
}

/// A non-empty file, or (when `recursive`) a directory holding one anywhere
/// below it.
pub(super) fn path_has_content(path: &Path, recursive: bool) -> bool {
    if path.is_dir() {
        return recursive && dir_has_nonempty_file(path);
    }
    fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
}

pub(super) fn dir_has_nonempty_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_nonempty_file(&path) {
                return true;
            }
        } else if fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
            return true;
        }
    }
    false
}
