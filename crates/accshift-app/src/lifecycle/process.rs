//! `tauri-plugin-process` 2.3.1 (`exit`, `restart`) on Tauri 2.11.5's
//! `process::restart` (src/process.rs lines 20-89). The spawn and the exit
//! go through [`ProcessControl`], so tests never relaunch or exit.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

/// `process::current_binary` (lines 20-27): the AppImage on Linux when
/// `APPIMAGE` is set, else the canonical current executable.
pub fn current_binary() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(appimage));
    }
    std::fs::canonicalize(std::env::current_exe()?)
}

/// `restart_macos_app` (lines 92-130): inside `X.app/Contents/MacOS`, the
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

/// `relaunch()`: `app.request_restart()`, which ends the event loop (the
/// plugins' `RunEvent::Exit` runs, so the single-instance mutex and window
/// go first: `before_exit`), then `process::restart`: start the binary with
/// this launch's arguments minus the first, then `exit(0)`. A spawn error is
/// only logged upstream; it is returned here and the exit still happens.
pub fn restart(
    control: &dyn ProcessControl,
    binary: std::io::Result<PathBuf>,
    args_os: &[OsString],
    before_exit: impl FnOnce(),
) -> Option<std::io::Error> {
    before_exit();
    let mut error = None;
    if let Ok(path) = binary {
        let program = relaunch_program(&path);
        if let Err(e) = control.spawn(&program, args_os.get(1..).unwrap_or_default()) {
            error = Some(e);
        }
    }
    control.exit(0);
    error
}

/// `exit(code)`: `app.exit(code)`, the same exit path with that code.
pub fn exit(control: &dyn ProcessControl, code: i32, before_exit: impl FnOnce()) {
    before_exit();
    control.exit(code);
}
