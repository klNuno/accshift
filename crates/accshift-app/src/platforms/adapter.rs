//! One platform's behaviour, ported from the `PlatformAdapter` interface of
//! `src/lib/shared/platform.ts` and the adapters that implement it
//! (`steam/adapter.ts`, `riot/adapter.ts`, `roblox/adapter.ts`,
//! `genericAdapter.ts` and the six `*/adapter.ts` built on it), plus
//! `addFlow.ts`.
//!
//! Why an enum and not a trait object: the set of adapters is closed (Steam,
//! Riot, Roblox and the generic family, user descriptors included), each
//! shares most methods with the generic case, and the UI needs the
//! platform-only calls (Steam's API key, the CS2 bridge, the split warning
//! loads) without downcasting. A match keeps every behaviour in sight in one
//! place and the adapter `Send + Sync` with no boxing. A user platform needs
//! no new code: it is [`AdapterKind::Generic`] with [`GenericConfig::user`].
//!
//! Every method that talks to the backend takes `&dyn Backend` and blocks, so
//! the shell runs it on the job pool. Session state the TS modules keep in
//! module variables (ban checks, Roblox dead sessions, the Riot profile
//! cache, the CS2 bridge rows) lives behind a mutex inside the adapter.

use super::generic::GenericConfig;
use super::riot::{self, RiotProfile, RiotProfileCache};
use super::roblox::{self, RobloxSessions};
use super::steam::api::{ApiKeyMemo, SteamLaunchConfig};
use super::steam::cs2::Cs2Bridge;
use super::steam::warnings::{self as steam_warnings, BanFetch, BanPlan, BanSession, BanStep};
use super::steam::{self, context_menu as steam_menu};
use super::warnings::{WarningLoad, WarningPresentation};
use super::{
    AddAccountResult, AddFlowStatus, PlatformAccount, PlatformProfileInfo, StartupSnapshot,
    WarningLoadOptions,
};
use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::menu::platform::build_platform_context_menu;
use crate::menu::{
    actions::run_safe, MenuAction, PlatformTask, SubmenuLoader, TaskEffect, TaskOutcome,
};
use crate::platforms::api::PlatformApi;
use crate::settings::AppSettings;
use crate::stores::ClientStores;
use crate::text::Text;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

/// Which implementation backs a platform.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterKind {
    Steam,
    Riot,
    Roblox,
    /// Driven entirely by the shared `platform_*` commands.
    Generic(GenericConfig),
}

/// The session state of the TS modules.
#[derive(Debug, Default)]
pub struct AdapterState {
    pub ban_session: BanSession,
    pub api_key: ApiKeyMemo,
    pub cs2: Cs2Bridge,
    pub riot_profiles: RiotProfileCache,
    pub roblox_sessions: RobloxSessions,
}

/// What the menu builders read from the UI (`PlatformContextMenuCallbacks`
/// minus the actions, which are [`crate::menu::MenuCommand`]s now).
#[derive(Debug, Clone, Copy)]
pub struct MenuContext<'a> {
    /// `getCurrentAccountId()`.
    pub current_account_id: Option<&'a str>,
    /// Steam's default games live in the client stores.
    pub stores: &'a ClientStores,
    /// The UI offers `openInputDialog` (rename).
    pub input_dialog: bool,
}

/// `PlatformAdapter`.
#[derive(Debug)]
pub struct PlatformAdapter {
    id: String,
    kind: AdapterKind,
    state: Mutex<AdapterState>,
}

fn steam_config(settings: &AppSettings) -> SteamLaunchConfig {
    SteamLaunchConfig::from_settings(&settings.platform_settings.steam)
}

fn generic_accounts(config: &GenericConfig, raw: &Value) -> Vec<PlatformAccount> {
    raw.as_array()
        .map(|rows| rows.iter().map(|row| config.to_account(row)).collect())
        .unwrap_or_default()
}

impl PlatformAdapter {
    pub fn new(id: impl Into<String>, kind: AdapterKind) -> Self {
        Self {
            id: id.into(),
            kind,
            state: Mutex::new(AdapterState::default()),
        }
    }

    /// The adapter of a shipped platform, `None` for an unknown id.
    pub fn shipped(id: &str) -> Option<Self> {
        let kind = match id {
            "steam" => AdapterKind::Steam,
            "riot" => AdapterKind::Riot,
            "roblox" => AdapterKind::Roblox,
            other => AdapterKind::Generic(GenericConfig::shipped(other)?),
        };
        Some(Self::new(id, kind))
    }

    /// `userAdapterLoader`: a platform from the user's descriptor folder.
    pub fn user(id: &str, name: &str) -> Self {
        Self::new(id, AdapterKind::Generic(GenericConfig::user(name)))
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> &AdapterKind {
        &self.kind
    }

    /// The session state. A poisoned lock is recovered: the state is plain
    /// data and stays usable.
    pub fn state(&self) -> MutexGuard<'_, AdapterState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn api<'a>(&'a self, backend: &'a dyn Backend) -> PlatformApi<'a> {
        PlatformApi::new(backend, &self.id)
    }

    /// `reloadAfterAdd`.
    pub fn reload_after_add(&self) -> bool {
        matches!(&self.kind, AdapterKind::Generic(config) if config.reload_after_add)
    }

    /// `setAccountLabel` exists.
    pub fn supports_account_labels(&self) -> bool {
        matches!(&self.kind, AdapterKind::Generic(config) if config.supports_account_labels())
    }

    /// Whether `getCachedWarningStates` / `loadWarningStates` exist.
    pub fn has_warnings(&self) -> bool {
        matches!(self.kind, AdapterKind::Steam | AdapterKind::Roblox)
    }

    // ------------------------------------------------------------ accounts

    /// Maps the raw `platform_get_accounts` list.
    pub fn to_accounts(&self, raw: &Value) -> Vec<PlatformAccount> {
        match &self.kind {
            AdapterKind::Steam => steam::to_steam_accounts(raw),
            AdapterKind::Riot => {
                let profiles = RiotProfile::list(raw);
                self.state().riot_profiles.remember(&profiles);
                profiles.iter().map(riot::to_riot_account).collect()
            }
            AdapterKind::Roblox => roblox::to_roblox_accounts(raw),
            AdapterKind::Generic(config) => generic_accounts(config, raw),
        }
    }

    /// Maps the raw `platform_get_startup_snapshot` payload.
    pub fn to_snapshot(&self, raw: &Value) -> StartupSnapshot {
        match &self.kind {
            AdapterKind::Steam => steam::to_steam_snapshot(raw),
            AdapterKind::Riot => {
                let (snapshot, profiles) = riot::to_riot_snapshot(raw);
                self.state().riot_profiles.remember(&profiles);
                snapshot
            }
            AdapterKind::Roblox => roblox::to_roblox_snapshot(raw),
            AdapterKind::Generic(config) => StartupSnapshot {
                accounts: raw
                    .get("accounts")
                    .map(|rows| generic_accounts(config, rows))
                    .unwrap_or_default(),
                current_account: raw
                    .get("currentAccount")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
        }
    }

    /// `loadAccounts`.
    pub fn load_accounts(&self, backend: &dyn Backend) -> BackendResult<Vec<PlatformAccount>> {
        Ok(self.to_accounts(&self.api(backend).get_accounts()?))
    }

    /// `getCurrentAccount`.
    pub fn current_account(&self, backend: &dyn Backend) -> BackendResult<String> {
        self.api(backend).get_current_account()
    }

    /// `getStartupSnapshot`.
    pub fn startup_snapshot(&self, backend: &dyn Backend) -> BackendResult<StartupSnapshot> {
        Ok(self.to_snapshot(&self.api(backend).get_startup_snapshot()?))
    }

    /// `switchAccount`. Steam reads its launch settings; Roblox keeps its
    /// dead-session marks in step with the answer.
    pub fn switch_account(
        &self,
        backend: &dyn Backend,
        account: &PlatformAccount,
        settings: &AppSettings,
    ) -> BackendResult<()> {
        match &self.kind {
            AdapterKind::Steam => {
                steam::api::switch_account(backend, &account.username, &steam_config(settings))
            }
            AdapterKind::Riot => self
                .api(backend)
                .switch_account(&account.id, &json!({}), None),
            AdapterKind::Roblox => {
                let result = self
                    .api(backend)
                    .switch_account(&account.id, &json!({}), None);
                let mut state = self.state();
                match &result {
                    Ok(()) => state.roblox_sessions.clear_expired(&account.id),
                    Err(error) if roblox::is_session_expired_error(&error.message) => {
                        state.roblox_sessions.mark_expired(&account.id);
                    }
                    Err(_) => {}
                }
                result
            }
            AdapterKind::Generic(config) => {
                let details = config
                    .mask_switch_log_id(&account.id)
                    .map(|masked| json!({ "accountId": masked }));
                self.api(backend)
                    .switch_account(&account.id, &json!({}), details)
            }
        }
    }

    // ------------------------------------------------------------ add flow

    /// `addAccount` (`createPlatformAddFlowHandlers`).
    pub fn add_account(
        &self,
        backend: &dyn Backend,
        settings: &AppSettings,
    ) -> BackendResult<AddAccountResult> {
        let status = match &self.kind {
            AdapterKind::Steam => {
                steam::api::begin_account_setup(backend, &steam_config(settings))?
            }
            _ => self.api(backend).begin_setup(&json!({}))?,
        };
        Ok(AddAccountResult {
            setup_status: Some(status),
        })
    }

    /// `pollAddFlow`. Roblox lifts a dead-session mark once a fresh cookie is
    /// stored for the account.
    pub fn poll_add_flow(
        &self,
        backend: &dyn Backend,
        setup_id: &str,
    ) -> BackendResult<AddFlowStatus> {
        let status = self.api(backend).get_setup_status(setup_id)?;
        if self.kind == AdapterKind::Roblox && status.state == "ready" {
            if let Some(account_id) = status.account_id.as_deref().filter(|id| !id.is_empty()) {
                self.state().roblox_sessions.clear_expired(account_id);
            }
        }
        Ok(status)
    }

    /// `cancelAddFlow`.
    pub fn cancel_add_flow(&self, backend: &dyn Backend, setup_id: &str) -> BackendResult<()> {
        self.api(backend).cancel_setup(setup_id)
    }

    /// `setAccountLabel`; an error for a platform without labels.
    pub fn set_account_label(
        &self,
        backend: &dyn Backend,
        account_id: &str,
        label: &str,
    ) -> BackendResult<()> {
        if !self.supports_account_labels() {
            return Err(BackendError::new(format!(
                "{} has no account labels",
                self.id
            )));
        }
        self.api(backend).set_account_label(account_id, label)
    }

    // ------------------------------------------------------------ menu

    /// `getContextMenuActions`.
    pub fn context_menu_actions(
        &self,
        account: &PlatformAccount,
        ctx: &MenuContext<'_>,
    ) -> Vec<MenuAction> {
        match &self.kind {
            AdapterKind::Steam => steam_menu::steam_context_menu_actions(
                account,
                ctx.current_account_id,
                ctx.stores.default_game(&account.id),
            ),
            AdapterKind::Riot => {
                let profile = self.state().riot_profiles.get(&account.id).cloned();
                riot::riot_context_menu_actions(account, profile.as_ref())
            }
            AdapterKind::Roblox => roblox::roblox_context_menu_actions(account),
            AdapterKind::Generic(config) => {
                let base = if account.display_name.is_empty() {
                    &account.id
                } else {
                    &account.display_name
                };
                let trimmed = crate::js::trim(base);
                let display = if trimmed.is_empty() {
                    account.id.clone()
                } else {
                    trimmed.to_string()
                };
                let mut forget = crate::menu::platform::ForgetConfig::keys(
                    &config.key("forgetConfirmTitle"),
                    &config.key("forgetConfirmMessage"),
                    &config.key("forget"),
                    &config.key("forgotAccount"),
                );
                forget.toast_params = config.forget_toast_params(&display);
                build_platform_context_menu(
                    &self.id,
                    account,
                    &config.copy_items(account),
                    &forget,
                    Some(display),
                )
            }
        }
    }

    /// Fills a `submenuLoader` submenu. Runs on the job pool; the UI groups
    /// the result with [`crate::menu::build_context_menu_items`].
    pub fn load_submenu(
        &self,
        loader: &SubmenuLoader,
        backend: &dyn Backend,
    ) -> BackendResult<Vec<MenuAction>> {
        match loader {
            SubmenuLoader::SteamAccountGames { username, steam_id } => {
                let games = backend.steam_get_account_games(steam_id)?;
                Ok(steam_menu::account_games_submenu(
                    username, steam_id, &games,
                ))
            }
            SubmenuLoader::SteamCopyableGames {
                from_steam_id,
                to_steam_id,
            } => {
                let games = backend.steam_get_copyable_games(from_steam_id, to_steam_id)?;
                Ok(steam_menu::copyable_games_submenu(
                    from_steam_id,
                    to_steam_id,
                    &games,
                ))
            }
        }
    }

    /// Runs a menu task as a safe action (`createSafeContextAction`): the
    /// effects to apply, in TS order, a failure turned into a toast.
    pub fn run_task(
        &self,
        task: &PlatformTask,
        backend: &dyn Backend,
        settings: &AppSettings,
    ) -> TaskOutcome {
        let mut outcome = TaskOutcome::default();
        run_safe(&mut outcome, |outcome| {
            self.task(task, backend, settings, outcome)
        });
        outcome
    }

    fn task(
        &self,
        task: &PlatformTask,
        backend: &dyn Backend,
        settings: &AppSettings,
        outcome: &mut TaskOutcome,
    ) -> BackendResult<()> {
        match task {
            PlatformTask::SteamSwitchMode {
                username,
                steam_id,
                mode,
            } => steam::api::switch_account_mode(
                backend,
                username,
                steam_id,
                mode,
                &steam_config(settings),
            ),
            PlatformTask::SteamRunGame {
                username,
                steam_id,
                app_id,
                remember,
            } => {
                if let Some(game) = remember {
                    outcome.push(TaskEffect::SetDefaultGame {
                        steam_id: steam_id.clone(),
                        game: game.clone(),
                    });
                }
                steam::api::switch_account_and_launch_game(
                    backend,
                    username,
                    steam_id,
                    app_id,
                    &steam_config(settings),
                )
            }
            PlatformTask::SteamClearDefaultGame { steam_id } => {
                outcome.push(TaskEffect::ClearDefaultGame {
                    steam_id: steam_id.clone(),
                });
                Ok(())
            }
            PlatformTask::SteamOpenUserdata { steam_id } => backend.steam_open_userdata(steam_id),
            PlatformTask::SteamClearBrowserCache => {
                backend.steam_clear_browser_cache()?;
                outcome.push(TaskEffect::Toast(Text::key(
                    "steam.clearedIntegratedBrowserCache",
                )));
                Ok(())
            }
            PlatformTask::SteamCopyGameSettings {
                from_steam_id,
                to_steam_id,
                app_id,
                game_name,
            } => {
                backend.steam_copy_game_settings(from_steam_id, to_steam_id, app_id)?;
                outcome.push(TaskEffect::Toast(Text::with(
                    "steam.copiedSettingsToCurrent",
                    [("game", game_name.as_str())],
                )));
                Ok(())
            }
            PlatformTask::RiotCapture {
                profile_id,
                display_name,
            } => {
                backend.riot_capture_profile(profile_id)?;
                outcome.push(TaskEffect::Toast(Text::with(
                    "riot.capturedSession",
                    [("profile", display_name.as_str())],
                )));
                outcome.push(TaskEffect::RefreshAccounts);
                Ok(())
            }
            PlatformTask::Forget {
                platform_id,
                account_id,
                toast,
            } => {
                if platform_id == "steam" {
                    outcome.push(TaskEffect::ClearDefaultGame {
                        steam_id: account_id.clone(),
                    });
                }
                PlatformApi::new(backend, platform_id).forget_account(account_id)?;
                if platform_id == "riot" {
                    self.state().riot_profiles.forget(account_id);
                }
                outcome.push(TaskEffect::Toast(toast.clone()));
                outcome.push(TaskEffect::RemoveAccount(account_id.clone()));
                Ok(())
            }
            PlatformTask::SetAccountLabel {
                account_id, label, ..
            } => {
                self.set_account_label(backend, account_id, label)?;
                outcome.push(TaskEffect::RefreshAccounts);
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------ profiles

    /// `getProfileInfo` without the M2 caches: Steam and Roblox ask the
    /// backend, Riot reads its profile cache, generic platforms have none.
    pub fn profile_info(
        &self,
        backend: &dyn Backend,
        account_id: &str,
    ) -> BackendResult<Option<PlatformProfileInfo>> {
        match &self.kind {
            AdapterKind::Steam => {
                let profile = backend.steam_get_profile_info(account_id)?;
                Ok(steam::to_platform_profile_info(profile.as_ref()))
            }
            AdapterKind::Riot => Ok(riot::riot_profile_info(
                self.state().riot_profiles.get(account_id),
            )),
            AdapterKind::Roblox => {
                let info = backend.roblox_get_profile_info(account_id)?;
                Ok(roblox::to_roblox_profile_info(Some(&info)))
            }
            AdapterKind::Generic(_) => Ok(None),
        }
    }

    // ------------------------------------------------------------ warnings

    /// `getCachedWarningStates`.
    pub fn cached_warning_states(
        &self,
        stores: &ClientStores,
    ) -> BTreeMap<String, WarningPresentation> {
        match self.kind {
            AdapterKind::Steam => steam_warnings::cached_warning_states(stores),
            AdapterKind::Roblox => self.state().roblox_sessions.warning_map().warnings,
            _ => BTreeMap::new(),
        }
    }

    /// Steam's load, first step (UI thread). `None` for other platforms.
    pub fn steam_ban_plan(
        &self,
        stores: &ClientStores,
        settings: &AppSettings,
        accounts: &[PlatformAccount],
        options: WarningLoadOptions,
        now_ms: f64,
    ) -> Option<BanStep> {
        if self.kind != AdapterKind::Steam {
            return None;
        }
        let state = self.state();
        Some(steam_warnings::plan(
            &state.ban_session,
            &state.api_key,
            stores,
            settings,
            accounts,
            options,
            now_ms,
        ))
    }

    /// Steam's load, last step (UI thread), after
    /// [`steam_warnings::fetch`] ran on the job pool.
    pub fn steam_ban_finish(
        &self,
        plan: &BanPlan,
        fetched: BanFetch,
        stores: &mut ClientStores,
    ) -> WarningLoad {
        let mut state = self.state();
        let AdapterState {
            ban_session,
            api_key,
            ..
        } = &mut *state;
        steam_warnings::finish(ban_session, api_key, plan, fetched, stores)
    }

    /// `loadWarningStates` in one blocking call: Steam bans or the Roblox
    /// session probe. `None` for platforms without warnings.
    pub fn load_warning_states_blocking(
        &self,
        backend: &dyn Backend,
        stores: &mut ClientStores,
        settings: &AppSettings,
        accounts: &[PlatformAccount],
        options: WarningLoadOptions,
        now_ms: f64,
    ) -> Option<WarningLoad> {
        match self.kind {
            AdapterKind::Steam => {
                let step = self.steam_ban_plan(stores, settings, accounts, options, now_ms)?;
                Some(match step {
                    BanStep::Done(load) => load,
                    BanStep::Fetch(plan) => {
                        let fetched = steam_warnings::fetch(&plan, backend);
                        self.steam_ban_finish(&plan, fetched, stores)
                    }
                })
            }
            AdapterKind::Roblox => {
                let needs_probe = self.state().roblox_sessions.needs_probe(
                    settings,
                    accounts,
                    options.force_refresh,
                );
                if !needs_probe {
                    return Some(self.state().roblox_sessions.warning_map());
                }
                let result = RobloxSessions::probe(backend);
                Some(self.state().roblox_sessions.finish_probe(result))
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------ Steam key

    /// `hasApiKey`, memoized for the session.
    pub fn steam_has_api_key(&self, backend: &dyn Backend) -> BackendResult<bool> {
        let generation = {
            let state = self.state();
            if let Some(known) = state.api_key.cached() {
                return Ok(known);
            }
            state.api_key.generation()
        };
        let value = backend.steam_has_api_key()?;
        self.state().api_key.finish_read(generation, value);
        Ok(value)
    }

    /// `setApiKey`: stores the key and forgets the memo.
    pub fn steam_set_api_key(&self, backend: &dyn Backend, key: &str) -> BackendResult<()> {
        backend.steam_set_api_key(key)?;
        self.state().api_key.invalidate();
        Ok(())
    }

    // ------------------------------------------------------------ toasts

    /// `getNoAccountsToastMessage`.
    pub fn no_accounts_toast(&self) -> Option<Text> {
        Some(match &self.kind {
            AdapterKind::Steam => Text::key(steam::NO_ACCOUNTS_TOAST_KEY),
            AdapterKind::Riot => Text::key(riot::NO_ACCOUNTS_TOAST_KEY),
            AdapterKind::Roblox => Text::key(roblox::NO_ACCOUNTS_TOAST_KEY),
            AdapterKind::Generic(config) => Text::with_optional(
                config.no_accounts_toast_key.clone(),
                config.message_params.clone(),
            ),
        })
    }

    /// `getNoAccountsHintMessage`.
    pub fn no_accounts_hint(&self) -> Option<Text> {
        match &self.kind {
            AdapterKind::Roblox => Some(Text::key(roblox::NO_ACCOUNTS_HINT_KEY)),
            AdapterKind::Generic(config) => config
                .no_accounts_hint_key
                .as_ref()
                .map(|key| Text::with_optional(key.clone(), config.message_params.clone())),
            _ => None,
        }
    }

    /// `getSwitchErrorToastMessage`.
    pub fn switch_error_toast(&self, message: &str) -> Option<Text> {
        match self.kind {
            AdapterKind::Steam => steam::switch_error_toast(message),
            AdapterKind::Roblox => roblox::switch_error_toast(message),
            _ => None,
        }
    }

    /// `getLoadErrorToastMessage`.
    pub fn load_error_toast(&self, message: &str) -> Option<Text> {
        match self.kind {
            AdapterKind::Steam => steam::load_error_toast(message),
            _ => None,
        }
    }
}
