use crate::context::{AppContext, AppCtx};
use crate::error::PlatformError;
use descriptor::{Descriptor, DescriptorOrigin, DescriptorService};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Canonical platform identifiers.
///
/// Single vocabulary shared by the service registry, the CLI and telemetry.
/// Note the dash in `battle-net` (telemetry used to emit `battle_net` for
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
// Battle.net has a real native macOS client, so it is available on Windows and
// macOS (its config format is identical; only the paths and launcher differ).
#[cfg(any(windows, target_os = "macos"))]
pub mod battle_net;
/// Platforms described by a JSON descriptor and run by a single engine. GOG,
/// Jagex, Epic, Ubisoft and Discord live here instead of in a module of their
/// own.
pub mod descriptor;
#[cfg(windows)]
pub mod riot;
#[cfg(windows)]
pub mod roblox;
pub(crate) mod setup_jobs;
pub mod steam;

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
/// `&dyn AppContext`. Callers with an `AppCtx` pass `&ctx` and let Deref
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

    /// Whether this launcher looks present on this machine.
    ///
    /// Only used to pick the platforms enabled by default on a fresh install,
    /// never to gate a feature: a user can always switch a platform on by
    /// hand, and detection has no way to be exhaustive.
    ///
    /// The default reads `get_path`, which every path-managing platform
    /// resolves from the real install (or from the user's override) and fails
    /// when it finds nothing. Existence is rechecked here because an override
    /// can outlive the folder it points at. Platforms without path management
    /// override this.
    fn is_installed(&self, app: AppCtx) -> bool {
        match self.get_path(app) {
            Ok(path) => {
                let trimmed = path.trim();
                !trimmed.is_empty() && Path::new(trimmed).exists()
            }
            Err(_) => false,
        }
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

    /// Whether [`Self::dry_run`] answers on this platform.
    ///
    /// Asked before calling, so a caller reports "this platform has no plan"
    /// without reading an error message to find out.
    fn supports_dry_run(&self) -> bool {
        false
    }

    /// Everything switching to `account_id` would read, copy, write and close,
    /// without doing any of it.
    ///
    /// Only descriptor-driven platforms answer this today: the hand-written
    /// modules would each need their own plan, and the point of the descriptors
    /// is that they no longer have to.
    fn dry_run(&self, _app: AppCtx, _account_id: &str) -> Result<Value, PlatformError> {
        Err(PlatformError::other(
            "Dry run is only available for platforms described by a descriptor",
        ))
    }
}

fn platform_registry() -> &'static HashMap<&'static str, &'static dyn PlatformService> {
    static REGISTRY: OnceLock<HashMap<&'static str, &'static dyn PlatformService>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map: HashMap<&'static str, &'static dyn PlatformService> = HashMap::new();
        map.insert(ids::STEAM, &steam::STEAM_SERVICE);
        #[cfg(target_os = "macos")]
        {
            map.insert(ids::BATTLE_NET, &battle_net::BATTLE_NET_SERVICE);
        }
        #[cfg(windows)]
        {
            map.insert(ids::RIOT, &riot::RIOT_SERVICE);
            map.insert(ids::BATTLE_NET, &battle_net::BATTLE_NET_SERVICE);
            map.insert(ids::ROBLOX, &roblox::ROBLOX_SERVICE);
        }
        // Descriptor-driven platforms register last so a hand-written module
        // always wins: converting one means deleting its module, never having
        // two services answer for the same id.
        for service in descriptor::services() {
            let id = ids::ALL
                .iter()
                .find(|known| **known == service.id())
                .copied()
                .unwrap_or_else(|| Box::leak(service.id().to_string().into_boxed_str()));
            map.entry(id)
                .or_insert(*service as &'static dyn PlatformService);
        }
        map
    })
}

/// Services built from the user's own descriptors, keyed by platform id.
///
/// Separate from [`platform_registry`] because that one is built once with no
/// context to hand, and a user descriptor is a file only an [`AppContext`] can
/// locate. Shipped platforms are looked up first, so a dropped-in file can
/// never take over an id this build already answers for.
fn user_registry() -> &'static RwLock<HashMap<String, &'static DescriptorService>> {
    static USER: OnceLock<RwLock<HashMap<String, &'static DescriptorService>>> = OnceLock::new();
    USER.get_or_init(|| RwLock::new(HashMap::new()))
}

/// A descriptor read fine but was not registered.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPlatform {
    pub id: String,
    pub reason: String,
}

/// A file in the user's folder that is not a usable descriptor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedDescriptor {
    /// The file name, as the user sees it in the folder.
    pub source: String,
    /// Dotted path of the offending field, empty when the whole file failed.
    pub field: String,
    pub problem: String,
}

/// What the last read of the user's descriptor folder produced.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPlatformReport {
    /// The folder that was read, so a caller can say where to drop a file.
    pub dir: String,
    /// The descriptors now answering to [`get_service`], in load order. The
    /// whole descriptor travels, not just the id: the frontend builds its
    /// platform entry from it and so needs no second description of a platform
    /// this build was never compiled to know about.
    pub loaded: Vec<Descriptor>,
    pub skipped: Vec<SkippedPlatform>,
    pub rejected: Vec<RejectedDescriptor>,
}

/// Reads the user's descriptor folder and makes every platform in it answer to
/// [`get_service`]. Calling it again is the hot reload: the folder is the
/// truth, so an id whose file is gone stops answering.
///
/// Services are leaked, because the registry hands out `&'static dyn
/// PlatformService`. A reload therefore leaks the descriptors it replaces, a
/// few kilobytes each: bounded by how often a human edits a file, and the
/// alternative is handing out a reference that can dangle mid-switch.
pub fn reload_user_platforms(app: &dyn AppContext) -> UserPlatformReport {
    let mut report = UserPlatformReport {
        dir: descriptor::user_dir(app)
            .map(|dir| dir.display().to_string())
            .unwrap_or_default(),
        ..UserPlatformReport::default()
    };

    let (descriptors, errors) = descriptor::load_user(app);
    report.rejected = errors
        .into_iter()
        .map(|error| RejectedDescriptor {
            source: error.source,
            field: error.field,
            problem: error.problem,
        })
        .collect();

    let mut fresh: HashMap<String, &'static DescriptorService> = HashMap::new();
    for (path, descriptor) in descriptors {
        let id = descriptor.id.clone();
        if platform_registry().contains_key(id.as_str()) {
            report.skipped.push(SkippedPlatform {
                id: id.clone(),
                reason: format!("`{id}` is a platform this build already ships"),
            });
            continue;
        }
        if fresh.contains_key(&id) {
            report.skipped.push(SkippedPlatform {
                id: id.clone(),
                reason: format!("another file in this folder already declares `{id}`"),
            });
            continue;
        }
        if descriptor.current_profile().is_none() {
            report.skipped.push(SkippedPlatform {
                id: id.clone(),
                reason: format!("`{id}` declares no profile for this operating system"),
            });
            continue;
        }
        report.loaded.push(descriptor.clone());
        let service = DescriptorService::new(descriptor, DescriptorOrigin::User(path));
        fresh.insert(id, Box::leak(Box::new(service)));
    }

    if let Ok(mut registry) = user_registry().write() {
        *registry = fresh;
    }
    report
}

/// Whether this build compiled a platform in under that id.
///
/// The one question that separates "the user may add this" from "this is ours":
/// a shipped id is never overridable by a file in a folder, and never removable
/// by deleting one.
pub fn is_shipped(platform_id: &str) -> bool {
    platform_registry().contains_key(platform_id)
}

pub fn get_service(platform_id: &str) -> Option<&'static dyn PlatformService> {
    if let Some(service) = platform_registry().get(platform_id) {
        return Some(*service);
    }
    let registry = user_registry().read().ok()?;
    registry
        .get(platform_id)
        .map(|service| *service as &'static dyn PlatformService)
}

/// Every platform id the app can reach right now: the shipped ones in display
/// order, then whatever the user added, sorted.
pub fn all_ids() -> Vec<String> {
    let mut all: Vec<String> = ids::ALL.iter().map(|id| (*id).to_string()).collect();
    if let Ok(registry) = user_registry().read() {
        let mut user: Vec<String> = registry.keys().cloned().collect();
        user.sort();
        all.extend(user);
    }
    all
}

/// Ids of the platforms whose launcher was found on this machine, in
/// [`all_ids`] order. Platforms with no service on this OS are skipped, so the
/// result is always a set the app can actually enable.
pub fn detect_installed(app: AppCtx) -> Vec<String> {
    all_ids()
        .into_iter()
        .filter(|id| get_service(id).is_some_and(|service| service.is_installed(app.clone())))
        .collect()
}

pub fn require_service(platform_id: &str) -> Result<&'static dyn PlatformService, PlatformError> {
    get_service(platform_id)
        .ok_or_else(|| PlatformError::other(format!("Unknown platform: {platform_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Detection reads the real machine, so what it finds cannot be asserted.
    // The shape can: an id nobody can enable, a duplicate or a reordering
    // would each break the caller, which feeds the result straight into
    // `enabledPlatforms`.
    struct TempCtx {
        root: std::path::PathBuf,
    }

    impl AppContext for TempCtx {
        fn app_config_dir(&self) -> Result<std::path::PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<std::path::PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<std::path::PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    #[test]
    fn detect_installed_only_returns_enableable_ids_in_display_order() {
        let ctx: AppCtx = std::sync::Arc::new(TempCtx {
            root: std::env::temp_dir().join(format!("accshift-detect-test-{}", std::process::id())),
        });

        let detected = detect_installed(ctx);

        for id in &detected {
            assert!(
                get_service(id).is_some(),
                "detected {id} has no service on this OS"
            );
        }

        let order = all_ids();
        let mut expected_order = detected.clone();
        expected_order.sort_by_key(|id| order.iter().position(|known| known == id));
        expected_order.dedup();
        assert_eq!(
            detected, expected_order,
            "ids must follow ids::ALL, once each"
        );
    }

    /// The whole life of a user descriptor, in one test because the registry
    /// it writes to is process-global: splitting these would let two of them
    /// overwrite each other's folder mid-assertion.
    #[test]
    fn a_descriptor_dropped_in_the_user_folder_becomes_a_platform() {
        use descriptor::test_support::{drop_in, fixture, scratch};

        let root = scratch("registry");
        let ctx = TempCtx { root: root.clone() };

        drop_in(&ctx, "acme.json", &fixture("acme", &root));
        let report = reload_user_platforms(&ctx);

        let loaded_ids = |report: &UserPlatformReport| -> Vec<String> {
            report.loaded.iter().map(|d| d.id.clone()).collect()
        };

        assert_eq!(loaded_ids(&report), vec!["acme".to_string()], "{report:?}");
        assert!(report.rejected.is_empty(), "{report:?}");
        assert!(
            get_service("acme").is_some(),
            "no compilation happened, and the platform answers"
        );
        assert!(all_ids().contains(&"acme".to_string()));

        // A shipped id is refused by name rather than shadowed: a file dropped
        // in a folder must not be able to take over Steam.
        drop_in(&ctx, "steam.json", &fixture("steam", &root));
        let report = reload_user_platforms(&ctx);
        assert!(loaded_ids(&report).contains(&"acme".to_string()));
        assert!(
            report
                .skipped
                .iter()
                .any(|skipped| skipped.id == "steam" && skipped.reason.contains("already ships")),
            "{report:?}"
        );
        assert!(
            !user_registry().read().unwrap().contains_key("steam"),
            "steam must still answer with the service this build shipped"
        );

        // A file that does not validate names its field, and the platforms
        // around it still load.
        drop_in(
            &ctx,
            "broken.json",
            &fixture("broken", &root).replace("\"schemaVersion\": 1", "\"schemaVersion\": 99"),
        );
        let report = reload_user_platforms(&ctx);
        assert!(loaded_ids(&report).contains(&"acme".to_string()));
        assert_eq!(report.rejected.len(), 1, "{report:?}");
        assert_eq!(report.rejected[0].source, "broken.json");
        assert_eq!(report.rejected[0].field, "schemaVersion");

        // The folder is the truth: deleting the file un-registers the platform
        // without restarting anything.
        let dir = descriptor::user_dir(&ctx).unwrap();
        std::fs::remove_file(dir.join("acme.json")).unwrap();
        let report = reload_user_platforms(&ctx);
        assert!(report.loaded.is_empty(), "{report:?}");
        assert!(get_service("acme").is_none());
        assert!(!all_ids().contains(&"acme".to_string()));

        let _ = std::fs::remove_dir_all(&root);
        // Leave the process-global registry as it was found.
        reload_user_platforms(&ctx);
    }

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
        // last_touched_at = 0 means it was set at epoch, always expired with any real TTL
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
        // Message is what the webview toast shows, so it must stay this string.
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
