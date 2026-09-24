//! The local Roblox client: the registry cookie, auth tickets and launch.

#[allow(unused_imports)]
use super::*;

/// Registry key Roblox Studio keeps the live session cookie under (HKCU).
pub(super) const ROBLOX_COOKIE_KEY: &str = "SOFTWARE\\Roblox\\RobloxStudioBrowser\\roblox.com";

pub(super) fn write_cookie_to_registry(cookie: &str) -> Result<(), String> {
    let value = format!("COOK::<{cookie}>");
    registry::write_string(
        registry::HKEY_CURRENT_USER,
        ROBLOX_COOKIE_KEY,
        ".ROBLOSECURITY",
        &value,
    )
}

/// Unwrap the `COOK::<cookie>` envelope Roblox / Studio store the cookie in.
/// Falls back to the raw value if it is not wrapped (older formats, manual
/// edits). Returns `None` for an empty result. Pure so it can be unit tested
/// without touching the registry; the inverse of the format written by
/// `write_cookie_to_registry`.
pub(super) fn unwrap_registry_cookie(value: &str) -> Option<String> {
    let unwrapped = value
        .strip_prefix("COOK::<")
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(value)
        .trim();

    if unwrapped.is_empty() {
        None
    } else {
        Some(unwrapped.to_string())
    }
}

/// Read the live `.ROBLOSECURITY` cookie out of HKCU. Returns `Ok(None)` when
/// the key or value is missing, which is the normal state on a fresh machine.
/// Mirrors `write_cookie_to_registry`.
pub(super) fn read_cookie_from_registry() -> Result<Option<String>, String> {
    let value = registry::try_read_raw_string(
        registry::HKEY_CURRENT_USER,
        ROBLOX_COOKIE_KEY,
        ".ROBLOSECURITY",
    )?;
    Ok(value.as_deref().and_then(unwrap_registry_cookie))
}

/// Before switching away from the active account, capture any cookie rotation
/// that Roblox Studio performed in HKCU. Studio refreshes `.ROBLOSECURITY`
/// in place; if we ignore it and later overwrite the registry with our stored
/// (now stale) cookie, the user gets logged out. Windows-only; no-op elsewhere.
/// Returns `true` only when the store was actually rewritten, so the caller can
/// skip re-reading it when nothing changed.
pub(super) fn persist_rotated_cookie(
    app_handle: &dyn AppContext,
    account: &RobloxAccountConfig,
) -> bool {
    let live = match read_cookie_from_registry() {
        Ok(Some(cookie)) => cookie,
        Ok(None) => return false,
        Err(e) => {
            log_platform_error(
                app_handle,
                "roblox.switch_account",
                "Could not read live Roblox cookie from registry",
                &e,
            );
            return false;
        }
    };

    let stored = match crate::os::decrypt_secret(&account.cookie_encrypted) {
        Ok(c) => c,
        Err(e) => {
            log_platform_error(
                app_handle,
                "roblox.switch_account",
                "Could not decrypt stored cookie for rotation check",
                format!("{e}"),
            );
            return false;
        }
    };

    if live.trim() == stored.trim() {
        return false;
    }

    // The active-account guess in switch_account (max last_used_at) is only a
    // hint: adding a cookie via add_account_by_cookie bumps last_used_at
    // without ever writing the registry, so the live registry cookie can
    // belong to a DIFFERENT account than the one guessed here. Overwriting
    // this account's stored cookie with a cookie that is not its own would
    // permanently destroy its real session and alias it to another account's
    // credentials. Before persisting, confirm the live cookie actually
    // authenticates as this same user; on any mismatch or doubt, leave the
    // stored cookie untouched (fail closed).
    match validate_cookie_blocking(&live) {
        Ok(user) if user.id.to_string() == account.user_id => {}
        Ok(_) => return false,
        Err(e) => {
            log_platform_error(
                app_handle,
                "roblox.switch_account",
                "Could not verify live cookie ownership before rotation; skipping",
                &e,
            );
            return false;
        }
    }

    let encrypted = match crate::os::encrypt_secret(&live) {
        Ok(enc) => enc,
        Err(e) => {
            log_platform_error(
                app_handle,
                "roblox.switch_account",
                "Could not re-encrypt rotated cookie",
                format!("{e}"),
            );
            return false;
        }
    };

    let mut accounts = load_account_configs(app_handle);
    // Capture the token the old encrypted cookie points at so we can free it
    // from the OS keyring (Linux / macOS) after the new one is persisted. The
    // rotation re-encrypts to a fresh UUID, leaving the previous entry orphaned
    // otherwise (finding K8). forget_account already does the same.
    let old_token;
    if let Some(a) = accounts.iter_mut().find(|a| a.user_id == account.user_id) {
        old_token = std::mem::replace(&mut a.cookie_encrypted, encrypted);
    } else {
        return false;
    }

    if let Err(e) = save_account_configs(app_handle, &accounts) {
        log_platform_error(
            app_handle,
            "roblox.switch_account",
            "Could not persist rotated cookie",
            &e,
        );
        return false;
    }

    // New token is persisted; drop the stale keyring entry. Guard on a
    // non-empty token that actually changed. Log and continue on failure: a
    // dangling keyring entry must not fail the rotation.
    let new_token = accounts
        .iter()
        .find(|a| a.user_id == account.user_id)
        .map(|a| a.cookie_encrypted.as_str())
        .unwrap_or("");
    if !old_token.is_empty() && old_token != new_token {
        if let Err(e) = crate::os::delete_secret(&old_token) {
            log_platform_error(
                app_handle,
                "roblox.switch_account",
                "Could not delete stale rotated cookie secret",
                format!("{e}"),
            );
        }
    }

    log_platform_info(
        app_handle,
        "roblox.switch_account",
        "Persisted rotated Roblox cookie from registry",
        format!("userId={}", crate::platforms::redact_id(&account.user_id)),
    );

    true
}

pub(super) fn kill_roblox() {
    crate::os::kill_processes(ROBLOX_PROCESS_NAMES);
}

pub(super) fn request_auth_ticket(cookie: &str) -> Result<String, String> {
    let response = post_with_csrf_and_headers(
        "https://auth.roblox.com/v1/authentication-ticket/",
        "{}",
        &[
            ("Cookie", &format!(".ROBLOSECURITY={cookie}")),
            ("Referer", "https://www.roblox.com/"),
        ],
    )
    .map_err(|e| format!("Could not request auth ticket: {e}"))?;

    if !response.status().is_success() {
        // The webview matches this wording on HTTP 401 to flag the account's
        // session as expired (roblox/adapter.ts). Keep it stable.
        return Err(format!(
            "Auth ticket request failed (HTTP {})",
            response.status()
        ));
    }

    response
        .headers()
        .get("rbx-authentication-ticket")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| "No auth ticket in response".to_string())
}

pub(super) fn launch_roblox_with_ticket(ticket: &str) -> Result<(), String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let uri = format!(
        "roblox-player:1+launchmode:app+gameinfo:{ticket}+launchtime:{now_ms}+browsertrackerid:0+robloxLocale:en_us+gameLocale:en_us"
    );

    crate::os::open_url(&uri).map_err(|e| format!("Could not launch Roblox: {e}"))
}
