//! Saved Roblox accounts in the config.

#[allow(unused_imports)]
use super::*;

pub(super) fn load_account_configs(app_handle: &dyn AppContext) -> Vec<RobloxAccountConfig> {
    if let Ok(path) = crate::storage::roblox_accounts_path(app_handle) {
        if let Ok(Some(accounts)) =
            crate::storage::read_json_if_exists::<Vec<RobloxAccountConfig>>(&path)
        {
            return accounts;
        }
    }

    let cfg = config::load_config(app_handle);
    cfg.roblox.accounts
}

pub(super) fn save_account_configs(
    app_handle: &dyn AppContext,
    accounts: &[RobloxAccountConfig],
) -> Result<(), String> {
    let path = crate::storage::roblox_accounts_path(app_handle)?;
    crate::storage::write_json_atomic(&path, &accounts)?;

    let accounts_clone = accounts.to_vec();
    config::update_config(app_handle, |cfg| {
        cfg.roblox.accounts = accounts_clone;
    })?;

    log_platform_info(
        app_handle,
        "roblox.account_store",
        "Saved Roblox account store",
        format!("path={}; accounts={}", path.display(), accounts.len()),
    );
    Ok(())
}

pub(super) fn store_account(
    app_handle: &dyn AppContext,
    user: &AuthenticatedUserResponse,
    encrypted_cookie: &str,
) -> Result<(), String> {
    let mut accounts = load_account_configs(app_handle);
    let user_id = user.id.to_string();
    let now = crate::platforms::now_unix_ms();

    if let Some(existing) = accounts.iter_mut().find(|a| a.user_id == user_id) {
        existing.username = user.name.clone();
        existing.display_name = user.display_name.clone();
        existing.cookie_encrypted = encrypted_cookie.to_string();
        existing.last_used_at = Some(now);
    } else {
        accounts.push(RobloxAccountConfig {
            user_id,
            username: user.name.clone(),
            display_name: user.display_name.clone(),
            cookie_encrypted: encrypted_cookie.to_string(),
            last_used_at: Some(now),
        });
    }

    save_account_configs(app_handle, &accounts)
}

pub(super) fn read_accounts(app_handle: &dyn AppContext) -> Vec<RobloxAccount> {
    load_account_configs(app_handle)
        .into_iter()
        .map(|a| RobloxAccount {
            user_id: a.user_id,
            username: a.username,
            display_name: a.display_name,
            last_login_at: a.last_used_at,
        })
        .collect()
}
