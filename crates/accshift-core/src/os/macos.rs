use crate::error::AppError;
use crate::platforms::steam::vdf::vdf_set_nested_value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        Err(AppError::RegistryRead(
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

fn registry_vdf_path() -> Result<PathBuf, AppError> {
    let dir =
        steam_support_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;
    Ok(dir.join("registry.vdf"))
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
    // On macOS the app bundle sits next to the data dir. `open -a Steam` is
    // the canonical way to launch; fall back to the binary if the bundle is
    // missing (e.g. manual install).
    let bundle = steam_path.join("Steam.app");
    if bundle.exists() {
        let mut cmd = Command::new("open");
        cmd.arg("-a").arg(&bundle);
        if !launch_options.is_empty() {
            cmd.arg("--args").args(launch_options);
        }
        cmd.spawn()
            .map_err(|e| AppError::ProcessStart(e.to_string()))?;
        return Ok(());
    }

    let binary = steam_path
        .join("Steam.app/Contents/MacOS")
        .join(steam_executable_name());
    Command::new(binary)
        .args(launch_options)
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

pub fn select_folder(_title: &str) -> Result<String, AppError> {
    Err(unsupported("Folder picker"))
}

pub fn select_file(_title: &str, _filter: &str) -> Result<String, AppError> {
    Err(unsupported("File picker"))
}
