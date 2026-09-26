use super::*;

#[test]
fn unwrap_registry_cookie_strips_envelope() {
    assert_eq!(
        unwrap_registry_cookie("COOK::<abc123>"),
        Some("abc123".to_string())
    );
}

#[test]
fn unwrap_registry_cookie_falls_back_to_raw() {
    // Older format / manual edit without the COOK::<> wrapper.
    assert_eq!(
        unwrap_registry_cookie("rawcookievalue"),
        Some("rawcookievalue".to_string())
    );
}

#[test]
fn unwrap_registry_cookie_trims_whitespace() {
    assert_eq!(
        unwrap_registry_cookie("COOK::<  spaced  >"),
        Some("spaced".to_string())
    );
}

#[test]
fn unwrap_registry_cookie_empty_is_none() {
    assert_eq!(unwrap_registry_cookie(""), None);
    assert_eq!(unwrap_registry_cookie("COOK::<>"), None);
    assert_eq!(unwrap_registry_cookie("   "), None);
}

#[test]
fn unwrap_registry_cookie_roundtrips_write_format() {
    // The value write_cookie_to_registry would set must read back identically.
    let cookie = "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will...|_FAKE";
    let written = format!("COOK::<{cookie}>");
    assert_eq!(unwrap_registry_cookie(&written), Some(cookie.to_string()));
}
