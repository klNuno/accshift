//! Roblox commands. Port of `src-tauri/src/commands/roblox.rs`: Windows
//! only, the cookie write goes through the registry.

use super::RealBackend;
use crate::error::BackendResult;
use crate::types::{RobloxAccount, RobloxProfileInfo};

/// `roblox_add_account_by_cookie`, roblox.rs lines 6-34: the cookie is
/// checked against Roblox unlocked, only the store write takes the
/// operation lock; failures recorded as `account_add` on Roblox.
#[cfg(windows)]
pub(super) fn add_account_by_cookie(
    backend: &RealBackend,
    cookie: &str,
) -> BackendResult<RobloxAccount> {
    use accshift_core::error::PlatformError;
    use accshift_core::platforms::{ids, roblox};
    let client = backend.http_client()?.clone();
    let result = match super::runtime::block_on(roblox::validate_pasted_cookie(
        cookie.to_string(),
        client,
    )) {
        Ok(pasted) => super::runtime::locked(&backend.ctx, |c| {
            roblox::store_pasted_account(&*c, pasted).map_err(PlatformError::from)
        }),
        Err(e) => Err(PlatformError::from(e)),
    };
    backend
        .track_operation("account_add", Some(ids::ROBLOX), result)
        .map(account)
}

#[cfg(windows)]
fn account(account: accshift_core::platforms::roblox::RobloxAccount) -> RobloxAccount {
    RobloxAccount {
        user_id: account.user_id,
        username: account.username,
        display_name: account.display_name,
        last_login_at: account.last_login_at,
    }
}

/// `roblox_get_profile_info`, roblox.rs lines 36-45.
#[cfg(windows)]
pub(super) fn profile_info(
    backend: &RealBackend,
    user_id: &str,
) -> BackendResult<RobloxProfileInfo> {
    use accshift_core::platforms::roblox;
    let client = backend.http_client()?.clone();
    let info = super::runtime::block_on(roblox::get_profile_info(user_id.to_string(), client))?;
    Ok(RobloxProfileInfo {
        avatar_url: info.avatar_url,
    })
}

/// `roblox_check_sessions`, roblox.rs lines 47-59: the user ids whose stored
/// session is dead, one blocking probe per account on this thread.
#[cfg(windows)]
pub(super) fn check_sessions(backend: &RealBackend) -> BackendResult<Vec<String>> {
    Ok(accshift_core::platforms::roblox::dead_session_user_ids(
        &*backend.ctx,
    ))
}

#[cfg(not(windows))]
pub(super) fn add_account_by_cookie(
    _backend: &RealBackend,
    _cookie: &str,
) -> BackendResult<RobloxAccount> {
    Err(super::not_registered("roblox_add_account_by_cookie"))
}

#[cfg(not(windows))]
pub(super) fn profile_info(
    _backend: &RealBackend,
    _user_id: &str,
) -> BackendResult<RobloxProfileInfo> {
    Err(super::not_registered("roblox_get_profile_info"))
}

#[cfg(not(windows))]
pub(super) fn check_sessions(_backend: &RealBackend) -> BackendResult<Vec<String>> {
    Err(super::not_registered("roblox_check_sessions"))
}
