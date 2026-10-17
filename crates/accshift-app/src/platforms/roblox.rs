//! Roblox, ported from `src/lib/platforms/roblox/{adapter,contextMenu,
//! robloxApi,types,warnings}.ts`. The profile cache is M2 work.

use super::warnings::{WarningChip, WarningLoad, WarningPresentation, WarningTone};
use super::{PlatformAccount, PlatformProfileInfo, StartupSnapshot};
use crate::backend::Backend;
use crate::error::BackendError;
use crate::menu::platform::{build_platform_context_menu, CopyItemConfig, ForgetConfig};
use crate::menu::MenuAction;
use crate::settings::AppSettings;
use crate::text::Text;
use crate::time::unix_ms_to_seconds;
use crate::types::RobloxProfileInfo;
use serde_json::Value;
use std::collections::BTreeSet;

/// `SESSION_EXPIRED_PATTERN`: the backend's wording when the stored
/// `.ROBLOSECURITY` cookie is refused, matched case-insensitively.
pub const SESSION_EXPIRED_PATTERN: &str = "auth ticket request failed (http 401";

/// `SESSION_EXPIRED_PATTERN.test(message)`.
pub fn is_session_expired_error(message: &str) -> bool {
    message.to_lowercase().contains(SESSION_EXPIRED_PATTERN)
}

/// `toRobloxAccount` on the camelCase `RobloxAccount` payload.
/// `lastLoginAt` is milliseconds despite its name.
pub fn to_roblox_account(raw: &Value) -> PlatformAccount {
    let text = |name: &str| raw.get(name).and_then(Value::as_str).unwrap_or_default();
    let username = text("username");
    let display_name = text("displayName");
    PlatformAccount {
        id: text("userId").to_string(),
        display_name: if display_name.is_empty() {
            username
        } else {
            display_name
        }
        .to_string(),
        username: username.to_string(),
        last_login_at_sec: unix_ms_to_seconds(raw.get("lastLoginAt").and_then(Value::as_f64)),
    }
}

pub fn to_roblox_accounts(raw: &Value) -> Vec<PlatformAccount> {
    raw.as_array()
        .map(|rows| rows.iter().map(to_roblox_account).collect())
        .unwrap_or_default()
}

/// The Roblox startup snapshot (`{ accounts, currentAccount }`).
pub fn to_roblox_snapshot(raw: &Value) -> StartupSnapshot {
    StartupSnapshot {
        accounts: raw
            .get("accounts")
            .map(to_roblox_accounts)
            .unwrap_or_default(),
        current_account: raw
            .get("currentAccount")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

/// `getProfileInfo` after the fetch: `None` is a failed fetch; a profile with
/// no picture or an unsafe URL still resolves, avatar-less.
pub fn to_roblox_profile_info(info: Option<&RobloxProfileInfo>) -> Option<PlatformProfileInfo> {
    let info = info?;
    let avatar = info
        .avatar_url
        .as_deref()
        .filter(|url| !url.is_empty() && super::steam::is_safe_http_url(url));
    Some(PlatformProfileInfo {
        avatar_url: avatar.map(str::to_string),
        display_name: None,
        avatar_loading: false,
    })
}

/// `getNoAccountsToastMessage`.
pub const NO_ACCOUNTS_TOAST_KEY: &str = "toast.noRobloxAccountsFound";
/// `getNoAccountsHintMessage`.
pub const NO_ACCOUNTS_HINT_KEY: &str = "roblox.noAccountsHint";

/// `getSwitchErrorToastMessage`.
pub fn switch_error_toast(message: &str) -> Option<Text> {
    is_session_expired_error(message).then(|| Text::key("roblox.sessionExpiredSwitchError"))
}

/// `getRobloxContextMenuItems`: username and user id copies, then forget.
pub fn roblox_context_menu_actions(account: &PlatformAccount) -> Vec<MenuAction> {
    let username = if account.username.is_empty() {
        account.id.clone()
    } else {
        account.username.clone()
    };
    let copy_items = [
        CopyItemConfig::new("username", username, "roblox.copyLabelUsername"),
        CopyItemConfig::new("userId", account.id.clone(), "roblox.copyLabelUserId"),
    ];
    let forget = ForgetConfig::keys(
        "roblox.forgetConfirmTitle",
        "roblox.forgetConfirmMessage",
        "roblox.forget",
        "roblox.forgotAccount",
    );
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
    build_platform_context_menu("roblox", account, &copy_items, &forget, Some(display))
}

/// The module state of `warnings.ts`: dead sessions this session knows of.
/// Roblox cookies carry no readable expiry, so the backend probes them over
/// the network, once per session unless forced.
#[derive(Debug, Clone, Default)]
pub struct RobloxSessions {
    dead: BTreeSet<String>,
    checked_this_session: bool,
}

/// `expiredPresentation`.
pub fn expired_presentation() -> WarningPresentation {
    WarningPresentation {
        tooltip: Text::key("roblox.sessionExpiredTooltip"),
        card_outline_tone: Some(WarningTone::Orange),
        list_has_red: false,
        list_has_orange: true,
        chips: vec![WarningChip {
            tone: WarningTone::Orange,
            text: Text::key("roblox.sessionExpired"),
        }],
    }
}

impl RobloxSessions {
    /// `markRobloxSessionExpired`: a switch refused with HTTP 401 proves the
    /// cookie dead, whatever the health-check setting.
    pub fn mark_expired(&mut self, user_id: &str) {
        self.dead.insert(user_id.to_string());
    }

    /// `clearRobloxSessionExpired`: a fresh cookie or a successful switch.
    pub fn clear_expired(&mut self, user_id: &str) {
        self.dead.remove(user_id);
    }

    pub fn is_expired(&self, user_id: &str) -> bool {
        self.dead.contains(user_id)
    }

    /// `getCachedRobloxWarningStates` (`toWarningMap`).
    pub fn warning_map(&self) -> WarningLoad {
        WarningLoad {
            warnings: self
                .dead
                .iter()
                .map(|id| (id.clone(), expired_presentation()))
                .collect(),
            outcome: None,
            toasts: Vec::new(),
        }
    }

    /// `loadRobloxWarningStates` up to the probe: whether to run it. The
    /// setting gates the probe only; marks from failed switches still show.
    pub fn needs_probe(
        &self,
        settings: &AppSettings,
        accounts: &[PlatformAccount],
        force_refresh: bool,
    ) -> bool {
        let enabled = settings
            .health_check_per_platform
            .get("roblox")
            .copied()
            .unwrap_or(true);
        enabled && !accounts.is_empty() && (force_refresh || !self.checked_this_session)
    }

    /// The probe, on the job pool.
    pub fn probe(backend: &dyn Backend) -> Result<Vec<String>, BackendError> {
        backend.roblox_check_sessions()
    }

    /// Stores the probe's answer. A failure keeps what was known.
    pub fn finish_probe(&mut self, result: Result<Vec<String>, BackendError>) -> WarningLoad {
        if let Ok(dead) = result {
            self.dead = dead.into_iter().collect();
            self.checked_this_session = true;
        }
        self.warning_map()
    }

    /// The whole load, for callers that may block.
    pub fn load_blocking(
        &mut self,
        backend: &dyn Backend,
        settings: &AppSettings,
        accounts: &[PlatformAccount],
        force_refresh: bool,
    ) -> WarningLoad {
        if !self.needs_probe(settings, accounts, force_refresh) {
            return self.warning_map();
        }
        let result = Self::probe(backend);
        self.finish_probe(result)
    }
}
