//! Steam ban warnings, ported from `src/lib/platforms/steam/warnings.ts`.
//!
//! The TS loader reads the client stores, awaits the backend, then writes the
//! stores back. Stores live on the UI thread and backend calls on the job
//! pool, so the load is split in three: [`plan`] (UI thread, reads stores and
//! the session state), [`fetch`] (job thread, the only backend calls) and
//! [`finish`] (UI thread, writes stores and the session state).
//! [`load_blocking`] chains them for callers that may block.

use super::api::ApiKeyMemo;
use crate::backend::Backend;
use crate::error::BackendError;
use crate::platforms::warnings::{WarningChip, WarningLoad, WarningPresentation, WarningTone};
use crate::platforms::{PlatformAccount, ToastRequest, WarningLoadOptions, WarningRefreshOutcome};
use crate::settings::AppSettings;
use crate::storage::{CLIENT_STORE_STEAM_BAN_CHECK_STATE, CLIENT_STORE_STEAM_BAN_INFO_CACHE};
use crate::stores::ClientStores;
use crate::text::Text;
use crate::types::BanInfo;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const BAN_ERROR_TOAST_COOLDOWN_MS: f64 = 30_000.0;
/// A Steam id left out of this many consecutive answers stops being asked
/// for this session, until a forced refresh.
pub const BAN_OMISSIONS_BEFORE_SKIP: u32 = 2;

/// The module state of `warnings.ts`: what this session already checked.
#[derive(Debug, Clone, Default)]
pub struct BanSession {
    pub checked: BTreeSet<String>,
    pub omissions: BTreeMap<String, u32>,
    pub last_error_toast_at: f64,
}

/// `BanCheckState`, the `cache.steam.ban-check-state` store.
#[derive(Debug, Clone, PartialEq)]
pub struct BanCheckState {
    pub last_success_at: f64,
    pub checked_steam_ids: Vec<String>,
}

fn unique(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    ids.into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// `readBanCheckState`.
pub fn read_ban_check_state(stores: &ClientStores) -> Option<BanCheckState> {
    let raw = stores.storage().get(CLIENT_STORE_STEAM_BAN_CHECK_STATE)?;
    if !raw.is_object() && !raw.is_array() {
        return None;
    }
    let last_success_at = crate::js::to_number(raw.get("lastSuccessAt"));
    if !last_success_at.is_finite() || last_success_at < 0.0 {
        return None;
    }
    let checked = raw
        .get("checkedSteamIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(BanCheckState {
        last_success_at,
        checked_steam_ids: unique(checked),
    })
}

/// `isBanInfo`, then the parse.
fn ban_info(value: &Value) -> Option<BanInfo> {
    let object = value.as_object()?;
    let is_bool = |name: &str| object.get(name).is_some_and(Value::is_boolean);
    let is_number = |name: &str| object.get(name).is_some_and(Value::is_number);
    let is_string = |name: &str| object.get(name).is_some_and(Value::is_string);
    let valid = is_string("steam_id")
        && is_bool("community_banned")
        && is_bool("vac_banned")
        && is_number("number_of_vac_bans")
        && is_number("days_since_last_ban")
        && is_number("number_of_game_bans")
        && is_string("economy_ban");
    if !valid {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

/// `readBanInfoCache`: the valid entries of `cache.steam.ban-info-cache`.
pub fn read_ban_info_cache(stores: &ClientStores) -> BTreeMap<String, BanInfo> {
    let Some(record) = stores
        .storage()
        .get(CLIENT_STORE_STEAM_BAN_INFO_CACHE)
        .and_then(Value::as_object)
    else {
        return BTreeMap::new();
    };
    record
        .iter()
        .filter_map(|(id, value)| ban_info(value).map(|info| (id.clone(), info)))
        .collect()
}

fn write_ban_info_cache(stores: &mut ClientStores, bans: &BTreeMap<String, BanInfo>) {
    let value = serde_json::to_value(bans).unwrap_or(Value::Null);
    stores
        .storage_mut()
        .set(CLIENT_STORE_STEAM_BAN_INFO_CACHE, value);
}

/// `formatBanTooltip`: English lines, as TS has them.
pub fn format_ban_tooltip(info: &BanInfo) -> String {
    let mut lines = Vec::new();
    if info.community_banned {
        lines.push("Community ban".to_string());
    }
    if info.vac_banned {
        let count = info.number_of_vac_bans.max(1);
        lines.push(format!(
            "{count} VAC ban{}",
            if count > 1 { "s" } else { "" }
        ));
    }
    if info.number_of_game_bans > 0 {
        let count = info.number_of_game_bans;
        lines.push(format!(
            "{count} game ban{}",
            if count > 1 { "s" } else { "" }
        ));
    }
    lines.join("\n")
}

/// `toSteamAccountWarningPresentation`. Every cached row gets one, chips or
/// not, as in TS.
pub fn ban_presentation(info: &BanInfo) -> WarningPresentation {
    let mut chips = Vec::new();
    if info.community_banned {
        chips.push(WarningChip {
            tone: WarningTone::Orange,
            text: Text::key("ban.community"),
        });
    }
    if info.vac_banned {
        let count = info.number_of_vac_bans.max(1);
        let key = if count > 1 {
            "ban.vac.multiple"
        } else {
            "ban.vac.single"
        };
        chips.push(WarningChip {
            tone: WarningTone::Red,
            text: Text::with(key, [("count", count)]),
        });
    }
    if info.number_of_game_bans > 0 {
        let count = info.number_of_game_bans;
        let key = if count > 1 {
            "ban.game.multiple"
        } else {
            "ban.game.single"
        };
        chips.push(WarningChip {
            tone: WarningTone::Red,
            text: Text::with(key, [("count", count)]),
        });
    }
    let red = info.vac_banned || info.number_of_game_bans > 0;
    let orange =
        info.community_banned || (!info.economy_ban.is_empty() && info.economy_ban != "none");
    WarningPresentation {
        tooltip: Text::raw(format_ban_tooltip(info)),
        card_outline_tone: if red {
            Some(WarningTone::Red)
        } else if orange {
            Some(WarningTone::Orange)
        } else {
            None
        },
        list_has_red: red,
        list_has_orange: info.community_banned,
        chips,
    }
}

/// `toWarningMap`.
pub fn to_warning_map(bans: &BTreeMap<String, BanInfo>) -> BTreeMap<String, WarningPresentation> {
    bans.iter()
        .map(|(id, info)| (id.clone(), ban_presentation(info)))
        .collect()
}

/// `getCachedSteamWarningStates`.
pub fn cached_warning_states(stores: &ClientStores) -> BTreeMap<String, WarningPresentation> {
    to_warning_map(&read_ban_info_cache(stores))
}

/// The set `alreadyCheckedBanIds` answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlreadyChecked {
    /// Nothing counts as checked: check everything.
    Nothing,
    /// What this session already fetched.
    Session,
    /// The persisted set of the delay window.
    Cached,
}

/// `alreadyCheckedBanIds`: with no delay configured, whatever this session
/// fetched; inside a delay window, the persisted set; a forced refresh or a
/// lapsed window checks everything again.
pub fn already_checked_ban_ids(
    force_refresh: bool,
    delay_days: i64,
    within_delay_window: bool,
) -> AlreadyChecked {
    if force_refresh {
        return AlreadyChecked::Nothing;
    }
    if delay_days == 0 {
        return AlreadyChecked::Session;
    }
    if within_delay_window {
        AlreadyChecked::Cached
    } else {
        AlreadyChecked::Nothing
    }
}

/// What [`plan`] decided: the ids to ask for and what [`finish`] needs.
#[derive(Debug, Clone, PartialEq)]
pub struct BanPlan {
    pub ids_to_fetch: Vec<String>,
    /// `Some(generation)` when the API key memo is unknown and [`fetch`] asks
    /// the backend first.
    pub key_lookup: Option<u64>,
    pub delay_days: i64,
    pub now_ms: f64,
    pub within_delay_window: bool,
    pub previously_checked: Vec<String>,
    pub force_refresh: bool,
    pub silent: bool,
    /// Show the sticky `toast.banChecking` until [`finish`] returns.
    pub checking_toast: bool,
}

/// The first step of a load.
#[derive(Debug, Clone, PartialEq)]
pub enum BanStep {
    /// Nothing to fetch: the answer is already known.
    Done(WarningLoad),
    Fetch(BanPlan),
}

/// The backend half of a load.
#[derive(Debug, Clone)]
pub enum BanFetch {
    /// `hasApiKey` failed.
    KeyLookupFailed(BackendError),
    /// Steam has no API key.
    NoApiKey { key_generation: Option<u64> },
    /// The key is there; `bans` is `None` when there was nothing to ask.
    Fetched {
        key_generation: Option<u64>,
        bans: Option<Result<Vec<BanInfo>, BackendError>>,
    },
}

/// `loadSteamWarningStates` up to the backend call.
pub fn plan(
    session: &BanSession,
    memo: &ApiKeyMemo,
    stores: &ClientStores,
    settings: &AppSettings,
    accounts: &[PlatformAccount],
    options: WarningLoadOptions,
    now_ms: f64,
) -> BanStep {
    let cached = || WarningLoad {
        warnings: cached_warning_states(stores),
        ..WarningLoad::default()
    };
    if accounts.is_empty() {
        return BanStep::Done(cached());
    }
    if memo.cached() == Some(false) {
        return BanStep::Done(WarningLoad {
            outcome: Some(WarningRefreshOutcome::NoApiKey),
            ..cached()
        });
    }
    let steam_ids = unique(accounts.iter().map(|account| account.id.clone()));
    let delay_days = settings.data_refresh.ban_check_days;
    let cached_state = read_ban_check_state(stores);
    let delay_ms = delay_days as f64 * 24.0 * 60.0 * 60.0 * 1000.0;
    let within_delay_window = delay_days > 0
        && cached_state
            .as_ref()
            .is_some_and(|state| now_ms - state.last_success_at < delay_ms);
    let previously_checked = cached_state
        .map(|state| state.checked_steam_ids)
        .unwrap_or_default();
    let cached_ids: BTreeSet<&String> = previously_checked.iter().collect();
    let already = already_checked_ban_ids(options.force_refresh, delay_days, within_delay_window);
    let is_checked = |id: &String| match already {
        AlreadyChecked::Nothing => false,
        AlreadyChecked::Session => session.checked.contains(id),
        AlreadyChecked::Cached => cached_ids.contains(id),
    };
    let ids_to_fetch: Vec<String> = steam_ids
        .into_iter()
        .filter(|id| {
            !is_checked(id)
                && (options.force_refresh
                    || session.omissions.get(id).copied().unwrap_or(0) < BAN_OMISSIONS_BEFORE_SKIP)
        })
        .collect();
    let key_lookup = memo.cached().is_none().then(|| memo.generation());
    if ids_to_fetch.is_empty() && key_lookup.is_none() {
        return BanStep::Done(WarningLoad {
            outcome: Some(WarningRefreshOutcome::Complete { checked: 0 }),
            ..cached()
        });
    }
    BanStep::Fetch(BanPlan {
        checking_toast: !options.silent && !ids_to_fetch.is_empty(),
        ids_to_fetch,
        key_lookup,
        delay_days,
        now_ms,
        within_delay_window,
        previously_checked,
        force_refresh: options.force_refresh,
        silent: options.silent,
    })
}

/// The backend calls of a load: the key lookup when the memo is unknown, then
/// the bans. Runs on the job pool.
pub fn fetch(plan: &BanPlan, backend: &dyn Backend) -> BanFetch {
    if plan.key_lookup.is_some() {
        match backend.steam_has_api_key() {
            Err(error) => return BanFetch::KeyLookupFailed(error),
            Ok(false) => {
                return BanFetch::NoApiKey {
                    key_generation: plan.key_lookup,
                }
            }
            Ok(true) => {}
        }
    }
    let bans =
        (!plan.ids_to_fetch.is_empty()).then(|| backend.steam_get_player_bans(&plan.ids_to_fetch));
    BanFetch::Fetched {
        key_generation: plan.key_lookup,
        bans,
    }
}

/// `showBanErrorToast`: at most once per cooldown, never on a silent load.
fn error_toast(session: &mut BanSession, plan: &BanPlan, toasts: &mut Vec<ToastRequest>) {
    if plan.silent || plan.now_ms - session.last_error_toast_at < BAN_ERROR_TOAST_COOLDOWN_MS {
        return;
    }
    toasts.push(ToastRequest {
        text: Text::key("toast.banCheckFailedGeneric"),
        error: true,
    });
    session.last_error_toast_at = plan.now_ms;
}

/// `loadSteamWarningStates` after the backend call: session, memo and stores
/// updated, the map and outcome returned.
pub fn finish(
    session: &mut BanSession,
    memo: &mut ApiKeyMemo,
    plan: &BanPlan,
    fetched: BanFetch,
    stores: &mut ClientStores,
) -> WarningLoad {
    let mut cached_bans = read_ban_info_cache(stores);
    let mut toasts = Vec::new();
    let outcome = match fetched {
        BanFetch::KeyLookupFailed(_) => {
            // Unknown is not absent: no "add a key" hint here.
            error_toast(session, plan, &mut toasts);
            WarningRefreshOutcome::Failed
        }
        BanFetch::NoApiKey { key_generation } => {
            if let Some(generation) = key_generation {
                memo.finish_read(generation, false);
            }
            WarningRefreshOutcome::NoApiKey
        }
        BanFetch::Fetched {
            key_generation,
            bans,
        } => {
            if let Some(generation) = key_generation {
                memo.finish_read(generation, true);
            }
            match bans {
                None => WarningRefreshOutcome::Complete { checked: 0 },
                Some(Err(_)) => {
                    error_toast(session, plan, &mut toasts);
                    WarningRefreshOutcome::Failed
                }
                Some(Ok(bans)) => {
                    apply_bans(session, plan, &bans, &mut cached_bans, stores, &mut toasts)
                }
            }
        }
    };
    WarningLoad {
        warnings: to_warning_map(&cached_bans),
        outcome: Some(outcome),
        toasts,
    }
}

fn apply_bans(
    session: &mut BanSession,
    plan: &BanPlan,
    bans: &[BanInfo],
    cached_bans: &mut BTreeMap<String, BanInfo>,
    stores: &mut ClientStores,
    toasts: &mut Vec<ToastRequest>,
) -> WarningRefreshOutcome {
    // mergeBanRows: rows without a Steam id are unusable.
    let mut banned_count = 0usize;
    let mut returned = BTreeSet::new();
    for ban in bans {
        if ban.steam_id.is_empty() {
            continue;
        }
        cached_bans.insert(ban.steam_id.clone(), ban.clone());
        returned.insert(ban.steam_id.clone());
        if ban.vac_banned || ban.community_banned || ban.number_of_game_bans > 0 {
            banned_count += 1;
        }
    }
    // An omitted id is not a checked id: marking it would skip the retry.
    let confirmed: Vec<String> = plan
        .ids_to_fetch
        .iter()
        .filter(|id| returned.contains(*id))
        .cloned()
        .collect();
    for id in &plan.ids_to_fetch {
        if returned.contains(id) {
            session.checked.insert(id.clone());
            session.omissions.remove(id);
        } else {
            *session.omissions.entry(id.clone()).or_insert(0) += 1;
        }
    }
    // recordBanCheckState.
    if plan.delay_days == 0 {
        stores
            .storage_mut()
            .set_immediate(CLIENT_STORE_STEAM_BAN_CHECK_STATE, Value::Null);
    } else {
        let covers_every_account = plan.force_refresh || !plan.within_delay_window;
        let checked = if covers_every_account {
            confirmed.clone()
        } else {
            unique(plan.previously_checked.iter().chain(&confirmed).cloned())
        };
        stores.storage_mut().set(
            CLIENT_STORE_STEAM_BAN_CHECK_STATE,
            json!({ "lastSuccessAt": plan.now_ms, "checkedSteamIds": checked }),
        );
    }
    write_ban_info_cache(stores, cached_bans);
    if !plan.silent && banned_count > 0 {
        let key = if banned_count > 1 {
            "toast.banCheckSummary.multiple"
        } else {
            "toast.banCheckSummary.single"
        };
        toasts.push(ToastRequest {
            text: Text::with(key, [("count", banned_count)]),
            error: false,
        });
    }
    if confirmed.len() == plan.ids_to_fetch.len() {
        WarningRefreshOutcome::Complete {
            checked: confirmed.len(),
        }
    } else {
        WarningRefreshOutcome::Partial {
            checked: confirmed.len(),
            requested: plan.ids_to_fetch.len(),
        }
    }
}

/// [`plan`], [`fetch`] and [`finish`] in one go, for callers that may block.
#[allow(clippy::too_many_arguments)]
pub fn load_blocking(
    session: &mut BanSession,
    memo: &mut ApiKeyMemo,
    backend: &dyn Backend,
    stores: &mut ClientStores,
    settings: &AppSettings,
    accounts: &[PlatformAccount],
    options: WarningLoadOptions,
    now_ms: f64,
) -> WarningLoad {
    match plan(session, memo, stores, settings, accounts, options, now_ms) {
        BanStep::Done(load) => load,
        BanStep::Fetch(plan) => {
            let fetched = fetch(&plan, backend);
            finish(session, memo, &plan, fetched, stores)
        }
    }
}
