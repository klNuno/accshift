use super::{schemes_need_registration, window_shadow_is_safe, FIRST_WINDOWS_11_BUILD};

fn schemes(names: &[&str]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

#[test]
fn skips_registration_when_every_scheme_points_here() {
    assert!(!schemes_need_registration(&schemes(&["accshift"]), &[true]));
}

#[test]
fn registers_when_any_scheme_is_missing() {
    assert!(schemes_need_registration(&schemes(&["accshift"]), &[false]));
}

#[test]
fn check_error_fails_open_to_registration() {
    // Callers map is_registered errors to false: a broken read must
    // behave like the old unconditional register, never skip silently.
    assert!(schemes_need_registration(
        &schemes(&["a", "b"]),
        &[true, false]
    ));
}

#[test]
fn empty_config_registers_nothing_either_way() {
    // register_all over zero schemes is a no-op, so skipping is identical.
    assert!(!schemes_need_registration(&[], &[]));
    // Defensive: answers that do not line up with the claims register.
    assert!(schemes_need_registration(&schemes(&["accshift"]), &[]));
}

#[test]
fn windows_10_drops_the_shadow_frame() {
    // 19045 is 22H2, the last Windows 10 release.
    assert!(!window_shadow_is_safe(Some(19045)));
    assert!(!window_shadow_is_safe(Some(FIRST_WINDOWS_11_BUILD - 1)));
}

#[test]
fn windows_11_keeps_it() {
    assert!(window_shadow_is_safe(Some(FIRST_WINDOWS_11_BUILD)));
    assert!(window_shadow_is_safe(Some(26100)));
}

#[test]
fn unreadable_build_keeps_the_old_behavior() {
    // A registry read that fails must not silently change how the window
    // is built on a machine we failed to identify.
    assert!(window_shadow_is_safe(None));
}
