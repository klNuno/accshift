//! The account loader of the active tab and everything that drives it, plus
//! the platforms launcher detection found.
//!
//! [`AccountLoading`] owns the [`AccountLoader`], the [`VisiblePriming`] and
//! the profile caches, starts every blocking call on [`Jobs`] and keeps the
//! [`Pending`] handles. The shell calls [`AccountLoading::poll`] once per
//! frame: it applies the results that landed, runs the work deferred to that
//! frame and returns [`LoadingEvent`]s (toasts, the add flow start, the
//! window minimize). Nothing here blocks the UI thread.
//!
//! Adapters are built up front by the registry, so getting one is a lookup
//! and [`AccountLoading::adapter_loading`] is always false.
//!
//! - [`handlers`]: what the UI asks for (load, refresh, add, switch, priming).
//! - [`frame`]: the per frame poll that lands finished jobs.

mod frame;
mod handlers;

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

/// Implemented and supported on this OS.
pub fn is_platform_usable(registry: &PlatformRegistry, platform_id: &str, os: RuntimeOs) -> bool {
    registry
        .definition(platform_id)
        .is_some_and(|def| def.implemented && def.supports(os))
}

/// What launcher detection found this launch, set once during boot on a
/// fresh install. Empty means detection did not run or found nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedPlatforms(Vec<String>);

impl DetectedPlatforms {
    pub fn set(&mut self, platform_ids: &[String]) {
        self.0 = platform_ids.to_vec();
    }

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
    /// The active tab's platform id.
    pub active_tab: &'a str,
    pub runtime_os: RuntimeOs,
    /// The account ids on screen, from the display pipeline.
    pub visible_ids: &'a [String],
    /// A search query is typed.
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

/// The account loader of the active tab and the jobs that feed it.
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
    /// Refresh runs deferred to the next frame.
    deferred_runs: Vec<RefreshTicket>,
    /// The sticky `toast.banChecking` is up.
    ban_checking_toast: bool,
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
            ban_checking_toast: false,
        }
    }

    pub fn caches(&self) -> &Arc<ProfileCaches> {
        &self.caches
    }

    /// Always false: adapters are never loaded lazily here.
    pub fn adapter_loading(&self) -> bool {
        false
    }

    /// An account load still to land.
    pub fn loads_in_flight(&self) -> bool {
        !self.loads.is_empty()
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
}
