use crate::error::AppError;
use crate::platforms::steam::vdf::vdf_set_nested_value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Secrets — no portable backend yet on Linux. Keyring-based support will
// land in a follow-up PR.
// ---------------------------------------------------------------------------

fn unsupported(feature: &str) -> AppError {
    AppError::UnsupportedOperatingSystem(format!(
        "{feature} is not supported on this operating system"
    ))
}

pub fn encrypt_secret(_secret: &str) -> Result<String, AppError> {
    Err(unsupported("Secret storage"))
}

pub fn decrypt_secret(_secret: &str) -> Result<String, AppError> {
    Err(unsupported("Secret storage"))
}

pub fn encrypt_bytes(_data: &[u8]) -> Result<Vec<u8>, AppError> {
    Err(unsupported("Secret storage"))
}

pub fn decrypt_bytes(_data: &[u8]) -> Result<Vec<u8>, AppError> {
    Err(unsupported("Secret storage"))
}

// ---------------------------------------------------------------------------
// Steam discovery
// ---------------------------------------------------------------------------

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn candidate_steam_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(xdg).join("Steam"));
    }

    if let Some(home) = home_dir() {
        // Native install, newer layout.
        candidates.push(home.join(".local/share/Steam"));
        // Legacy symlink created by the Steam runtime.
        candidates.push(home.join(".steam/steam"));
        // Flatpak.
        candidates.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        candidates.push(home.join(".var/app/com.valvesoftware.Steam/.steam/steam"));
    }

    candidates
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    for candidate in candidate_steam_paths() {
        if candidate.join("config").join("loginusers.vdf").exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::RegistryRead(
        "Steam installation not found under ~/.local/share/Steam, ~/.steam/steam, or Flatpak paths"
            .into(),
    ))
}

pub fn steam_executable_name() -> &'static str {
    "steam"
}

pub fn steam_process_name() -> &'static str {
    "steam"
}

pub fn steam_web_helper_process_name() -> &'static str {
    "steamwebhelper"
}

// ---------------------------------------------------------------------------
// AutoLoginUser via Steam's registry.vdf
// ---------------------------------------------------------------------------

fn registry_vdf_path() -> Result<PathBuf, AppError> {
    let home = home_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;

    // Prefer the Flatpak path when present, otherwise the native one.
    let flatpak = home.join(".var/app/com.valvesoftware.Steam/.steam/registry.vdf");
    if flatpak.exists() {
        return Ok(flatpak);
    }
    Ok(home.join(".steam/registry.vdf"))
}

const REGISTRY_PATH: &[&str] = &["HKCU", "Software", "Valve", "Steam", "AutoLoginUser"];
const REMEMBER_PATH: &[&str] = &["HKCU", "Software", "Valve", "Steam", "RememberPassword"];

pub fn get_auto_login_user() -> Result<String, AppError> {
    let path = registry_vdf_path()?;
    let content = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    Ok(extract_registry_value(&content, "AutoLoginUser").unwrap_or_default())
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    let path = registry_vdf_path()?;
    let existing = fs::read_to_string(&path).unwrap_or_else(|_| empty_registry_vdf());
    let updated = vdf_set_nested_value(&existing, REGISTRY_PATH, username);
    let updated = vdf_set_nested_value(&updated, REMEMBER_PATH, "1");
    fs::write(&path, updated).map_err(|e| AppError::RegistryWrite(e.to_string()))
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    let path = registry_vdf_path()?;
    let existing = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    let updated = vdf_set_nested_value(&existing, REGISTRY_PATH, "");
    fs::write(&path, updated).map_err(|e| AppError::RegistryWrite(e.to_string()))
}

fn extract_registry_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        let parts: Vec<&str> = trimmed.split('"').collect();
        if parts.len() >= 4 && parts[1].eq_ignore_ascii_case(key) {
            return Some(parts[3].to_string());
        }
    }
    None
}

fn empty_registry_vdf() -> String {
    "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n".to_string()
}

// ---------------------------------------------------------------------------
// Process launch — no UAC concept on Linux, so `run_as_admin` is ignored.
// Callers that really need root should invoke pkexec themselves.
// ---------------------------------------------------------------------------

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

pub fn launch_steam(
    steam_path: &Path,
    _run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    // The user-visible launcher is the `steam` shell script sitting next to
    // the data dir. Native installs place it in /usr/bin too, but using the
    // known path avoids PATH lookups.
    let steam_exe = resolve_steam_launcher(steam_path);
    Command::new(steam_exe)
        .args(launch_options)
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

fn resolve_steam_launcher(steam_path: &Path) -> PathBuf {
    let candidate = steam_path.join("steam.sh");
    if candidate.exists() {
        return candidate;
    }
    PathBuf::from("steam")
}

// ---------------------------------------------------------------------------
// File / folder pickers — deferred. Tauri's dialog plugin covers the GUI side
// and the CLI doesn't need pickers.
// ---------------------------------------------------------------------------

pub fn select_folder(_title: &str) -> Result<String, AppError> {
    Err(unsupported("Folder picker"))
}

pub fn select_file(_title: &str, _filter: &str) -> Result<String, AppError> {
    Err(unsupported("File picker"))
}
