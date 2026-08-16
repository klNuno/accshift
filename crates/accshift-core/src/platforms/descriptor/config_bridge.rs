//! Reading and writing the per-platform config sections from a platform id.
//!
//! The config keeps one typed section per platform, and existing installs
//! already hold user data under those exact keys. Rather than migrate everyone
//! onto a generic map, the engine reaches the typed sections through this
//! bridge: `config.rs` stays untouched and a converted platform keeps every
//! label and timestamp its users already have.
//!
//! Adding a platform to the engine means adding one line to each match here,
//! which is the one place a JSON descriptor still needs a compiled counterpart.
//! That is deliberate: the alternative is silently dropping the accounts of
//! anyone who upgrades.

use crate::config::{self, GogAccountConfig, JagexAccountConfig};
use crate::AppContext;

/// One account as the config stores it, whichever section it came from.
#[derive(Debug, Clone, Default)]
pub struct AccountRecord {
    pub account_id: String,
    pub label: String,
    pub last_used_at: Option<u64>,
}

/// Runs the same body against whichever config section the platform owns.
///
/// Each arm binds `$accounts` to that section's account vector and `$new`
/// to a constructor for its account type; the vectors have different element
/// types, so a function taking `&mut Vec<_>` could not do this.
macro_rules! with_section {
    ($cfg:expr, $platform:expr, |$accounts:ident, $new:ident| $body:block) => {
        match $platform {
            crate::platforms::ids::GOG => {
                let $accounts = &mut $cfg.gog.accounts;
                #[allow(unused)]
                let $new = |account_id: String, label: String, last_used_at: Option<u64>| {
                    GogAccountConfig {
                        account_id,
                        label,
                        last_used_at,
                    }
                };
                $body
            }
            crate::platforms::ids::JAGEX => {
                let $accounts = &mut $cfg.jagex.accounts;
                #[allow(unused)]
                let $new = |account_id: String, label: String, last_used_at: Option<u64>| {
                    JagexAccountConfig {
                        account_id,
                        label,
                        last_used_at,
                    }
                };
                $body
            }
            _ => {}
        }
    };
}

/// Every account the config holds for this platform, in stored order.
pub fn accounts(app: &dyn AppContext, platform_id: &str) -> Vec<AccountRecord> {
    let cfg = config::load_config(app);
    let mapped = |account_id: &str, label: &str, last_used_at: Option<u64>| AccountRecord {
        account_id: account_id.trim().to_string(),
        label: label.trim().to_string(),
        last_used_at,
    };
    match platform_id {
        crate::platforms::ids::GOG => cfg
            .gog
            .accounts
            .iter()
            .map(|a| mapped(&a.account_id, &a.label, a.last_used_at))
            .collect(),
        crate::platforms::ids::JAGEX => cfg
            .jagex
            .accounts
            .iter()
            .map(|a| mapped(&a.account_id, &a.label, a.last_used_at))
            .collect(),
        _ => Vec::new(),
    }
}

/// Stamps the account as just used, adding it if the config never saw it.
pub fn touch_account(
    app: &dyn AppContext,
    platform_id: &str,
    account_id: &str,
    now: u64,
) -> Result<(), String> {
    let key = account_id.trim().to_string();
    config::update_config(app, |cfg| {
        with_section!(cfg, platform_id, |accounts, new| {
            match accounts.iter_mut().find(|a| a.account_id.trim() == key) {
                Some(existing) => existing.last_used_at = Some(now),
                None => accounts.push(new(key.clone(), String::new(), Some(now))),
            }
        });
    })
}

/// Sets the account's label, adding it if the config never saw it.
pub fn set_label(
    app: &dyn AppContext,
    platform_id: &str,
    account_id: &str,
    label: &str,
) -> Result<(), String> {
    let key = account_id.trim().to_string();
    let label = label.trim().to_string();
    config::update_config(app, |cfg| {
        with_section!(cfg, platform_id, |accounts, new| {
            match accounts.iter_mut().find(|a| a.account_id.trim() == key) {
                Some(existing) => existing.label = label.clone(),
                None => accounts.push(new(key.clone(), label.clone(), None)),
            }
        });
    })
}

/// Drops the account, and the current-account marker when it pointed at it.
pub fn remove_account(
    app: &dyn AppContext,
    platform_id: &str,
    account_id: &str,
) -> Result<(), String> {
    let key = account_id.trim().to_string();
    config::update_config(app, |cfg| {
        with_section!(cfg, platform_id, |accounts, _new| {
            accounts.retain(|a| a.account_id.trim() != key);
        });
        if current_account_field(cfg, platform_id).is_some_and(|current| current.trim() == key) {
            set_current_account_field(cfg, platform_id, String::new());
        }
    })
}

/// The account the config remembers as signed in, for platforms whose
/// launcher writes no readable marker. `None` when the section has no such
/// field, which is the normal case for a platform with a live identity source.
pub fn current_account(app: &dyn AppContext, platform_id: &str) -> Option<String> {
    let cfg = config::load_config(app);
    current_account_field(&cfg, platform_id).map(|value| value.trim().to_string())
}

pub fn set_current_account(
    app: &dyn AppContext,
    platform_id: &str,
    account_id: &str,
) -> Result<(), String> {
    let value = account_id.trim().to_string();
    config::update_config(app, |cfg| {
        set_current_account_field(cfg, platform_id, value.clone());
    })
}

fn current_account_field<'a>(cfg: &'a config::AppConfig, platform_id: &str) -> Option<&'a String> {
    match platform_id {
        crate::platforms::ids::JAGEX => Some(&cfg.jagex.current_account),
        _ => None,
    }
}

fn set_current_account_field(cfg: &mut config::AppConfig, platform_id: &str, value: String) {
    if platform_id == crate::platforms::ids::JAGEX {
        cfg.jagex.current_account = value;
    }
}

/// The user's manual path to the launcher, empty when they never set one.
pub fn path_override(app: &dyn AppContext, platform_id: &str) -> String {
    let cfg = config::load_config(app);
    match platform_id {
        crate::platforms::ids::GOG => cfg.gog.path_override.trim().to_string(),
        crate::platforms::ids::JAGEX => cfg.jagex.path_override.trim().to_string(),
        _ => String::new(),
    }
}

pub fn set_path_override(
    app: &dyn AppContext,
    platform_id: &str,
    path: &str,
) -> Result<(), String> {
    let path = path.trim().to_string();
    config::update_config(app, |cfg| match platform_id {
        crate::platforms::ids::GOG => cfg.gog.path_override = path.clone(),
        crate::platforms::ids::JAGEX => cfg.jagex.path_override = path.clone(),
        _ => {}
    })
}
