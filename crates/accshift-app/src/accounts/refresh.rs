//! The settings "refresh now" actions for avatars and bans, ported from
//! `src/lib/app/useProfileRefresh.ts`.
//!
//! The TS loop over platforms is [`refresh_avatars_now`] and
//! [`refresh_bans_now`], generic over [`RefreshEnv`] (what the loop asks of
//! the app and its adapters). The whole run blocks, so the shell runs it on the job pool
//! with [`run_refresh_job`]: the adapter calls go to the backend, the client
//! stores the warning loads write are a copy, and the UI applies what came
//! back with [`apply_refresh_job`].

use super::loader::LoadOptions;
use super::profile_cache::{CacheWrite, ProfileCaches};
use super::{Toast, ToastKind};
use crate::backend::Backend;
use crate::error::BackendError;
use crate::platforms::warnings::WarningLoad;
use crate::platforms::{
    PlatformAccount, PlatformAdapter, PlatformDef, PlatformProfileInfo, PlatformRegistry,
    WarningLoadOptions, WarningRefreshOutcome,
};
use crate::settings::AppSettings;
use crate::storage::{StoreId, CLIENT_STORE_IDS};
use crate::stores::ClientStores;
use crate::text::Text;
use serde_json::Value;
use std::sync::Arc;

/// `AvatarRefreshOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarRefreshOutcome {
    Complete { count: usize },
    Failed,
    Partial { ok: usize, count: usize },
}

/// `avatarRefreshOutcome`: an error or a `None` is a miss (an adapter answers
/// `None` only for a failed fetch).
pub fn avatar_refresh_outcome(
    settled: &[Result<Option<PlatformProfileInfo>, BackendError>],
) -> AvatarRefreshOutcome {
    let count = settled.len();
    let ok = settled
        .iter()
        .filter(|result| matches!(result, Ok(Some(_))))
        .count();
    if ok == count {
        AvatarRefreshOutcome::Complete { count }
    } else if ok == 0 {
        AvatarRefreshOutcome::Failed
    } else {
        AvatarRefreshOutcome::Partial { ok, count }
    }
}

/// What the refresh loop asks of the app: `ensureAdapterReady` and the
/// adapter methods it calls.
pub trait RefreshEnv {
    type Adapter;
    /// `ensureAdapterReady`.
    fn ensure(&mut self, platform_id: &str) -> Option<Self::Adapter>;
    /// `getProfileInfo` exists.
    fn has_profile_info(&self, adapter: &Self::Adapter) -> bool;
    /// `loadWarningStates` exists.
    fn has_warnings(&self, adapter: &Self::Adapter) -> bool;
    fn load_accounts(
        &mut self,
        adapter: &Self::Adapter,
    ) -> Result<Vec<PlatformAccount>, BackendError>;
    fn profile_info(
        &mut self,
        adapter: &Self::Adapter,
        account_id: &str,
    ) -> Result<Option<PlatformProfileInfo>, BackendError>;
    /// `loadWarningStates(accounts, { forceRefresh, silent })`.
    fn load_warning_states(
        &mut self,
        adapter: &Self::Adapter,
        accounts: &[PlatformAccount],
        options: WarningLoadOptions,
    ) -> Result<WarningLoad, BackendError>;
    /// `getNoAccountsToastMessage`.
    fn no_accounts_toast(&self, adapter: &Self::Adapter) -> Option<Text>;
}

/// What a run asks the UI to do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RefreshReport {
    pub toasts: Vec<Toast>,
    /// `void loadAccounts(...)` on the active tab, with these flags.
    pub reload_active: Option<LoadOptions>,
}

fn toast(report: &mut RefreshReport, text: Text, kind: ToastKind) {
    report.toasts.push(Toast::new(text, kind));
}

/// `refreshAvatarsNow`. `ensure` is `ensureAdapterReady`.
pub fn refresh_avatars_now<E: RefreshEnv>(
    defs: &[PlatformDef],
    active_tab: &str,
    env: &mut E,
) -> RefreshReport {
    let mut report = RefreshReport::default();
    for def in defs {
        if !def
            .capabilities
            .profile_refresh
            .is_some_and(|refresh| refresh.avatars)
        {
            continue;
        }
        let Some(adapter) = env.ensure(&def.id) else {
            continue;
        };
        if !env.has_profile_info(&adapter) {
            continue;
        }
        let accounts = match env.load_accounts(&adapter) {
            Ok(accounts) => accounts,
            Err(_) => {
                toast(
                    &mut report,
                    Text::key("toast.refreshFailed"),
                    ToastKind::Error,
                );
                continue;
            }
        };
        if accounts.is_empty() {
            if let Some(text) = env.no_accounts_toast(&adapter) {
                toast(&mut report, text, ToastKind::Default);
            }
            continue;
        }
        let settled: Vec<_> = accounts
            .iter()
            .map(|account| env.profile_info(&adapter, &account.id))
            .collect();
        match avatar_refresh_outcome(&settled) {
            AvatarRefreshOutcome::Failed => {
                toast(
                    &mut report,
                    Text::key("toast.refreshFailed"),
                    ToastKind::Error,
                );
                continue;
            }
            outcome => {
                if active_tab == def.id {
                    report.reload_active = Some(LoadOptions {
                        silent: true,
                        force_refresh: true,
                        ..LoadOptions::default()
                    });
                }
                match outcome {
                    AvatarRefreshOutcome::Partial { ok, count } => toast(
                        &mut report,
                        Text::with(
                            "toast.refreshPartial",
                            [("ok", ok as f64), ("count", count as f64)],
                        ),
                        ToastKind::Info,
                    ),
                    _ => toast(
                        &mut report,
                        Text::with(
                            "toast.avatarRefreshComplete",
                            [("count", accounts.len() as f64)],
                        ),
                        ToastKind::Success,
                    ),
                }
            }
        }
    }
    report
}

/// `refreshBansNow`.
pub fn refresh_bans_now<E: RefreshEnv>(
    defs: &[PlatformDef],
    active_tab: &str,
    env: &mut E,
) -> RefreshReport {
    let mut report = RefreshReport::default();
    for def in defs {
        if !def
            .capabilities
            .profile_refresh
            .is_some_and(|refresh| refresh.bans)
        {
            continue;
        }
        let Some(adapter) = env.ensure(&def.id) else {
            continue;
        };
        if !env.has_warnings(&adapter) {
            continue;
        }
        let accounts = match env.load_accounts(&adapter) {
            Ok(accounts) => accounts,
            Err(_) => {
                toast(
                    &mut report,
                    Text::key("toast.refreshFailed"),
                    ToastKind::Error,
                );
                continue;
            }
        };
        if accounts.is_empty() {
            if let Some(text) = env.no_accounts_toast(&adapter) {
                toast(&mut report, text, ToastKind::Default);
            }
            continue;
        }
        let load = match env.load_warning_states(
            &adapter,
            &accounts,
            WarningLoadOptions {
                force_refresh: true,
                silent: false,
            },
        ) {
            Ok(load) => load,
            Err(_) => {
                toast(
                    &mut report,
                    Text::key("toast.refreshFailed"),
                    ToastKind::Error,
                );
                continue;
            }
        };
        // The adapter's own toasts (its error toast among them).
        for request in &load.toasts {
            let kind = if request.error {
                ToastKind::Error
            } else {
                ToastKind::Default
            };
            toast(&mut report, request.text.clone(), kind);
        }
        let outcome = load.outcome.unwrap_or(WarningRefreshOutcome::Complete {
            checked: accounts.len(),
        });
        match outcome {
            // The adapter already showed its error toast.
            WarningRefreshOutcome::Failed => continue,
            WarningRefreshOutcome::NoApiKey => {
                toast(
                    &mut report,
                    Text::key("toast.banRefreshNoApiKey"),
                    ToastKind::Error,
                );
                continue;
            }
            _ => {}
        }
        if active_tab == def.id {
            report.reload_active = Some(LoadOptions {
                silent: true,
                check_bans: true,
                ..LoadOptions::default()
            });
        }
        if let WarningRefreshOutcome::Partial { checked, requested } = outcome {
            toast(
                &mut report,
                Text::with(
                    "toast.refreshPartial",
                    [("ok", checked as f64), ("count", requested as f64)],
                ),
                ToastKind::Info,
            );
            continue;
        }
        toast(
            &mut report,
            Text::with(
                "toast.banRefreshComplete",
                [("count", accounts.len() as f64)],
            ),
            ToastKind::Success,
        );
    }
    report
}

// ------------------------------------------------------------ real adapters

/// [`RefreshEnv`] over the real adapters, for a run on the job pool.
pub struct AdapterEnv<'a> {
    pub registry: &'a PlatformRegistry,
    pub backend: &'a dyn Backend,
    pub caches: &'a ProfileCaches,
    /// A copy of the client stores; the Steam ban load writes its caches here.
    pub stores: ClientStores,
    pub settings: &'a AppSettings,
    pub now_ms: &'a dyn Fn() -> f64,
    /// Profile cache writes, by platform.
    pub writes: Vec<(String, Vec<CacheWrite>)>,
}

impl RefreshEnv for AdapterEnv<'_> {
    type Adapter = Arc<PlatformAdapter>;

    fn ensure(&mut self, platform_id: &str) -> Option<Self::Adapter> {
        self.registry.adapter(platform_id)
    }

    fn has_profile_info(&self, adapter: &Self::Adapter) -> bool {
        has_profile_info(adapter)
    }

    fn has_warnings(&self, adapter: &Self::Adapter) -> bool {
        adapter.has_warnings()
    }

    fn load_accounts(
        &mut self,
        adapter: &Self::Adapter,
    ) -> Result<Vec<PlatformAccount>, BackendError> {
        adapter.load_accounts(self.backend)
    }

    fn profile_info(
        &mut self,
        adapter: &Self::Adapter,
        account_id: &str,
    ) -> Result<Option<PlatformProfileInfo>, BackendError> {
        let fetched = self
            .caches
            .profile_info(adapter, self.backend, account_id, self.now_ms);
        self.writes.push((adapter.id().to_string(), fetched.writes));
        Ok(fetched.value)
    }

    fn load_warning_states(
        &mut self,
        adapter: &Self::Adapter,
        accounts: &[PlatformAccount],
        options: WarningLoadOptions,
    ) -> Result<WarningLoad, BackendError> {
        Ok(adapter
            .load_warning_states_blocking(
                self.backend,
                &mut self.stores,
                self.settings,
                accounts,
                options,
                (self.now_ms)(),
            )
            .unwrap_or_default())
    }

    fn no_accounts_toast(&self, adapter: &Self::Adapter) -> Option<Text> {
        adapter.no_accounts_toast()
    }
}

/// Whether an adapter has `getProfileInfo`: Steam, Riot and Roblox.
pub fn has_profile_info(adapter: &PlatformAdapter) -> bool {
    matches!(adapter.id(), "steam" | "riot" | "roblox")
}

/// Which "refresh now" button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshKind {
    Avatars,
    Bans,
}

/// What [`run_refresh_job`] hands back to the UI thread.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RefreshJobResult {
    pub report: RefreshReport,
    pub cache_writes: Vec<(String, Vec<CacheWrite>)>,
    /// Raw stores the run wrote in its copy (the Steam ban caches).
    pub store_values: Vec<(StoreId, Value)>,
}

/// A whole "refresh now" run, blocking: on the job pool, with a clone of the
/// client stores.
#[allow(clippy::too_many_arguments)]
pub fn run_refresh_job(
    kind: RefreshKind,
    registry: &PlatformRegistry,
    active_tab: &str,
    backend: &dyn Backend,
    caches: &ProfileCaches,
    stores: ClientStores,
    settings: &AppSettings,
    now_ms: &dyn Fn() -> f64,
) -> RefreshJobResult {
    let before: Vec<(StoreId, u64)> = CLIENT_STORE_IDS
        .iter()
        .map(|id| (*id, stores.storage().revision(id)))
        .collect();
    let defs = registry.defs().to_vec();
    let mut env = AdapterEnv {
        registry,
        backend,
        caches,
        stores,
        settings,
        now_ms,
        writes: Vec::new(),
    };
    let report = match kind {
        RefreshKind::Avatars => refresh_avatars_now(&defs, active_tab, &mut env),
        RefreshKind::Bans => refresh_bans_now(&defs, active_tab, &mut env),
    };
    let stores = env.stores;
    let store_values = before
        .into_iter()
        .filter(|(id, revision)| stores.storage().revision(id) != *revision)
        .map(|(id, _)| (id, stores.storage().get(id).cloned().unwrap_or(Value::Null)))
        .collect();
    RefreshJobResult {
        report,
        cache_writes: env.writes,
        store_values,
    }
}

/// Applies a finished run on the UI thread: the profile cache writes, then
/// the raw stores it changed. Returns the run's report.
pub fn apply_refresh_job(
    result: RefreshJobResult,
    stores: &mut ClientStores,
    caches: &ProfileCaches,
) -> RefreshReport {
    for (platform_id, writes) in result.cache_writes {
        caches.apply_writes(&platform_id, stores.storage_mut(), writes);
    }
    for (id, value) in result.store_values {
        stores.storage_mut().set(id, value);
    }
    stores.sync();
    result.report
}
