use super::*;
use crate::config::AppConfig;
use std::path::PathBuf;

struct TempCtx {
    root: PathBuf,
}

impl AppContext for TempCtx {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.clone())
    }
}

/// The config cache and the poisoned-local flag are process-global, so
/// every test that writes a config takes the same lock as `config`'s own.
fn config_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-listing-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// Bytes of both config files, which is what a listing must leave alone.
fn config_bytes(ctx: &TempCtx) -> Vec<(PathBuf, Vec<u8>)> {
    [
        crate::storage::portable_config_path(ctx).unwrap(),
        crate::storage::local_config_path(ctx).unwrap(),
    ]
    .into_iter()
    .map(|path| {
        let bytes = fs::read(&path).unwrap_or_default();
        (path, bytes)
    })
    .collect()
}

fn seed_account(ctx: &TempCtx, email: &str, last_used_at: Option<u64>) {
    config::update_config(ctx, |cfg| {
        cfg.battle_net.accounts.push(BattleNetAccountConfig {
            email: email.to_string(),
            battle_tag: "Seeded#0001".into(),
            last_used_at,
        });
    })
    .unwrap();
}

#[test]
fn listing_twice_writes_nothing_and_keeps_the_previous_last_used_at() {
    // The finding: listing called `remember_account_usage`, which took the
    // cross-process lock and stamped `last_used_at` with "now" on every
    // poll, so "last used" was really "last listed".
    let _config = config_guard();
    let root = scratch("no-write");
    let ctx = TempCtx { root: root.clone() };
    seed_account(&ctx, "one@example.com", Some(1_000));
    let before = config_bytes(&ctx);

    let saved = vec!["one@example.com".to_string()];
    let first = list_accounts_from_saved(&ctx, saved.clone()).unwrap();
    let second = list_accounts_from_saved(&ctx, saved).unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].last_login_at, Some(1_000));
    assert_eq!(second[0].last_login_at, Some(1_000));
    assert_eq!(config_bytes(&ctx), before, "listing rewrote the config");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_newly_discovered_email_is_recorded_without_a_timestamp() {
    let _config = config_guard();
    let root = scratch("discovery");
    let ctx = TempCtx { root: root.clone() };
    seed_account(&ctx, "known@example.com", Some(1_000));

    // The known account stays first, so it is still the current one and
    // the newcomer never claims its battle tag.
    let accounts = list_accounts_from_saved(
        &ctx,
        vec![
            "known@example.com".to_string(),
            "fresh@example.com".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[1].email, "fresh@example.com");
    assert_eq!(accounts[1].last_login_at, None);
    assert_eq!(accounts[0].last_login_at, Some(1_000));

    // And it is persisted, so the next listing has nothing to write.
    let stored = config::load_config(&ctx);
    let fresh = stored
        .battle_net
        .accounts
        .iter()
        .find(|account| account.email == "fresh@example.com")
        .expect("the discovered account is in the config");
    assert_eq!(fresh.last_used_at, None);
    assert!(fresh.battle_tag.is_empty());

    let before = config_bytes(&ctx);
    let _ = list_accounts_from_saved(
        &ctx,
        vec![
            "known@example.com".to_string(),
            "fresh@example.com".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(config_bytes(&ctx), before);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_use_still_stamps_the_account() {
    // What `switch_account` and setup completion call. Listing does not.
    let _config = config_guard();
    let root = scratch("stamp");
    let ctx = TempCtx { root: root.clone() };
    seed_account(&ctx, "one@example.com", Some(1_000));

    remember_account_usage(&ctx, "one@example.com", false).unwrap();

    let stored = config::load_config(&ctx);
    let stamped = stored.battle_net.accounts[0].last_used_at.unwrap();
    assert!(stamped > 1_000, "last_used_at was not refreshed: {stamped}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn unknown_emails_only_reports_what_the_config_is_missing() {
    let mut cfg = AppConfig::default();
    cfg.battle_net.accounts.push(BattleNetAccountConfig {
        email: "Known@Example.com".into(),
        battle_tag: String::new(),
        last_used_at: Some(7),
    });

    // Case-insensitive, and the same newcomer listed twice counts once.
    assert_eq!(
        unknown_emails(
            &cfg,
            &[
                "known@example.com".to_string(),
                "  ".to_string(),
                " fresh@example.com ".to_string(),
                "FRESH@example.com".to_string(),
            ]
        ),
        vec!["fresh@example.com".to_string()]
    );
    assert!(unknown_emails(&cfg, &["KNOWN@EXAMPLE.COM".to_string()]).is_empty());
}

#[test]
fn only_the_signed_in_newcomer_takes_the_battle_tag() {
    let mut cfg = AppConfig::default();
    let emails = vec![
        "current@example.com".to_string(),
        "other@example.com".to_string(),
    ];

    add_new_accounts(
        &mut cfg,
        &emails,
        Some("current@example.com"),
        Some("Tag#1234"),
    );

    assert_eq!(cfg.battle_net.accounts.len(), 2);
    assert_eq!(cfg.battle_net.accounts[0].battle_tag, "Tag#1234");
    assert!(cfg.battle_net.accounts[1].battle_tag.is_empty());
    assert!(cfg
        .battle_net
        .accounts
        .iter()
        .all(|account| account.last_used_at.is_none()));

    // Running again adds nothing: this is what makes the write under the
    // lock safe when the stored config already moved on.
    add_new_accounts(
        &mut cfg,
        &emails,
        Some("current@example.com"),
        Some("Tag#1234"),
    );
    assert_eq!(cfg.battle_net.accounts.len(), 2);
}
