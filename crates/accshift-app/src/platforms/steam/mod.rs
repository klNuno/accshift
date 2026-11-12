//! Steam.
//!
//! - [`api`]: the calls with logic of their own.
//! - [`warnings`]: ban warnings.
//! - [`context_menu`]: account menu entries.
//! - [`cs2`]: CS2 card content and the CS2 bridge cache.
//! - [`friend_code`]: CS2 friend codes.
//! - [`settings_schema`]: the Steam settings schema.
//! - here: account and profile mappings, SteamID helpers, the toast wording.

pub mod api;
pub mod context_menu;
pub mod cs2;
pub mod friend_code;
pub mod settings_schema;
pub mod warnings;

use super::{PlatformAccount, PlatformProfileInfo, StartupSnapshot};
use crate::text::Text;
use crate::types::ProfileInfo;
use serde_json::Value;

/// The community profile URL of a SteamID64.
pub fn to_profile_url(steam_id64: &str) -> String {
    format!("https://steamcommunity.com/profiles/{steam_id64}")
}

/// A Steam account payload as a platform account. Steam is the one platform
/// whose stamp is already Unix SECONDS (read out of `loginusers.vdf`), so it
/// passes through.
pub fn to_steam_account(raw: &Value) -> PlatformAccount {
    let text = |name: &str| {
        raw.get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    PlatformAccount {
        id: text("steam_id"),
        display_name: text("persona_name"),
        username: text("account_name"),
        last_login_at_sec: raw.get("last_login_at").and_then(Value::as_f64),
    }
}

/// `getAccounts().map(toSteamAccount)`.
pub fn to_steam_accounts(raw: &Value) -> Vec<PlatformAccount> {
    raw.as_array()
        .map(|list| list.iter().map(to_steam_account).collect())
        .unwrap_or_default()
}

/// The Steam startup snapshot (`{ accounts, currentAccount }`).
pub fn to_steam_snapshot(raw: &Value) -> StartupSnapshot {
    StartupSnapshot {
        accounts: raw
            .get("accounts")
            .map(to_steam_accounts)
            .unwrap_or_default(),
        current_account: raw
            .get("currentAccount")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

/// An absolute `http:` or `https:` URL with a host: surrounding C0 controls
/// and spaces are ignored, the scheme is case-insensitive and the host must
/// be non-empty.
pub fn is_safe_http_url(value: &str) -> bool {
    let trimmed = value.trim_matches(|c: char| c <= ' ');
    let Some((scheme, rest)) = trimmed.split_once(':') else {
        return false;
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let after_slashes = rest.trim_start_matches(['/', '\\']);
    let authority = after_slashes
        .split(['/', '\\', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    !host.is_empty()
}

/// `None` (no data, keep what is on screen) stays `None`; an unsafe or blank
/// avatar URL becomes "no avatar".
pub fn to_platform_profile_info(profile: Option<&ProfileInfo>) -> Option<PlatformProfileInfo> {
    let profile = profile?;
    let avatar = crate::js::trim(profile.avatar_url.as_deref().unwrap_or_default());
    Some(PlatformProfileInfo {
        avatar_url: (!avatar.is_empty() && is_safe_http_url(avatar)).then(|| avatar.to_string()),
        display_name: profile.display_name.clone(),
        avatar_loading: false,
    })
}

/// The toast for a Steam tab with no accounts.
pub const NO_ACCOUNTS_TOAST_KEY: &str = "toast.noSteamAccountsFound";

/// The dedicated toast for a failed switch, `None` for the generic one.
pub fn switch_error_toast(message: &str) -> Option<Text> {
    message
        .to_lowercase()
        .contains("steam is running as administrator")
        .then(|| Text::key("toast.steamElevated"))
}

/// The dedicated toast for a failed account load, `None` for the generic one.
pub fn load_error_toast(message: &str) -> Option<Text> {
    let normalized = crate::js::trim(message).to_lowercase();
    (normalized.contains("could not locate steam installation")
        || normalized.contains("could not read steam configuration"))
    .then(|| Text::key("toast.steamFolderNotFound"))
}
