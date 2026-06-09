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

fn keyring_set_password(id: &str, value: &str) -> Result<(), keyring::Error> {
    entry(id)?.set_password(value)
}

fn keyring_get_password(id: &str) -> Result<String, keyring::Error> {
    entry(id)?.get_password()
}

fn keyring_set_bytes(id: &str, value: &[u8]) -> Result<(), keyring::Error> {
    entry(id)?.set_secret(value)
}

fn keyring_get_bytes(id: &str) -> Result<Vec<u8>, keyring::Error> {
    entry(id)?.get_secret()
}

// The "ciphertext" returned by `encrypt_*` is a UUID that points at the real
// secret stored in the OS keyring. Same threat model as DPAPI: secrets are
// bound to the user session, moving the JSON to another machine won't decrypt.

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.is_empty() {
        return Ok(String::new());
    }
    let id = uuid::Uuid::new_v4().to_string();
    keyring_set_password(&id, secret).map_err(secret_error)?;
    Ok(id)
}

pub fn decrypt_secret(token: &str) -> Result<String, AppError> {
    if token.is_empty() {
        return Ok(String::new());
    }
    keyring_get_password(token).map_err(secret_error)
}

pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let id = uuid::Uuid::new_v4().to_string();
    keyring_set_bytes(&id, data).map_err(secret_error)?;
    Ok(id.into_bytes())
}

pub fn decrypt_bytes(token: &[u8]) -> Result<Vec<u8>, AppError> {
    if token.is_empty() {
        return Ok(Vec::new());
    }
    let id = std::str::from_utf8(token).map_err(|e| secret_error(e.to_string()))?;
    keyring_get_bytes(id).map_err(secret_error)
}
