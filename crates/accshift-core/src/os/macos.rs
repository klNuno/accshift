use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// Secrets live in the Keychain via `keyring`. Shared implementation in
// os/secrets.rs.
pub use super::secrets::{decrypt_bytes, decrypt_secret, encrypt_bytes, encrypt_secret};

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn steam_support_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join("Library/Application Support/Steam"))
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    let path =
        steam_support_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;
    if path.join("config").join("loginusers.vdf").exists() {
        Ok(path)
    } else {
        Err(AppError::RegistryOpen(
            "Steam installation not found under ~/Library/Application Support/Steam".into(),
        ))
    }
}

pub fn steam_executable_name() -> &'static str {
    "steam_osx"
}

pub fn steam_process_name() -> &'static str {
    "steam_osx"
}

pub fn steam_web_helper_process_name() -> &'static str {
    "Steam Helper"
}

pub fn steam_htmlcache_path() -> Result<PathBuf, AppError> {
    let dir =
        steam_support_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;
    Ok(dir.join("config").join("htmlcache"))
}

fn registry_vdf_path() -> Result<PathBuf, AppError> {
    let dir =
        steam_support_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;
    Ok(dir.join("registry.vdf"))
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    super::steam_registry::get_auto_login_user(&registry_vdf_path()?)
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    super::steam_registry::set_auto_login_user(&registry_vdf_path()?, username)
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    super::steam_registry::clear_auto_login_user(&registry_vdf_path()?)
}

pub fn kill_and_relaunch_steam_elevated(
    steam_path: &Path,
    launch_options: &[String],
) -> Result<(), AppError> {
    let _ = super::common::kill_process(steam_process_name());
    let _ = super::common::kill_process(steam_web_helper_process_name());
    super::common::wait_for_process_exit(steam_process_name(), 10_000);
    super::common::wait_for_process_exit(steam_web_helper_process_name(), 5_000);
    launch_steam(steam_path, false, launch_options)
}

pub fn request_steam_shutdown(steam_path: &Path) -> bool {
    // Same mechanism as Windows: invoke the client binary with -shutdown,
    // which forwards the request to the running instance and exits. The
    // alternatives are dead ends: current Steam builds ignore steam://exit
    // (while `open` still reports success), and a Quit Apple event is
    // rejected with "user cancelled" (-128).
    let bundled = steam_path
        .join("Steam.AppBundle/Steam/Contents/MacOS")
        .join(steam_executable_name());
    let binary = if bundled.exists() {
        bundled
    } else {
        // Bootstrapper bundle ships the same binary.
        PathBuf::from("/Applications/Steam.app/Contents/MacOS/steam_osx")
    };
    let child = Command::new(binary)
        .arg("-shutdown")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };

    // The messenger is itself named steam_osx and takes a few seconds to
    // deliver (it partially boots first). While it lives — or lingers as an
    // unreaped zombie afterwards — the process-exit wait in the caller counts
    // it as Steam still running. Reap it before returning, with a watchdog
    // thread so a hung messenger cannot block the switch forever.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = child.wait();
        let _ = tx.send(());
    });
    // Past the timeout, assume the request was delivered; the detached
    // thread still reaps the messenger whenever it exits.
    let _ = rx.recv_timeout(std::time::Duration::from_secs(15));
    true
}

pub fn launch_steam(
    steam_path: &Path,
    _run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    // Steam.app lives in /Applications (or wherever the user dropped it),
    // never inside the data dir that `steam_path` points to. Let Launch
    // Services resolve it by name; `open` exits non-zero when the app is
    // missing, so wait for its status instead of fire-and-forget.
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg("Steam");
    if !launch_options.is_empty() {
        cmd.arg("--args").args(launch_options);
    }
    match cmd.status() {
        Ok(status) if status.success() => return Ok(()),
        _ => {}
    }

    // Fall back to the self-updated bundle Steam keeps inside its data dir.
    let binary = steam_path
        .join("Steam.AppBundle/Steam/Contents/MacOS")
        .join(steam_executable_name());
    Command::new(binary)
        .args(launch_options)
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

fn run_osascript(script: &str) -> Result<String, AppError> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    if !output.status.success() {
        // User cancelled (osascript exits 1). An empty string would be read
        // downstream as "clear the custom path", so signal the cancellation
        // explicitly and let the caller leave the path alone.
        return Err(AppError::Cancelled);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn select_folder(title: &str) -> Result<String, AppError> {
    let escaped = title.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("POSIX path of (choose folder with prompt \"{escaped}\")");
    run_osascript(&script)
}

pub fn select_file(title: &str, _filter: &str) -> Result<String, AppError> {
    let escaped = title.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("POSIX path of (choose file with prompt \"{escaped}\")");
    run_osascript(&script)
}
