use super::*;

const MINIMAL: &str = r#"{
  "id": "demo",
  "schemaVersion": 1,
  "name": "Demo Launcher",
  "shortName": "Demo",
  "os": {
    "windows": {
      "roots": {
        "files": ["${LOCALAPPDATA}/Demo"],
        "registry": [{ "root": "HKCU", "key": "Software\\Demo" }]
      },
      "detect": { "executableResolves": true },
      "executable": {
        "fileName": "Demo.exe",
        "candidates": [{ "kind": "path", "template": "${ProgramFiles}/Demo/Demo.exe" }]
      },
      "identity": {
        "source": { "kind": "registry", "root": "HKCU", "key": "Software\\Demo", "value": "userId" },
        "format": { "charset": "digits", "maxLength": 32 },
        "current": "identity"
      },
      "state": {
        "files": [{ "live": "${LOCALAPPDATA}/Demo/session.json", "snapshot": "session.json" }]
      },
      "close": { "processes": ["Demo.exe"] },
      "launch": {}
    }
  }
}"#;

fn with_windows(mutate: impl Fn(&mut serde_json::Value)) -> Result<Descriptor, DescriptorError> {
    let mut value: serde_json::Value = serde_json::from_str(MINIMAL).unwrap();
    mutate(&mut value);
    Descriptor::parse("test", &value.to_string())
}

#[test]
fn minimal_descriptor_is_accepted() {
    let descriptor = Descriptor::parse("test", MINIMAL).unwrap();
    assert_eq!(descriptor.id, "demo");
    assert!(descriptor.os.contains_key(&Os::Windows));
}

#[test]
fn wrong_schema_version_names_the_field_and_the_expectation() {
    let err = with_windows(|v| v["schemaVersion"] = serde_json::json!(2)).unwrap_err();
    assert_eq!(err.field, "schemaVersion");
    assert_eq!(err.problem, "expected 1, found 2");
    assert_eq!(
        err.to_string(),
        "Invalid platform descriptor test: field `schemaVersion` expected 1, found 2"
    );
}

#[test]
fn unknown_field_is_refused_rather_than_ignored() {
    // A typo in a key would otherwise quietly drop the step it describes.
    let err = with_windows(|v| v["os"]["windows"]["lunch"] = serde_json::json!({})).unwrap_err();
    assert!(
        err.problem.contains("unknown field `lunch`"),
        "{}",
        err.problem
    );
}

#[test]
fn state_path_outside_the_declared_roots_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["files"][0]["live"] =
            serde_json::json!("${APPDATA}/Elsewhere/session.json");
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.state.files[0].live");
    assert!(err.problem.contains("declared roots"), "{}", err.problem);
}

#[test]
fn parent_segments_are_refused_in_templates() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["files"][0]["live"] =
            serde_json::json!("${LOCALAPPDATA}/Demo/../../Roaming/session.json");
    })
    .unwrap_err();
    assert!(err.problem.contains("`..`"), "{}", err.problem);
}

#[test]
fn registry_value_outside_the_declared_roots_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["registryValues"] = serde_json::json!([{
            "root": "HKLM",
            "key": "SOFTWARE\\Elsewhere",
            "value": "token",
            "snapshot": "token.txt"
        }]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.state.registryValues[0].key");
    assert!(
        err.problem.contains("declared registry roots"),
        "{}",
        err.problem
    );
}

#[test]
fn duplicate_snapshot_names_are_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["directories"] = serde_json::json!([{
            "live": "${LOCALAPPDATA}/Demo/cache",
            "snapshot": "session.json"
        }]);
    })
    .unwrap_err();
    assert!(err.problem.contains("twice"), "{}", err.problem);
}

#[test]
fn detect_with_no_condition_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["detect"] = serde_json::json!({});
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.detect");
}

/// A `nativeHook` identity source naming the one compiled hook, on the
/// demo descriptor's own roots.
fn hook_source() -> serde_json::Value {
    serde_json::json!({
        "kind": "nativeHook",
        "name": "discord-leveldb",
        "paths": { "leveldb": "${LOCALAPPDATA}/Demo/store" },
    })
}

#[test]
fn native_hook_is_refused_outside_the_allowlist() {
    let err = with_windows(|v| {
        v["os"]["windows"]["identity"]["source"] = hook_source();
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.source.name");
    assert!(err.problem.contains("riot, discord"), "{}", err.problem);
}

#[test]
fn native_hook_is_accepted_for_the_two_platforms_entitled_to_one() {
    let descriptor = with_windows(|v| {
        v["id"] = serde_json::json!("discord");
        v["os"]["windows"]["identity"]["source"] = hook_source();
    })
    .unwrap();
    match &descriptor.os[&Os::Windows].identity.source {
        IdentitySource::NativeHook { name, paths } => {
            assert_eq!(name, "discord-leveldb");
            assert!(paths.contains_key("leveldb"));
        }
        other => panic!("expected a native hook source, found {other:?}"),
    }
}

#[test]
fn a_hook_this_build_does_not_have_names_the_field_and_the_known_names() {
    let err = with_windows(|v| {
        v["id"] = serde_json::json!("discord");
        v["os"]["windows"]["identity"]["source"] = serde_json::json!({
            "kind": "nativeHook",
            "name": "scan_leveldb",
            "paths": { "leveldb": "${LOCALAPPDATA}/Demo/store" },
        });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.source.name");
    assert!(err.problem.contains("discord-leveldb"), "{}", err.problem);
}

#[test]
fn a_hook_missing_the_path_it_works_on_is_refused() {
    let err = with_windows(|v| {
        v["id"] = serde_json::json!("discord");
        v["os"]["windows"]["identity"]["source"] = serde_json::json!({
            "kind": "nativeHook",
            "name": "discord-leveldb",
        });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.source.paths");
    assert!(
        err.problem.contains("`leveldb`") && err.problem.contains("none"),
        "{}",
        err.problem
    );
}

#[test]
fn a_hook_path_outside_the_roots_is_refused() {
    let err = with_windows(|v| {
        v["id"] = serde_json::json!("discord");
        v["os"]["windows"]["identity"]["source"] = serde_json::json!({
            "kind": "nativeHook",
            "name": "discord-leveldb",
            "paths": { "leveldb": "${APPDATA}/Elsewhere/store" },
        });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.source.paths.leveldb");
}

#[test]
fn synthetic_identity_cannot_claim_a_live_current_account() {
    let err = with_windows(|v| {
        v["os"]["windows"]["identity"]["source"] = serde_json::json!({ "kind": "synthetic" });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.current");
}

#[test]
fn state_without_a_process_to_close_is_refused() {
    // Replacing session files under a running launcher loses them when it
    // writes its own copy back on exit.
    let err = with_windows(|v| {
        v["os"]["windows"]["close"] = serde_json::json!({ "processes": [] });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.close.processes");
}

#[test]
fn unbalanced_placeholder_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["detect"] = serde_json::json!({ "pathExists": ["${LOCALAPPDATA/Demo"] });
    })
    .unwrap_err();
    assert!(err.problem.contains("closed by"), "{}", err.problem);
}

#[test]
fn placeholders_are_listed_in_order() {
    let template = PathTemplate::new("${LOCALAPPDATA}/Demo/${installDir}/x");
    assert_eq!(template.placeholders(), vec!["LOCALAPPDATA", "installDir"]);
}

#[test]
fn a_live_path_may_list_several_candidates() {
    let descriptor = with_windows(|v| {
        v["os"]["windows"]["state"]["files"][0]["live"] = serde_json::json!([
            "${LOCALAPPDATA}/Demo/Config/Windows/session.json",
            "${LOCALAPPDATA}/Demo/Config/WindowsEditor/session.json"
        ]);
    })
    .unwrap();
    let profile = &descriptor.os[&Os::Windows];
    assert_eq!(profile.state.files[0].live.candidates().len(), 2);
}

#[test]
fn every_candidate_of_a_path_list_is_checked_against_the_roots() {
    // The engine picks whichever exists, so one candidate outside the
    // sandbox is enough to let a switch write where it must not.
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["files"][0]["live"] = serde_json::json!([
            "${LOCALAPPDATA}/Demo/session.json",
            "${APPDATA}/Elsewhere/session.json"
        ]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.state.files[0].live[1]");
    assert!(err.problem.contains("declared roots"), "{}", err.problem);
}

#[test]
fn an_empty_path_list_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["files"][0]["live"] = serde_json::json!([]);
    })
    .unwrap_err();
    assert!(err.problem.contains("empty list"), "{}", err.problem);
}

#[test]
fn a_cache_outside_the_roots_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["caches"] = serde_json::json!(["${APPDATA}/Elsewhere/Cache"]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.state.caches[0]");
    assert!(err.problem.contains("declared roots"), "{}", err.problem);
}

#[test]
fn min_length_above_max_length_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["identity"]["format"]["minLength"] = serde_json::json!(64);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.format.minLength");
    assert_eq!(err.problem, "expected 1 to maxLength (32), found 64");
}

#[test]
fn a_relative_probe_that_climbs_out_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["executable"]["relativeProbes"] =
            serde_json::json!(["../../Windows/System32"]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.executable.relativeProbes[0]");
    assert!(
        err.problem.contains("relative sub-directory"),
        "{}",
        err.problem
    );
}

#[test]
fn any_of_with_no_nested_condition_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["setup"] = serde_json::json!({
            "trigger": [{ "kind": "anyOf", "conditions": [] }]
        });
    })
    .unwrap_err();
    assert!(err.field.ends_with(".conditions"), "{}", err.field);
}

#[test]
fn a_nested_condition_is_validated_like_any_other() {
    let err = with_windows(|v| {
        v["os"]["windows"]["setup"] = serde_json::json!({
            "trigger": [{ "kind": "anyOf", "conditions": [
                { "kind": "pathFresh", "path": "${LOCALAPPDATA}/Demo/session.json", "windowMs": 1000 },
                { "kind": "pathFresh", "path": "${APPDATA}/Elsewhere/session.json", "windowMs": 1000 }
            ]}]
        });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.setup[0].conditions[1].path");
}

#[test]
fn forgetting_cannot_be_made_sticky_without_something_to_hold_back() {
    let err = with_windows(|v| {
        v["os"]["windows"]["identity"]["blocklistOnForget"] = serde_json::json!(true);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.blocklistOnForget");
    assert!(err.problem.contains("discovery"), "{}", err.problem);
}

#[test]
fn a_log_source_with_no_way_to_find_the_id_is_refused() {
    // Without a prefix or a nearby word, any identifier on the line would
    // match, and log lines carry plenty that are not account ids.
    let err = with_windows(|v| {
        v["os"]["windows"]["identity"]["source"] = serde_json::json!({
            "kind": "logTail",
            "path": "${LOCALAPPDATA}/Demo/launcher.log",
            "lineContains": "Login"
        });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.source.prefix");
}

#[test]
fn a_log_source_outside_the_roots_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["identity"]["source"] = serde_json::json!({
            "kind": "logTail",
            "path": "${APPDATA}/Elsewhere/launcher.log",
            "lineContains": "Login",
            "prefix": "User: "
        });
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.identity.source.path");
}

#[test]
fn an_uninstall_entry_needs_the_name_the_launcher_registers_under() {
    let err = with_windows(|v| {
        v["os"]["windows"]["executable"]["candidates"] =
            serde_json::json!([{ "kind": "uninstallEntry", "displayName": "  " }]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.executable.candidates[0].displayName");
}

#[test]
fn charset_guards_the_account_id() {
    assert!(Charset::Digits.accepts("12345"));
    assert!(!Charset::Digits.accepts("12a45"));
    assert!(Charset::Alphanumeric.accepts("a3f0c2d1"));
    assert!(!Charset::Alphanumeric.accepts("a3f0-c2d1"));
    assert!(!Charset::Alphanumeric.accepts("../evil"));
}

#[test]
fn uuid_charset_wants_the_canonical_shape_and_nothing_else() {
    assert!(Charset::Uuid.accepts("a9da419c-1234-5678-9abc-def012345678"));
    assert!(Charset::Uuid.accepts("A9DA419C-1234-5678-9ABC-DEF012345678"));
    // Dashes in the wrong places, a non-hex digit, and the wrong length.
    assert!(!Charset::Uuid.accepts("a9da41-c-1234-5678-9abc-def012345678"));
    assert!(!Charset::Uuid.accepts("g9da419c-1234-5678-9abc-def012345678"));
    assert!(!Charset::Uuid.accepts("a9da419c-1234-5678-9abc-def01234567"));
    assert!(!Charset::Uuid.accepts(""));
}

#[test]
fn os_profiles_may_be_partial_without_breaking_the_descriptor() {
    // A platform present on Windows only is not a broken descriptor.
    let descriptor = Descriptor::parse("test", MINIMAL).unwrap();
    assert!(!descriptor.os.contains_key(&Os::Linux));
    assert_eq!(descriptor.os.len(), 1);
}

#[test]
fn a_root_too_shallow_to_name_the_launcher_is_refused() {
    for root in ["C:/", "C:/Demo", "${USERPROFILE}", "${SystemDrive}/"] {
        let err = with_windows(|v| {
            v["os"]["windows"]["roots"]["files"] = serde_json::json!([root]);
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.roots.files[0]", "{root}");
        assert!(
            err.problem.contains("own folder"),
            "{root}: {}",
            err.problem
        );
    }
}

#[test]
fn an_executable_candidate_off_the_known_anchors_is_refused() {
    for template in ["${ComSpec}", "C:/Windows/System32/cmd.exe"] {
        let err = with_windows(|v| {
            v["os"]["windows"]["executable"]["candidates"][0]["template"] =
                serde_json::json!(template);
        })
        .unwrap_err();
        assert_eq!(
            err.field, "os.windows.executable.candidates[0].template",
            "{template}"
        );
    }
}

#[test]
fn a_live_path_or_cache_equal_to_a_root_is_refused() {
    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["directories"] =
            serde_json::json!([{ "live": "${LOCALAPPDATA}/Demo", "snapshot": "all" }]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.state.directories[0].live");

    let err = with_windows(|v| {
        v["os"]["windows"]["state"]["caches"] = serde_json::json!(["${LOCALAPPDATA}/Demo"]);
    })
    .unwrap_err();
    assert_eq!(err.field, "os.windows.state.caches[0]");

    with_windows(|v| {
        v["os"]["windows"]["state"]["caches"] = serde_json::json!(["${LOCALAPPDATA}/Demo/Cache"]);
    })
    .unwrap();
}
