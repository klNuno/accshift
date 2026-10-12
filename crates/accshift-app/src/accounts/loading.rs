//! The account loader of the active tab and everything that drives it,
//! ported from `src/lib/app/useAccountLoading.svelte.ts`, plus
//! `src/lib/app/detectedPlatforms.svelte.ts`.
//!
//! [`AccountLoading`] owns the [`AccountLoader`], the [`VisiblePriming`] and
//! the profile caches, starts every blocking call on [`Jobs`] and keeps the
//! [`Pending`] handles. The shell calls [`AccountLoading::poll`] once per
//! frame: it applies the results that landed, runs the work deferred to that
//! frame and returns [`LoadingEvent`]s (toasts, the add flow start, the
//! window minimize). Nothing here blocks the UI thread.
//!
//! Adapters are built up front by the registry, so `ensureAdapterReady` is a
//! lookup and `adapterLoading` is always false.

use super::avatars::{ProfileSource, BATCH_SIZE};
use super::loader::{
    AccountLoader, AddOutcome, AddTicket, BackgroundRefresh, LoadOptions, LoadTicket, LoaderHooks,
    ProfileResults, RefreshTicket, RefreshWork, SwitchTicket,
};
use super::priming::VisiblePriming;
use super::profile_cache::{CacheExpiry, CacheWrite, Fetched, ProfileCaches};
use super::refresh::has_profile_info;
use super::{LoadingEvent, Toast, ToastKind};
use crate::error::{BackendError, BackendResult};
use crate::jobs::{Jobs, Pending, Poll};
use crate::platforms::roblox::RobloxSessions;
use crate::platforms::steam::warnings::{self as steam_warnings, BanFetch, BanPlan, BanStep};
use crate::platforms::warnings::{WarningLoad, WarningPresentation};
use crate::platforms::{
    AddAccountResult, PlatformAccount, PlatformAdapter, PlatformProfileInfo, PlatformRegistry,
    RuntimeOs, StartupSnapshot, WarningLoadOptions,
};
use crate::stores::ClientStores;
use crate::text::Text;
use crate::time::Clock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

/// `isPlatformUsable`: implemented and supported on this OS.
pub fn is_platform_usable(registry: &PlatformRegistry, platform_id: &str, os: RuntimeOs) -> bool {
    registry
        .definition(platform_id)
        .is_some_and(|def| def.implemented && def.supports(os))
}

/// `detectedPlatforms.svelte.ts`: what launcher detection found this launch,
/// set once during boot on a fresh install. Empty means detection did not
/// run or found nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedPlatforms(Vec<String>);

impl DetectedPlatforms {
    /// `setDetectedPlatforms`.
    pub fn set(&mut self, platform_ids: &[String]) {
        self.0 = platform_ids.to_vec();
    }

    /// `getDetectedPlatforms`.
    pub fn get(&self) -> &[String] {
        &self.0
    }
}

/// [`LoaderHooks`] over a real adapter and the client stores.
pub struct AdapterHooks<'a> {
    pub adapter: &'a PlatformAdapter,
    pub stores: &'a ClientStores,
    pub caches: &'a ProfileCaches,
    pub expiry: CacheExpiry,
}

impl ProfileSource for AdapterHooks<'_> {
    fn cached_profile(&self, account_id: &str) -> Option<crate::platforms::CachedPlatformProfile> {
        self.caches.cached_profile(
            self.adapter.id(),
            self.stores.storage(),
            account_id,
            &self.expiry,
        )
    }

    fn has_profile_info(&self) -> bool {
        has_profile_info(self.adapter)
    }
}

impl LoaderHooks for AdapterHooks<'_> {
    fn platform_id(&self) -> &str {
        self.adapter.id()
    }

    fn no_accounts_toast(&self) -> Option<Text> {
        self.adapter.no_accounts_toast()
    }

    fn load_error_toast(&self, message: &str) -> Option<Text> {
        self.adapter.load_error_toast(message)
    }

    fn switch_error_toast(&self, message: &str) -> Option<Text> {
        self.adapter.switch_error_toast(message)
    }

    fn cached_warning_states(&self) -> Option<BTreeMap<String, WarningPresentation>> {
        self.adapter
            .has_warnings()
            .then(|| self.adapter.cached_warning_states(self.stores))
    }

    fn has_warnings(&self) -> bool {
        self.adapter.has_warnings()
    }
}

/// What [`AccountLoading::poll`] and the handlers read from the app.
pub struct LoadingContext<'a> {
    pub jobs: &'a Jobs,
    pub registry: &'a PlatformRegistry,
    pub stores: &'a mut ClientStores,
    /// `shell.activeTab`.
    pub active_tab: &'a str,
    pub runtime_os: RuntimeOs,
    /// The grid's `visibleRenderedAccountIds`, from the display pipeline.
    pub visible_ids: &'a [String],
    /// `navigation.isSearching`.
    pub is_searching: bool,
    /// Unix milliseconds for toasts cooldowns and cache stamps.
    pub clock: &'a Clock,
    /// Frame time for the priming deadlines.
    pub now: Instant,
}

impl LoadingContext<'_> {
    fn now_ms(&self) -> f64 {
        self.clock.now_ms()
    }

    fn adapter(&self) -> Option<Arc<PlatformAdapter>> {
        self.registry.adapter(self.active_tab)
    }
}

enum WarningJob {
    SteamBans {
        plan: BanPlan,
        pending: Pending<BanFetch>,
    },
    RobloxProbe(Pending<Result<Vec<String>, BackendError>>),
}

type ProfileJob = Pending<(ProfileResults, Vec<CacheWrite>)>;

/// `createAccountLoading`.
pub struct AccountLoading {
    pub loader: AccountLoader,
    pub priming: VisiblePriming,
    pub detected: DetectedPlatforms,
    caches: Arc<ProfileCaches>,
    loads: Vec<(LoadTicket, Pending<BackendResult<StartupSnapshot>>)>,
    switches: Vec<(SwitchTicket, Pending<BackendResult<()>>)>,
    switch_profiles: Vec<(
        PlatformAccount,
        Pending<Fetched<Option<PlatformProfileInfo>>>,
    )>,
    adds: Vec<(AddTicket, Pending<BackendResult<AddAccountResult>>)>,
    profiles: Vec<(RefreshTicket, ProfileJob)>,
    warnings: Vec<(RefreshTicket, WarningJob)>,
    /// Background refreshes waiting for the next frame's visible ids.
    deferred_background: Vec<BackgroundRefresh>,
    /// Refresh runs deferred to the next frame (`requestIdleCallback`).
    deferred_runs: Vec<RefreshTicket>,
}

impl AccountLoading {
    pub fn new(active_tab: Option<String>, caches: Arc<ProfileCaches>) -> Self {
        Self {
            loader: AccountLoader::new(active_tab),
            priming: VisiblePriming::new(),
            detected: DetectedPlatforms::default(),
            caches,
            loads: Vec::new(),
            switches: Vec::new(),
            switch_profiles: Vec::new(),
            adds: Vec::new(),
            profiles: Vec::new(),
            warnings: Vec::new(),
            deferred_background: Vec::new(),
            deferred_runs: Vec::new(),
        }
    }

    pub fn caches(&self) -> &Arc<ProfileCaches> {
        &self.caches
    }

    /// `adapterLoading`: adapters are never loaded lazily here.
    pub fn adapter_loading(&self) -> bool {
        false
    }

    /// Jobs or deferred work still to land.
    pub fn has_pending_work(&self) -> bool {
        !(self.loads.is_empty()
            && self.switches.is_empty()
            && self.switch_profiles.is_empty()
            && self.adds.is_empty()
            && self.profiles.is_empty()
            && self.warnings.is_empty()
            && self.deferred_background.is_empty()
            && self.deferred_runs.is_empty())
            || self.loader.avatars().has_pending()
    }

    /// The next deadline [`AccountLoading::poll`] has to run at (request a
    /// repaint for it). Deferred work wants the very next frame.
    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let soon = (!self.deferred_background.is_empty()
            || !self.deferred_runs.is_empty()
            || self.loader.avatars().has_pending())
        .then_some(now);
        match (soon, self.priming.next_deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn expiry(&self, ctx: &LoadingContext<'_>) -> CacheExpiry {
        self.caches
            .expiry(ctx.stores.cache_duration_ms(), ctx.now_ms())
    }

    /// The tab changed: drop everything in flight and point the loader at
    /// `platform_id`'s adapter.
    pub fn switch_tab(&mut self, registry: &PlatformRegistry, platform_id: &str) {
        let adapter = registry.adapter(platform_id).map(|a| a.id().to_string());
        self.loader.set_adapter(adapter);
        self.loader.clear_for_platform_change();
        self.priming.reset();
        self.deferred_background.clear();
        self.deferred_runs.clear();
    }

    // -------------------------------------------------------- handlers

    /// `loadAccounts`. False when the tab is not usable or has no adapter.
    pub fn load_accounts(&mut self, ctx: &mut LoadingContext<'_>, options: LoadOptions) -> bool {
        if !is_platform_usable(ctx.registry, ctx.active_tab, ctx.runtime_os) {
            return false;
        }
        let Some(adapter) = ctx.adapter() else {
            return false;
        };
        self.loader.set_adapter(Some(adapter.id().to_string()));
        let caches = Arc::clone(&self.caches);
        let hooks = AdapterHooks {
            adapter: &adapter,
            stores: ctx.stores,
            caches: &caches,
            expiry: self.expiry(ctx),
        };
        let ticket = self.loader.begin_load(&hooks, options);
        self.spawn_load(ctx, adapter, ticket);
        true
    }

    fn spawn_load(
        &mut self,
        ctx: &LoadingContext<'_>,
        adapter: Arc<PlatformAdapter>,
        ticket: LoadTicket,
    ) {
        let pending = ctx
            .jobs
            .run(move |backend| adapter.startup_snapshot(backend));
        self.loads.push((ticket, pending));
    }

    /// `loadPlatformAccounts`: accounts of any platform, not just the active
    /// tab (personas, deep links). `None` for an unknown platform.
    pub fn load_platform_accounts(
        jobs: &Jobs,
        registry: &PlatformRegistry,
        platform_id: &str,
    ) -> Option<Pending<BackendResult<Vec<PlatformAccount>>>> {
        let adapter = registry.adapter(platform_id)?;
        Some(jobs.run(move |backend| adapter.load_accounts(backend)))
    }

    /// `handleRefreshClick`.
    pub fn handle_refresh_click(&mut self, ctx: &mut LoadingContext<'_>) -> bool {
        self.load_accounts(
            ctx,
            LoadOptions {
                silent: false,
                show_refreshed_toast: true,
                force_refresh: false,
                check_bans: true,
                defer_background: true,
            },
        )
    }

    /// `handleAddAccountClick` and `handleAddAccount`. `add_flow_active` is
    /// `addFlow.flow` being set.
    pub fn handle_add_account_click(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        add_flow_active: bool,
    ) -> bool {
        if !is_platform_usable(ctx.registry, ctx.active_tab, ctx.runtime_os)
            || self.loader.adding()
            || add_flow_active
        {
            return false;
        }
        let Some(adapter) = ctx.adapter() else {
            return false;
        };
        let Some(ticket) = self.loader.begin_add() else {
            return false;
        };
        let settings = ctx.stores.settings().clone();
        let pending = ctx
            .jobs
            .run(move |backend| adapter.add_account(backend, &settings));
        self.adds.push((ticket, pending));
        true
    }

    /// `handleAccountSwitch`: false when a persona switch runs, a switch is
    /// already running or there is no adapter. The result arrives as
    /// [`LoadingEvent::SwitchFinished`].
    pub fn handle_account_switch(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        account: &PlatformAccount,
        persona_switching: bool,
    ) -> bool {
        if persona_switching {
            return false;
        }
        let Some(adapter) = ctx.adapter() else {
            return false;
        };
        let Some(ticket) = self.loader.begin_switch(account) else {
            return false;
        };
        let settings = ctx.stores.settings().clone();
        let target = account.clone();
        let pending = ctx
            .jobs
            .run(move |backend| adapter.switch_account(backend, &target, &settings));
        self.switches.push((ticket, pending));
        true
    }

    /// `trackVisiblePriming`'s effect: call every frame.
    pub fn track_priming(&mut self, ctx: &LoadingContext<'_>, grid_hidden: bool) {
        if ctx.adapter().is_none() {
            self.priming.reset();
            return;
        }
        if self.loader.loading() || grid_hidden || ctx.visible_ids.is_empty() {
            self.priming.pause();
            return;
        }
        self.priming
            .process_visible(ctx.visible_ids, ctx.active_tab, ctx.is_searching, ctx.now);
    }

    // -------------------------------------------------------- frame

    /// Applies every finished job, runs the deferred work and the priming
    /// timer, then flushes the avatar states. Call once per frame.
    pub fn poll(&mut self, ctx: &mut LoadingContext<'_>) -> Vec<LoadingEvent> {
        let mut events = Vec::new();
        self.run_deferred(ctx, &mut events);
        self.poll_loads(ctx, &mut events);
        self.poll_switches(ctx, &mut events);
        self.poll_adds(ctx, &mut events);
        self.poll_profiles(ctx);
        self.poll_warnings(ctx, &mut events);
        if let Some(request) = self.priming.poll(ctx.now) {
            if let Some(adapter) = ctx.adapter() {
                let caches = Arc::clone(&self.caches);
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader
                    .prepare_account_ids(&request.visible_ids, false, &hooks);
            }
            if !request.due_ids.is_empty() {
                if let Some(ticket) =
                    self.loader
                        .refresh_account_ids(&request.due_ids, true, false, true, true)
                {
                    self.deferred_runs.push(ticket);
                }
            }
        }
        self.loader.flush();
        events
    }

    fn run_deferred(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        for background in std::mem::take(&mut self.deferred_background) {
            let visible = ctx.visible_ids.to_vec();
            if let Some(ticket) = self.loader.run_background(&background, Some(&visible)) {
                self.start_run(ctx, ticket, events);
            }
        }
        for ticket in std::mem::take(&mut self.deferred_runs) {
            self.start_run(ctx, ticket, events);
        }
    }

    fn start_run(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        ticket: RefreshTicket,
        events: &mut Vec<LoadingEvent>,
    ) {
        let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
            return;
        };
        let caches = Arc::clone(&self.caches);
        let work = {
            let hooks = AdapterHooks {
                adapter: &adapter,
                stores: ctx.stores,
                caches: &caches,
                expiry: self.expiry(ctx),
            };
            self.loader.run_refresh(ticket, &hooks)
        };
        if let Some(work) = work {
            self.start_work(ctx, adapter, work, events);
        }
    }

    fn start_work(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        adapter: Arc<PlatformAdapter>,
        work: RefreshWork,
        events: &mut Vec<LoadingEvent>,
    ) {
        let RefreshWork {
            ticket,
            profiles,
            warnings,
        } = work;
        if !profiles.is_empty() {
            let guard = self.loader.prime_guard(&ticket);
            let caches = Arc::clone(&self.caches);
            let clock = ctx.clock.clone();
            let adapter = Arc::clone(&adapter);
            let pending = ctx.jobs.run(move |backend| {
                let now_ms = || clock.now_ms();
                if adapter.id() == "steam" {
                    let ids: Vec<String> = profiles.iter().map(|a| a.id.clone()).collect();
                    let fetched = caches.steam_profile_infos(backend, &ids, &now_ms);
                    return (
                        ProfileResults::Batch {
                            accounts: profiles,
                            profiles: fetched.value,
                        },
                        fetched.writes,
                    );
                }
                let mut results = Vec::new();
                let mut writes = Vec::new();
                for chunk in profiles.chunks(BATCH_SIZE) {
                    if !guard.is_current() {
                        break;
                    }
                    let fetched: Vec<_> = std::thread::scope(|scope| {
                        let handles: Vec<_> = chunk
                            .iter()
                            .map(|account| {
                                let caches = &caches;
                                let adapter = &adapter;
                                let now_ms = &now_ms;
                                scope.spawn(move || {
                                    caches.profile_info(adapter, backend, &account.id, now_ms)
                                })
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|handle| handle.join().ok())
                            .collect()
                    });
                    for (account, fetched) in chunk.iter().zip(fetched) {
                        let fetched = fetched.unwrap_or(Fetched {
                            value: None,
                            writes: Vec::new(),
                        });
                        writes.extend(fetched.writes);
                        results.push((account.clone(), fetched.value));
                    }
                }
                (ProfileResults::Each(results), writes)
            });
            self.profiles.push((ticket.clone(), pending));
        }
        if warnings {
            self.start_warnings(ctx, &adapter, ticket, events);
        }
    }

    fn start_warnings(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        adapter: &Arc<PlatformAdapter>,
        ticket: RefreshTicket,
        events: &mut Vec<LoadingEvent>,
    ) {
        let options = WarningLoadOptions {
            force_refresh: ticket.force_refresh,
            silent: ticket.silent,
        };
        match adapter.id() {
            "steam" => {
                let settings = ctx.stores.settings().clone();
                let step = adapter.steam_ban_plan(
                    ctx.stores,
                    &settings,
                    &ticket.targets,
                    options,
                    ctx.now_ms(),
                );
                match step {
                    Some(BanStep::Done(load)) => self.land_warnings(&ticket, load, events),
                    Some(BanStep::Fetch(plan)) => {
                        let job_plan = plan.clone();
                        let pending = ctx
                            .jobs
                            .run(move |backend| steam_warnings::fetch(&job_plan, backend));
                        self.warnings
                            .push((ticket, WarningJob::SteamBans { plan, pending }));
                    }
                    None => {}
                }
            }
            "roblox" => {
                let settings = ctx.stores.settings().clone();
                let needs_probe = adapter.state().roblox_sessions.needs_probe(
                    &settings,
                    &ticket.targets,
                    options.force_refresh,
                );
                if needs_probe {
                    let pending = ctx.jobs.run(RobloxSessions::probe);
                    self.warnings
                        .push((ticket, WarningJob::RobloxProbe(pending)));
                } else {
                    let load = adapter.state().roblox_sessions.warning_map();
                    self.land_warnings(&ticket, load, events);
                }
            }
            _ => {}
        }
    }

    fn land_warnings(
        &mut self,
        ticket: &RefreshTicket,
        load: WarningLoad,
        events: &mut Vec<LoadingEvent>,
    ) {
        for request in load.toasts {
            let kind = if request.error {
                ToastKind::Error
            } else {
                ToastKind::Default
            };
            events.push(LoadingEvent::Toast(Toast::new(request.text, kind)));
        }
        self.loader.finish_refresh_warnings(ticket, load.warnings);
    }

    fn poll_loads(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.loads) {
            let result = match pending.poll() {
                Poll::Waiting => {
                    waiting.push((ticket, pending));
                    continue;
                }
                Poll::Ready(result) => result,
                Poll::Gone => Err(BackendError::new("account load job died")),
            };
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            let caches = Arc::clone(&self.caches);
            let visible = ctx.visible_ids.to_vec();
            let outcome = {
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader
                    .finish_load(&ticket, result, &hooks, Some(&visible), ctx.now_ms())
            };
            events.extend(outcome.toasts.into_iter().map(LoadingEvent::Toast));
            if let Some(platform_id) = outcome.after_load {
                let ids: Vec<String> = self
                    .loader
                    .accounts()
                    .iter()
                    .map(|account| account.id.clone())
                    .collect();
                ctx.stores.sync_accounts(&ids, &platform_id);
                events.push(LoadingEvent::AccountsLoaded { platform_id });
            }
            // Deferred or not, the background refresh reads the visible ids
            // of the next frame, once the grid shows the new accounts.
            if let Some(background) = outcome.background {
                self.deferred_background.push(background);
            }
        }
        self.loads.extend(waiting);
    }

    fn poll_switches(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.switches) {
            let result = match pending.poll() {
                Poll::Waiting => {
                    waiting.push((ticket, pending));
                    continue;
                }
                Poll::Ready(result) => result,
                Poll::Gone => Err(BackendError::new("account switch job died")),
            };
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            let caches = Arc::clone(&self.caches);
            let outcome = {
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader.finish_switch(&ticket, result, &hooks)
            };
            events.extend(outcome.toasts.into_iter().map(LoadingEvent::Toast));
            if let Some(steam_id) = outcome.cs2_check {
                events.push(LoadingEvent::Cs2Check { steam_id });
            }
            if let Some(account) = outcome.fetch_profile {
                let clock = ctx.clock.clone();
                let adapter = Arc::clone(&adapter);
                let account_id = account.id.clone();
                let pending = ctx.jobs.run(move |backend| {
                    caches.profile_info(&adapter, backend, &account_id, &|| clock.now_ms())
                });
                self.switch_profiles.push((account, pending));
            }
            events.push(LoadingEvent::SwitchFinished {
                account_id: ticket.account.id.clone(),
                succeeded: outcome.succeeded,
            });
            if outcome.succeeded && ctx.stores.settings().minimize_on_account_switch {
                events.push(LoadingEvent::MinimizeWindow);
            }
        }
        self.switches.extend(waiting);

        let mut waiting = Vec::new();
        for (account, mut pending) in std::mem::take(&mut self.switch_profiles) {
            match pending.poll() {
                Poll::Waiting => waiting.push((account, pending)),
                Poll::Ready(fetched) => {
                    if let Some(adapter_id) = self.loader.adapter_id().map(str::to_string) {
                        self.caches.apply_writes(
                            &adapter_id,
                            ctx.stores.storage_mut(),
                            fetched.writes,
                        );
                    }
                    self.loader.finish_switch_profile(&account, fetched.value);
                }
                Poll::Gone => self.loader.finish_switch_profile(&account, None),
            }
        }
        self.switch_profiles.extend(waiting);
    }

    fn poll_adds(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.adds) {
            let result = match pending.poll() {
                Poll::Waiting => {
                    waiting.push((ticket, pending));
                    continue;
                }
                Poll::Ready(result) => result,
                Poll::Gone => Err(BackendError::new("add account job died")),
            };
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            let name = ctx
                .registry
                .definition(&ticket.platform_id)
                .map_or_else(|| ticket.platform_id.clone(), |def| def.name.clone());
            let caches = Arc::clone(&self.caches);
            let outcome = {
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader
                    .finish_add(&ticket, result, &name, adapter.reload_after_add(), &hooks)
            };
            match outcome {
                AddOutcome::SetupStarted(status) => events.push(LoadingEvent::AddFlowStarted {
                    platform_id: ticket.platform_id.clone(),
                    status,
                }),
                AddOutcome::Reload(load) => self.spawn_load(ctx, adapter, load),
                AddOutcome::Failed(toast) => events.push(LoadingEvent::Toast(toast)),
                AddOutcome::Done => {}
            }
        }
        self.adds.extend(waiting);
    }

    fn poll_profiles(&mut self, ctx: &mut LoadingContext<'_>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.profiles) {
            match pending.poll() {
                Poll::Waiting => waiting.push((ticket, pending)),
                Poll::Ready((results, writes)) => {
                    self.caches
                        .apply_writes(&ticket.platform_id, ctx.stores.storage_mut(), writes);
                    self.loader.finish_refresh_profiles(&ticket, results);
                }
                Poll::Gone => {
                    self.loader
                        .finish_refresh_profiles(&ticket, ProfileResults::Each(Vec::new()));
                }
            }
        }
        self.profiles.extend(waiting);
    }

    fn poll_warnings(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, job) in std::mem::take(&mut self.warnings) {
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            match job {
                WarningJob::SteamBans { plan, mut pending } => match pending.poll() {
                    Poll::Waiting => {
                        waiting.push((ticket, WarningJob::SteamBans { plan, pending }));
                    }
                    Poll::Ready(fetched) => {
                        let load = adapter.steam_ban_finish(&plan, fetched, ctx.stores);
                        self.land_warnings(&ticket, load, events);
                    }
                    Poll::Gone => {}
                },
                WarningJob::RobloxProbe(mut pending) => match pending.poll() {
                    Poll::Waiting => waiting.push((ticket, WarningJob::RobloxProbe(pending))),
                    Poll::Ready(result) => {
                        let load = adapter.state().roblox_sessions.finish_probe(result);
                        self.land_warnings(&ticket, load, events);
                    }
                    Poll::Gone => {}
                },
            }
        }
        self.warnings.extend(waiting);
    }
}
