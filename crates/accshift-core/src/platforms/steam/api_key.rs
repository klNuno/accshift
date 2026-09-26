//! The Steam Web API key: encrypted at rest, rotated without leaving stale secrets.

#[allow(unused_imports)]
use super::*;

pub(super) fn encrypt_api_key(api_key: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Ok(String::new());
    }
    os::encrypt_secret(api_key).map_err(|e| e.to_string())
}

pub(super) fn decrypt_api_key(encrypted_api_key: &str) -> Result<String, String> {
    if encrypted_api_key.trim().is_empty() {
        return Ok(String::new());
    }
    os::decrypt_secret(encrypted_api_key).map_err(|e| e.to_string())
}

pub(super) enum SecretPersistence {
    Replaced(String),
    Unused,
}

pub(super) fn rotate_secret_with<Encrypt, Persist, Delete, Warn>(
    plaintext: &str,
    encrypt: Encrypt,
    persist: Persist,
    mut delete: Delete,
    mut warn: Warn,
) -> Result<bool, String>
where
    Encrypt: FnOnce(&str) -> Result<String, String>,
    Persist: FnOnce(String) -> Result<SecretPersistence, String>,
    Delete: FnMut(&str) -> Result<(), String>,
    Warn: FnMut(&'static str, String),
{
    let plaintext = plaintext.trim();
    let replacement = if plaintext.is_empty() {
        String::new()
    } else {
        encrypt(plaintext)?
    };

    let previous = match persist(replacement.clone()) {
        Ok(SecretPersistence::Replaced(previous)) => previous,
        Ok(SecretPersistence::Unused) => {
            // A concurrent writer replaced the legacy value while this token
            // was being prepared. It never became active and has no owner.
            if !replacement.is_empty() {
                if let Err(cleanup_error) = delete(&replacement) {
                    warn("unused", cleanup_error);
                }
            }
            return Ok(false);
        }
        Err(error) => {
            // Encryption may already have created a keyring entry. If the
            // config write fails, the new token has no owner and must be
            // removed while the previous token stays untouched.
            if !replacement.is_empty() {
                if let Err(cleanup_error) = delete(&replacement) {
                    warn("replacement", cleanup_error);
                }
            }
            return Err(error);
        }
    };

    // The config now points at the replacement (or at no token when clearing),
    // so the superseded keyring entry can be released. Cleanup is best-effort:
    // failing after the config commit must not make the UI retry the rotation.
    if !previous.is_empty() && previous != replacement {
        if let Err(cleanup_error) = delete(&previous) {
            warn("previous", cleanup_error);
        }
    }
    Ok(true)
}

pub(super) fn warn_secret_cleanup(
    app_handle: &dyn AppContext,
    log_target: &str,
    phase: &'static str,
    error: String,
) {
    log_platform_error(
        app_handle,
        log_target,
        "Could not remove superseded secret from OS storage",
        format!("phase={phase}; error={error}"),
    );
}

pub(super) fn replace_config_secret(
    app_handle: &dyn AppContext,
    plaintext: &str,
    log_target: &str,
    update: impl FnOnce(&mut config::AppConfig, String) -> String,
) -> Result<(), String> {
    rotate_secret_with(
        plaintext,
        |value| os::encrypt_secret(value).map_err(|e| e.to_string()),
        |replacement| {
            let mut previous = String::new();
            config::update_config(app_handle, |cfg| {
                previous = update(cfg, replacement);
            })?;
            Ok(SecretPersistence::Replaced(previous))
        },
        |token| os::delete_secret(token).map_err(|e| e.to_string()),
        |phase, error| warn_secret_cleanup(app_handle, log_target, phase, error),
    )
    .map(|_| ())
}

pub(super) fn read_api_key(app_handle: &dyn AppContext) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    let encrypted = cfg.steam.api_key_encrypted.trim();
    if !encrypted.is_empty() {
        return decrypt_api_key(encrypted).map(|v| v.trim().to_string());
    }

    let legacy = cfg.steam.api_key.trim().to_string();
    if legacy.is_empty() {
        return Ok(String::new());
    }

    let migrated = rotate_secret_with(
        &legacy,
        encrypt_api_key,
        |replacement| {
            let mut persistence = SecretPersistence::Unused;
            config::update_config(app_handle, |latest| {
                if latest.steam.api_key_encrypted.trim().is_empty()
                    && latest.steam.api_key.trim() == legacy
                {
                    let previous =
                        std::mem::replace(&mut latest.steam.api_key_encrypted, replacement);
                    latest.steam.api_key.clear();
                    persistence = SecretPersistence::Replaced(previous);
                }
            })?;
            Ok(persistence)
        },
        |token| os::delete_secret(token).map_err(|e| e.to_string()),
        |phase, error| warn_secret_cleanup(app_handle, "steam.migrate_api_key", phase, error),
    )?;
    if migrated {
        return Ok(legacy);
    }

    // A concurrent setter won the race. Return its value instead of the stale
    // plaintext observed before taking the config write lock.
    let latest = config::load_config(app_handle);
    if !latest.steam.api_key_encrypted.trim().is_empty() {
        decrypt_api_key(&latest.steam.api_key_encrypted).map(|value| value.trim().to_string())
    } else {
        Ok(latest.steam.api_key.trim().to_string())
    }
}

pub fn set_api_key(app_handle: AppCtx, key: String) -> Result<(), PlatformError> {
    replace_config_secret(
        &app_handle,
        &key,
        "steam.set_api_key",
        |cfg, replacement| {
            let previous = std::mem::replace(&mut cfg.steam.api_key_encrypted, replacement);
            cfg.steam.api_key = String::new();
            previous
        },
    )
    .map_err(Into::into)
}

pub fn has_api_key(app_handle: AppCtx) -> bool {
    read_api_key(&app_handle)
        .map(|api_key| !api_key.trim().is_empty())
        .unwrap_or(false)
}
