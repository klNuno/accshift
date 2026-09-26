/// Guards every `track_operation` call site against a typo.
///
/// `code_from` maps an unlisted operation onto `other` rather than
/// dropping it, which is the right behaviour on the wire and the worst one
/// to debug: a misspelt name produces a plausible event that quietly joins
/// the pile of unclassified failures. Reading the call sites out of the
/// source keeps this from becoming a second list that drifts from them.
#[test]
fn tracked_operations_are_in_the_vocabulary() {
    let source = [
        include_str!("mod.rs"),
        include_str!("app.rs"),
        include_str!("backdrop.rs"),
        include_str!("descriptors.rs"),
        include_str!("platform.rs"),
        include_str!("riot.rs"),
        include_str!("roblox.rs"),
        include_str!("steam.rs"),
        include_str!("themes.rs"),
        include_str!("utility.rs"),
        include_str!("window.rs"),
    ]
    .concat();
    // Assembled at runtime so the marker never appears whole in this file:
    // written as one literal, the scan below would match its own source.
    let marker = format!("{}(&app_handle, {}", "track_operation", '"');
    let mut sites = 0;
    for call in source.split(marker.as_str()).skip(1) {
        let name = call.split('"').next().expect("operation name literal");
        assert!(
            crate::telemetry::OPERATIONS.contains(&name),
            "`{name}` is not in crate::telemetry::OPERATIONS and would be reported as `other`"
        );
        sites += 1;
    }
    assert!(sites >= 8, "expected the wired call sites, found {sites}");
}
