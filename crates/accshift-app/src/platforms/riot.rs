//! Riot: account mapping, the profile cache of the last load, profile info
//! and menu entries.

use super::api::log_app_event;
use super::{PlatformAccount, PlatformProfileInfo, StartupSnapshot};
use crate::backend::Backend;
use crate::error::BackendResult;
use crate::menu::platform::{build_platform_context_menu, ForgetConfig};
use crate::menu::{MenuAction, MenuActionBody, MenuCommand, PlatformTask, SubmenuLoader};
use crate::text::Text;
use crate::time::unix_ms_to_seconds;
use crate::types::RiotGame;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A Riot profile. `snapshot_state` is `setup_pending`, `awaiting_capture`,
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

/// `name#tag`, `name`, or empty.
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

/// The status label, in English.
fn status_label(profile: &RiotProfile) -> &'static str {
    match profile.snapshot_state.as_str() {
        "setup_pending" => "Waiting for connection",
        "capturing" => "Saving session",
        "ready" => "",
        _ => "Capture required",
    }
}

/// The secondary label: nothing once ready, else the label and the status.
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

/// A Riot profile as a platform account. Riot stamps milliseconds; the last
/// use wins over the last capture.
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

/// The profiles of the last load, by id.
#[derive(Debug, Clone, Default)]
pub struct RiotProfileCache {
    profiles: BTreeMap<String, RiotProfile>,
}

impl RiotProfileCache {
    /// Replaces the whole cache.
    pub fn remember(&mut self, profiles: &[RiotProfile]) {
        self.profiles = profiles
            .iter()
            .map(|profile| (profile.id.clone(), profile.clone()))
            .collect();
    }

    pub fn get(&self, profile_id: &str) -> Option<&RiotProfile> {
        self.profiles.get(profile_id)
    }

    pub fn forget(&mut self, profile_id: &str) {
        self.profiles.remove(profile_id);
    }
}

/// No avatar (shared fallback gradients), the alias or the label, and a
/// spinner while the session is being set up or saved.
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

/// The toast for a Riot tab with no profiles.
pub const NO_ACCOUNTS_TOAST_KEY: &str = "toast.noRiotProfilesFound";

/// Start of the backend's refusal to start a game on a session whose account
/// it could not confirm (`RIOT_ACCOUNT_UNCONFIRMED` in the core).
pub const ACCOUNT_UNCONFIRMED_PATTERN: &str = "riot account not confirmed";

/// Start of the backend's refusal to adopt the signed-in account on a profile
/// with a saved session but no recorded account
/// (`RIOT_PROFILE_WITHOUT_ACCOUNT` in the core).
pub const PROFILE_WITHOUT_ACCOUNT_PATTERN: &str = "riot profile has no recorded account";

/// The dedicated toast for a failed switch or game launch, `None` for the
/// generic one.
pub fn switch_error_toast(message: &str) -> Option<Text> {
    let message = message.to_lowercase();
    if message.starts_with(ACCOUNT_UNCONFIRMED_PATTERN) {
        Some(Text::key("riot.accountNotConfirmed"))
    } else if message.starts_with(PROFILE_WITHOUT_ACCOUNT_PATTERN) {
        Some(Text::key("riot.profileWithoutAccount"))
    } else {
        None
    }
}

/// The "switch and run a game" entry of a ready profile: its submenu lists
/// the installed games once open.
fn run_game_item(account: &PlatformAccount, is_current: bool) -> MenuAction {
    MenuAction {
        id: format!("riot.switch.run.{}", account.id),
        group: Some("platform.primary".to_string()),
        label: Text::key(if is_current {
            "riot.runPick"
        } else {
            "riot.switchAndRunPick"
        }),
        body: MenuActionBody::Item {
            command: None,
            submenu: None,
            loader: Some(SubmenuLoader::RiotInstalledGames {
                profile_id: account.id.clone(),
            }),
        },
    }
}

/// Switch and run a game (ready profiles only), capture (or recapture),
/// then forget. No copy entries.
pub fn riot_context_menu_actions(
    account: &PlatformAccount,
    profile: Option<&RiotProfile>,
    current_account_id: Option<&str>,
) -> Vec<MenuAction> {
    let ready = profile.is_some_and(|profile| profile.snapshot_state == "ready");
    let capture_label = if ready {
        "riot.recaptureSession"
    } else {
        "riot.captureSession"
    };
    let mut actions = Vec::new();
    if ready {
        let is_current = current_account_id == Some(account.id.as_str());
        actions.push(run_game_item(account, is_current));
    }
    actions.push(MenuAction::item(
        format!("riot.capture.{}", account.id),
        Some("platform.primary"),
        Text::key(capture_label),
        MenuCommand::Task(PlatformTask::RiotCapture {
            profile_id: account.id.clone(),
            display_name: account.display_name.clone(),
        }),
    ));
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

/// The Riot profiles whose saved session is past its restore window, read
/// offline by the backend from each snapshot when the list loads and after
/// each switch. A failed read keeps what was known: unknown is never expired.
#[derive(Debug, Clone, Default)]
pub struct RiotSessions {
    expired: std::collections::BTreeSet<String>,
}

/// The warning of an expired session: blue chip and outline, never a ban colour.
pub fn session_expired_presentation() -> super::warnings::WarningPresentation {
    use super::warnings::{WarningChip, WarningPresentation, WarningTone};
    WarningPresentation {
        tooltip: Text::key("session.expiredTooltip"),
        card_outline_tone: Some(WarningTone::Blue),
        list_has_red: false,
        list_has_orange: false,
        list_has_blue: true,
        chips: vec![WarningChip {
            tone: WarningTone::Blue,
            text: Text::key("session.expired"),
        }],
    }
}

/// The notice shown before switching to an expired profile.
pub const SESSION_EXPIRED_NOTICE_KEY: &str = "session.expiredSwitchNotice";

impl RiotSessions {
    /// Stores the backend's answer; a failure keeps what was known.
    pub fn finish_check(&mut self, result: crate::error::BackendResult<Vec<String>>) {
        if let Ok(expired) = result {
            self.expired = expired.into_iter().collect();
        }
    }

    pub fn is_expired(&self, profile_id: &str) -> bool {
        self.expired.contains(profile_id)
    }

    /// The known expired sessions as a warning map.
    pub fn warning_map(&self) -> BTreeMap<String, super::warnings::WarningPresentation> {
        self.expired
            .iter()
            .map(|id| (id.clone(), session_expired_presentation()))
            .collect()
    }
}

/// The run submenu, once the installed games are in: one entry per game, or
/// a closing entry when none is installed.
pub fn installed_games_submenu(profile_id: &str, games: &[RiotGame]) -> Vec<MenuAction> {
    if games.is_empty() {
        return vec![MenuAction::item(
            format!("riot.switch.run.{profile_id}.empty"),
            None,
            Text::key("riot.noInstalledGames"),
            MenuCommand::Close,
        )];
    }
    games
        .iter()
        .map(|game| {
            MenuAction::item(
                format!("riot.switch.run.{profile_id}.{}", game.product_id),
                None,
                Text::raw(game.name.clone()),
                MenuCommand::Task(PlatformTask::RiotRunGame {
                    profile_id: profile_id.to_string(),
                    product_id: game.product_id.clone(),
                }),
            )
        })
        .collect()
}

/// Switches to the profile and starts a game, logged before and after.
pub fn switch_profile_and_launch_game(
    backend: &dyn Backend,
    profile_id: &str,
    product_id: &str,
) -> BackendResult<()> {
    const SOURCE: &str = "frontend.riot.switch_and_launch_game";
    let details = json!({ "profileId": profile_id, "productId": product_id });
    log_app_event(
        backend,
        "info",
        SOURCE,
        "Switch+launch request started",
        Some(&details),
    );
    match backend.riot_switch_profile_and_launch_game(profile_id, product_id) {
        Ok(()) => {
            log_app_event(
                backend,
                "info",
                SOURCE,
                "Switch+launch request completed",
                Some(&details),
            );
            Ok(())
        }
        Err(error) => {
            let mut failed = details.as_object().cloned().unwrap_or_default();
            failed.insert("error".to_string(), Value::String(error.message.clone()));
            log_app_event(
                backend,
                "error",
                SOURCE,
                "Switch+launch failed",
                Some(&Value::Object(failed)),
            );
            Err(error)
        }
    }
}
