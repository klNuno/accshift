use crate::context::{AppContext, AppCtx};
use crate::error::PlatformError;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Canonical platform identifiers.
///
/// Single vocabulary shared by the service registry, the CLI and telemetry —
/// note the dash in `battle-net` (telemetry used to emit `battle_net` for
/// account snapshots; it now uses these constants, see
/// `emit_accounts_snapshots` in the Tauri commands).
pub mod ids {
    pub const STEAM: &str = "steam";
    pub const RIOT: &str = "riot";
    pub const BATTLE_NET: &str = "battle-net";
    pub const UBISOFT: &str = "ubisoft";
    pub const ROBLOX: &str = "roblox";
    pub const EPIC: &str = "epic";
    pub const GOG: &str = "gog";
    pub const JAGEX: &str = "jagex";
    pub const DISCORD: &str = "discord";

    /// Every platform the app knows about, in display order.
    pub const ALL: [&str; 9] = [
        STEAM, RIOT, BATTLE_NET, UBISOFT, ROBLOX, EPIC, GOG, JAGEX, DISCORD,
    ];
}

// Native clients for Battle.net, Epic, Riot, Ubisoft and Roblox don't exist
// on Linux / macOS. We gate them to Windows to keep the non-Windows build
// green; `get_service("riot")` etc. return None outside Windows, and the CLI
// advertises `available: false` for those platforms via `accshift platforms`.
#[cfg(windows)]
pub mod battle_net;
#[cfg(windows)]
pub mod discord;
#[cfg(windows)]
pub mod epic;
#[cfg(windows)]
pub mod gog;
#[cfg(windows)]
pub mod jagex;
#[cfg(windows)]
pub mod riot;
#[cfg(windows)]
pub mod roblox;
#[cfg(windows)]
pub(crate) mod setup_jobs;
pub mod steam;
#[cfg(windows)]
pub mod ubisoft;

pub(crate) fn redact_id(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 2 {
        "***".into()
    } else {
        format!("{}***", chars[..2].iter().collect::<String>())
    }
}

pub(crate) fn redact_opt(value: Option<&str>) -> serde_json::Value {
    match value {
        Some(v) => serde_json::Value::String(redact_id(v)),
        None => serde_json::Value::Null,
    }
}

pub(crate) fn log_platform_event(
    app_handle: &dyn AppContext,
    level: &str,
    source: &str,
    message: &str,
    details: impl Into<String>,
) {
    let details = details.into();
    let _ = crate::logging::append_app_log(
        app_handle,
        level,
        source,
        message,
        if details.is_empty() {
            None
        } else {
            Some(details.as_str())
        },
    );
}

pub(crate) fn log_platform_info(
    app_handle: &dyn AppContext,
    source: &str,
    message: &str,
    details: impl Into<String>,
) {
    log_platform_event(app_handle, "info", source, message, details);
}

pub(crate) fn log_platform_error(
    app_handle: &dyn AppContext,
    source: &str,
    message: &str,
    details: impl Into<String>,
) {
    log_platform_event(app_handle, "error", source, message, details);
}

/// Logs the failure and returns the error unchanged, preserving its
/// [`crate::error::PlatformErrorKind`]. Typical use:
/// `.map_err(|e| log_platform_failure(&app, "steam.get_accounts", e.into()))`.
pub(crate) fn log_platform_failure(
    app_handle: &dyn AppContext,
    source: &str,
    error: PlatformError,
) -> PlatformError {
    log_platform_error(
        app_handle,
        source,
        "Platform operation failed",
        &error.message,
    );
    error
}

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn setup_expired(last_touched_at: u64, ttl_ms: u64) -> bool {
    now_unix_ms().saturating_sub(last_touched_at) > ttl_ms
}

pub(crate) fn make_setup_status(
    setup_id: &str,
    state: &str,
    account_id: impl Into<String>,
    display_name: impl Into<String>,
    error: impl Into<String>,
) -> SetupStatus {
    SetupStatus {
        setup_id: setup_id.to_string(),
        state: state.to_string(),
        account_id: account_id.into(),
        account_display_name: display_name.into(),
        error_message: error.into(),
    }
}

/// Common setup status returned by all platforms.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub setup_id: String,
    pub state: String,
    pub account_id: String,
    pub account_display_name: String,
    pub error_message: String,
}

/// Core trait that all platforms implement.
///
/// Methods take `AppCtx` by value because several impls move the context
/// into `spawn_blocking` closures. Helpers that only borrow take
/// `&dyn AppContext` — callers with an `AppCtx` pass `&ctx` and let Deref
/// coercion handle the rest.
pub trait PlatformService: Send + Sync {
    // Account operations: returns platform-specific JSON.
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError>;
    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError>;
    fn get_current_account(&self, app: AppCtx) -> Result<String, PlatformError>;
    /// `params` carries platform-specific extras (e.g. Steam's runAsAdmin/launchOptions).
    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        params: Value,
    ) -> Result<(), PlatformError>;
    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError>;

    // Setup flow
    fn begin_setup(&self, app: AppCtx, params: Value) -> Result<SetupStatus, PlatformError>;
    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError>;
    fn cancel_setup(&self, app: AppCtx, setup_id: &str) -> Result<(), PlatformError>;

    // Path management (default: not supported)
    fn get_path(&self, _app: AppCtx) -> Result<String, PlatformError> {
        Err(PlatformError::other("Path management not supported"))
    }
    fn set_path(&self, _app: AppCtx, _path: &str) -> Result<(), PlatformError> {
        Ok(())
    }
    fn select_path(&self) -> Result<String, PlatformError> {
        Err(PlatformError::other("Path management not supported"))
    }

    // Account labeling (default: not supported)
    fn set_account_label(
        &self,
        _app: AppCtx,
        _account_id: &str,
        _label: &str,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::other("Account labeling not supported"))
    }
}

fn platform_registry() -> &'static HashMap<&'static str, &'static dyn PlatformService> {
    static REGISTRY: OnceLock<HashMap<&'static str, &'static dyn PlatformService>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map: HashMap<&'static str, &'static dyn PlatformService> = HashMap::new();
        map.insert(ids::STEAM, &steam::STEAM_SERVICE);
        #[cfg(windows)]
        {
            map.insert(ids::RIOT, &riot::RIOT_SERVICE);
            map.insert(ids::BATTLE_NET, &battle_net::BATTLE_NET_SERVICE);
            map.insert(ids::UBISOFT, &ubisoft::UBISOFT_SERVICE);
            map.insert(ids::ROBLOX, &roblox::ROBLOX_SERVICE);
            map.insert(ids::EPIC, &epic::EPIC_SERVICE);
            map.insert(ids::GOG, &gog::GOG_SERVICE);
            map.insert(ids::JAGEX, &jagex::JAGEX_SERVICE);
            map.insert(ids::DISCORD, &discord::DISCORD_SERVICE);
        }
        map
    })
}

pub fn get_service(platform_id: &str) -> Option<&'static dyn PlatformService> {
    platform_registry().get(platform_id).copied()
}

pub fn require_service(platform_id: &str) -> Result<&'static dyn PlatformService, PlatformError> {
    get_service(platform_id)
        .ok_or_else(|| PlatformError::other(format!("Unknown platform: {platform_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_unix_ms_returns_positive_timestamp() {
        let ts = now_unix_ms();
        assert!(ts > 0, "timestamp should be positive, got {ts}");
    }

    #[test]
    fn now_unix_ms_is_within_reasonable_range() {
        let ts = now_unix_ms();
        // Should be after 2024-01-01 and within an hour of the actual system time
        let jan_2024 = 1_704_067_200_000u64;
        assert!(ts > jan_2024, "timestamp {ts} should be after 2024-01-01");

        let one_hour_ms = 3_600_000u64;
        let system_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let diff = ts.abs_diff(system_ms);
        assert!(
            diff < one_hour_ms,
            "timestamp drift {diff}ms exceeds 1 hour"
        );
    }

    #[test]
    fn setup_expired_true_when_elapsed_exceeds_ttl() {
        let old_time = now_unix_ms() - 10_000; // 10 seconds ago
        assert!(setup_expired(old_time, 5_000)); // 5s TTL
    }

    #[test]
    fn setup_expired_false_when_within_ttl() {
        let recent = now_unix_ms() - 1_000; // 1 second ago
        assert!(!setup_expired(recent, 5_000)); // 5s TTL
    }

    #[test]
    fn setup_expired_boundary_at_exact_ttl() {
        // At exactly the TTL boundary, elapsed == ttl, not > ttl, so should be false.
        let ts = now_unix_ms();
        // last_touched_at = ts means elapsed ≈ 0, well within any TTL
        assert!(!setup_expired(ts, 0));
    }

    #[test]
    fn setup_expired_handles_zero_last_touched() {
        // last_touched_at = 0 means it was set at epoch — always expired with any real TTL
        assert!(setup_expired(0, 1_000));
    }

    #[test]
    fn make_setup_status_builds_correct_fields() {
        let status = make_setup_status("sid-1", "pending", "acc-42", "Player One", "");

        assert_eq!(status.setup_id, "sid-1");
        assert_eq!(status.state, "pending");
        assert_eq!(status.account_id, "acc-42");
        assert_eq!(status.account_display_name, "Player One");
        assert_eq!(status.error_message, "");
    }

    #[test]
    fn make_setup_status_with_error() {
        let status = make_setup_status("sid-2", "failed", "", "", "connection refused");

        assert_eq!(status.setup_id, "sid-2");
        assert_eq!(status.state, "failed");
        assert!(status.account_id.is_empty());
        assert!(status.account_display_name.is_empty());
        assert_eq!(status.error_message, "connection refused");
    }

    #[test]
    fn make_setup_status_accepts_string_types() {
        let id = String::from("acc-owned");
        let name = String::from("Named");
        let err = String::from("err");
        let status = make_setup_status("s", "done", id, name, err);
        assert_eq!(status.account_id, "acc-owned");
        assert_eq!(status.account_display_name, "Named");
        assert_eq!(status.error_message, "err");
    }

    #[test]
    fn require_service_returns_err_for_unknown_platform() {
        let result = require_service("nintendo");
        assert!(result.is_err());
        let err = result.err().unwrap();
        // Message is what the webview toast shows — must stay this string.
        assert_eq!(err.to_string(), "Unknown platform: nintendo");
        assert_eq!(err.kind, crate::error::PlatformErrorKind::Other);
    }

    #[test]
    fn require_service_returns_ok_for_known_platforms() {
        #[cfg(windows)]
        let platforms: &[&str] = &[
            "steam",
            "riot",
            "battle-net",
            "ubisoft",
            "roblox",
            "epic",
            "gog",
            "jagex",
            "discord",
        ];
        #[cfg(not(windows))]
        let platforms: &[&str] = &["steam"];
        for platform in platforms {
            let result = require_service(platform);
            assert!(
                result.is_ok(),
                "require_service should succeed for '{platform}'"
            );
        }
    }

    #[test]
    fn get_service_returns_none_for_unknown() {
        assert!(get_service("playstation").is_none());
    }
}
