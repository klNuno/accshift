//! CS2 card content, ported from `src/lib/platforms/steam/cs2CardContent.ts`,
//! and the data half of `cs2Bridge.svelte.ts` (the cached rows, the TTL and
//! the enabled flag; the settings form is UI work).

use crate::backend::Backend;
use crate::platforms::api::log_app_event;
use crate::platforms::card::{CardChip, CardProgress, CardSection, ChipTone, UsernameBadge};
use crate::text::Text;
use crate::types::Cs2BridgeAccount;
use serde_json::json;
use std::collections::BTreeMap;

/// The source refreshes GC stats about every 3 h; 5 min keeps hover data
/// fresh without hammering the bridge on every tab switch.
pub const CS2_BRIDGE_CACHE_TTL_MS: f64 = 5.0 * 60.0 * 1000.0;

fn case_chip(case_earned: bool) -> (ChipTone, Text) {
    if case_earned {
        (ChipTone::Green, Text::key("card.cs2CaseEarned"))
    } else {
        (ChipTone::Slate, Text::key("card.cs2CaseNotEarned"))
    }
}

/// `cs2ExtensionSections`: the level, XP bar and weekly-case chip. Empty until
/// the bridge has both a level and an XP reading.
pub fn cs2_extension_sections(data: Option<&Cs2BridgeAccount>) -> Vec<CardSection> {
    let Some(data) = data else {
        return Vec::new();
    };
    let (Some(level), Some(xp)) = (data.level, data.xp) else {
        return Vec::new();
    };
    let (tone, text) = case_chip(data.case_earned);
    vec![CardSection {
        title: Some(Text::key("card.cs2Section")),
        text: Some(Text::with("card.cs2Level", [("level", level)])),
        progress: Some(CardProgress {
            value: f64::from(xp),
            max: f64::from(data.xp_max),
            label: Some(format!("{xp}/{}", data.xp_max)),
        }),
        chips: vec![CardChip { text, tone }],
        ..CardSection::default()
    }]
}

/// `cs2UsernameBadge`: the weekly-case state next to the username.
pub fn cs2_username_badge(data: Option<&Cs2BridgeAccount>) -> Option<UsernameBadge> {
    let (tone, label) = case_chip(data?.case_earned);
    Some(UsernameBadge { tone, label })
}

/// The bridge cache of `cs2Bridge.svelte.ts`. Loads are silent: on failure
/// the card shows nothing.
#[derive(Debug, Clone, Default)]
pub struct Cs2Bridge {
    data: BTreeMap<String, Cs2BridgeAccount>,
    version: u64,
    /// `None` until the settings were read.
    enabled: Option<bool>,
    last_fetch_at: f64,
}

/// What a load needs from the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cs2LoadPlan {
    /// The enabled flag is unknown: read the settings first.
    pub read_settings: bool,
    pub enabled: bool,
}

/// What the backend answered.
#[derive(Debug, Clone)]
pub struct Cs2LoadResult {
    pub enabled: Option<bool>,
    /// `Some` when the bridge was asked and answered.
    pub accounts: Option<Vec<Cs2BridgeAccount>>,
}

impl Cs2Bridge {
    /// `getCs2BridgeData`.
    pub fn data(&self, steam_id: &str) -> Option<&Cs2BridgeAccount> {
        self.data.get(steam_id)
    }

    /// `getCs2BridgeVersion`: bumps whenever the data changes.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// `invalidateCs2Bridge`: after a settings change, refetch next time.
    pub fn invalidate(&mut self) {
        self.enabled = None;
        self.last_fetch_at = 0.0;
        self.data.clear();
        self.version += 1;
    }

    /// `loadCs2BridgeData` up to the backend: `None` while the cache is
    /// fresh.
    pub fn begin_load(&self, force: bool, now_ms: f64) -> Option<Cs2LoadPlan> {
        if !force
            && self.last_fetch_at > 0.0
            && now_ms - self.last_fetch_at < CS2_BRIDGE_CACHE_TTL_MS
        {
            return None;
        }
        Some(Cs2LoadPlan {
            read_settings: self.enabled.is_none(),
            enabled: self.enabled.unwrap_or(false),
        })
    }

    /// The backend half, on the job pool.
    pub fn fetch(plan: Cs2LoadPlan, backend: &dyn Backend) -> Cs2LoadResult {
        let mut result = Cs2LoadResult {
            enabled: None,
            accounts: None,
        };
        let enabled = if plan.read_settings {
            match backend.cs2_bridge_get_settings() {
                Ok(settings) => {
                    result.enabled = Some(settings.enabled);
                    settings.enabled
                }
                Err(error) => {
                    warn(backend, "Bridge fetch failed", &error.message);
                    return result;
                }
            }
        } else {
            plan.enabled
        };
        if !enabled {
            return result;
        }
        match backend.cs2_bridge_fetch() {
            Ok(accounts) => {
                log_app_event(
                    backend,
                    "info",
                    "frontend.cs2_bridge",
                    "Bridge data refreshed",
                    Some(&json!({ "accountCount": accounts.len() })),
                );
                result.accounts = Some(accounts);
            }
            Err(error) => warn(backend, "Bridge fetch failed", &error.message),
        }
        result
    }

    /// Stores what [`Self::fetch`] got.
    pub fn finish_load(&mut self, result: Cs2LoadResult, now_ms: f64) {
        if let Some(enabled) = result.enabled {
            self.enabled = Some(enabled);
        }
        if let Some(accounts) = result.accounts {
            self.data = accounts
                .into_iter()
                .map(|account| (account.steam_id.clone(), account))
                .collect();
            self.version += 1;
        }
        self.last_fetch_at = now_ms;
    }

    /// The three steps in one go.
    pub fn load_blocking(&mut self, backend: &dyn Backend, force: bool, now_ms: f64) {
        if let Some(plan) = self.begin_load(force, now_ms) {
            let result = Self::fetch(plan, backend);
            self.finish_load(result, now_ms);
        }
    }

    /// Records the enabled flag a bridge check read from the settings
    /// (`enabledKnown = true` in `triggerCs2BridgeCheck`).
    pub fn note_enabled(&mut self, enabled: bool) {
        self.enabled = Some(enabled);
    }

    /// `triggerCs2BridgeCheck` result: merges a re-checked row.
    pub fn apply_check(&mut self, account: Cs2BridgeAccount) {
        self.data.insert(account.steam_id.clone(), account);
        self.version += 1;
    }
}

fn warn(backend: &dyn Backend, message: &str, error: &str) {
    log_app_event(
        backend,
        "warn",
        "frontend.cs2_bridge",
        message,
        Some(&json!({ "error": error })),
    );
}
