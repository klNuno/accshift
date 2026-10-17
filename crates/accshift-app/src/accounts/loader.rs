//! The accounts of the active tab, ported from `src/lib/shared/useAccountLoader.svelte.ts`.
//!
//! Each async TS function is split in two around its `await`: a `begin_*`
//! that changes the state the way TS does before awaiting and returns a
//! ticket, and a `finish_*` that takes the ticket and the job's result. The
//! generation checks TS makes after each `await` (`loadId`, `switchId`, the
//! prime run, the account epoch) are made on the ticket in `finish_*`, so a
//! result that lands late changes nothing it should not.
//!
//! What the adapter answers on the UI thread (cached warnings, cached
//! profiles, toast wording) comes through [`LoaderHooks`]; what blocks runs
//! on the job pool (see [`super::loading`]).

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
    /// `adapter.id`.
    fn platform_id(&self) -> &str;
    /// `getNoAccountsToastMessage`.
    fn no_accounts_toast(&self) -> Option<Text>;
    /// `getLoadErrorToastMessage`.
    fn load_error_toast(&self, message: &str) -> Option<Text>;
    /// `getSwitchErrorToastMessage`.
    fn switch_error_toast(&self, message: &str) -> Option<Text>;
    /// `getCachedWarningStates`; `None` when the adapter has none.
    fn cached_warning_states(&self) -> Option<BTreeMap<String, WarningPresentation>>;
    /// `loadWarningStates` exists.
    fn has_warnings(&self) -> bool;
}

/// The flags of `load(onAfterLoad, silent, showRefreshedToast, forceRefresh,
/// checkBans, deferBackground)`. `Default` is the TS defaults (all false).
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

/// `runBackgroundTasks`: the refresh of the visible accounts a load asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundRefresh {
    load_id: u64,
    pub check_bans: bool,
    pub force_refresh: bool,
    pub silent: bool,
    /// `deferBackground`: run it on a later frame (`requestIdleCallback`).
    pub deferred: bool,
}

/// What [`AccountLoader::finish_load`] asks of the caller.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadOutcome {
    /// False when a newer load or a platform change superseded this one.
    pub applied: bool,
    pub toasts: Vec<Toast>,
    /// `onAfterLoad(platformId)`: the platform the accounts came from.
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
    /// The account being left (`currentAccountId` before the switch).
    pub previous_account_id: Option<String>,
    pub platform_id: String,
}

/// What [`AccountLoader::finish_switch`] asks of the caller.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SwitchOutcome {
    /// What `switchTo` resolves to.
    pub succeeded: bool,
    pub toasts: Vec<Toast>,
    /// Steam: `triggerCs2BridgeCheck(previousAccountId)`.
    pub cs2_check: Option<String>,
    /// Fetch this account's profile and hand it to
    /// [`AccountLoader::finish_switch_profile`].
    pub fetch_profile: Option<PlatformAccount>,
}

/// An add in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTicket {
    pub platform_id: String,
}

/// What [`AccountLoader::finish_add`] resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    /// The add flow runs; `addFlow.start(platformId, status)`.
    SetupStarted(AddFlowStatus),
    /// `reloadAfterAdd`: run this load; `adding` stays set until it lands.
    Reload(LoadTicket),
    /// Nothing more to do.
    Done,
    /// The add failed with this toast.
    Failed(Toast),
}

/// A refresh of some accounts (`refreshAccounts`), between its prime guard
/// and its run.
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshTicket {
    run_id: u64,
    pub platform_id: String,
    pub targets: Vec<PlatformAccount>,
    pub check_bans: bool,
    pub force_refresh: bool,
    pub silent: bool,
    /// `deferBackground`.
    pub deferred: bool,
}

/// What a refresh run has to fetch, decided by [`AccountLoader::run_refresh`].
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshWork {
    pub ticket: RefreshTicket,
    /// `needsRefresh` of `loadProfilesForAccounts`; empty means no fetch.
    pub profiles: Vec<PlatformAccount>,
    /// `checkBans && adapter.loadWarningStates`.
    pub warnings: bool,
}

/// The profiles a refresh fetched.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileResults {
    /// `getProfileInfos` answered for `accounts` (`needsRefresh`): one
    /// commit, an id missing from `profiles` read as no data.
    Batch {
        accounts: Vec<PlatformAccount>,
        profiles: BTreeMap<String, Option<PlatformProfileInfo>>,
    },
    /// The per account path, in fetch order.
    Each(Vec<(PlatformAccount, Option<PlatformProfileInfo>)>),
}

/// Lets a job thread ask `shouldContinue()` while it runs.
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

/// `createAccountLoader`.
#[derive(Debug, Default)]
pub struct AccountLoader {
    /// `getAdapter()?.id`.
    adapter_id: Option<String>,
    accounts: Vec<PlatformAccount>,
    accounts_version: u64,
    current_account: String,
    loading: bool,
    adding: bool,
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
    /// The reload `addNew` awaits before it clears `adding`.
    add_reload_load_id: Option<u64>,
}

/// `resolveAccountsByIds`: the accounts of `ids`, in `ids` order, once each.
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
    /// A loader for `adapter_id`, `loading` set like the TS initial state.
    pub fn new(adapter_id: Option<String>) -> Self {
        Self {
            adapter_id,
            loading: true,
            ..Self::default()
        }
    }

    // -------------------------------------------------------- getters

    pub fn adapter_id(&self) -> Option<&str> {
        self.adapter_id.as_deref()
    }

    /// `getAdapter` changed (tab switch, registry reload). Call
    /// [`AccountLoader::clear_for_platform_change`] too when the tab moved.
    pub fn set_adapter(&mut self, adapter_id: Option<String>) {
        self.adapter_id = adapter_id;
    }

    pub fn accounts(&self) -> &[PlatformAccount] {
        &self.accounts
    }

    /// The `accounts` setter.
    pub fn set_accounts(&mut self, accounts: Vec<PlatformAccount>) {
        self.accounts = accounts;
        self.accounts_version += 1;
    }

    /// Bumped by each change of the account list.
    pub fn accounts_version(&self) -> u64 {
        self.accounts_version
    }

    /// `accountMap[id]`.
    pub fn account(&self, account_id: &str) -> Option<&PlatformAccount> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
    }

    pub fn current_account(&self) -> &str {
        &self.current_account
    }

    /// `currentAccountId`: the account whose id, username or display name
    /// matches the backend's current account, case-insensitive.
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

    /// `avatarStates`.
    pub fn avatars(&self) -> &AvatarStates {
        &self.avatars
    }

    pub fn avatars_mut(&mut self) -> &mut AvatarStates {
        &mut self.avatars
    }

    pub fn warning_states(&self) -> &BTreeMap<String, WarningPresentation> {
        &self.warning_states
    }

    /// Bumped by each commit that changed the warnings.
    pub fn warnings_version(&self) -> u64 {
        self.warnings_version
    }

    /// The per frame flush (`requestAnimationFrame`). True when the avatar
    /// states changed.
    pub fn flush(&mut self) -> bool {
        self.avatars.flush()
    }

    // -------------------------------------------------------- warnings

    /// `commitWarningStates`: skipped when nothing changed.
    fn commit_warning_states(&mut self, next: BTreeMap<String, WarningPresentation>) {
        if next == self.warning_states {
            return;
        }
        self.warning_states = next;
        self.warnings_version += 1;
    }

    /// `queueWarningStates`. TS coalesces a tick's results in a microtask;
    /// each result here is applied as it lands, which is the same last write.
    pub fn queue_warning_states(&mut self, next: BTreeMap<String, WarningPresentation>) {
        self.commit_warning_states(next);
    }

    /// `replaceWarningStates`.
    pub fn replace_warning_states(&mut self, next: BTreeMap<String, WarningPresentation>) {
        self.commit_warning_states(next);
    }

    // -------------------------------------------------------- guards

    /// `createPrimeGuard`: starts a new prime run.
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

    /// `shouldContinue()`: the same run, and the same adapter.
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

    /// `pruneOrphanedAvatarStates`.
    fn prune_orphaned_avatar_states(&mut self) {
        for id in self.avatars.ids() {
            if self.account(&id).is_none() {
                self.avatars.remove(&id);
            }
        }
    }

    // -------------------------------------------------------- load

    /// `load`, up to its first `await`. The caller runs
    /// `adapter.startup_snapshot` on the job pool and hands the answer to
    /// [`AccountLoader::finish_load`].
    pub fn begin_load(&mut self, hooks: &dyn LoaderHooks, options: LoadOptions) -> LoadTicket {
        self.latest_load_id += 1;
        let ticket = LoadTicket {
            load_id: self.latest_load_id,
            epoch: self.current_account_epoch,
            platform_id: hooks.platform_id().to_string(),
            options,
        };
        self.bump_prime_run();
        self.loading = true;
        self.error = None;
        self.replace_warning_states(hooks.cached_warning_states().unwrap_or_default());
        ticket
    }

    /// `load`, after the snapshot. `visible` is `getVisibleAccountIds()`
    /// (`None` when the loader has no such reader); `now_ms` drives the toast
    /// cooldowns.
    pub fn finish_load(
        &mut self,
        ticket: &LoadTicket,
        result: Result<StartupSnapshot, BackendError>,
        hooks: &dyn LoaderHooks,
        visible: Option<&[String]>,
        now_ms: f64,
    ) -> LoadOutcome {
        if self.add_reload_load_id == Some(ticket.load_id) {
            self.add_reload_load_id = None;
            self.adding = false;
        }
        let mut outcome = LoadOutcome::default();
        if ticket.load_id != self.latest_load_id {
            return outcome;
        }
        outcome.applied = true;
        let options = ticket.options;
        match result {
            Ok(snapshot) => {
                self.set_accounts(snapshot.accounts);
                if ticket.epoch == self.current_account_epoch {
                    self.current_account = snapshot.current_account;
                }
                let seed = self.resolve_visible(visible);
                self.avatars
                    .seed_for_accounts(&seed, options.force_refresh, hooks);
                if self.accounts.is_empty() {
                    if let Some(text) = hooks.no_accounts_toast() {
                        if now_ms - self.last_no_accounts_toast_at >= LOAD_TOAST_COOLDOWN_MS {
                            outcome.toasts.push(Toast::new(text, ToastKind::Default));
                            self.last_no_accounts_toast_at = now_ms;
                        }
                    }
                } else if options.show_refreshed_toast && !options.silent {
                    let count = self.accounts.len();
                    let key = if count == 1 {
                        "toast.accountsRefreshed.single"
                    } else {
                        "toast.accountsRefreshed.multiple"
                    };
                    outcome.toasts.push(Toast::new(
                        Text::with(key, [("count", count as f64)]),
                        ToastKind::Success,
                    ));
                }
                outcome.after_load = Some(ticket.platform_id.clone());
                outcome.background = Some(BackgroundRefresh {
                    load_id: ticket.load_id,
                    check_bans: options.check_bans,
                    force_refresh: options.force_refresh,
                    silent: options.silent,
                    deferred: options.defer_background,
                });
            }
            Err(error) => {
                let message = error.message;
                self.error = Some(message.clone());
                self.set_accounts(Vec::new());
                self.current_account.clear();
                self.replace_warning_states(BTreeMap::new());
                if let Some(text) = hooks.load_error_toast(&message) {
                    if now_ms - self.last_load_error_toast_at >= LOAD_TOAST_COOLDOWN_MS {
                        outcome.toasts.push(Toast::new(text, ToastKind::Error));
                        self.last_load_error_toast_at = now_ms;
                    }
                }
            }
        }
        self.loading = false;
        outcome
    }

    /// `runBackgroundTasks`: the refresh of the visible accounts, unless a
    /// newer load started. Never deferred again (`deferBackground = false`).
    pub fn run_background(
        &mut self,
        background: &BackgroundRefresh,
        visible: Option<&[String]>,
    ) -> Option<RefreshTicket> {
        if background.load_id != self.latest_load_id {
            return None;
        }
        self.refresh_visible_accounts(
            visible,
            background.check_bans,
            background.force_refresh,
            background.silent,
            false,
        )
    }

    // -------------------------------------------------------- switch

    /// `switchTo`, up to its `await`: `None` when a switch already runs or
    /// there is no adapter. The caller runs the switch on the job pool.
    pub fn begin_switch(&mut self, account: &PlatformAccount) -> Option<SwitchTicket> {
        let platform_id = self.adapter_id.clone()?;
        if self.switching {
            return None;
        }
        self.latest_switch_id += 1;
        let previous_account_id = self.current_account_id().map(str::to_string);
        self.switching = true;
        self.switching_account_id = Some(account.id.clone());
        self.error = None;
        Some(SwitchTicket {
            switch_id: self.latest_switch_id,
            account: account.clone(),
            previous_account_id,
            platform_id,
        })
    }

    /// `switchTo`, after the switch.
    pub fn finish_switch(
        &mut self,
        ticket: &SwitchTicket,
        result: Result<(), BackendError>,
        hooks: &dyn LoaderHooks,
    ) -> SwitchOutcome {
        let mut outcome = SwitchOutcome::default();
        if ticket.switch_id != self.latest_switch_id {
            return outcome;
        }
        let account = &ticket.account;
        match result {
            Ok(()) => {
                outcome.succeeded = true;
                // A load in flight read the current account before this
                // switch landed: the epoch keeps it from clobbering ours.
                self.current_account_epoch += 1;
                self.current_account = account.id.clone();
                if ticket.platform_id == "steam" {
                    if let Some(previous) = &ticket.previous_account_id {
                        if previous != &account.id {
                            outcome.cs2_check = Some(previous.clone());
                        }
                    }
                }
                if hooks.has_profile_info() {
                    self.avatars.update_state(
                        &account.id,
                        AvatarPatch {
                            refreshing: Some(true),
                            ..AvatarPatch::default()
                        },
                    );
                    outcome.fetch_profile = Some(account.clone());
                }
            }
            Err(error) => {
                // Refused for want of the PIN: the lock screen is up, nothing
                // failed that a toast could explain.
                if !error.is_pin_locked() {
                    let message = error.message;
                    self.error = Some(message.clone());
                    let text = hooks
                        .switch_error_toast(&message)
                        .unwrap_or_else(|| Text::key("toast.switchFailed"));
                    outcome.toasts.push(Toast::new(text, ToastKind::Error));
                    if let Some(cached) = hooks.cached_warning_states() {
                        self.replace_warning_states(cached);
                    }
                }
            }
        }
        self.switching = false;
        self.switching_account_id = None;
        outcome
    }

    /// The profile fetch after a switch. `None` is a failed fetch.
    pub fn finish_switch_profile(
        &mut self,
        account: &PlatformAccount,
        profile: Option<PlatformProfileInfo>,
    ) {
        // The account may have been removed while this fetch was in flight.
        if self.account(&account.id).is_none() {
            return;
        }
        let mut accounts = std::mem::take(&mut self.accounts);
        let changed = self
            .avatars
            .apply_profile_update(account, profile.as_ref(), &mut accounts);
        self.accounts = accounts;
        if changed {
            self.accounts_version += 1;
        }
    }

    // -------------------------------------------------------- add

    /// `addNew`, up to its `await`: `None` when an add already runs or there
    /// is no adapter.
    pub fn begin_add(&mut self) -> Option<AddTicket> {
        if self.adding {
            return None;
        }
        let platform_id = self.adapter_id.clone()?;
        self.adding = true;
        self.current_account.clear();
        Some(AddTicket { platform_id })
    }

    /// `addNew`, after `adapter.addAccount()`. `platform_name` is the
    /// registry name for the "client missing" toast; `reload_after_add` is
    /// the adapter's flag.
    pub fn finish_add(
        &mut self,
        _ticket: &AddTicket,
        result: Result<AddAccountResult, BackendError>,
        platform_name: &str,
        reload_after_add: bool,
        hooks: &dyn LoaderHooks,
    ) -> AddOutcome {
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                let message = error.message;
                self.error = Some(message.clone());
                self.adding = false;
                let text = if is_client_missing(&message) {
                    Text::with(
                        "toast.addAccountClientMissing",
                        [("platform", platform_name)],
                    )
                } else {
                    Text::key("toast.addAccountFailed")
                };
                return AddOutcome::Failed(Toast::new(text, ToastKind::Error));
            }
        };
        if let Some(status) = result.setup_status {
            self.adding = false;
            return AddOutcome::SetupStarted(status);
        }
        if reload_after_add {
            let ticket = self.begin_load(
                hooks,
                LoadOptions {
                    silent: true,
                    ..LoadOptions::default()
                },
            );
            self.add_reload_load_id = Some(ticket.load_id);
            return AddOutcome::Reload(ticket);
        }
        self.adding = false;
        AddOutcome::Done
    }

    // -------------------------------------------------------- refresh

    /// `refreshVisibleAccounts`.
    pub fn refresh_visible_accounts(
        &mut self,
        visible: Option<&[String]>,
        check_bans: bool,
        force_refresh: bool,
        silent: bool,
        deferred: bool,
    ) -> Option<RefreshTicket> {
        let targets = self.resolve_visible(visible);
        self.refresh_accounts(targets, check_bans, force_refresh, silent, deferred)
    }

    /// `refreshAccountIds` (`primeAccountIds`).
    pub fn refresh_account_ids(
        &mut self,
        account_ids: &[String],
        check_bans: bool,
        force_refresh: bool,
        silent: bool,
        deferred: bool,
    ) -> Option<RefreshTicket> {
        let targets = resolve_accounts_by_ids(&self.accounts, account_ids);
        self.refresh_accounts(targets, check_bans, force_refresh, silent, deferred)
    }

    /// `refreshAccounts`, up to its deferral: `None` (TS resolves 0) when
    /// there is nothing to refresh or no adapter. Starts a prime run.
    fn refresh_accounts(
        &mut self,
        targets: Vec<PlatformAccount>,
        check_bans: bool,
        force_refresh: bool,
        silent: bool,
        deferred: bool,
    ) -> Option<RefreshTicket> {
        if targets.is_empty() {
            return None;
        }
        let platform_id = self.adapter_id.clone()?;
        let run_id = self.new_prime_run();
        Some(RefreshTicket {
            run_id,
            platform_id,
            targets,
            check_bans,
            force_refresh,
            silent,
            deferred,
        })
    }

    /// `run()` of `refreshAccounts`: seeds the avatars and says what to fetch.
    /// `None` when the run was superseded (TS resolves 0).
    pub fn run_refresh(
        &mut self,
        ticket: RefreshTicket,
        hooks: &dyn LoaderHooks,
    ) -> Option<RefreshWork> {
        if !self.should_continue(&ticket) {
            return None;
        }
        let profiles = self
            .avatars
            .seed_for_accounts(&ticket.targets, ticket.force_refresh, hooks);
        let warnings = ticket.check_bans && hooks.has_warnings();
        Some(RefreshWork {
            ticket,
            profiles,
            warnings,
        })
    }

    /// The profile half of a run, landed. False when superseded.
    pub fn finish_refresh_profiles(
        &mut self,
        ticket: &RefreshTicket,
        results: ProfileResults,
    ) -> bool {
        let applied = self.should_continue(ticket);
        if applied {
            let mut accounts = std::mem::take(&mut self.accounts);
            let changed = match &results {
                ProfileResults::Batch {
                    accounts: targets,
                    profiles,
                } => self
                    .avatars
                    .apply_batch_profile_updates(targets, profiles, &mut accounts),
                ProfileResults::Each(list) => {
                    let mut changed = false;
                    for (account, profile) in list {
                        changed |= self.avatars.apply_profile_update(
                            account,
                            profile.as_ref(),
                            &mut accounts,
                        );
                    }
                    changed
                }
            };
            self.accounts = accounts;
            if changed {
                self.accounts_version += 1;
            }
        }
        self.prune_orphaned_avatar_states();
        applied
    }

    /// The warning half of a run, landed. False when superseded.
    pub fn finish_refresh_warnings(
        &mut self,
        ticket: &RefreshTicket,
        warnings: BTreeMap<String, WarningPresentation>,
    ) -> bool {
        let applied = self.should_continue(ticket);
        if applied {
            self.queue_warning_states(warnings);
        }
        self.prune_orphaned_avatar_states();
        applied
    }

    /// `prepareVisibleAccounts`.
    pub fn prepare_visible_accounts(
        &mut self,
        visible: Option<&[String]>,
        force_refresh: bool,
        hooks: &dyn LoaderHooks,
    ) -> usize {
        let targets = self.resolve_visible(visible);
        self.avatars
            .seed_for_accounts(&targets, force_refresh, hooks);
        targets.len()
    }

    /// `prepareAccountIds`.
    pub fn prepare_account_ids(
        &mut self,
        account_ids: &[String],
        force_refresh: bool,
        hooks: &dyn LoaderHooks,
    ) -> usize {
        let targets = resolve_accounts_by_ids(&self.accounts, account_ids);
        self.avatars
            .seed_for_accounts(&targets, force_refresh, hooks);
        targets.len()
    }

    // -------------------------------------------------------- removal

    /// `removeAccount`.
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

    /// `clearForPlatformChange`: every load, run and switch in flight is
    /// dropped.
    pub fn clear_for_platform_change(&mut self) {
        self.latest_load_id += 1;
        self.bump_prime_run();
        self.latest_switch_id += 1;
        self.set_accounts(Vec::new());
        self.current_account.clear();
        self.loading = true;
        self.error = None;
        self.switching = false;
        self.switching_account_id = None;
        self.avatars.clear();
        self.replace_warning_states(BTreeMap::new());
    }
}
