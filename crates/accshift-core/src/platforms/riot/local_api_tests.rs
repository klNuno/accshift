//! Login state against a synthetic local API. Bodies are shaped like the
//! client's answers; no value comes from a real client or account.

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

type Route = (&'static str, &'static str, &'static str);

/// Serve `routes` by request path until the test process ends; an unknown
/// path answers 404 like a client without that plugin. Returns the access
/// and the paths requested so far, in order.
fn synthetic_api(routes: Vec<Route>) -> (RiotLocalApiAccess, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte).unwrap_or(0) != 1 {
                    break;
                }
                request.push(byte[0]);
            }
            let line = String::from_utf8_lossy(&request);
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            log.lock().unwrap().push(path.clone());
            let (status, body) = routes
                .iter()
                .find(|(route, _, _)| *route == path)
                .map(|(_, status, body)| (*status, *body))
                .unwrap_or(("404 Not Found", r#"{"errorCode":"RESOURCE_NOT_FOUND"}"#));
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (
        RiotLocalApiAccess {
            protocol: "http".into(),
            port,
            password: "synthetic-only".into(),
        },
        seen,
    )
}

const RSO_SESSION: &str = "/rso-auth/v1/session";
const LEGACY_STATUS: &str = "/riot-login/v1/status";

fn login(state: &RiotLoginState) -> (bool, Option<bool>) {
    (state.logged_in, state.persist)
}

/// #111: a PSL client (Riot Client 137+) answers the RSO session and has no
/// legacy `riot-login` status, so the setup never saw the login.
#[test]
fn psl_client_login_is_read_from_the_rso_session() {
    let (access, _) = synthetic_api(vec![(
        RSO_SESSION,
        "200 OK",
        r#"{"type":"authenticated","persistLogin":true,"authenticationType":"RiotAuth"}"#,
    )]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, Some(true)));
}

/// The legacy route may still answer on a PSL client, with `persist` left
/// null: the RSO session wins.
#[test]
fn rso_session_wins_over_a_null_legacy_persist_flag() {
    let (access, _) = synthetic_api(vec![
        (
            RSO_SESSION,
            "200 OK",
            r#"{"type":"authenticated","persistLogin":true}"#,
        ),
        (
            LEGACY_STATUS,
            "200 OK",
            r#"{"phase":"logged_in","persist":null}"#,
        ),
    ]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, Some(true)));
}

#[test]
fn rso_session_without_stay_signed_in_is_not_persisted() {
    let (access, _) = synthetic_api(vec![(
        RSO_SESSION,
        "200 OK",
        r#"{"type":"authenticated","persistLogin":false}"#,
    )]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, Some(false)));
}

#[test]
fn rso_session_waiting_for_credentials_is_not_logged_in() {
    for body in [
        r#"{"type":"needs_credentials","persistLogin":true}"#,
        r#"{"type":"needs_multifactor_verification","persistLogin":true}"#,
        r#"{"type":"error","persistLogin":true}"#,
    ] {
        let (access, _) = synthetic_api(vec![
            (RSO_SESSION, "200 OK", body),
            (
                LEGACY_STATUS,
                "200 OK",
                r#"{"phase":"logged_in","persist":true}"#,
            ),
        ]);
        assert!(!read_riot_login_state(&access).logged_in, "{body}");
    }
}

/// A booting client answers 404 "RSO is not yet initialized"; an older
/// client has no RSO session route. Both fall back to the legacy status.
#[test]
fn legacy_status_is_the_fallback_when_the_rso_session_is_unavailable() {
    let (access, seen) = synthetic_api(vec![
        (
            RSO_SESSION,
            "404 Not Found",
            r#"{"errorCode":"RPC_ERROR","message":"RSO is not yet initialized"}"#,
        ),
        (
            LEGACY_STATUS,
            "200 OK",
            r#"{"phase":"logged_in","persist":true}"#,
        ),
    ]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, Some(true)));
    assert_eq!(*seen.lock().unwrap(), [RSO_SESSION, LEGACY_STATUS]);

    let (access, _) = synthetic_api(vec![(
        LEGACY_STATUS,
        "200 OK",
        r#"{"phase":"logged_in","persist":false}"#,
    )]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, Some(false)));
}

#[test]
fn no_usable_login_route_fails_closed() {
    let (access, _) = synthetic_api(vec![(RSO_SESSION, "200 OK", "not json")]);
    assert_eq!(login(&read_riot_login_state(&access)), (false, None));
}

/// A Google sign-in with "Stay signed in" ticked answers `persistLogin: false`
/// while the client writes the PSL refresh token (Dev Build `ae609bf`, log of
/// 2026-11-22). The setup clears the live session before it starts, so that
/// token can only come from this login: it proves the login is kept.
#[test]
fn a_psl_token_captures_even_when_the_api_says_persist_login_false() {
    let (access, _) = synthetic_api(vec![(
        RSO_SESSION,
        "200 OK",
        r#"{"type":"authenticated","persistLogin":false}"#,
    )]);
    let psl = RiotSettingsState {
        has_tokens: true,
        psl_login: true,
    };
    assert!(setup_can_capture(&read_riot_login_state(&access), psl));
    let unticked = RiotSettingsState {
        has_tokens: true,
        psl_login: false,
    };
    assert!(!setup_can_capture(
        &read_riot_login_state(&access),
        unticked
    ));
}

/// No bool on either route reads as unknown; a bool on the legacy route
/// fills a signed-in RSO session that has none.
#[test]
fn persist_is_none_only_when_no_route_gives_a_bool() {
    let (access, _) = synthetic_api(vec![
        (RSO_SESSION, "200 OK", r#"{"type":"authenticated"}"#),
        (
            LEGACY_STATUS,
            "200 OK",
            r#"{"phase":"logged_in","persist":null}"#,
        ),
    ]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, None));

    let (access, _) = synthetic_api(vec![
        (RSO_SESSION, "200 OK", r#"{"type":"authenticated"}"#),
        (
            LEGACY_STATUS,
            "200 OK",
            r#"{"phase":"logged_in","persist":true}"#,
        ),
    ]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, Some(true)));

    let (access, _) = synthetic_api(vec![(
        LEGACY_STATUS,
        "200 OK",
        r#"{"phase":"logged_in","persist":null}"#,
    )]);
    assert_eq!(login(&read_riot_login_state(&access)), (true, None));
}

#[test]
fn riot_local_client_bypasses_configured_proxy() {
    // A proxy listener that always rejects requests with 502; if the local
    // Riot client routes 127.0.0.1 through it, the request fails.
    let proxy_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = proxy_listener.local_addr().unwrap().port();
    let proxy_hits = Arc::new(Mutex::new(0usize));
    let hits = Arc::clone(&proxy_hits);
    thread::spawn(move || {
        for stream in proxy_listener.incoming() {
            let Ok(mut stream) = stream else { return };
            *hits.lock().unwrap() += 1;
            let _ = write!(
                stream,
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
        }
    });

    let (access, seen) = synthetic_api(vec![(
        RSO_SESSION,
        "200 OK",
        r#"{"type":"authenticated","persistLogin":true}"#,
    )]);

    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}")).unwrap();
    let client = configure_riot_local_client(crate::http::client_builder().proxy(proxy))
        .build()
        .unwrap();

    let url = format!(
        "{}://127.0.0.1:{}{}",
        access.protocol, access.port, RSO_SESSION
    );
    let response = crate::runtime::block_on(async {
        client
            .get(url)
            .basic_auth("riot", Some(access.password.as_str()))
            .send()
            .await
            .unwrap()
    });
    assert!(
        response.status().is_success(),
        "expected direct 200 OK, got {} (proxy hits: {})",
        response.status(),
        *proxy_hits.lock().unwrap()
    );
    assert_eq!(*proxy_hits.lock().unwrap(), 0);
    assert_eq!(*seen.lock().unwrap(), [RSO_SESSION]);
}
