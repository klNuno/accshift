//! The updater against a local HTTP server on 127.0.0.1: a `latest.json`
//! and an artifact signed by a key generated here, as `tauri signer` does.
//! The install side effects go to a recording host; no installer runs.

use accshift_app::lifecycle::update_flow::{run_command, ApplyHost, CommandResult, UpdateCommand};
use accshift_app::lifecycle::updater::{
    bundle_type, bundle_type_from_marker, escape_msi_property_arg, escape_nsis_current_exe_arg,
    fallback_targets, installer_for_bundle_type, parse_manifest, updater_arch, updater_os,
    verify_signature, BundleType, DownloadEvent, Installer, InstallerHost, RemoteReleaseInner,
    Update, Updater, UpdaterConfig, UpdaterError, WindowsUpdateInstallMode, WindowsUpdaterType,
    PUBKEY, UPDATER_USER_AGENT,
};
use base64::Engine;
use semver::Version;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use url::Url;

// ---------------------------------------------------------------------------
// Local HTTP server

#[derive(Debug, Clone)]
struct Request {
    path: String,
    headers: HashMap<String, String>,
}

type Route = dyn Fn(&str) -> (u16, Vec<u8>) + Send + Sync;

/// One-request-per-connection HTTP/1.1 server; stops on drop.
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(route: impl Fn(&str) -> (u16, Vec<u8>) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let route: Arc<Route> = Arc::new(route);
        let thread = {
            let requests = requests.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        serve(stream, &route, &requests);
                    }
                }
            })
        };
        Self {
            base,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(mut stream: TcpStream, route: &Arc<Route>, requests: &Mutex<Vec<Request>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => raw.extend_from_slice(&buffer[..read]),
        }
    }
    let text = String::from_utf8_lossy(&raw).to_string();
    let mut lines = text.split("\r\n");
    let path = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default()
        .to_string();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_lowercase(), value.trim().to_string()))
        .collect();
    requests.lock().unwrap().push(Request {
        path: path.clone(),
        headers,
    });
    let (status, body) = route(&path);
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let head = if status == 204 {
        format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
    };
    let _ = stream.write_all(head.as_bytes());
    if status != 204 {
        let _ = stream.write_all(&body);
    }
    let _ = stream.flush();
}

// ---------------------------------------------------------------------------
// Signing, as `tauri signer sign`: base64 of the key box and of the `.sig`.

struct TestKey {
    pair: minisign::KeyPair,
}

impl TestKey {
    fn new() -> Self {
        Self {
            pair: minisign::KeyPair::generate_unencrypted_keypair().expect("keypair"),
        }
    }

    fn pubkey(&self) -> String {
        let text = self.pair.pk.to_box().unwrap().to_string();
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    fn sign(&self, data: &[u8]) -> String {
        let signature = minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            std::io::Cursor::new(data),
            Some("timestamp:0\tfile:accshift-m5test.exe"),
            Some("signature from tauri secret key"),
        )
        .expect("sign");
        base64::engine::general_purpose::STANDARD.encode(signature.to_string())
    }
}

/// An NSIS setup as `infer` sees it: `MZ` and anything.
fn fake_setup() -> Vec<u8> {
    let mut bytes = b"MZ".to_vec();
    bytes.extend((0..200_000u32).map(|i| (i % 251) as u8));
    bytes
}

fn os_arch() -> (String, String) {
    (
        updater_os().unwrap().to_string(),
        updater_arch().unwrap().to_string(),
    )
}

fn config(
    server: &Server,
    pubkey: &str,
    current: &str,
    bundle: Option<BundleType>,
) -> UpdaterConfig {
    UpdaterConfig {
        pubkey: pubkey.to_string(),
        endpoints: vec![Url::parse(&server.url("/latest.json")).unwrap()],
        install_mode: WindowsUpdateInstallMode::Passive,
        installer_args: Vec::new(),
        current_version: Version::parse(current).unwrap(),
        app_name: "accshift-m5test".into(),
        target: None,
        timeout: Some(Duration::from_secs(20)),
        headers: Vec::new(),
        current_exe_args: vec!["accshift.exe".into(), "--flag".into(), "some space".into()],
        restart_after_install: true,
        bundle_type: bundle,
        executable_path: None,
        allow_downgrades: false,
    }
}

/// Serves `manifest` at `/latest.json` and `artifact` at `/artifact`.
fn release_server(
    manifest: impl Fn(&str) -> Value + Send + Sync + 'static,
    artifact: Vec<u8>,
) -> Server {
    Server::start(move |path| {
        if path.starts_with("/latest.json") {
            (200, manifest(path).to_string().into_bytes())
        } else if path.starts_with("/artifact") {
            (200, artifact.clone())
        } else {
            (404, Vec::new())
        }
    })
}

fn static_manifest(version: &str, base: &str, platforms: &[(&str, &str)]) -> Value {
    let platforms: serde_json::Map<String, Value> = platforms
        .iter()
        .map(|(key, signature)| {
            (
                key.to_string(),
                json!({"url": format!("{base}/artifact/{key}"), "signature": signature}),
            )
        })
        .collect();
    json!({
        "version": version,
        "notes": "Bug fixes",
        "pub_date": "2026-10-01T12:00:00Z",
        "platforms": platforms,
    })
}

/// A server whose base URL is known before the manifest is built.
fn server_with(build: impl Fn(&str) -> Value + Send + Sync + 'static, artifact: Vec<u8>) -> Server {
    let base = Arc::new(Mutex::new(String::new()));
    let server = {
        let base = base.clone();
        release_server(move |_| build(&base.lock().unwrap()), artifact)
    };
    *base.lock().unwrap() = server.base.clone();
    server
}

fn check(config: UpdaterConfig) -> Result<Option<Update>, UpdaterError> {
    Updater::new(config).expect("updater builds").check()
}

// ---------------------------------------------------------------------------
// Check and download

#[test]
fn a_good_signature_is_accepted_and_progress_is_reported() {
    let key = TestKey::new();
    let artifact = fake_setup();
    let signature = key.sign(&artifact);
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let server = server_with(
        move |base| static_manifest("v1.2.0", base, &[(platform.as_str(), signature.as_str())]),
        artifact.clone(),
    );

    let update = check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .expect("1.2.0 is newer than 1.0.5");
    assert_eq!(update.version, "1.2.0");
    assert_eq!(update.current_version, "1.0.5");
    assert_eq!(update.body.as_deref(), Some("Bug fixes"));
    assert_eq!(
        update.date_rfc3339().as_deref(),
        Some("2026-10-01T12:00:00Z")
    );
    assert_eq!(update.target, os);
    assert_eq!(update.raw_json["version"], "v1.2.0");

    let mut events = Vec::new();
    let bytes = update
        .download_with_events(|event| events.push(event))
        .unwrap();
    assert_eq!(bytes, artifact);
    assert_eq!(
        events.first(),
        Some(&DownloadEvent::Started {
            content_length: Some(artifact.len() as u64)
        })
    );
    assert_eq!(events.last(), Some(&DownloadEvent::Finished));
    let received: usize = events
        .iter()
        .map(|event| match event {
            DownloadEvent::Progress { chunk_length } => *chunk_length,
            _ => 0,
        })
        .sum();
    assert_eq!(received, artifact.len());

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].headers["user-agent"], UPDATER_USER_AGENT);
    assert_eq!(requests[0].headers["accept"], "application/json");
    assert_eq!(requests[1].headers["accept"], "application/octet-stream");
    assert_eq!(requests[1].path, format!("/artifact/{os}-{arch}"));
}

#[test]
fn a_bad_signature_is_refused() {
    let key = TestKey::new();
    let artifact = fake_setup();
    let signature = key.sign(b"some other file");
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let server = server_with(
        move |base| static_manifest("1.2.0", base, &[(platform.as_str(), signature.as_str())]),
        artifact,
    );
    let update = check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .unwrap();
    let error = update.download(|_, _| {}, || {}).unwrap_err();
    assert!(matches!(error, UpdaterError::Minisign(_)), "{error:?}");
}

#[test]
fn a_signature_from_another_key_is_refused() {
    let signer = TestKey::new();
    let trusted = TestKey::new();
    let artifact = fake_setup();
    let signature = signer.sign(&artifact);
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let server = server_with(
        move |base| static_manifest("1.2.0", base, &[(platform.as_str(), signature.as_str())]),
        artifact,
    );
    let update = check(config(&server, &trusted.pubkey(), "1.0.5", None))
        .unwrap()
        .unwrap();
    let error = update.download(|_, _| {}, || {}).unwrap_err();
    assert!(matches!(error, UpdaterError::Minisign(_)), "{error:?}");
}

#[test]
fn a_missing_or_undecodable_signature_is_refused() {
    let key = TestKey::new();
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");

    // Static shape without `signature`: the manifest does not parse.
    let server = server_with(
        {
            let platform = platform.clone();
            move |base| {
                json!({
                    "version": "1.2.0",
                    "platforms": { platform.as_str(): { "url": format!("{base}/artifact") } }
                })
            }
        },
        fake_setup(),
    );
    let error = check(config(&server, &key.pubkey(), "1.0.5", None)).unwrap_err();
    assert!(
        matches!(&error, UpdaterError::Serialization(text) if text.contains("missing field `signature`")),
        "{error:?}"
    );

    // Dynamic shape without `signature`.
    let server = server_with(
        |base| json!({"version": "1.2.0", "url": format!("{base}/artifact")}),
        fake_setup(),
    );
    let error = check(config(&server, &key.pubkey(), "1.0.5", None)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "the `signature` field was not set on the updater response"
    );

    // An empty signature string: refused at download.
    let server = server_with(
        move |base| static_manifest("1.2.0", base, &[(platform.as_str(), "")]),
        fake_setup(),
    );
    let update = check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .unwrap();
    let error = update.download(|_, _| {}, || {}).unwrap_err();
    assert!(matches!(error, UpdaterError::Minisign(_)), "{error:?}");

    // Not base64 at all.
    let error = verify_signature(b"data", "%%%", &key.pubkey()).unwrap_err();
    assert!(matches!(error, UpdaterError::Base64(_)), "{error:?}");
}

#[test]
fn an_older_or_equal_version_is_ignored() {
    let key = TestKey::new();
    let artifact = fake_setup();
    let signature = key.sign(&artifact);
    let (os, arch) = os_arch();
    for (remote, current) in [("1.0.5", "1.0.5"), ("1.0.4", "1.0.5"), ("0.9.0", "1.0.5")] {
        let platform = format!("{os}-{arch}");
        let signature = signature.clone();
        let server = server_with(
            move |base| static_manifest(remote, base, &[(platform.as_str(), signature.as_str())]),
            artifact.clone(),
        );
        assert!(
            check(config(&server, &key.pubkey(), current, None))
                .unwrap()
                .is_none(),
            "{remote} against {current}"
        );
        // `allowDowngrades`: anything different counts.
        let mut downgrade = config(&server, &key.pubkey(), current, None);
        downgrade.allow_downgrades = true;
        assert_eq!(check(downgrade).unwrap().is_some(), remote != current);
    }
}

#[test]
fn a_prerelease_and_a_build_suffix_compare_as_semver() {
    let key = TestKey::new();
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let signature = key.sign(b"x");
    let server = server_with(
        move |base| {
            static_manifest(
                "1.1.0-beta.1",
                base,
                &[(platform.as_str(), signature.as_str())],
            )
        },
        Vec::new(),
    );
    assert!(check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .is_some());
    assert!(check(config(&server, &key.pubkey(), "1.1.0", None))
        .unwrap()
        .is_none());
}

#[test]
fn the_installer_key_wins_over_the_plain_platform_key() {
    let key = TestKey::new();
    let (os, arch) = os_arch();
    let with_installer = format!("{os}-{arch}-nsis");
    let plain = format!("{os}-{arch}");
    let server = server_with(
        {
            let (with_installer, plain) = (with_installer.clone(), plain.clone());
            move |base| {
                static_manifest(
                    "1.2.0",
                    base,
                    &[(with_installer.as_str(), "c2ln"), (plain.as_str(), "c2ln")],
                )
            }
        },
        Vec::new(),
    );
    let update = check(config(
        &server,
        &key.pubkey(),
        "1.0.5",
        Some(BundleType::Nsis),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(
        update.download_url.path(),
        format!("/artifact/{with_installer}")
    );

    // An unpatched binary (no bundle type) asks for the plain key.
    let update = check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .unwrap();
    assert_eq!(update.download_url.path(), format!("/artifact/{plain}"));

    // An MSI build falls back to the plain key when its own is missing.
    let update = check(config(
        &server,
        &key.pubkey(),
        "1.0.5",
        Some(BundleType::Msi),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(update.download_url.path(), format!("/artifact/{plain}"));
}

#[test]
fn every_platform_key_of_the_release_matrix_resolves() {
    // The keys `tauri-action` writes into accshift's latest.json.
    let keys = [
        "windows-x86_64",
        "windows-x86_64-nsis",
        "windows-x86_64-msi",
        "windows-aarch64-nsis",
        "darwin-aarch64",
        "darwin-aarch64-app",
        "darwin-x86_64",
        "darwin-x86_64-app",
        "linux-x86_64",
        "linux-x86_64-appimage",
        "linux-x86_64-deb",
        "linux-x86_64-rpm",
    ];
    let manifest = static_manifest(
        "1.2.0",
        "https://example.invalid",
        &keys.iter().map(|key| (*key, "c2ln")).collect::<Vec<_>>(),
    );
    let release = parse_manifest(manifest).unwrap();
    let mut sorted = keys.map(String::from).to_vec();
    sorted.sort();
    assert_eq!(release.platform_keys(), sorted);
    for (os, arch, installer, expected) in [
        (
            "windows",
            "x86_64",
            Some(Installer::Nsis),
            "windows-x86_64-nsis",
        ),
        (
            "windows",
            "x86_64",
            Some(Installer::Msi),
            "windows-x86_64-msi",
        ),
        ("windows", "x86_64", None, "windows-x86_64"),
        (
            "windows",
            "aarch64",
            Some(Installer::Nsis),
            "windows-aarch64-nsis",
        ),
        (
            "darwin",
            "aarch64",
            Some(Installer::App),
            "darwin-aarch64-app",
        ),
        ("darwin", "x86_64", None, "darwin-x86_64"),
        (
            "linux",
            "x86_64",
            Some(Installer::AppImage),
            "linux-x86_64-appimage",
        ),
        ("linux", "x86_64", Some(Installer::Deb), "linux-x86_64-deb"),
        ("linux", "x86_64", Some(Installer::Rpm), "linux-x86_64-rpm"),
    ] {
        let targets = fallback_targets(os, arch, installer);
        let found = targets
            .iter()
            .find(|target| release.download_url(target).is_ok())
            .unwrap();
        assert_eq!(found, expected);
        assert_eq!(
            release.download_url(found).unwrap().as_str(),
            format!("https://example.invalid/artifact/{expected}")
        );
    }
    assert_eq!(
        fallback_targets("windows", "x86_64", Some(Installer::Nsis)),
        ["windows-x86_64-nsis", "windows-x86_64"]
    );
    assert_eq!(
        fallback_targets("linux", "riscv64", None),
        ["linux-riscv64"]
    );
}

#[test]
fn a_manifest_without_this_platform_fails_even_when_nothing_is_newer() {
    let key = TestKey::new();
    let server = server_with(
        |base| static_manifest("1.0.0", base, &[("plan9-mips", "c2ln")]),
        Vec::new(),
    );
    let (os, arch) = os_arch();
    let error = check(config(
        &server,
        &key.pubkey(),
        "1.0.5",
        Some(BundleType::Nsis),
    ))
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "None of the fallback platforms `[\"{os}-{arch}-nsis\", \"{os}-{arch}\"]` were found in the response `platforms` object"
        )
    );

    let mut custom = config(&server, &key.pubkey(), "1.0.5", None);
    custom.target = Some("custom-target".into());
    let error = check(custom).unwrap_err();
    assert_eq!(
        error.to_string(),
        "the platform `custom-target` was not found in the response `platforms` object"
    );
}

#[test]
fn the_dynamic_shape_and_the_name_alias_parse() {
    let release = parse_manifest(json!({
        "name": "v2.0.0",
        "url": "https://example.invalid/setup.exe",
        "signature": "c2ln",
    }))
    .unwrap();
    assert_eq!(release.version, Version::new(2, 0, 0));
    assert!(matches!(release.data, RemoteReleaseInner::Dynamic(_)));
    assert_eq!(
        release.download_url("anything").unwrap().as_str(),
        "https://example.invalid/setup.exe"
    );
    assert!(release.platform_keys().is_empty());

    let error = parse_manifest(json!({"version": "1.0.0", "signature": "c2ln"})).unwrap_err();
    assert_eq!(
        error.to_string(),
        "the `url` field was not set on the updater response"
    );
    let error = parse_manifest(json!({"version": "1.0.0", "pub_date": "yesterday", "url": "https://a.invalid", "signature": "c2ln"})).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("invalid value for `pub_date`: "),
        "{error}"
    );
    let error =
        parse_manifest(json!({"version": "one", "url": "https://a.invalid", "signature": "c2ln"}))
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unexpected character 'o' while parsing major version number"),
        "{error}"
    );
    let error = parse_manifest(json!({"url": "https://a.invalid"})).unwrap_err();
    assert!(
        error.to_string().contains("missing field `version`"),
        "{error}"
    );
}

#[test]
fn a_204_means_no_update() {
    let server = Server::start(|_| (204, Vec::new()));
    let key = TestKey::new();
    assert!(check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .is_none());
}

#[test]
fn a_failing_endpoint_falls_through_to_the_next_one() {
    let key = TestKey::new();
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let good = server_with(
        move |base| static_manifest("1.2.0", base, &[(platform.as_str(), "c2ln")]),
        Vec::new(),
    );
    let broken = Server::start(|_| (500, b"oops".to_vec()));
    let mut both = config(&good, &key.pubkey(), "1.0.5", None);
    both.endpoints = vec![
        Url::parse(&broken.url("/latest.json")).unwrap(),
        Url::parse(&good.url("/latest.json")).unwrap(),
    ];
    assert!(check(both).unwrap().is_some());
    assert_eq!(broken.requests().len(), 1);

    // Only failing statuses: no last error is kept, so the release is "not found".
    let missing = Server::start(|_| (404, Vec::new()));
    let error = check(config(&missing, &key.pubkey(), "1.0.5", None)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Could not fetch a valid release JSON from the remote"
    );
}

#[test]
fn an_unreachable_endpoint_is_a_request_error() {
    // A port nothing listens on: bind, read the port, close.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let server = Server::start(|_| (404, Vec::new()));
    let key = TestKey::new();
    let mut unreachable = config(&server, &key.pubkey(), "1.0.5", None);
    unreachable.endpoints =
        vec![Url::parse(&format!("http://127.0.0.1:{port}/latest.json")).unwrap()];
    let error = check(unreachable).unwrap_err();
    assert!(
        matches!(&error, UpdaterError::Reqwest(text) if text.contains("error sending request")),
        "{error:?}"
    );
}

#[test]
fn a_failed_download_reports_its_status() {
    let key = TestKey::new();
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let base = Arc::new(Mutex::new(String::new()));
    let server = {
        let base = base.clone();
        Server::start(move |path| {
            if path.starts_with("/latest.json") {
                let manifest = static_manifest(
                    "1.2.0",
                    &base.lock().unwrap(),
                    &[(platform.as_str(), "c2ln")],
                );
                (200, manifest.to_string().into_bytes())
            } else {
                (500, Vec::new())
            }
        })
    };
    *base.lock().unwrap() = server.base.clone();
    let update = check(config(&server, &key.pubkey(), "1.0.5", None))
        .unwrap()
        .unwrap();
    let error = update.download(|_, _| {}, || {}).unwrap_err();
    assert_eq!(
        error.to_string(),
        "`Download request failed with status: 500 Internal Server Error`"
    );
}

#[test]
fn the_endpoint_variables_are_replaced_raw_and_encoded() {
    let server = Server::start(|_| (204, Vec::new()));
    let key = TestKey::new();
    let mut templated = config(
        &server,
        &key.pubkey(),
        "1.0.5+build.7",
        Some(BundleType::Nsis),
    );
    templated.endpoints = vec![Url::parse(
        &server.url("/{{target}}/{{arch}}/{{bundle_type}}/latest.json?v={{current_version}}"),
    )
    .unwrap()];
    // The path braces are percent-encoded by the URL parser, the query ones are not.
    assert!(templated.endpoints[0]
        .as_str()
        .contains("%7B%7Btarget%7D%7D"));
    assert!(check(templated).unwrap().is_none());
    let (os, arch) = os_arch();
    assert_eq!(
        server.requests()[0].path,
        format!("/{os}/{arch}/nsis/latest.json?v=1.0.5%2Bbuild.7")
    );

    let updater = Updater::new(config(&server, &key.pubkey(), "1.0.5", None)).unwrap();
    let url = updater
        .endpoint_url(
            &Url::parse("https://example.invalid/{{bundle_type}}/x").unwrap(),
            "windows",
        )
        .unwrap();
    assert_eq!(url.as_str(), "https://example.invalid/unknown/x");
}

#[test]
fn the_config_is_validated_and_matches_tauri_conf() {
    let accshift = UpdaterConfig::accshift();
    assert_eq!(
        accshift.endpoints[0].as_str(),
        "https://github.com/klNuno/accshift/releases/latest/download/latest.json"
    );
    assert_eq!(accshift.pubkey, PUBKEY);
    assert_eq!(accshift.install_mode, WindowsUpdateInstallMode::Passive);
    // The shipped key decodes: a wrong signature fails on the signature, not the key.
    let key = TestKey::new();
    let error = verify_signature(b"data", &key.sign(b"data"), PUBKEY).unwrap_err();
    assert!(matches!(error, UpdaterError::Minisign(_)), "{error:?}");

    let mut empty = accshift.clone();
    empty.endpoints.clear();
    assert!(matches!(
        Updater::new(empty).err(),
        Some(UpdaterError::EmptyEndpoints)
    ));
}

#[test]
fn the_bundle_marker_maps_to_the_installer() {
    for (marker, bundle, installer) in [
        (
            "__TAURI_BUNDLE_TYPE_VAR_NSS",
            BundleType::Nsis,
            Installer::Nsis,
        ),
        (
            "__TAURI_BUNDLE_TYPE_VAR_MSI",
            BundleType::Msi,
            Installer::Msi,
        ),
        (
            "__TAURI_BUNDLE_TYPE_VAR_DEB",
            BundleType::Deb,
            Installer::Deb,
        ),
        (
            "__TAURI_BUNDLE_TYPE_VAR_RPM",
            BundleType::Rpm,
            Installer::Rpm,
        ),
        (
            "__TAURI_BUNDLE_TYPE_VAR_APP",
            BundleType::AppImage,
            Installer::AppImage,
        ),
    ] {
        assert_eq!(bundle_type_from_marker(marker), Some(bundle));
        assert_eq!(installer_for_bundle_type(Some(bundle)), Some(installer));
    }
    let unpatched = bundle_type_from_marker("__TAURI_BUNDLE_TYPE_VAR_UNK");
    assert_eq!(
        unpatched,
        cfg!(target_os = "macos").then_some(BundleType::App)
    );
    // A test binary is never patched by the bundler.
    assert_eq!(bundle_type(), unpatched);
}

// ---------------------------------------------------------------------------
// Install

#[derive(Default)]
struct RecordingHost {
    calls: Mutex<Vec<String>>,
    launched: Mutex<Option<(OsString, OsString, bool)>>,
}

impl InstallerHost for RecordingHost {
    fn before_exit(&self) {
        self.calls.lock().unwrap().push("before_exit".into());
    }

    fn shell_execute(&self, file: &OsStr, parameters: &OsStr) -> std::io::Result<()> {
        self.calls.lock().unwrap().push("shell_execute".into());
        let exists = std::path::Path::new(file).exists();
        *self.launched.lock().unwrap() = Some((file.to_owned(), parameters.to_owned(), exists));
        Ok(())
    }

    fn exit(&self, code: i32) {
        self.calls.lock().unwrap().push(format!("exit({code})"));
    }
}

fn offline_update(version: &str, args: &[&str], mode: WindowsUpdateInstallMode) -> Update {
    let server = Server::start(|_| (404, Vec::new()));
    let key = TestKey::new();
    let mut config = config(&server, &key.pubkey(), "1.0.5", Some(BundleType::Nsis));
    config.install_mode = mode;
    config.current_exe_args = args.iter().map(OsString::from).collect();
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let release = parse_manifest(static_manifest(
        version,
        "https://example.invalid",
        &[(platform.as_str(), "c2ln")],
    ))
    .unwrap();
    let updater = Updater::new(config.clone()).unwrap();
    let (url, signature) = updater.get_urls(&release, None).unwrap();
    Update {
        body: None,
        current_version: "1.0.5".into(),
        version: version.into(),
        date: None,
        target: os,
        download_url: url.clone(),
        signature: signature.clone(),
        raw_json: Value::Null,
        timeout: None,
        headers: Vec::new(),
        install: accshift_app::lifecycle::updater::InstallContext {
            app_name: config.app_name,
            install_mode: config.install_mode,
            installer_args: config.installer_args,
            current_exe_args: config.current_exe_args,
            restart_after_install: config.restart_after_install,
            bundle_type: config.bundle_type,
            executable_path: None,
            pubkey: config.pubkey,
        },
    }
}

#[cfg(windows)]
#[test]
fn the_windows_install_launches_the_nsis_setup_then_exits() {
    let update = offline_update(
        "1.2.0",
        &["accshift.exe", "--flag", "some space", "/start"],
        WindowsUpdateInstallMode::Passive,
    );
    let host = RecordingHost::default();
    update.install(&fake_setup(), &host).unwrap();
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["before_exit", "shell_execute", "exit(0)"]
    );
    let (file, parameters, existed) = host.launched.lock().unwrap().take().unwrap();
    let file = std::path::PathBuf::from(file);
    assert!(existed, "the setup is on disk when it is launched");
    assert_eq!(
        file.file_name().unwrap(),
        "accshift-m5test-1.2.0-installer.exe"
    );
    let folder = file.parent().unwrap().to_path_buf();
    assert!(folder
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("accshift-m5test-1.2.0-updater-"));
    assert_eq!(
        parameters,
        "/P /UPDATE /R /ARGS --flag \"some space\" \"/start\""
    );
    // The kept folder (upstream keeps it for the setup); removed here.
    std::fs::remove_dir_all(folder).unwrap();
}

#[cfg(windows)]
#[test]
fn the_windows_install_takes_the_setup_out_of_a_zip() {
    let update = offline_update("1.3.0", &["accshift.exe"], WindowsUpdateInstallMode::Quiet);
    let mut zipped = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut zipped);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file("Accshift_1.3.0_x64-setup.exe", options)
            .unwrap();
        writer.write_all(&fake_setup()).unwrap();
        writer.finish().unwrap();
    }
    let host = RecordingHost::default();
    update.install(zipped.get_ref(), &host).unwrap();
    let (file, parameters, existed) = host.launched.lock().unwrap().take().unwrap();
    let file = std::path::PathBuf::from(file);
    assert!(existed);
    assert_eq!(file.file_name().unwrap(), "Accshift_1.3.0_x64-setup.exe");
    assert_eq!(parameters, "/S /UPDATE /R /ARGS");
    std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
}

#[test]
fn an_unknown_payload_is_refused_before_anything_runs() {
    let update = offline_update(
        "1.2.0",
        &["accshift.exe"],
        WindowsUpdateInstallMode::Passive,
    );
    let host = RecordingHost::default();
    let error = update.extract(b"not an installer").unwrap_err();
    assert_eq!(error.to_string(), "invalid updater binary format");
    if cfg!(windows) {
        assert!(update.install(b"not an installer", &host).is_err());
        assert!(host.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn the_installer_command_lines_match_the_plugin() {
    let args = ["accshift.exe", "--flag", "some space"];
    let nsis = WindowsUpdaterType::nsis("C:\\t\\setup.exe".into(), None);
    let msi = WindowsUpdaterType::msi("C:\\t\\Accshift 1.2.0.msi".into(), None);
    assert_eq!(
        msi.path(),
        std::path::Path::new("\"C:\\t\\Accshift 1.2.0.msi\"")
    );

    let passive = offline_update("1.2.0", &args, WindowsUpdateInstallMode::Passive);
    assert_eq!(
        passive.updater_parameters(&nsis),
        "/P /UPDATE /R /ARGS --flag \"some space\""
    );
    assert_eq!(
        passive.updater_parameters(&msi),
        "/i \"C:\\t\\Accshift 1.2.0.msi\" /passive /promptrestart AUTOLAUNCHAPP=True LAUNCHAPPARGS=\"--flag \"\"some space\"\"\""
    );
    let basic = offline_update("1.2.0", &args, WindowsUpdateInstallMode::BasicUi);
    assert_eq!(
        basic.updater_parameters(&nsis),
        "/UPDATE /ARGS --flag \"some space\""
    );
    let mut quiet = offline_update("1.2.0", &args, WindowsUpdateInstallMode::Quiet);
    quiet.install.restart_after_install = false;
    quiet.install.installer_args = vec!["/D=C:\\Apps".into()];
    assert_eq!(quiet.updater_parameters(&nsis), "/S /UPDATE /D=C:\\Apps");
    assert_eq!(
        quiet.updater_parameters(&msi),
        "/i \"C:\\t\\Accshift 1.2.0.msi\" /quiet /promptrestart /D=C:\\Apps"
    );

    use accshift_app::lifecycle::updater::windows_installer_file;
    assert_eq!(windows_installer_file(&nsis, None), "C:\\t\\setup.exe");
    assert_eq!(
        windows_installer_file(&msi, Some("C:\\Windows".into())),
        "C:\\Windows\\System32\\msiexec.exe"
    );
    assert_eq!(windows_installer_file(&msi, None), "msiexec.exe");
}

/// updater.rs `it_escapes_correctly_for_msi`.
#[test]
fn msi_arguments_escape_as_the_plugin_tests() {
    let cases = [
        "something",
        "--flag",
        "--empty=",
        "--arg=value",
        "some space",
        "--arg value",
        "--arg=unwrapped space",
        "--arg=\"wrapped\"",
        "--arg=\"wrapped space\"",
        "--arg=midword\"wrapped space\"",
        "",
    ];
    let cases_escaped = [
        "something",
        "--flag",
        "--empty=",
        "--arg=value",
        "\"\"some space\"\"",
        "\"\"--arg value\"\"",
        "--arg=\"\"unwrapped space\"\"",
        r#"--arg=""""""wrapped"""""""#,
        r#"--arg=""""""wrapped space"""""""#,
        r#"--arg=""midword""""wrapped space"""""""#,
        "\"\"\"\"",
    ];
    for (orig, escaped) in cases.iter().zip(cases_escaped) {
        assert_eq!(escape_msi_property_arg(orig), escaped);
    }
}

/// updater.rs `it_escapes_correctly_for_nsis`.
#[test]
fn nsis_arguments_escape_as_the_plugin_tests() {
    let cases = [
        "something",
        "--flag",
        "--empty=",
        "--arg=value",
        "some space",
        "--arg value",
        "--arg=unwrapped space",
        "--arg=\"wrapped\"",
        "--arg=\"wrapped space\"",
        "--arg=midword\"wrapped space\"",
        "",
    ];
    let cases_escaped = [
        "something",
        "--flag",
        "--empty=",
        "--arg=value",
        "\"some space\"",
        "\"--arg value\"",
        "\"--arg=unwrapped space\"",
        "--arg=\\\"wrapped\\\"",
        "\"--arg=\\\"wrapped space\\\"\"",
        "\"--arg=midword\\\"wrapped space\\\"\"",
        "\"\"",
    ];
    for (orig, escaped) in cases.iter().zip(cases_escaped) {
        assert_eq!(escape_nsis_current_exe_arg(OsStr::new(orig)), escaped);
    }
    // A trailing backslash inside quotes is doubled, `/` forces quotes.
    assert_eq!(escape_nsis_current_exe_arg("C:\\a b\\"), "\"C:\\a b\\\\\"");
    assert_eq!(escape_nsis_current_exe_arg("/S"), "\"/S\"");
}

// ---------------------------------------------------------------------------
// run_command: the update flow on the real updater

struct ApplyRecorder {
    inner: RecordingHost,
    flush: Result<(), String>,
    relaunch: Result<(), String>,
}

impl InstallerHost for ApplyRecorder {
    fn before_exit(&self) {
        self.inner.before_exit();
    }

    fn shell_execute(&self, file: &OsStr, parameters: &OsStr) -> std::io::Result<()> {
        self.inner.shell_execute(file, parameters)
    }

    fn exit(&self, code: i32) {
        self.inner.exit(code);
    }
}

impl ApplyHost for ApplyRecorder {
    fn before_relaunch(&self) -> Result<(), String> {
        self.inner
            .calls
            .lock()
            .unwrap()
            .push("before_relaunch".into());
        self.flush.clone()
    }

    fn relaunch(&self) -> Result<(), String> {
        self.inner.calls.lock().unwrap().push("relaunch".into());
        self.relaunch.clone()
    }
}

#[test]
fn run_command_drives_the_real_updater_and_the_host() {
    let key = TestKey::new();
    let artifact = fake_setup();
    let signature = key.sign(&artifact);
    let (os, arch) = os_arch();
    let platform = format!("{os}-{arch}");
    let server = server_with(
        move |base| static_manifest("1.2.0", base, &[(platform.as_str(), signature.as_str())]),
        artifact.clone(),
    );
    let updater = Updater::new(config(&server, &key.pubkey(), "1.0.5", None)).unwrap();
    let host = ApplyRecorder {
        inner: RecordingHost::default(),
        flush: Err("settings flush failed".into()),
        relaunch: Err("spawn failed".into()),
    };

    let Some(CommandResult::Checked(checked)) = run_command(&updater, UpdateCommand::Check, &host)
    else {
        panic!("check result");
    };
    let update = (*checked).unwrap().unwrap();
    assert_eq!(update.version, "1.2.0");

    let Some(CommandResult::Downloaded(bytes)) =
        run_command(&updater, UpdateCommand::Download(update.clone()), &host)
    else {
        panic!("download result");
    };
    assert_eq!(bytes.unwrap(), artifact);

    // A failed flush stops before the installer.
    let Some(CommandResult::Installed(installed)) = run_command(
        &updater,
        UpdateCommand::Install {
            update,
            bytes: artifact,
        },
        &host,
    ) else {
        panic!("install result");
    };
    assert_eq!(installed, Err("settings flush failed".to_string()));
    assert_eq!(*host.inner.calls.lock().unwrap(), ["before_relaunch"]);

    let Some(CommandResult::RelaunchFailed(error)) =
        run_command(&updater, UpdateCommand::Relaunch, &host)
    else {
        panic!("relaunch result");
    };
    assert_eq!(error, "spawn failed");
}

/// Acceptance check against the real endpoint of tauri.conf.json: one GET of
/// `latest.json`, no download, no install. Ignored; runs only with
/// `ACCSHIFT_M5_REAL_ENDPOINT=1 cargo test --test lifecycle_updater -- --ignored real_endpoint`.
#[test]
#[ignore = "network: the real release manifest"]
fn real_endpoint_manifest_reads() {
    if std::env::var("ACCSHIFT_M5_REAL_ENDPOINT").as_deref() != Ok("1") {
        return;
    }
    let mut config = UpdaterConfig::accshift();
    config.timeout = Some(Duration::from_secs(30));
    let updater = Updater::new(config).unwrap();
    let release = updater.fetch_release().unwrap().expect("a release");
    println!("M5-REAL version={}", release.version);
    println!("M5-REAL pub_date={:?}", release.pub_date);
    println!("M5-REAL platforms={:?}", release.platform_keys());
    let current = Version::parse(accshift_app::lifecycle::updater::APP_VERSION).unwrap();
    println!("M5-REAL newer_than_{current}={}", release.version > current);
    match updater.get_urls(&release, Some(Installer::Nsis)) {
        Ok((url, signature)) => {
            println!("M5-REAL nsis_url={url} signature_chars={}", signature.len())
        }
        Err(error) => println!("M5-REAL nsis_error={error}"),
    }
}
