//! Reading and writing the per-platform config sections from a platform id.
//!
//! The config keeps one typed section per platform, and existing installs
//! already hold user data under those exact keys. Rather than migrate everyone
//! onto a generic map, the engine reaches the typed sections through this
//! bridge: `config.rs` stays untouched and a converted platform keeps every
//! label and timestamp its users already have.
//!
//! Converting a shipped platform to the engine means adding one line to each
//! match here, so its users keep the accounts they already have. A platform
//! that arrives as a user descriptor has no such history and needs no line: it
//! falls through to `config.custom_platforms`, a generic section keyed by id.

use crate::config::{
    self, CustomAccountConfig, CustomPlatformConfig, DiscordAccountConfig, EpicAccountConfig,
    GogAccountConfig, JagexAccountConfig, UbisoftAccountConfig,
};
use crate::platforms::ids;
use crate::AppContext;

/// One account as the config stores it, whichever section it came from.
#[derive(Debug, Clone, Default)]
pub struct AccountRecord {
    pub account_id: String,
    pub label: String,
    pub last_used_at: Option<u64>,
}

/// One stored account row, whichever typed section it lives in.
///
/// The sections hold different types with the same three fields under
/// different names, so the operations below are written once against this and
/// monomorphised per section.
trait AccountRow {
    fn create(account_id: String, label: String, last_used_at: Option<u64>) -> Self;
    fn account_id(&self) -> &str;
    fn label(&self) -> &str;
    fn set_label(&mut self, label: String);
    fn last_used_at(&self) -> Option<u64>;
    fn set_last_used_at(&mut self, at: Option<u64>);
}

macro_rules! impl_account_row {
    ($type:ty, $id_field:ident) => {
        impl AccountRow for $type {
            fn create(account_id: String, label: String, last_used_at: Option<u64>) -> Self {
                Self {
                    $id_field: account_id,
                    label,
                    last_used_at,
                }
            }
            fn account_id(&self) -> &str {
                &self.$id_field
            }
            fn label(&self) -> &str {
                &self.label
            }
            fn set_label(&mut self, label: String) {
                self.label = label;
            }
            fn last_used_at(&self) -> Option<u64> {
                self.last_used_at
            }
            fn set_last_used_at(&mut self, at: Option<u64>) {
                self.last_used_at = at;
            }
        }
    };
}

impl_account_row!(GogAccountConfig, account_id);
impl_account_row!(JagexAccountConfig, account_id);
impl_account_row!(EpicAccountConfig, account_id);
impl_account_row!(UbisoftAccountConfig, uuid);
impl_account_row!(DiscordAccountConfig, account_id);
impl_account_row!(CustomAccountConfig, account_id);

/// Runs the same body against whichever section the platform owns.
///
/// Each arm binds `$accounts` to that section's account vector. The vectors
/// hold different types, so a function taking `&mut Vec<_>` could not do this;
/// the body is generic over [`AccountRow`] instead.
macro_rules! with_accounts {
    ($cfg:expr, $platform:expr, |$accounts:ident| $body:block) => {
        match $platform {
            ids::GOG => {
                let $accounts = &mut $cfg.gog.accounts;
                $body
            }
            ids::JAGEX => {
                let $accounts = &mut $cfg.jagex.accounts;
                $body
            }
            ids::EPIC => {
                let $accounts = &mut $cfg.epic.accounts;
                $body
            }
            ids::UBISOFT => {
                let $accounts = &mut $cfg.ubisoft.accounts;
                $body
            }
            ids::DISCORD => {
                let $accounts = &mut $cfg.discord.accounts;
                $body
            }
            other => {
                let $accounts = &mut custom_section($cfg, other).accounts;
                $body
            }
        }
    };
}

/// The generic section for `platform_id`, created on first write.
///
/// Only ever reached for an id the build has no typed section for, which today
/// means a platform the user added themselves.
fn custom_section<'a>(
    cfg: &'a mut config::AppConfig,
    platform_id: &str,
) -> &'a mut CustomPlatformConfig {
    cfg.custom_platforms
        .entry(platform_id.to_string())
        .or_default()
}

/// Ids are compared folded: a config written before the platform declared a
/// spelling still matches the id the engine hands in today.
fn same_account(stored: &str, key: &str) -> bool {
    stored.trim().eq_ignore_ascii_case(key)
}

fn rows<T: AccountRow>(accounts: &[T]) -> Vec<AccountRecord> {
    accounts
        .iter()
        .map(|account| AccountRecord {
            account_id: account.account_id().trim().to_string(),
            label: account.label().trim().to_string(),
            last_used_at: account.last_used_at(),
        })
        .collect()
}

fn touch_row<T: AccountRow>(accounts: &mut Vec<T>, key: &str, now: u64) {
    match accounts
        .iter_mut()
        .find(|account| same_account(account.account_id(), key))
    {
        Some(existing) => existing.set_last_used_at(Some(now)),
        None => accounts.push(T::create(key.to_string(), String::new(), Some(now))),
    }
}

fn label_row<T: AccountRow>(accounts: &mut Vec<T>, key: &str, label: &str) {
    match accounts
        .iter_mut()
        .find(|account| same_account(account.account_id(), key))
    {
        Some(existing) => existing.set_label(label.to_string()),
        None => accounts.push(T::create(key.to_string(), label.to_string(), None)),
    }
}

/// Every account the config holds for this platform, in stored order.
pub fn accounts(app: &dyn AppContext, platform_id: &str) -> Vec<AccountRecord> {
    let cfg = config::load_config(app);
    match platform_id {
        ids::GOG => rows(&cfg.gog.accounts),
        ids::JAGEX => rows(&cfg.jagex.accounts),
        ids::EPIC => rows(&cfg.epic.accounts),
        ids::UBISOFT => rows(&cfg.ubisoft.accounts),
        ids::DISCORD => rows(&cfg.discord.accounts),
        other => match cfg.custom_platforms.get(other) {
            Some(section) => rows(&section.accounts),
            None => Vec::new(),
        },
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
        with_accounts!(cfg, platform_id, |accounts| {
            touch_row(accounts, &key, now);
        });
        // Using an account again is the plainest possible statement that the
        // user no longer wants it forgotten.
        unblock_in(cfg, platform_id, &key);
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
        with_accounts!(cfg, platform_id, |accounts| {
            label_row(accounts, &key, &label);
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
        with_accounts!(cfg, platform_id, |accounts| {
            accounts.retain(|account| !same_account(account.account_id(), &key));
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
        ids::JAGEX => Some(&cfg.jagex.current_account),
        ids::DISCORD => Some(&cfg.discord.current_account_id),
        // None here means "never stored", not "no such field": a custom
        // platform whose launcher does expose an id simply leaves it empty.
        other => cfg
            .custom_platforms
            .get(other)
            .map(|section| &section.current_account),
    }
}

fn set_current_account_field(cfg: &mut config::AppConfig, platform_id: &str, value: String) {
    match platform_id {
        ids::JAGEX => cfg.jagex.current_account = value,
        ids::DISCORD => cfg.discord.current_account_id = value,
        other => custom_section(cfg, other).current_account = value,
    }
}

// ---------------------------------------------------------------------------
// Forget blocklist
// ---------------------------------------------------------------------------

/// Ids the user forgot while they were still on disk. Only a platform whose
/// accounts can be rediscovered from the filesystem carries one.
pub fn blocklist(app: &dyn AppContext, platform_id: &str) -> Vec<String> {
    let cfg = config::load_config(app);
    match platform_id {
        ids::UBISOFT => cfg.ubisoft.forgotten_uuids.clone(),
        other => match cfg.custom_platforms.get(other) {
            Some(section) => section.forgotten_ids.clone(),
            None => Vec::new(),
        },
    }
}

pub fn block_account(
    app: &dyn AppContext,
    platform_id: &str,
    account_id: &str,
) -> Result<(), String> {
    let key = account_id.trim().to_string();
    config::update_config(app, |cfg| {
        let blocked = match platform_id {
            ids::UBISOFT => &mut cfg.ubisoft.forgotten_uuids,
            other => &mut custom_section(cfg, other).forgotten_ids,
        };
        if !blocked.iter().any(|stored| same_account(stored, &key)) {
            blocked.push(key.clone());
        }
    })
}

fn unblock_in(cfg: &mut config::AppConfig, platform_id: &str, key: &str) {
    let blocked = match platform_id {
        ids::UBISOFT => &mut cfg.ubisoft.forgotten_uuids,
        // Reached before the section exists on a first switch, and creating an
        // empty one to remove nothing from it would be noise in the file.
        other => match cfg.custom_platforms.get_mut(other) {
            Some(section) => &mut section.forgotten_ids,
            None => return,
        },
    };
    blocked.retain(|stored| !same_account(stored, key));
}

/// The user's manual path to the launcher, empty when they never set one.
pub fn path_override(app: &dyn AppContext, platform_id: &str) -> String {
    let cfg = config::load_config(app);
    match platform_id {
        ids::GOG => cfg.gog.path_override.trim().to_string(),
        ids::JAGEX => cfg.jagex.path_override.trim().to_string(),
        ids::EPIC => cfg.epic.path_override.trim().to_string(),
        ids::UBISOFT => cfg.ubisoft.path_override.trim().to_string(),
        ids::DISCORD => cfg.discord.path_override.trim().to_string(),
        other => match cfg.custom_platforms.get(other) {
            Some(section) => section.path_override.trim().to_string(),
            None => String::new(),
        },
    }
}

pub fn set_path_override(
    app: &dyn AppContext,
    platform_id: &str,
    path: &str,
) -> Result<(), String> {
    let path = path.trim().to_string();
    config::update_config(app, |cfg| match platform_id {
        ids::GOG => cfg.gog.path_override = path.clone(),
        ids::JAGEX => cfg.jagex.path_override = path.clone(),
        ids::EPIC => cfg.epic.path_override = path.clone(),
        ids::UBISOFT => cfg.ubisoft.path_override = path.clone(),
        ids::DISCORD => cfg.discord.path_override = path.clone(),
        other => custom_section(cfg, other).path_override = path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platforms::descriptor::test_support::{scratch, TempCtx};

    fn config_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::config::config_io_test_mutex()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_platform_this_build_never_heard_of_still_keeps_everything() {
        // The point of the generic section: a descriptor the user added has
        // accounts, labels, a current account and a path just like a shipped
        // platform, without a struct being compiled for it.
        let _guard = config_guard();
        let root = scratch("bridge-custom");
        let ctx = TempCtx { root: root.clone() };

        touch_account(&ctx, "acme", "user-1", 100).unwrap();
        set_label(&ctx, "acme", "user-1", "Main").unwrap();
        set_current_account(&ctx, "acme", "user-1").unwrap();
        set_path_override(&ctx, "acme", "C:\\Acme\\acme.exe").unwrap();

        let stored = accounts(&ctx, "acme");
        assert_eq!(stored.len(), 1, "{stored:?}");
        assert_eq!(stored[0].account_id, "user-1");
        assert_eq!(stored[0].label, "Main");
        assert_eq!(stored[0].last_used_at, Some(100));
        assert_eq!(current_account(&ctx, "acme").as_deref(), Some("user-1"));
        assert_eq!(path_override(&ctx, "acme"), "C:\\Acme\\acme.exe");

        block_account(&ctx, "acme", "user-2").unwrap();
        assert_eq!(blocklist(&ctx, "acme"), vec!["user-2".to_string()]);
        // Using it again is the plainest statement that it is wanted back.
        touch_account(&ctx, "acme", "user-2", 200).unwrap();
        assert!(blocklist(&ctx, "acme").is_empty());

        remove_account(&ctx, "acme", "user-1").unwrap();
        let left: Vec<String> = accounts(&ctx, "acme")
            .into_iter()
            .map(|account| account.account_id)
            .collect();
        assert_eq!(left, vec!["user-2".to_string()]);
        assert_eq!(
            current_account(&ctx, "acme").as_deref(),
            Some(""),
            "removing the signed-in account clears the marker"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_added_platforms_do_not_share_a_section() {
        let _guard = config_guard();
        let root = scratch("bridge-two");
        let ctx = TempCtx { root: root.clone() };

        touch_account(&ctx, "acme", "user-1", 100).unwrap();
        touch_account(&ctx, "other", "user-2", 100).unwrap();

        assert_eq!(accounts(&ctx, "acme").len(), 1);
        assert_eq!(accounts(&ctx, "other")[0].account_id, "user-2");
        assert!(accounts(&ctx, "third").is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shipped_platform_still_writes_to_the_section_its_users_already_have() {
        // Falling through to the generic map for a platform that has a typed
        // section would strand every account written before the conversion.
        let _guard = config_guard();
        let root = scratch("bridge-shipped");
        let ctx = TempCtx { root: root.clone() };

        touch_account(&ctx, ids::GOG, "aaaa1111", 100).unwrap();

        let cfg = config::load_config(&ctx);
        assert_eq!(cfg.gog.accounts.len(), 1);
        assert!(
            cfg.custom_platforms.is_empty(),
            "{:?}",
            cfg.custom_platforms
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
