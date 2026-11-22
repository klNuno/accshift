//! Real filesystem failures for tests, without an injection seam: an entry
//! a folder lists but whose metadata cannot be read, and a file the system
//! refuses to read until it is released.

use std::fs;
use std::path::{Path, PathBuf};

/// Makes `path` a symbolic link to itself: its folder lists it, while its
/// metadata and its bytes fail with a loop error, not `NotFound`. `false`
/// when this system does not let the test make a link (Windows without the
/// symbolic link privilege), and the caller skips the case.
pub fn link_to_itself(path: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(path, path);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(path, path);
    match made {
        Ok(()) => true,
        Err(error) => {
            eprintln!("no symbolic link here ({error}): the case is skipped");
            false
        }
    }
}

/// A file holding its bytes, which no read gets while this lives: Windows
/// keeps it open with no sharing, Unix takes every permission away (and,
/// for root, whom permissions do not stop, makes it a link to itself).
/// Dropped, the file reads again with the same bytes.
pub struct Unreadable {
    path: PathBuf,
    body: Vec<u8>,
    #[cfg(windows)]
    held: Option<fs::File>,
}

impl Unreadable {
    pub fn new(path: &Path, body: &[u8]) -> Self {
        fs::write(path, body).unwrap();
        #[cfg(windows)]
        let held = {
            use std::os::windows::fs::OpenOptionsExt;
            Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(path)
                    .unwrap(),
            )
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            if fs::read(path).is_ok() {
                fs::remove_file(path).unwrap();
                assert!(link_to_itself(path));
            }
        }
        assert!(fs::read(path).is_err(), "{} still reads", path.display());
        Self {
            path: path.to_path_buf(),
            body: body.to_vec(),
            #[cfg(windows)]
            held,
        }
    }
}

impl Drop for Unreadable {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.held.take());
        #[cfg(unix)]
        {
            let _ = fs::remove_file(&self.path);
            let _ = fs::write(&self.path, &self.body);
        }
        debug_assert!(fs::read(&self.path).is_ok_and(|bytes| bytes == self.body));
    }
}
