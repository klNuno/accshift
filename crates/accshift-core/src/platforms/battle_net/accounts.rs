//! Account listing and metadata: known emails, display names and usage.

use super::*;

pub(super) fn battle_net_display_name(email: &str) -> String {
    let trimmed = email.trim();
    let candidate = trimmed
        .split('@')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(trimmed);
    candidate.to_string()
}

pub(super) fn build_battle_net_switch_details(target_email: Option<&str>) -> String {
    let current_account = read_saved_accounts()
        .ok()
        .and_then(|accounts| accounts.into_iter().next())
        .unwrap_or_default();
    let running_processes = crate::os::running_process_names(BATTLE_NET_PROCESS_NAMES);

    use crate::platforms::{redact_id, redact_opt};
    serde_json::json!({
        "targetEmail": redact_opt(target_email),
        "currentAccount": redact_id(&current_account),
        "launcherRunning": !running_processes.is_empty(),
        "runningProcesses": running_processes,
    })
    .to_string()
}

pub(super) fn normalize_account_key(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub(super) fn validate_account_email(email: &str) -> Result<String, String> {
    let trimmed = email.trim();
    if trimmed.is_empty()
        || trimmed.len() > 320
        || trimmed.chars().any(|ch| ch == '\0' || ch.is_control())
    {
        return Err("Invalid Battle.net account identifier".into());
    }
    Ok(trimmed.to_string())
}

/// Merge the launcher's saved account list with the ones we track in our own
/// config, first occurrence wins. Takes both inputs so callers that already
/// read them do not pay for a second disk read and config clone.
pub(super) fn known_account_emails_from(
    saved_accounts: Vec<String>,
    cfg: &AppConfig,
) -> Vec<String> {
    let mut accounts = Vec::new();
    let mut seen = HashSet::new();

    for email in saved_accounts {
        let key = normalize_account_key(&email);
        if email.trim().is_empty() || !seen.insert(key) {
            continue;
        }
        accounts.push(email);
    }

    for account in &cfg.battle_net.accounts {
        let email = account.email.trim().to_string();
        let key = normalize_account_key(&email);
        if email.is_empty() || !seen.insert(key) {
            continue;
        }
        accounts.push(email);
    }

    accounts
}

pub(super) fn known_account_emails(app_handle: &dyn AppContext) -> Result<Vec<String>, String> {
    let saved_accounts = read_saved_accounts()?;
    let cfg = config::load_config(app_handle);
    Ok(known_account_emails_from(saved_accounts, &cfg))
}

pub(super) fn read_accounts(app_handle: &dyn AppContext) -> Result<Vec<BattleNetAccount>, String> {
    list_accounts_from_saved(app_handle, read_saved_accounts()?)
}

/// Build the account list from what the launcher saved plus what our own
/// config already knows.
///
/// Listing is a read. It used to call `remember_account_usage` for the first
/// saved account, which took the cross-process config lock and stamped
/// `last_used_at` with "now" on every poll, so "last used" meant "last listed"
/// and the frontend's sort order was noise. The only write left is registering
/// an email the launcher knows and our config does not, and it carries no
/// timestamp: an account we have never seen used has no usage to report.
pub(super) fn list_accounts_from_saved(
    app_handle: &dyn AppContext,
    saved_accounts: Vec<String>,
) -> Result<Vec<BattleNetAccount>, String> {
    let mut cfg = config::load_config(app_handle);

    // The one write listing is allowed, and only when the launcher shows an
    // account our config has never seen. Everything below is a read.
    let newcomers = unknown_emails(&cfg, &saved_accounts);
    if !newcomers.is_empty() {
        let current_key = saved_accounts
            .first()
            .map(|email| normalize_account_key(email));
        // The tag lives in the client's cache under the id of the account that
        // is signed in, so it can only be claimed for that one, and only while
        // it is the account being registered.
        let current_tag = current_key
            .as_ref()
            .filter(|key| {
                newcomers
                    .iter()
                    .any(|email| &normalize_account_key(email) == *key)
            })
            .and_then(|_| current_battle_tag_from_cache().ok().flatten());

        add_new_accounts(
            &mut cfg,
            &newcomers,
            current_key.as_deref(),
            current_tag.as_deref(),
        );
        config::update_config(app_handle, |stored| {
            add_new_accounts(
                stored,
                &newcomers,
                current_key.as_deref(),
                current_tag.as_deref(),
            );
        })?;
    }

    let account_emails = known_account_emails_from(saved_accounts, &cfg);
    let metadata_by_key = cfg
        .battle_net
        .accounts
        .into_iter()
        .filter_map(|account| {
            let email = account.email.trim().to_string();
            if email.is_empty() {
                return None;
            }
            Some((normalize_account_key(&email), account))
        })
        .collect::<HashMap<_, _>>();

    Ok(account_emails
        .into_iter()
        .map(|email| {
            let key = normalize_account_key(&email);
            let meta = metadata_by_key.get(&key);
            BattleNetAccount {
                battle_tag: meta
                    .map(|account| account.battle_tag.trim().to_string())
                    .filter(|battle_tag| !battle_tag.is_empty())
                    .unwrap_or_default(),
                last_login_at: meta.and_then(|account| account.last_used_at),
                email,
            }
        })
        .collect())
}

/// The launcher accounts our own config does not hold yet, deduplicated, in
/// the order the launcher lists them. An empty result means listing has
/// nothing to write and takes no lock.
pub(super) fn unknown_emails(cfg: &AppConfig, saved_accounts: &[String]) -> Vec<String> {
    let mut known = cfg
        .battle_net
        .accounts
        .iter()
        .map(|account| normalize_account_key(&account.email))
        .collect::<HashSet<_>>();

    saved_accounts
        .iter()
        .filter_map(|email| {
            let email = email.trim().to_string();
            if email.is_empty() || !known.insert(normalize_account_key(&email)) {
                return None;
            }
            Some(email)
        })
        .collect()
}

/// Record accounts the launcher knows and we do not, with no `last_used_at`:
/// seeing an account is not using it, and a listing that stamped one would
/// make "last used" mean "last listed". Only the account that is signed in
/// gets the battle tag, and only if the caller could read it.
///
/// Re-checks what the config holds, because the copy this runs on inside
/// `update_config` is re-read under the lock and may already have the account.
pub(super) fn add_new_accounts(
    cfg: &mut AppConfig,
    emails: &[String],
    current_key: Option<&str>,
    current_tag: Option<&str>,
) {
    let mut known = cfg
        .battle_net
        .accounts
        .iter()
        .map(|account| normalize_account_key(&account.email))
        .collect::<HashSet<_>>();

    for email in emails {
        let key = normalize_account_key(email);
        if !known.insert(key.clone()) {
            continue;
        }
        let is_current = current_key == Some(key.as_str());
        cfg.battle_net.accounts.push(BattleNetAccountConfig {
            email: email.clone(),
            battle_tag: match current_tag {
                Some(tag) if is_current => tag.to_string(),
                _ => String::new(),
            },
            last_used_at: None,
        });
    }
}

pub(super) fn current_account(accounts: &[BattleNetAccount]) -> String {
    accounts
        .first()
        .map(|account| account.email.clone())
        .unwrap_or_default()
}

pub(super) fn remember_account_usage(
    app_handle: &dyn AppContext,
    email: &str,
    is_current_account: bool,
) -> Result<(), String> {
    let email = validate_account_email(email)?;
    let key = normalize_account_key(&email);
    let now = crate::platforms::now_unix_ms();
    // Only query the battle tag for the account that is actually logged in
    // right now. Applying it to other accounts would overwrite their tags.
    // Only query battle tag from cache if we don't already have one stored.
    // After a switch, the log-based account_id_lo still points to the PREVIOUS
    // account, so current_battle_tag_from_cache() would return the wrong tag.
    // The cache read (up to eight logs and a sqlite query) runs before the
    // config lock, and only for the signed-in account that has no tag yet.
    let has_tag = |cfg: &AppConfig| {
        cfg.battle_net.accounts.iter().any(|account| {
            normalize_account_key(&account.email) == key && !account.battle_tag.trim().is_empty()
        })
    };
    let battle_tag = if is_current_account && !has_tag(&config::load_config(app_handle)) {
        current_battle_tag_from_cache().ok().flatten()
    } else {
        None
    };
    config::update_config(app_handle, |cfg| {
        let index = cfg
            .battle_net
            .accounts
            .iter()
            .position(|account| normalize_account_key(&account.email) == key);
        // Another writer may have stored a tag since the read above.
        let battle_tag = battle_tag.filter(|_| !has_tag(cfg));

        if let Some(i) = index {
            let existing = &mut cfg.battle_net.accounts[i];
            existing.email = email;
            if let Some(tag) = battle_tag {
                existing.battle_tag = tag;
            }
            existing.last_used_at = Some(now);
        } else {
            cfg.battle_net.accounts.push(BattleNetAccountConfig {
                email,
                battle_tag: battle_tag.unwrap_or_default(),
                last_used_at: Some(now),
            });
        }
    })
}

pub(super) fn forget_account_metadata(
    app_handle: &dyn AppContext,
    email: &str,
) -> Result<(), String> {
    let key = normalize_account_key(email);
    config::update_config(app_handle, |cfg| {
        cfg.battle_net
            .accounts
            .retain(|account| normalize_account_key(&account.email) != key);
    })
}
