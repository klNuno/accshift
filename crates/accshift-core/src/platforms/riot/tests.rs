use super::yaml_has_auth_tokens;

#[test]
fn empty_private_and_sessions_are_not_ready() {
    // A freshly reset settings file: only a tdid cookie, empty auth blocks.
    let yaml = "\
private: ''
sessions: {}
tdid: some-tracking-cookie-value
";
    assert!(!yaml_has_auth_tokens(yaml));
}

#[test]
fn null_and_tilde_private_are_not_ready() {
    assert!(!yaml_has_auth_tokens("private: null\nsessions: {}\n"));
    assert!(!yaml_has_auth_tokens("private: ~\nsessions: []\n"));
    assert!(!yaml_has_auth_tokens("private:\nsessions: {}\n"));
}

#[test]
fn non_empty_private_blob_is_ready() {
    // Riot writes the persistent credentials as an inline base64 blob.
    let yaml = "private: eyJhbGciOiJ...base64-blob...XVCJ9\nsessions: {}\n";
    assert!(yaml_has_auth_tokens(yaml));
}

#[test]
fn populated_sessions_map_is_ready() {
    let yaml = "\
private: ''
sessions:
  some-session-id:
type: account
";
    assert!(yaml_has_auth_tokens(yaml));
}

#[test]
fn bare_sessions_key_requires_a_real_indented_child() {
    assert!(!yaml_has_auth_tokens("sessions:\n"));
    assert!(!yaml_has_auth_tokens(
        "sessions: # no entries yet\n  # comment only\n\nother: value\n"
    ));
    assert!(!yaml_has_auth_tokens(
        "sessions:\n# same-level comment\nnext_setting: true\n"
    ));
}

#[test]
fn token_entries_are_ready() {
    assert!(yaml_has_auth_tokens("data:\n  access_token: abc.def.ghi\n"));
    assert!(yaml_has_auth_tokens("refresh_token: zzz\n"));
    assert!(yaml_has_auth_tokens("id_token: yyy\n"));
    assert!(yaml_has_auth_tokens(
        "access_token: abc.def.ghi # refreshed token\n"
    ));
    assert!(yaml_has_auth_tokens(
        "refresh_token: \"abc # part-of-token\"\n"
    ));
}

#[test]
fn token_entries_require_exact_keys_and_non_empty_values() {
    let yaml = "\
# access_token: only-a-comment
access_token_backup: not-the-key
my_refresh_token: not-the-key
id_token_suffix: not-the-key
access_token:
refresh_token: ''
id_token: null
";
    assert!(!yaml_has_auth_tokens(yaml));
    assert!(!yaml_has_auth_tokens("access_token: # comment only\n"));
    assert!(!yaml_has_auth_tokens("refresh_token: \"\"\nid_token: ~\n"));
}

#[test]
fn keys_that_merely_start_with_private_do_not_match() {
    // `privateKey` is a different key and must not be read as `private`.
    assert!(!yaml_has_auth_tokens("privateKey: should-not-count\n"));
    assert!(!yaml_has_auth_tokens("sessionsCount: 3\n"));
}

#[test]
fn indented_keys_still_match() {
    // The real file nests these under a top-level key.
    let yaml = "\
riot-login:
  private: real-blob-here
  sessions: {}
";
    assert!(yaml_has_auth_tokens(yaml));
}

#[test]
fn cookie_format_with_ssid_is_ready() {
    // Newer Riot Client format: persistent login stored as cookies, the
    // `ssid` cookie being the auth session token.
    let yaml = "\
riot-login:
persist:
    region: \"EUW\"
    session:
        cookies:
        -   domain: \"auth.riotgames.com\"
            name: \"asid\"
            persistent: false
        -   domain: \"auth.riotgames.com\"
            name: \"ssid\"
            persistent: true
            value: \"opaque-session-token\"
";
    assert!(yaml_has_auth_tokens(yaml));
}

#[test]
fn cookie_format_ssid_as_first_mapping_key_is_ready() {
    // `name` can be the first key of the cookie entry, carrying the dash.
    let yaml = "cookies:\n- name: \"ssid\"\n  value: \"tok\"\n";
    assert!(yaml_has_auth_tokens(yaml));
}

#[test]
fn ssid_cookie_requires_a_non_empty_value_in_the_same_entry() {
    assert!(!yaml_has_auth_tokens("cookies:\n- name: \"ssid\"\n"));
    assert!(!yaml_has_auth_tokens(
        "cookies:\n- name: \"ssid\"\n  value: \"\"\n"
    ));
    assert!(!yaml_has_auth_tokens(
        "cookies:\n- name: \"ssid\"\n  value: null\n"
    ));
    assert!(!yaml_has_auth_tokens(
        "cookies:\n- name: \"ssid\"\n- name: \"tdid\"\n  value: token\n"
    ));
    assert!(!yaml_has_auth_tokens(
        "cookies:\n- name: \"ssid\"\n  value: # comment only\n"
    ));
}

#[test]
fn ssid_cookie_accepts_value_before_name() {
    let yaml = "cookies:\n- value: opaque-session-token\n  persistent: true\n  name: \"ssid\"\n";
    assert!(yaml_has_auth_tokens(yaml));
    assert!(yaml_has_auth_tokens(
        "cookies:\n- name: \"ssid\" # auth cookie\n  value: token # current value\n"
    ));
}

#[test]
fn cookie_format_with_only_tracking_cookies_is_not_ready() {
    // Logged-out file: tracking cookies only, no ssid auth cookie.
    let yaml = "\
riot-login:
persist:
    session:
        cookies:
        -   domain: \"auth.riotgames.com\"
            name: \"tdid\"
            persistent: true
        -   domain: \"auth.riotgames.com\"
            name: \"clid\"
            persistent: true
";
    assert!(!yaml_has_auth_tokens(yaml));
}

#[test]
fn non_ssid_name_keys_do_not_match() {
    assert!(!yaml_has_auth_tokens("name: \"ssidfoo\"\n"));
    assert!(!yaml_has_auth_tokens("nickname: \"ssid\"\n"));
}

#[test]
fn small_token_file_is_ready_despite_being_under_old_size_threshold() {
    // The old heuristic required >1000 bytes; a small file with a real token
    // would have been wrongly rejected. The structural check accepts it.
    let yaml = "private: tok\n";
    assert!(yaml.len() < 1000);
    assert!(yaml_has_auth_tokens(yaml));
}
