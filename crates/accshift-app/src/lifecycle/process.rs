//! Exit and relaunch. The spawn and the exit go through [`ProcessControl`],
//! so tests never relaunch or exit.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::deep_link;

/// The two process side effects.
pub trait ProcessControl {
    /// `Command::new(program).args(args).spawn()`.
    fn spawn(&self, program: &Path, args: &[OsString]) -> std::io::Result<()>;
    /// `std::process::exit(code)`. The real one never returns.
    fn exit(&self, code: i32);
}

/// The real one.
pub struct SystemProcess;

impl ProcessControl for SystemProcess {
    fn spawn(&self, program: &Path, args: &[OsString]) -> std::io::Result<()> {
        std::process::Command::new(program)
            .args(args)
            .spawn()
            .map(|_| ())
    }

    fn exit(&self, code: i32) {
        std::process::exit(code);
    }
}

/// The binary resolved on the first [`current_binary`] call, which the
/// updater config makes at startup.
static STARTUP_BINARY: OnceLock<PathBuf> = OnceLock::new();

/// The binary a relaunch starts: the AppImage on Linux when
/// `APPIMAGE` is set, else the canonical current executable. Kept from the
/// first call on: once a deb or rpm update has replaced the file,
/// `/proc/self/exe` names a deleted inode that no longer resolves.
pub fn current_binary() -> std::io::Result<PathBuf> {
    if let Some(path) = STARTUP_BINARY.get() {
        return Ok(path.clone());
    }
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(STARTUP_BINARY
            .get_or_init(|| PathBuf::from(appimage))
            .clone());
    }
    let path = resolve_binary(&std::env::current_exe()?)?;
    Ok(STARTUP_BINARY.get_or_init(|| path).clone())
}

/// The canonical form of `exe`. Linux reports a replaced executable as
/// `<path> (deleted)`; the package manager put the new binary at `<path>`,
/// which is what a relaunch must start.
pub fn resolve_binary(exe: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(exe).or_else(|error| {
        let replaced = exe
            .as_os_str()
            .as_encoded_bytes()
            .strip_suffix(b" (deleted)")
            // SAFETY: cut on an ASCII boundary of bytes from an `OsStr`.
            .map(|bytes| unsafe { OsStr::from_encoded_bytes_unchecked(bytes) });
        match replaced {
            Some(path) => std::fs::canonicalize(path),
            None => Err(error),
        }
    })
}

/// macOS relaunch target: inside `X.app/Contents/MacOS`, the
/// binary `Info.plist` names, which an update may have renamed.
#[cfg(target_os = "macos")]
fn macos_bundle_binary(current_binary: &Path) -> Option<PathBuf> {
    let macos_directory = current_binary.parent()?;
    if macos_directory.file_name()? != "MacOS" {
        return None;
    }
    let contents_directory = macos_directory.parent()?;
    if contents_directory.file_name()? != "Contents" {
        return None;
    }
    let info: plist::Dictionary = plist::from_file(contents_directory.join("Info.plist")).ok()?;
    let binary_name = info.get("CFBundleExecutable")?.as_string()?;
    Some(macos_directory.join(binary_name))
}

/// The program a relaunch starts.
pub fn relaunch_program(current_binary: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(bundle_binary) = macos_bundle_binary(current_binary) {
        return bundle_binary;
    }
    current_binary.to_path_buf()
}

/// The arguments an update relaunch passes on: this launch's `args_os`
/// without the executable, `--tray` (a restart the user asked for must come
/// back with a window) and any `accshift://` URL (its action already ran).
pub fn relaunch_args(args_os: &[OsString]) -> Vec<OsString> {
    args_os
        .iter()
        .skip(1)
        .filter(|arg| !is_launch_only(arg))
        .cloned()
        .collect()
}

fn is_launch_only(arg: &OsStr) -> bool {
    let Some(text) = arg.to_str() else {
        return false;
    };
    text == "--tray"
        || text
            .parse::<url::Url>()
            .is_ok_and(|url| deep_link::SCHEMES.contains(&url.scheme()))
}

/// The two ways the update relaunch moves the single-instance guard.
struct InstanceHooks {
    release: Box<dyn Fn() + Send + Sync>,
    restore: Box<dyn Fn() + Send + Sync>,
}

/// Set once the guard is acquired ([`set_instance_hooks`]).
static INSTANCE_HOOKS: OnceLock<InstanceHooks> = OnceLock::new();

/// Registers how to free this run's single-instance guard and how to take
/// it back ([`super::single_instance::SingleInstance::releaser`] and
/// [`super::single_instance::SingleInstance::restorer`]).
pub fn set_instance_hooks(
    release: impl Fn() + Send + Sync + 'static,
    restore: impl Fn() + Send + Sync + 'static,
) {
    let _ = INSTANCE_HOOKS.set(InstanceHooks {
        release: Box::new(release),
        restore: Box::new(restore),
    });
}

/// Frees the guard registered with [`set_instance_hooks`], if any.
pub fn release_instance() {
    if let Some(hooks) = INSTANCE_HOOKS.get() {
        (hooks.release)();
    }
}

/// Takes the guard registered with [`set_instance_hooks`] back, if any.
pub fn restore_instance() {
    if let Some(hooks) = INSTANCE_HOOKS.get() {
        (hooks.restore)();
    }
}

/// The update relaunch: start `binary` with `args`, then exit. The
/// single-instance guard goes first (`release_instance`): `exit` skips
/// `Drop`, and a new process that still found it would hand its launch to
/// this one and quit, leaving nothing running. Nothing exits when no new
/// process started: the guard is taken back (`restore_instance`), the error
/// comes back and the app stays open as the single instance.
pub fn restart(
    control: &dyn ProcessControl,
    binary: std::io::Result<PathBuf>,
    args: &[OsString],
    release_instance: impl FnOnce(),
    restore_instance: impl FnOnce(),
    before_exit: impl FnOnce(),
) -> std::io::Result<()> {
    let program = relaunch_program(&binary?);
    release_instance();
    if let Err(error) = control.spawn(&program, args) {
        restore_instance();
        return Err(error);
    }
    before_exit();
    control.exit(0);
    Ok(())
}

/// `exit(code)`: `app.exit(code)`, the same exit path with that code.
pub fn exit(control: &dyn ProcessControl, code: i32, before_exit: impl FnOnce()) {
    before_exit();
    control.exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_binary_resolves_to_the_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("accshift-gui");
        std::fs::write(&binary, b"new").unwrap();
        let mut deleted = binary.clone().into_os_string();
        deleted.push(" (deleted)");
        let expected = std::fs::canonicalize(&binary).unwrap();
        assert_eq!(resolve_binary(Path::new(&deleted)).unwrap(), expected);
        assert_eq!(resolve_binary(&binary).unwrap(), expected);
        assert!(resolve_binary(&dir.path().join("missing (deleted)")).is_err());
    }

    #[test]
    fn the_current_binary_is_kept_from_the_first_call() {
        let first = current_binary().unwrap();
        assert_eq!(STARTUP_BINARY.get(), Some(&first));
        assert_eq!(current_binary().unwrap(), first);
    }

    #[test]
    fn the_relaunch_drops_the_tray_flag_and_deep_links() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(relaunch_args(&args(&["accshift-gui", "--tray"])).is_empty());
        assert!(relaunch_args(&args(&["accshift-gui", "accshift://switch/steam/x"])).is_empty());
        assert!(relaunch_args(&args(&["accshift-gui", "ACCSHIFT://switch/steam/x"])).is_empty());
        assert_eq!(
            relaunch_args(&args(&["accshift-gui", "--flag", "--tray", "some space"])),
            args(&["--flag", "some space"])
        );
        assert_eq!(
            relaunch_args(&args(&["accshift-gui", "https://example.invalid"])),
            args(&["https://example.invalid"])
        );
    }
}
