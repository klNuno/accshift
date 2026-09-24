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
