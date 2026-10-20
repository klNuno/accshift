//! Riot, ported from `src/lib/platforms/riot/{adapter,accountCache,profile,
//! contextMenu,riotApi,types}.ts`.

use super::{PlatformAccount, PlatformProfileInfo, StartupSnapshot};
use crate::menu::platform::{build_platform_context_menu, ForgetConfig};
use crate::menu::{MenuAction, MenuCommand, PlatformTask};
use crate::text::Text;
use crate::time::unix_ms_to_seconds;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// `RiotProfile`. `snapshot_state` is `setup_pending`, `awaiting_capture`,
/// `capturing`, `ready` or whatever a newer backend sends.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RiotProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_tag_line: Option<String>,
    #[serde(default)]
    pub snapshot_state: String,
    /// Unix MILLISECONDS.
    #[serde(default)]
    pub last_captured_at: Option<f64>,
    /// Unix MILLISECONDS.
    #[serde(default)]
    pub last_used_at: Option<f64>,
}

impl RiotProfile {
    /// Lenient parse: a row that does not fit is skipped by the callers.
    pub fn from_value(raw: &Value) -> Option<Self> {
        serde_json::from_value(raw.clone()).ok()
    }

    pub fn list(raw: &Value) -> Vec<Self> {
        raw.as_array()
            .map(|rows| rows.iter().filter_map(Self::from_value).collect())
            .unwrap_or_default()
    }
}

/// `getRiotAlias`: `name#tag`, `name`, or empty.
fn riot_alias(profile: &RiotProfile) -> String {
    let name = crate::js::trim(profile.account_name.as_deref().unwrap_or_default());
    let tag_line = crate::js::trim(profile.account_tag_line.as_deref().unwrap_or_default());
    if name.is_empty() {
        return String::new();
    }
    if tag_line.is_empty() {
        name.to_string()
    } else {
        format!("{name}#{tag_line}")
    }
}

/// `profileStatusLabel`. English, as TS has it.
fn status_label(profile: &RiotProfile) -> &'static str {
    match profile.snapshot_state.as_str() {
        "setup_pending" => "Waiting for connection",
        "capturing" => "Saving session",
        "ready" => "",
        _ => "Capture required",
    }
}

/// `profileSecondaryLabel`.
fn secondary_label(profile: &RiotProfile) -> String {
    if profile.snapshot_state == "ready" {
        return String::new();
    }
    let status = status_label(profile);
    let label = crate::js::trim(&profile.label);
    let alias = riot_alias(profile);
    if label.is_empty() || alias.is_empty() || label == alias {
        return status.to_string();
    }
    format!("{label} \u{b7} {status}")
}

/// `toRiotAccount`. Riot stamps milliseconds; the last use wins over the
/// last capture.
pub fn to_riot_account(profile: &RiotProfile) -> PlatformAccount {
    let alias = riot_alias(profile);
    PlatformAccount {
        id: profile.id.clone(),
        display_name: if alias.is_empty() {
            profile.label.clone()
        } else {
            alias
        },
        username: secondary_label(profile),
        last_login_at_sec: unix_ms_to_seconds(profile.last_used_at.or(profile.last_captured_at)),
    }
}

/// The Riot startup snapshot (`{ profiles, currentProfile }`), with the
/// profiles kept for the cache.
pub fn to_riot_snapshot(raw: &Value) -> (StartupSnapshot, Vec<RiotProfile>) {
    let profiles = raw
        .get("profiles")
        .map(RiotProfile::list)
        .unwrap_or_default();
    let snapshot = StartupSnapshot {
        accounts: profiles.iter().map(to_riot_account).collect(),
        current_account: raw
            .get("currentProfile")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };
    (snapshot, profiles)
}

/// `accountCache.ts`: the profiles of the last load, by id.
#[derive(Debug, Clone, Default)]
pub struct RiotProfileCache {
    profiles: BTreeMap<String, RiotProfile>,
}

impl RiotProfileCache {
    /// `rememberRiotProfiles`: replaces the whole cache.
    pub fn remember(&mut self, profiles: &[RiotProfile]) {
        self.profiles = profiles
            .iter()
            .map(|profile| (profile.id.clone(), profile.clone()))
            .collect();
    }

    /// `getCachedRiotProfileMeta`.
    pub fn get(&self, profile_id: &str) -> Option<&RiotProfile> {
        self.profiles.get(profile_id)
    }

    /// `forgetCachedRiotProfile`.
    pub fn forget(&mut self, profile_id: &str) {
        self.profiles.remove(profile_id);
    }
}

/// `getRiotProfile`: no avatar (shared fallback gradients), the alias or the
/// label, and a spinner while the session is being set up or saved.
pub fn riot_profile_info(profile: Option<&RiotProfile>) -> Option<PlatformProfileInfo> {
    let profile = profile?;
    let alias = riot_alias(profile);
    Some(PlatformProfileInfo {
        avatar_url: None,
        display_name: Some(if alias.is_empty() {
            profile.label.clone()
        } else {
            alias
        }),
        avatar_loading: matches!(
            profile.snapshot_state.as_str(),
            "setup_pending" | "capturing"
        ),
    })
}

/// `getNoAccountsToastMessage`.
pub const NO_ACCOUNTS_TOAST_KEY: &str = "toast.noRiotProfilesFound";

/// `getRiotContextMenuItems`: capture (or recapture), then forget. No copy
/// entries.
pub fn riot_context_menu_actions(
    account: &PlatformAccount,
    profile: Option<&RiotProfile>,
) -> Vec<MenuAction> {
    let capture_label = if profile.is_some_and(|profile| profile.snapshot_state == "ready") {
        "riot.recaptureSession"
    } else {
        "riot.captureSession"
    };
    let mut actions = vec![MenuAction::item(
        format!("riot.capture.{}", account.id),
        Some("platform.primary"),
        Text::key(capture_label),
        MenuCommand::Task(PlatformTask::RiotCapture {
            profile_id: account.id.clone(),
            display_name: account.display_name.clone(),
        }),
    )];
    let mut forget = ForgetConfig::keys(
        "riot.forgetConfirmTitle",
        "riot.forgetConfirmMessage",
        "riot.forget",
        "riot.forgotProfile",
    );
    forget.toast_params = vec![("profile".to_string(), account.display_name.as_str().into())];
    let base = if account.display_name.is_empty() {
        &account.username
    } else {
        &account.display_name
    };
    let trimmed = crate::js::trim(base);
    let display = if trimmed.is_empty() {
        account.username.clone()
    } else {
        trimmed.to_string()
    };
    actions.extend(build_platform_context_menu(
        "riot",
        account,
        &[],
        &forget,
        Some(display),
    ));
    actions
}
