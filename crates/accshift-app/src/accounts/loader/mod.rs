//! The accounts of the active tab.
//!
//! Each async operation is split in two around its blocking call: a
//! `begin_*` that changes the state up front and returns a ticket, and a
//! `finish_*` that takes the ticket and the job's result. The generation
//! checks (the load id, the switch id, the prime run, the account epoch) are
//! made on the ticket in `finish_*`, so a result that lands late changes
//! nothing it should not.
//!
//! What the adapter answers on the UI thread (cached warnings, cached
//! profiles, toast wording) comes through [`LoaderHooks`]; what blocks runs
//! on the job pool (see [`super::loading`]).
//!
//! - [`load`]: loading the account list and the background refresh after it.
//! - [`switch`]: switching account and the profile fetch after it.
//! - [`add`]: the add-account flow.
//! - [`refresh`]: profile and warning refreshes of some accounts.
//! - here: the tickets, the state, its readers, warnings, guards and removal.

mod add;
mod load;
mod refresh;
mod switch;

use super::avatars::{AvatarPatch, AvatarStates, ProfileSource};
use super::{Toast, ToastKind};
use crate::error::BackendError;
use crate::platforms::warnings::WarningPresentation;
use crate::platforms::{
    AddAccountResult, AddFlowStatus, PlatformAccount, PlatformProfileInfo, StartupSnapshot,
};
use crate::text::Text;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// `LOAD_TOAST_COOLDOWN_MS`.
pub const LOAD_TOAST_COOLDOWN_MS: f64 = 30_000.0;

/// What the loader reads from the active adapter on the UI thread.
pub trait LoaderHooks: ProfileSource {
    /// The adapter's platform id.
    fn platform_id(&self) -> &str;
    /// The toast for a tab with no accounts.
    fn no_accounts_toast(&self) -> Option<Text>;
    /// The dedicated toast for a failed load, `None` for the generic one.
    fn load_error_toast(&self, message: &str) -> Option<Text>;
    /// The dedicated toast for a failed switch, `None` for the generic one.
    fn switch_error_toast(&self, message: &str) -> Option<Text>;
    /// The warnings known without a load; `None` when the adapter has none.
    fn cached_warning_states(&self) -> Option<BTreeMap<String, WarningPresentation>>;
    /// Whether the adapter loads warnings.
    fn has_warnings(&self) -> bool;
}

/// The flags of a load. `Default` is all false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadOptions {
    pub silent: bool,
    pub show_refreshed_toast: bool,
    pub force_refresh: bool,
    pub check_bans: bool,
    pub defer_background: bool,
}

/// A load in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadTicket {
    pub load_id: u64,
    epoch: u64,
    pub platform_id: String,
    pub options: LoadOptions,
}

/// The refresh of the visible accounts a load asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundRefresh {
    load_id: u64,
    pub check_bans: bool,
    pub force_refresh: bool,
    pub silent: bool,
    /// Run it on a later frame, once the UI is idle.
    pub deferred: bool,
}

/// What [`AccountLoader::finish_load`] asks of the caller.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadOutcome {
    /// False when a newer load or a platform change superseded this one.
    pub applied: bool,
    pub toasts: Vec<Toast>,
    /// The platform the accounts came from, for the after-load work.
    pub after_load: Option<String>,
    /// Hand to [`AccountLoader::run_background`] once the grid's visible ids
    /// reflect the new accounts.
    pub background: Option<BackgroundRefresh>,
}

/// A switch in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchTicket {
    switch_id: u64,
    pub account: PlatformAccount,
    /// The account being left.
    pub previous_account_id: Option<String>,
    pub platform_id: String,
}

/// What [`AccountLoader::finish_switch`] asks of the caller.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SwitchOutcome {
    /// Whether the switch went through.
    pub succeeded: bool,
    pub toasts: Vec<Toast>,
    /// Steam: the previous account, whose CS2 data needs a re-check.
    pub cs2_check: Option<String>,
    /// Fetch this account's profile and hand it to
    /// [`AccountLoader::finish_switch_profile`].
    pub fetch_profile: Option<PlatformAccount>,
}

/// An add in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTicket {
    pub platform_id: String,
    epoch: u64,
}

/// What [`AccountLoader::finish_add`] resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    /// The platform started its own setup; the add flow takes over.
    SetupStarted(AddFlowStatus),
    /// Run this load; `adding` stays set until it lands.
    Reload(LoadTicket),
    /// Nothing more to do.
    Done,
    /// The add failed with this toast.
    Failed(Toast),
}

/// A refresh of some accounts, between its prime guard and its run.
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshTicket {
    run_id: u64,
    pub platform_id: String,
    pub targets: Vec<PlatformAccount>,
    pub check_bans: bool,
    pub force_refresh: bool,
    pub silent: bool,
    /// Run it on a later frame.
    pub deferred: bool,
}

/// What a refresh run has to fetch, decided by [`AccountLoader::run_refresh`].
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshWork {
    pub ticket: RefreshTicket,
    /// The accounts whose profile needs a fetch; empty means no fetch.
    pub profiles: Vec<PlatformAccount>,
    /// Ban checks are on and the platform has warnings to load.
    pub warnings: bool,
}

/// The profiles a refresh fetched.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileResults {
    /// The batch fetch answered for `accounts`: one commit, an id missing
    /// from `profiles` read as no data.
    Batch {
        accounts: Vec<PlatformAccount>,
        profiles: BTreeMap<String, Option<PlatformProfileInfo>>,
    },
    /// The per account path, in fetch order.
    Each(Vec<(PlatformAccount, Option<PlatformProfileInfo>)>),
}

/// Lets a job thread ask whether its run is still the latest while it runs.
#[derive(Debug, Clone)]
pub struct PrimeGuard {
    latest: Arc<AtomicU64>,
    run_id: u64,
}

impl PrimeGuard {
    /// False once a newer load, refresh or platform change started.
    pub fn is_current(&self) -> bool {
        self.latest.load(Ordering::SeqCst) == self.run_id
    }
}

/// The accounts of the active tab and every operation on them.
#[derive(Debug, Default)]
pub struct AccountLoader {
    /// The active adapter's platform id.
    adapter_id: Option<String>,
    accounts: Vec<PlatformAccount>,
    accounts_version: u64,
    current_account: String,
    loading: bool,
    adding: bool,
    add_epoch: u64,
    switching: bool,
    switching_account_id: Option<String>,
    error: Option<String>,
    avatars: AvatarStates,
    warning_states: BTreeMap<String, WarningPresentation>,
    warnings_version: u64,
    last_load_error_toast_at: f64,
    last_no_accounts_toast_at: f64,
    latest_load_id: u64,
    latest_prime_run: Arc<AtomicU64>,
    latest_switch_id: u64,
    current_account_epoch: u64,
    /// The reload an add waits for before it clears `adding`.
    add_reload_load_id: Option<u64>,
}

/// The accounts of `ids`, in `ids` order, once each.
pub fn resolve_accounts_by_ids(source: &[PlatformAccount], ids: &[String]) -> Vec<PlatformAccount> {
    let mut seen = HashSet::new();
    ids.iter()
        .filter(|id| seen.insert(id.as_str()))
        .filter_map(|id| source.iter().find(|account| &account.id == id).cloned())
        .collect()
}

fn is_client_missing(message: &str) -> bool {
    // `/could not locate .* executable/i`
    let lower = message.to_lowercase();
    lower
        .find("could not locate ")
        .is_some_and(|start| lower[start + "could not locate ".len()..].contains(" executable"))
}

impl AccountLoader {
    /// A loader for `adapter_id`, `loading` set until the first load lands.
    pub fn new(adapter_id: Option<String>) -> Self {
        Self {
            adapter_id,
            loading: true,
            ..Self::default()
        }
    }

    pub fn adapter_id(&self) -> Option<&str> {
        self.adapter_id.as_deref()
    }

    /// The adapter changed (tab switch, registry reload). Call
    /// [`AccountLoader::clear_for_platform_change`] too when the tab moved.
    pub fn set_adapter(&mut self, adapter_id: Option<String>) {
        self.adapter_id = adapter_id;
    }

    pub fn accounts(&self) -> &[PlatformAccount] {
        &self.accounts
    }

    /// Replaces the accounts and bumps their version.
    pub fn set_accounts(&mut self, accounts: Vec<PlatformAccount>) {
        self.accounts = accounts;
        self.accounts_version += 1;
    }

    /// Bumped by each change of the account list.
    pub fn accounts_version(&self) -> u64 {
        self.accounts_version
    }

    /// The loaded account with this id.
    pub fn account(&self, account_id: &str) -> Option<&PlatformAccount> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
    }

    pub fn current_account(&self) -> &str {
        &self.current_account
    }

    /// The account whose id, username or display name matches the backend's
    /// current account, case-insensitive.
    pub fn current_account_id(&self) -> Option<&str> {
        let raw = crate::js::trim(&self.current_account);
        if raw.is_empty() {
            return None;
        }
        let needle = raw.to_lowercase();
        let same = |value: &str| crate::js::trim(value).to_lowercase() == needle;
        if let Some(direct) = self.accounts.iter().find(|account| same(&account.id)) {
            return Some(&direct.id);
        }
        self.accounts
            .iter()
            .find(|account| same(&account.username) || same(&account.display_name))
            .map(|account| account.id.as_str())
    }

    pub fn loading(&self) -> bool {
        self.loading
    }

    pub fn adding(&self) -> bool {
        self.adding
    }

    pub fn switching(&self) -> bool {
        self.switching
    }

    pub fn switching_account_id(&self) -> Option<&str> {
        self.switching_account_id.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The avatar state of every account.
    pub fn avatars(&self) -> &AvatarStates {
        &self.avatars
    }

    pub fn warning_states(&self) -> &BTreeMap<String, WarningPresentation> {
        &self.warning_states
    }

    /// Bumped by each commit that changed the warnings.
    pub fn warnings_version(&self) -> u64 {
        self.warnings_version
    }

    /// The per frame flush. True when the avatar states changed.
    pub fn flush(&mut self) -> bool {
        self.avatars.flush()
    }

    /// Skipped when nothing changed.
    fn commit_warning_states(&mut self, next: BTreeMap<String, WarningPresentation>) {
        if next == self.warning_states {
            return;
        }
        self.warning_states = next;
        self.warnings_version += 1;
    }

    /// Applies a warning load's result as it lands.
    pub fn queue_warning_states(&mut self, next: BTreeMap<String, WarningPresentation>) {
        self.commit_warning_states(next);
    }

    /// Replaces the warning map.
    pub fn replace_warning_states(&mut self, next: BTreeMap<String, WarningPresentation>) {
        self.commit_warning_states(next);
    }

    /// Starts a new prime run.
    fn new_prime_run(&mut self) -> u64 {
        self.latest_prime_run.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn bump_prime_run(&mut self) {
        self.latest_prime_run.fetch_add(1, Ordering::SeqCst);
    }

    /// The guard of `ticket`, for a job thread.
    pub fn prime_guard(&self, ticket: &RefreshTicket) -> PrimeGuard {
        PrimeGuard {
            latest: Arc::clone(&self.latest_prime_run),
            run_id: ticket.run_id,
        }
    }

    /// Whether the run is still the latest, on the same adapter.
    pub fn should_continue(&self, ticket: &RefreshTicket) -> bool {
        self.latest_prime_run.load(Ordering::SeqCst) == ticket.run_id
            && self.adapter_id.as_deref() == Some(ticket.platform_id.as_str())
    }

    fn resolve_visible(&self, visible: Option<&[String]>) -> Vec<PlatformAccount> {
        match visible {
            Some(ids) => resolve_accounts_by_ids(&self.accounts, ids),
            None => self.accounts.clone(),
        }
    }

    /// Drops the avatar states of accounts that are gone.
    fn prune_orphaned_avatar_states(&mut self) {
        for id in self.avatars.ids() {
            if self.account(&id).is_none() {
                self.avatars.remove(&id);
            }
        }
    }

    /// Takes an account out of the list, its avatar and its warning.
    pub fn remove_account(&mut self, account_id: &str) {
        let before = self.accounts.len();
        self.accounts.retain(|account| account.id != account_id);
        if self.accounts.len() != before {
            self.accounts_version += 1;
        }
        if self.current_account == account_id {
            self.current_account.clear();
        }
        self.avatars.remove(account_id);
        self.prune_orphaned_avatar_states();
        let mut rest = self.warning_states.clone();
        rest.remove(account_id);
        self.replace_warning_states(rest);
    }

    /// Invalidate loads, runs, adds and switches from the previous tab.
    pub fn clear_for_platform_change(&mut self) {
        self.latest_load_id += 1;
        self.bump_prime_run();
        self.latest_switch_id += 1;
        self.set_accounts(Vec::new());
        self.current_account.clear();
        self.loading = true;
        self.add_epoch += 1;
        self.adding = false;
        self.add_reload_load_id = None;
        self.error = None;
        self.switching = false;
        self.switching_account_id = None;
        self.avatars.clear();
        self.replace_warning_states(BTreeMap::new());
    }
}
