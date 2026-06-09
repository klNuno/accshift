//! Steam `registry.vdf` helpers shared by the Linux and macOS backends.
//!
//! On these platforms Steam stores what Windows keeps in HKCU inside a
//! `registry.vdf` file. The backends differ only in where that file lives;
//! everything else is identical and lives here.

use crate::error::AppError;
use crate::platforms::steam::vdf::vdf_set_nested_value;
use std::fs;
use std::path::Path;

const REGISTRY_PATH: &[&str] = &["HKCU", "Software", "Valve", "Steam", "AutoLoginUser"];
const REMEMBER_PATH: &[&str] = &["HKCU", "Software", "Valve", "Steam", "RememberPassword"];

pub fn get_auto_login_user(path: &Path) -> Result<String, AppError> {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    Ok(extract_registry_value(&content, "AutoLoginUser").unwrap_or_default())
}

pub fn set_auto_login_user(path: &Path, username: &str) -> Result<(), AppError> {
    // Only fall back to the empty template when the file genuinely does not
    // exist. Any other read error (permissions, transient lock) must not
    // silently replace the user's registry.vdf with an empty template.
    let existing = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => empty_registry_vdf(),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    let updated = vdf_set_nested_value(&existing, REGISTRY_PATH, username);
    let updated = vdf_set_nested_value(&updated, REMEMBER_PATH, "1");
    crate::storage::write_bytes_atomic(path, updated.as_bytes()).map_err(AppError::RegistryWrite)
}

pub fn clear_auto_login_user(path: &Path) -> Result<(), AppError> {
    let existing = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    let updated = vdf_set_nested_value(&existing, REGISTRY_PATH, "");
    crate::storage::write_bytes_atomic(path, updated.as_bytes()).map_err(AppError::RegistryWrite)
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
