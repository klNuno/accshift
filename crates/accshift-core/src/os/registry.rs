//! Windows registry helpers for the string get / set / delete pattern the
//! platform modules share.
//!
//! Bespoke key enumeration (Battle.net / Epic install discovery) stays in the
//! platform modules with raw `winreg`; simple value access goes through here.
//! Steam's auto-login registry access lives in `os::windows`.

use winreg::enums::KEY_WRITE;
use winreg::{RegKey, HKEY};

pub use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

/// Read a string value. A missing key / value, a wrong type, or an
/// empty-after-trim value all read as `None`; the result is trimmed.
pub fn read_string(root: HKEY, key_path: &str, value_name: &str) -> Option<String> {
    let key = RegKey::predef(root).open_subkey(key_path).ok()?;
    let value: String = key.get_value(value_name).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Read a string value, distinguishing "not set" (`Ok(None)`) from real
/// errors (permissions, wrong type). The value is returned untrimmed.
pub fn try_read_raw_string(
    root: HKEY,
    key_path: &str,
    value_name: &str,
) -> Result<Option<String>, String> {
    let key = match RegKey::predef(root).open_subkey(key_path) {
        Ok(key) => key,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Could not open registry key {key_path}: {e}")),
    };
    match key.get_value::<String, _>(value_name) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "Could not read registry value {key_path}\\{value_name}: {e}"
        )),
    }
}

/// Create-or-open `key_path` and set `value_name` to `data`.
pub fn write_string(
    root: HKEY,
    key_path: &str,
    value_name: &str,
    data: &str,
) -> Result<(), String> {
    let (key, _) = RegKey::predef(root)
        .create_subkey(key_path)
        .map_err(|e| format!("Could not open registry key {key_path}: {e}"))?;
    key.set_value(value_name, &data)
        .map_err(|e| format!("Could not write registry value {key_path}\\{value_name}: {e}"))?;
    Ok(())
}

/// Delete a value, best effort: a missing key or value is ignored.
pub fn delete_value(root: HKEY, key_path: &str, value_name: &str) {
    if let Ok(key) = RegKey::predef(root).open_subkey_with_flags(key_path, KEY_WRITE) {
        let _ = key.delete_value(value_name);
    }
}
