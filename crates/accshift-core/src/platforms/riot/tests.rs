use super::{
    reset_riot_settings_file, riot_settings_device_block, setup_can_capture,
    validate_riot_client_path, yaml_has_auth_tokens, yaml_has_psl_refresh_token, RiotLoginState,
    RiotSettingsState,
};

/// Shaped like a Riot Client 137+ (PSL) settings file. Synthetic values only.
const PSL_SETTINGS: &str = "\
psl:
  authorization:
    riot-client:
      claims: []
      id_token: synthetic-id-token
      is_dpop_bound: false
      max_duration_between_restores: 39169575
      refresh_token: synthetic-refresh-token
      refresh_token_write_count: 3
      scopes: [openid, link, ban, lol_region, lol, account]
riot-login:
  persist: null
rso-authenticator:
  tdid:
    domain: riotgames.com
    expiryTime: 1819599615
    httpOnly: true
    persistent: true
    secureOnly: true
    value: synthetic-device-cookie
";

const DEVICE_BLOCK: &str = "\
rso-authenticator:
  tdid:
    domain: riotgames.com
    expiryTime: 1819599615
    httpOnly: true
    persistent: true
    secureOnly: true
    value: synthetic-device-cookie
";

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

#[test]
fn riot_path_override_must_name_riot_client_services() {
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-path-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let client = root.join("RiotClientServices.exe");
    let other = root.join("LeagueClient.exe");
    std::fs::write(&client, b"stub").unwrap();
    std::fs::write(&other, b"stub").unwrap();

    // The install folder: the switch would take its parent as install dir.
    assert!(validate_riot_client_path(&root).is_err());
    assert!(validate_riot_client_path(&other).is_err());
    assert!(validate_riot_client_path(&root.join("missing.exe")).is_err());
    assert!(validate_riot_client_path(&client).is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

// -- PSL settings file (Riot Client 137+) ------------------------------

#[test]
fn psl_refresh_token_is_a_persisted_login() {
    assert!(yaml_has_auth_tokens(PSL_SETTINGS));
    assert!(yaml_has_psl_refresh_token(PSL_SETTINGS));
    assert_eq!(
        RiotSettingsState::from_contents(PSL_SETTINGS),
        RiotSettingsState {
            has_tokens: true,
            psl_login: true,
        }
    );
}

#[test]
fn only_a_non_empty_refresh_token_under_top_level_psl_counts() {
    assert!(!yaml_has_psl_refresh_token("refresh_token: zzz\n"));
    assert!(!yaml_has_psl_refresh_token(
        "psl: null\nriot-login:\n  persist: null\n"
    ));
    assert!(!yaml_has_psl_refresh_token(
        "psl:\n  authorization:\n    riot-client:\n      refresh_token: ''\n"
    ));
    assert!(!yaml_has_psl_refresh_token(
        "other:\n  psl:\n    refresh_token: not-under-top-level-psl\n"
    ));
    assert!(!yaml_has_psl_refresh_token(
        "psl:\n  authorization: {}\nother:\n  refresh_token: belongs-to-other\n"
    ));
    assert!(yaml_has_psl_refresh_token(
        "# header\npsl:\n  authorization:\n    riot-client:\n      refresh_token: \"tok\" # c\n"
    ));
}

/// #111: on a PSL client the legacy `persist` flag is null even with "Stay
/// signed in" ticked. The refresh token in the settings file proves it.
#[test]
fn setup_capture_accepts_a_psl_login_without_the_legacy_persist_flag() {
    let signed_in_no_flag = RiotLoginState {
        logged_in: true,
        persist: None,
    };
    let psl = RiotSettingsState::from_contents(PSL_SETTINGS);
    assert!(setup_can_capture(&signed_in_no_flag, psl));

    // A device cookie alone is no login, and a signed-out client is not
    // captured whatever the file holds.
    let device_only = RiotSettingsState::from_contents(DEVICE_BLOCK);
    assert!(!setup_can_capture(&signed_in_no_flag, device_only));
    let signed_out = RiotLoginState {
        logged_in: false,
        persist: Some(true),
    };
    assert!(!setup_can_capture(&signed_out, psl));

    // A legacy file without PSL still needs the API flag.
    let legacy = RiotSettingsState::from_contents("private: legacy-blob\n");
    assert!(!setup_can_capture(&signed_in_no_flag, legacy));
    let flagged = RiotLoginState {
        logged_in: true,
        persist: Some(true),
    };
    assert!(setup_can_capture(&flagged, legacy));
}

// -- Setup clear keeps the trusted-device cookie -------------------------

#[test]
fn device_block_is_the_rso_authenticator_section_only() {
    assert_eq!(
        riot_settings_device_block(PSL_SETTINGS).as_deref(),
        Some(DEVICE_BLOCK)
    );
    assert!(!yaml_has_auth_tokens(DEVICE_BLOCK));
    assert_eq!(
        riot_settings_device_block("private: blob\nsessions: {}\n"),
        None
    );
    // CRLF line ends, and a section that is not the last one.
    let crlf = "rso-authenticator:\r\n  tdid:\r\n    value: c\r\npsl:\r\n  authorization: {}\r\n";
    assert_eq!(
        riot_settings_device_block(crlf).as_deref(),
        Some("rso-authenticator:\n  tdid:\n    value: c\n")
    );
}

#[test]
fn setup_reset_keeps_the_device_cookie_and_drops_the_login() {
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-reset-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("RiotGamesPrivateSettings.yaml");

    std::fs::write(&file, PSL_SETTINGS).unwrap();
    reset_riot_settings_file(&file).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), DEVICE_BLOCK);

    // Nothing to keep: the file goes, as before.
    std::fs::write(&file, "private: blob\nsessions: {}\n").unwrap();
    reset_riot_settings_file(&file).unwrap();
    assert!(!file.exists());

    // Already gone is fine.
    reset_riot_settings_file(&file).unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn riot_close_list_handles_crash_handlers_without_counting_them_as_survivors() {
    let close = super::close_process_names();
    for client in super::RIOT_CLIENT_PROCESS_NAMES {
        assert!(
            close.contains(client),
            "close list must include client process {client}"
        );
    }
    assert!(close.contains(&"RiotClientCrashHandler.exe"));
    assert!(close.contains(&"LeagueCrashHandler64.exe"));
    // Crash handlers can linger under Vanguard without holding session tokens:
    // they are killed best-effort but must not trigger the survivor check.
    assert!(!super::RIOT_CLIENT_PROCESS_NAMES.contains(&"RiotClientCrashHandler.exe"));
    assert!(!super::RIOT_CLIENT_PROCESS_NAMES.contains(&"LeagueCrashHandler64.exe"));
}
