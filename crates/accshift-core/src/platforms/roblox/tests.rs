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

#[test]
fn a_switch_never_kills_roblox_studio() {
    // Studio can hold unsaved work: only the game client is closed.
    assert_eq!(ROBLOX_SWITCH_KILL_NAMES, ["RobloxPlayerBeta.exe"]);
}

#[test]
fn an_account_without_a_stored_cookie_never_owns_the_live_cookie() {
    // The portable config half drops Roblox cookies, so a config carried to
    // another machine has accounts with a recent date but no cookie. The live
    // cookie still belongs to the account last switched to with one, whose
    // rotation must be kept.
    let account = |id: &str, cookie: &str, last_used_at| RobloxAccountConfig {
        user_id: id.into(),
        username: id.into(),
        display_name: id.into(),
        cookie_encrypted: cookie.into(),
        last_used_at: Some(last_used_at),
    };
    let accounts = [
        account("active", "encrypted", 1_000),
        account("carried", "", 2_000),
    ];
    let owner = live_cookie_owner(&accounts).map(|a| a.user_id.as_str());
    assert_eq!(owner, Some("active"));
    assert!(live_cookie_owner(&accounts[1..]).is_none());
}
