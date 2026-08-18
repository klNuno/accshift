//! Registry access for descriptors, in a shape that compiles everywhere.
//!
//! Descriptors name a hive as data, so the engine cannot use the `winreg`
//! constants directly, and the engine itself is not Windows-only: a descriptor
//! may describe a Linux profile with no registry at all. On a non-Windows
//! build these become the honest answer, "there is no registry here".
//!
//! No sandbox check happens at run time: registry keys are literal strings in
//! the descriptor, never derived from the environment, so the roots check at
//! load time already covers every key that can ever be touched.

use super::schema::RegistryHive;

/// Renders a value's full location for messages and dry-run output.
pub fn display(hive: RegistryHive, key: &str, value: &str) -> String {
    format!("{}\\{key}\\{value}", hive.as_str())
}

/// Where Windows records installed programs. Both are scanned because a 32-bit
/// launcher on a 64-bit system registers under the redirected view.
#[cfg(windows)]
const UNINSTALL_ROOTS: &[&str] = &[
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

#[cfg(windows)]
mod imp {
    use super::RegistryHive;
    use crate::os::registry::{self, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::{RegKey, HKEY};

    fn hive(hive: RegistryHive) -> HKEY {
        match hive {
            RegistryHive::CurrentUser => HKEY_CURRENT_USER,
            RegistryHive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    /// The install path an uninstall entry records, found by the name the
    /// launcher displays in "Apps and features".
    ///
    /// Some launchers register no path of their own anywhere else, so this is
    /// the only place their install directory can be read from.
    pub fn uninstall_entry(display_name: &str, value: &str) -> Option<String> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        for root in super::UNINSTALL_ROOTS {
            let Ok(root_key) = hklm.open_subkey(root) else {
                continue;
            };
            for subkey_name in root_key.enum_keys().flatten() {
                let Ok(subkey) = root_key.open_subkey(&subkey_name) else {
                    continue;
                };
                let Ok(found) = subkey.get_value::<String, _>("DisplayName") else {
                    continue;
                };
                if found.trim() != display_name {
                    continue;
                }
                if let Ok(location) = subkey.get_value::<String, _>(value) {
                    if !location.trim().is_empty() {
                        return Some(location);
                    }
                }
            }
        }
        None
    }

    pub fn read(root: RegistryHive, key: &str, value: &str) -> Option<String> {
        registry::read_string(hive(root), key, value)
    }

    pub fn write(root: RegistryHive, key: &str, value: &str, data: &str) -> Result<(), String> {
        registry::write_string(hive(root), key, value, data)
    }

    pub fn delete(root: RegistryHive, key: &str, value: &str) {
        registry::delete_value(hive(root), key, value);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::RegistryHive;

    pub fn read(_root: RegistryHive, _key: &str, _value: &str) -> Option<String> {
        None
    }

    pub fn write(_root: RegistryHive, key: &str, value: &str, _data: &str) -> Result<(), String> {
        Err(format!(
            "Could not write registry value {key}\\{value}: this system has no registry"
        ))
    }

    pub fn delete(_root: RegistryHive, _key: &str, _value: &str) {}

    pub fn uninstall_entry(_display_name: &str, _value: &str) -> Option<String> {
        None
    }
}

pub use imp::{delete, read, uninstall_entry, write};
