//! `src/lib/shared/useAccountLoader.test.ts`, ported. The TS deferred
//! promises become the begin and finish halves of each call, finished in the
//! order the TS test resolves them.

use accshift_app::accounts::avatars::ProfileSource;
use accshift_app::accounts::loader::{AccountLoader, AddOutcome, LoadOptions, LoaderHooks};
use accshift_app::error::BackendError;
use accshift_app::platforms::warnings::WarningPresentation;
use accshift_app::platforms::{
    AddAccountResult, AddFlowStatus, CachedPlatformProfile, PlatformAccount, StartupSnapshot,
};
use accshift_app::text::Text;
use std::collections::BTreeMap;

struct Hooks(&'static str);

impl ProfileSource for Hooks {
    fn cached_profile(&self, _account_id: &str) -> Option<CachedPlatformProfile> {
        None
    }
    fn has_profile_info(&self) -> bool {
        false
    }
}

impl LoaderHooks for Hooks {
    fn platform_id(&self) -> &str {
        self.0
    }
    fn no_accounts_toast(&self) -> Option<Text> {
        None
    }
    fn load_error_toast(&self, _message: &str) -> Option<Text> {
        None
    }
    fn switch_error_toast(&self, _message: &str) -> Option<Text> {
        None
    }
    fn cached_warning_states(&self) -> Option<BTreeMap<String, WarningPresentation>> {
        None
    }
    fn has_warnings(&self) -> bool {
        false
    }
}

fn account(id: &str, name: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.into(),
        display_name: name.into(),
        username: id.into(),
        last_login_at_sec: None,
    }
}

fn alice() -> PlatformAccount {
    account("alice", "Alice")
}

fn bob() -> PlatformAccount {
    account("bob", "Bob")
}

fn snapshot(
    accounts: Vec<PlatformAccount>,
    current: &str,
) -> Result<StartupSnapshot, BackendError> {
    Ok(StartupSnapshot {
        accounts,
        current_account: current.into(),
    })
}

// ------------------------------------------------------------ add flow

#[test]
fn coalesces_duplicate_add_requests_while_setup_is_starting() {
    let hooks = Hooks("steam");
    let mut loader = AccountLoader::new(Some("steam".into()));

    let first = loader.begin_add().expect("first add starts");
    assert!(loader.begin_add().is_none(), "the duplicate is dropped");
    assert!(loader.adding());

    let outcome = loader.finish_add(
        &first,
        Ok(AddAccountResult {
            setup_status: Some(AddFlowStatus {
                setup_id: "setup-1".into(),
                state: "waiting_for_login".into(),
                account_id: None,
                account_display_name: None,
                error_message: None,
            }),
        }),
        "Steam",
        false,
        &hooks,
    );

    assert!(matches!(outcome, AddOutcome::SetupStarted(status) if status.setup_id == "setup-1"));
    assert!(!loader.adding());
}

// ------------------------------------------------------------ load and switch

fn riot_loader() -> AccountLoader {
    AccountLoader::new(Some("riot".into()))
}

#[test]
fn clears_loading_when_a_switch_lands_while_a_load_is_in_flight() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();

    let load = loader.begin_load(&hooks, LoadOptions::default());
    assert!(loader.loading());
    let switch = loader.begin_switch(&bob()).expect("switch starts");
    loader.finish_switch(&switch, Ok(()), &hooks);
    loader.finish_load(
        &load,
        snapshot(vec![alice(), bob()], "alice"),
        &hooks,
        None,
        0.0,
    );

    assert!(!loader.loading());
    assert_eq!(loader.accounts(), [alice(), bob()]);
}

#[test]
fn keeps_the_switched_account_over_a_snapshot_read_before_the_switch() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();

    let load = loader.begin_load(&hooks, LoadOptions::default());
    let switch = loader.begin_switch(&bob()).unwrap();
    loader.finish_switch(&switch, Ok(()), &hooks);
    loader.finish_load(
        &load,
        snapshot(vec![alice(), bob()], "alice"),
        &hooks,
        None,
        0.0,
    );

    assert_eq!(loader.current_account(), "bob");
}

#[test]
fn keeps_the_account_a_load_read_when_the_switch_is_refused() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();

    let load = loader.begin_load(&hooks, LoadOptions::default());
    let switch = loader.begin_switch(&bob()).unwrap();
    let outcome = loader.finish_switch(
        &switch,
        Err(BackendError::new("pin_locked: locked")),
        &hooks,
    );
    loader.finish_load(
        &load,
        snapshot(vec![alice(), bob()], "alice"),
        &hooks,
        None,
        0.0,
    );

    assert!(!outcome.succeeded);
    assert!(outcome.toasts.is_empty(), "a PIN refusal shows no toast");
    assert_eq!(loader.current_account(), "alice");
}

#[test]
fn still_lets_a_newer_load_supersede_an_older_one() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();

    let older = loader.begin_load(&hooks, LoadOptions::default());
    let newer = loader.begin_load(&hooks, LoadOptions::default());
    loader.finish_load(&newer, snapshot(vec![bob()], "bob"), &hooks, None, 0.0);
    let stale = loader.finish_load(&older, snapshot(vec![alice()], "alice"), &hooks, None, 0.0);

    assert!(!stale.applied);
    assert_eq!(loader.accounts(), [bob()]);
    assert_eq!(loader.current_account(), "bob");
    assert!(!loader.loading());
}

// ------------------------------------------------------------ platform change

#[test]
fn names_the_platform_the_accounts_came_from_not_the_one_active_at_the_end() {
    // The tab moved to another platform while Steam's load ran: the folder
    // sync after the load must target Steam.
    let steam = Hooks("steam");
    let riot = Hooks("riot");
    let mut loader = AccountLoader::new(Some("steam".into()));

    let load = loader.begin_load(&steam, LoadOptions::default());
    loader.set_adapter(Some("riot".into()));
    let outcome = loader.finish_load(&load, snapshot(vec![alice()], "alice"), &riot, None, 0.0);

    assert_eq!(outcome.after_load.as_deref(), Some("steam"));
}

// ------------------------------------------------------------ beyond the TS tests

#[test]
fn a_failed_load_clears_the_list_and_keeps_the_error() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();
    let first = loader.begin_load(&hooks, LoadOptions::default());
    loader.finish_load(&first, snapshot(vec![alice()], "alice"), &hooks, None, 0.0);

    let second = loader.begin_load(&hooks, LoadOptions::default());
    let outcome = loader.finish_load(&second, Err(BackendError::new("boom")), &hooks, None, 0.0);

    assert!(outcome.after_load.is_none());
    assert!(loader.accounts().is_empty());
    assert_eq!(loader.current_account(), "");
    assert_eq!(loader.error(), Some("boom"));
}

#[test]
fn an_add_without_setup_reloads_silently_and_holds_adding_until_it_lands() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();
    let add = loader.begin_add().unwrap();

    let AddOutcome::Reload(load) =
        loader.finish_add(&add, Ok(AddAccountResult::default()), "Riot", true, &hooks)
    else {
        panic!("expected a reload");
    };
    assert!(load.options.silent);
    assert!(loader.adding());

    loader.finish_load(&load, snapshot(vec![alice()], "alice"), &hooks, None, 0.0);
    assert!(!loader.adding());
}

#[test]
fn a_missing_client_gets_its_own_add_toast() {
    let hooks = Hooks("riot");
    let mut loader = riot_loader();
    let add = loader.begin_add().unwrap();
    let outcome = loader.finish_add(
        &add,
        Err(BackendError::new("Could not locate Riot Client executable")),
        "Riot",
        true,
        &hooks,
    );
    let AddOutcome::Failed(toast) = outcome else {
        panic!("expected a failure toast");
    };
    assert_eq!(
        toast.text,
        Text::with("toast.addAccountClientMissing", [("platform", "Riot")])
    );
}
