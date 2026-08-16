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

#[cfg(windows)]
mod imp {
    use super::RegistryHive;
    use crate::os::registry::{self, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::HKEY;

    fn hive(hive: RegistryHive) -> HKEY {
        match hive {
            RegistryHive::CurrentUser => HKEY_CURRENT_USER,
            RegistryHive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
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
}

pub use imp::{delete, read, write};
