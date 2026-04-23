//! Shared secret-storage helpers for Linux / macOS.
//!
//! Backend is the `keyring` crate, which maps to the Secret Service on Linux
//! (GNOME Keyring / KWallet) and the Keychain on macOS. We wrap the ugly bits
//! so linux.rs and macos.rs can stay readable.
//!
//! The service identifier is the same the CLI and the GUI use for their app
//! dirs, so entries show up grouped in keyring UIs.

use crate::error::AppError;

const SERVICE: &str = "com.accshift.desktop";

pub fn unsupported(feature: &str) -> AppError {
    AppError::UnsupportedOperatingSystem(format!(
        "{feature} is not supported on this operating system"
    ))
}

pub fn secret_error(err: impl std::fmt::Display) -> AppError {
    AppError::ProcessStart(format!("keyring: {err}"))
}

fn entry(id: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, id)
}

pub fn keyring_set_password(id: &str, value: &str) -> Result<(), keyring::Error> {
    entry(id)?.set_password(value)
}

pub fn keyring_get_password(id: &str) -> Result<String, keyring::Error> {
    entry(id)?.get_password()
}

pub fn keyring_set_bytes(id: &str, value: &[u8]) -> Result<(), keyring::Error> {
    entry(id)?.set_secret(value)
}

pub fn keyring_get_bytes(id: &str) -> Result<Vec<u8>, keyring::Error> {
    entry(id)?.get_secret()
}
