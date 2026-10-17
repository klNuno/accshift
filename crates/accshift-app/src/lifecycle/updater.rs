//! The port of `tauri-plugin-updater` 2.11.0, the version the Tauri app
//! builds (Cargo.lock), with the settings of `crates/accshift-egui/tauri.conf.json`.
//!
//! Same contract on the wire and on disk: the same `latest.json` shapes
//! (`platforms` map or a single `url` and `signature`), the same target keys
//! (`windows-x86_64-nsis` then `windows-x86_64`), the same minisign check
//! with the same public key, the same installer launch (`ShellExecuteW` on
//! the NSIS setup with `/P /UPDATE /R /ARGS ...`, then exit). Source lines
//! cited below are in the plugin's `src/updater.rs` unless named otherwise.
//!
//! Blocking: [`Updater::check`] and [`Update::download`] use
//! `reqwest::blocking`, so call them from a [`crate::jobs::Jobs`] thread,
//! never from the UI thread and never inside a tokio runtime.
//! [`Update::install`] reaches the OS through [`InstallerHost`]: tests pass
//! a fake and no installer or exit ever runs.

pub mod error;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

pub use error::{UpdaterError, UpdaterResult};

use base64::Engine;
use percent_encoding::{AsciiSet, CONTROLS};
use semver::Version;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use time::OffsetDateTime;
use url::Url;

/// The plugin version this port follows (Cargo.lock of the Tauri app).
pub const PLUGIN_VERSION: &str = "2.11.0";
/// updater.rs line 44: `tauri-plugin-updater/{CARGO_PKG_VERSION}`. The
/// release host sees the same client.
pub const UPDATER_USER_AGENT: &str = "tauri-plugin-updater/2.11.0";
/// `plugins.updater.pubkey` of tauri.conf.json: the base64 of the minisign
/// public key file.
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDY5QjgxOUY2RjJFMjU5NTMKUldSVFdlTHk5aG00YVVla0RIQ25oZGlMc2tWRVVMTUQrSG96bUtiQ2ZLNFlSRkpmS1pGcVlKSzcK";
/// `plugins.updater.endpoints` of tauri.conf.json.
pub const ENDPOINTS: &[&str] =
    &["https://github.com/klNuno/accshift/releases/latest/download/latest.json"];
/// The `current_version` the check compares against: the workspace version,
/// which `version` of crates/accshift-egui/tauri.conf.json repeats.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// `productName`: `package_info().name`, the prefix of the installer temp
/// folder. crates/accshift-egui/tauri.linux.conf.json overrides it.
#[cfg(target_os = "linux")]
pub const PRODUCT_NAME: &str = "accshift";
#[cfg(not(target_os = "linux"))]
pub const PRODUCT_NAME: &str = "Accshift";

/// config.rs lines 11-25: `plugins.updater.windows.installMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowsUpdateInstallMode {
    BasicUi,
    Quiet,
    #[default]
    Passive,
}

impl WindowsUpdateInstallMode {
    /// config.rs lines 27-33.
    pub fn msiexec_args(self) -> &'static [&'static str] {
        match self {
            Self::BasicUi => &["/qb+"],
            Self::Quiet => &["/quiet"],
            Self::Passive => &["/passive"],
        }
    }

    /// config.rs lines 35-38.
    pub fn msi_restart_after_install_args(self) -> &'static [&'static str] {
        &["AUTOLAUNCHAPP=True"]
    }

    /// config.rs lines 41-50.
    pub fn nsis_args(self) -> &'static [&'static str] {
        match self {
            Self::Passive => &["/P"],
            Self::Quiet => &["/S"],
            Self::BasicUi => &[],
        }
    }

    /// config.rs lines 52-58.
    pub fn nsis_restart_after_install_args(self) -> &'static [&'static str] {
        match self {
            Self::BasicUi => &[],
            _ => &["/R"],
        }
    }
}

/// updater.rs lines 47-69.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installer {
    AppImage,
    Deb,
    Rpm,
    App,
    Msi,
    Nsis,
}

impl Installer {
    pub fn name(self) -> &'static str {
        match self {
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::App => "app",
            Self::Msi => "msi",
            Self::Nsis => "nsis",
        }
    }
}

/// tauri-utils 2.9.3 `platform::BundleType`, the kinds the bundler patches
/// into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleType {
    Deb,
    Rpm,
    AppImage,
    Msi,
    Nsis,
    App,
}

/// tauri-utils 2.9.3 `src/platform.rs` line 349. The Tauri bundler finds
/// this exact byte string in the executable and overwrites its last three
/// letters with the bundle kind (`NSS`, `MSI`, `DEB`, `RPM`, `APP`), so the
/// egui binary bundled by the same tool learns it was installed by NSIS and
/// asks for `windows-x86_64-nsis` first, like the Tauri one. `static mut`
/// for the same reason as upstream: an immutable static is folded away.
#[used]
static mut TAURI_BUNDLE_TYPE: &str = "__TAURI_BUNDLE_TYPE_VAR_UNK";

/// tauri-utils 2.9.3 `platform::bundle_type`, lines 353-370.
pub fn bundle_type() -> Option<BundleType> {
    // SAFETY: read only; the value is only ever changed in the file on disk.
    let marker: &str = unsafe { std::ptr::read_volatile(&raw const TAURI_BUNDLE_TYPE) };
    bundle_type_from_marker(marker)
}

/// The match of [`bundle_type`], on any marker (tests).
pub fn bundle_type_from_marker(marker: &str) -> Option<BundleType> {
    match marker {
        "__TAURI_BUNDLE_TYPE_VAR_DEB" => Some(BundleType::Deb),
        "__TAURI_BUNDLE_TYPE_VAR_RPM" => Some(BundleType::Rpm),
        "__TAURI_BUNDLE_TYPE_VAR_APP" => Some(BundleType::AppImage),
        "__TAURI_BUNDLE_TYPE_VAR_MSI" => Some(BundleType::Msi),
        "__TAURI_BUNDLE_TYPE_VAR_NSS" => Some(BundleType::Nsis),
        _ => {
            if cfg!(target_os = "macos") {
                Some(BundleType::App)
            } else {
                None
            }
        }
    }
}

/// updater.rs lines 1502-1512.
pub fn installer_for_bundle_type(bundle: Option<BundleType>) -> Option<Installer> {
    Some(match bundle? {
        BundleType::Deb => Installer::Deb,
        BundleType::Rpm => Installer::Rpm,
        BundleType::AppImage => Installer::AppImage,
        BundleType::Msi => Installer::Msi,
        BundleType::Nsis => Installer::Nsis,
        BundleType::App => Installer::App,
    })
}

/// updater.rs lines 1395-1406 (`darwin`, not `macos`, as upstream).
pub fn updater_os() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("linux")
    } else if cfg!(target_os = "macos") {
        Some("darwin")
    } else if cfg!(target_os = "windows") {
        Some("windows")
    } else {
        None
    }
}

/// updater.rs lines 1408-1422.
pub fn updater_arch() -> Option<&'static str> {
    if cfg!(target_arch = "x86") {
        Some("i686")
    } else if cfg!(target_arch = "x86_64") {
        Some("x86_64")
    } else if cfg!(target_arch = "arm") {
        Some("armv7")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64")
    } else if cfg!(target_arch = "riscv64") {
        Some("riscv64")
    } else {
        None
    }
}

/// updater.rs lines 1387-1393: `{os}-{arch}`.
pub fn target() -> Option<String> {
    Some(format!("{}-{}", updater_os()?, updater_arch()?))
}

/// updater.rs lines 71-77.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReleaseManifestPlatform {
    pub url: Url,
    pub signature: String,
}

/// updater.rs lines 79-86.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteReleaseInner {
    Dynamic(ReleaseManifestPlatform),
    Static {
        platforms: HashMap<String, ReleaseManifestPlatform>,
    },
}

/// updater.rs lines 88-127: a parsed `latest.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteRelease {
    pub version: Version,
    pub notes: Option<String>,
    pub pub_date: Option<OffsetDateTime>,
    pub data: RemoteReleaseInner,
}

impl RemoteRelease {
    /// updater.rs lines 105-114.
    pub fn download_url(&self, target: &str) -> UpdaterResult<&Url> {
        match &self.data {
            RemoteReleaseInner::Dynamic(platform) => Ok(&platform.url),
            RemoteReleaseInner::Static { platforms } => platforms
                .get(target)
                .map(|platform| &platform.url)
                .ok_or_else(|| UpdaterError::TargetNotFound(target.to_string())),
        }
    }

    /// updater.rs lines 117-126.
    pub fn signature(&self, target: &str) -> UpdaterResult<&String> {
        match &self.data {
            RemoteReleaseInner::Dynamic(platform) => Ok(&platform.signature),
            RemoteReleaseInner::Static { platforms } => platforms
                .get(target)
                .map(|platform| &platform.signature)
                .ok_or_else(|| UpdaterError::TargetNotFound(target.to_string())),
        }
    }

    /// The `platforms` keys, sorted (empty for the dynamic shape).
    pub fn platform_keys(&self) -> Vec<String> {
        match &self.data {
            RemoteReleaseInner::Dynamic(_) => Vec::new(),
            RemoteReleaseInner::Static { platforms } => {
                let mut keys: Vec<String> = platforms.keys().cloned().collect();
                keys.sort();
                keys
            }
        }
    }
}

/// updater.rs lines 1454-1500, with the same error messages.
impl<'de> Deserialize<'de> for RemoteRelease {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct InnerRemoteRelease {
            #[serde(alias = "name", deserialize_with = "parse_version")]
            version: Version,
            notes: Option<String>,
            pub_date: Option<String>,
            platforms: Option<HashMap<String, ReleaseManifestPlatform>>,
            url: Option<Url>,
            signature: Option<String>,
        }

        let release = InnerRemoteRelease::deserialize(deserializer)?;
        let pub_date = match release.pub_date {
            Some(date) => Some(
                OffsetDateTime::parse(&date, &time::format_description::well_known::Rfc3339)
                    .map_err(|e| DeError::custom(format!("invalid value for `pub_date`: {e}")))?,
            ),
            None => None,
        };
        Ok(RemoteRelease {
            version: release.version,
            notes: release.notes,
            pub_date,
            data: match release.platforms {
                Some(platforms) => RemoteReleaseInner::Static { platforms },
                None => RemoteReleaseInner::Dynamic(ReleaseManifestPlatform {
                    url: release.url.ok_or_else(|| {
                        DeError::custom("the `url` field was not set on the updater response")
                    })?,
                    signature: release.signature.ok_or_else(|| {
                        DeError::custom("the `signature` field was not set on the updater response")
                    })?,
                }),
            },
        })
    }
}

/// updater.rs lines 1514-1521: a leading `v` is dropped.
fn parse_version<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    Version::parse(text.trim_start_matches('v')).map_err(DeError::custom)
}

/// Parses a manifest body the way the check does (`serde_json::from_value`).
pub fn parse_manifest(json: serde_json::Value) -> UpdaterResult<RemoteRelease> {
    serde_json::from_value(json).map_err(|e| UpdaterError::Serialization(e.to_string()))
}

/// updater.rs lines 1524-1534: base64 of the key file, base64 of the `.sig`
/// file, legacy (non-prehashed) signatures accepted.
pub fn verify_signature(data: &[u8], release_signature: &str, pub_key: &str) -> UpdaterResult<()> {
    let pub_key_decoded = base64_to_string(pub_key)?;
    let public_key = minisign_verify::PublicKey::decode(&pub_key_decoded)
        .map_err(|e| UpdaterError::Minisign(e.to_string()))?;
    let signature_decoded = base64_to_string(release_signature)?;
    let signature = minisign_verify::Signature::decode(&signature_decoded)
        .map_err(|e| UpdaterError::Minisign(e.to_string()))?;
    public_key
        .verify(data, &signature, true)
        .map_err(|e| UpdaterError::Minisign(e.to_string()))
}

/// updater.rs lines 1536-1543.
fn base64_to_string(base64_string: &str) -> UpdaterResult<String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(base64_string)
        .map_err(|e| UpdaterError::Base64(e.to_string()))?;
    String::from_utf8(decoded).map_err(|_| UpdaterError::SignatureUtf8(base64_string.into()))
}

/// What `UpdaterBuilder` collects (updater.rs lines 150-405) and the plugin
/// fills from its config and the app (lib.rs lines 79-123).
#[derive(Clone)]
pub struct UpdaterConfig {
    pub pubkey: String,
    pub endpoints: Vec<Url>,
    pub install_mode: WindowsUpdateInstallMode,
    /// `plugins.updater.windows.installerArgs`; none in tauri.conf.json.
    pub installer_args: Vec<OsString>,
    pub current_version: Version,
    pub app_name: String,
    /// `UpdaterBuilder::target`, unset by the app.
    pub target: Option<String>,
    /// `None`: no timeout, as the async client the plugin uses.
    pub timeout: Option<Duration>,
    pub headers: Vec<(String, String)>,
    /// `env().args_os`, the whole command line including the executable.
    pub current_exe_args: Vec<OsString>,
    pub restart_after_install: bool,
    /// `bundle_type()`, read once.
    pub bundle_type: Option<BundleType>,
    /// The current executable (`tauri_utils::platform::current_exe`), or the
    /// `APPIMAGE` file on Linux.
    pub executable_path: Option<PathBuf>,
    /// `version_comparator`: `None` is `release > current`. The JS `check`
    /// with `allowDowngrades` passes `release != current`.
    pub allow_downgrades: bool,
}

impl UpdaterConfig {
    /// The app's settings: tauri.conf.json, this process's arguments and
    /// bundle marker.
    pub fn accshift() -> Self {
        Self {
            pubkey: PUBKEY.to_string(),
            endpoints: ENDPOINTS
                .iter()
                .map(|endpoint| Url::parse(endpoint).expect("tauri.conf.json endpoint parses"))
                .collect(),
            install_mode: WindowsUpdateInstallMode::Passive,
            installer_args: Vec::new(),
            current_version: Version::parse(APP_VERSION).expect("APP_VERSION is semver"),
            app_name: PRODUCT_NAME.to_string(),
            target: None,
            timeout: None,
            headers: Vec::new(),
            current_exe_args: std::env::args_os().collect(),
            restart_after_install: true,
            bundle_type: bundle_type(),
            executable_path: default_executable_path(),
            allow_downgrades: false,
        }
    }

    /// config.rs `validate_endpoints`: anything but https is refused in a
    /// release build and only warned about in a debug one.
    pub fn validate(&self) -> UpdaterResult<()> {
        if self.endpoints.is_empty() {
            return Err(UpdaterError::EmptyEndpoints);
        }
        for endpoint in &self.endpoints {
            if endpoint.scheme() != "https" && !cfg!(debug_assertions) {
                return Err(UpdaterError::InsecureTransportProtocol);
            }
        }
        Ok(())
    }
}

/// lib.rs lines 105-113 and updater.rs lines 376-383: the AppImage on Linux
/// when `APPIMAGE` is set, else the canonical current executable.
fn default_executable_path() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(PathBuf::from(appimage));
    }
    std::env::current_exe().and_then(std::fs::canonicalize).ok()
}

/// updater.rs lines 1424-1452: the folder the update replaces. The
/// executable's folder, or the `.app` on macOS; Linux uses the file itself.
pub fn extract_path_from_executable(executable_path: &Path) -> UpdaterResult<PathBuf> {
    let extract_path = executable_path
        .parent()
        .map(PathBuf::from)
        .ok_or(UpdaterError::FailedToDetermineExtractPath)?;
    #[cfg(target_os = "macos")]
    if extract_path
        .display()
        .to_string()
        .contains("Contents/MacOS")
    {
        return extract_path
            .parent()
            .and_then(Path::parent)
            .map(PathBuf::from)
            .ok_or(UpdaterError::FailedToDetermineExtractPath);
    }
    Ok(extract_path)
}

/// `CONTROLS` plus `+`, updater.rs line 468.
const VERSION_ENCODE_SET: &AsciiSet = &CONTROLS.add(b'+');

/// One configured updater.
pub struct Updater {
    config: UpdaterConfig,
    arch: &'static str,
}

impl Updater {
    /// `UpdaterBuilder::build` (updater.rs lines 365-405).
    pub fn new(config: UpdaterConfig) -> UpdaterResult<Self> {
        config.validate()?;
        let arch = updater_arch().ok_or(UpdaterError::UnsupportedArch)?;
        Ok(Self { config, arch })
    }

    pub fn config(&self) -> &UpdaterConfig {
        &self.config
    }

    /// updater.rs lines 459-488: `{{current_version}}`, `{{target}}`,
    /// `{{arch}}` and `{{bundle_type}}`, raw and percent-encoded.
    pub fn endpoint_url(&self, endpoint: &Url, target: &str) -> UpdaterResult<Url> {
        let version = self.config.current_version.to_string();
        let encoded_version =
            percent_encoding::percent_encode(version.as_bytes(), VERSION_ENCODE_SET).to_string();
        let installer = installer_for_bundle_type(self.config.bundle_type)
            .map(Installer::name)
            .unwrap_or("unknown");
        endpoint
            .to_string()
            .replace("%7B%7Bcurrent_version%7D%7D", &encoded_version)
            .replace("%7B%7Btarget%7D%7D", target)
            .replace("%7B%7Barch%7D%7D", self.arch)
            .replace("%7B%7Bbundle_type%7D%7D", installer)
            .replace("{{current_version}}", &encoded_version)
            .replace("{{target}}", target)
            .replace("{{arch}}", self.arch)
            .replace("{{bundle_type}}", installer)
            .parse()
            .map_err(|e: url::ParseError| UpdaterError::UrlParse(e.to_string()))
    }

    fn client(&self) -> UpdaterResult<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .user_agent(UPDATER_USER_AGENT)
            .timeout(self.config.timeout)
            .build()
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))
    }

    fn headers(&self, accept: &'static str) -> UpdaterResult<reqwest::header::HeaderMap> {
        headers_with_accept(&self.config.headers, accept)
    }

    /// `Updater::check`, updater.rs lines 432-606. `Ok(None)`: no update
    /// (204, or the release is not newer).
    pub fn check(&self) -> UpdaterResult<Option<Update>> {
        let headers = self.headers("application/json")?;

        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("SSL_CERT_FILE").is_none() {
                std::env::set_var("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt");
            }
            if std::env::var_os("SSL_CERT_DIR").is_none() {
                std::env::set_var("SSL_CERT_DIR", "/etc/ssl/certs");
            }
        }

        let target = match &self.config.target {
            Some(target) => target.clone(),
            None => updater_os().ok_or(UpdaterError::UnsupportedOs)?.to_string(),
        };

        let mut remote_release: Option<RemoteRelease> = None;
        let mut raw_json: Option<serde_json::Value> = None;
        let mut last_error: Option<UpdaterError> = None;
        for endpoint in &self.config.endpoints {
            let url = self.endpoint_url(endpoint, &target)?;
            let response = self.client()?.get(url).headers(headers.clone()).send();
            match response {
                Ok(response) => {
                    if response.status().is_success() {
                        if response.status() == reqwest::StatusCode::NO_CONTENT {
                            return Ok(None);
                        }
                        let body: serde_json::Value = response
                            .json()
                            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
                        raw_json = Some(body.clone());
                        match parse_manifest(body) {
                            Ok(release) => {
                                last_error = None;
                                remote_release = Some(release);
                                break;
                            }
                            Err(error) => last_error = Some(error),
                        }
                    }
                    // A non-success status only logs upstream (line 545):
                    // it is not kept as the last error.
                }
                Err(error) => last_error = Some(UpdaterError::Reqwest(error.to_string())),
            }
        }

        if let Some(error) = last_error {
            return Err(error);
        }
        let release = remote_release.ok_or(UpdaterError::ReleaseNotFound)?;

        let should_update = if self.config.allow_downgrades {
            release.version != self.config.current_version
        } else {
            release.version > self.config.current_version
        };

        // Resolved before the version test, as upstream (lines 581-582): a
        // manifest without this platform fails even when nothing is newer.
        let installer = installer_for_bundle_type(self.config.bundle_type);
        let (download_url, signature) = self.get_urls(&release, installer)?;

        if !should_update {
            return Ok(None);
        }
        Ok(Some(Update {
            body: release.notes.clone(),
            current_version: self.config.current_version.to_string(),
            version: release.version.to_string(),
            date: release.pub_date,
            target,
            download_url: download_url.clone(),
            signature: signature.clone(),
            raw_json: raw_json.unwrap_or_default(),
            timeout: None,
            headers: self.config.headers.clone(),
            install: InstallContext {
                app_name: self.config.app_name.clone(),
                install_mode: self.config.install_mode,
                installer_args: self.config.installer_args.clone(),
                current_exe_args: self.config.current_exe_args.clone(),
                restart_after_install: self.config.restart_after_install,
                bundle_type: self.config.bundle_type,
                executable_path: self.config.executable_path.clone(),
                pubkey: self.config.pubkey.clone(),
            },
        }))
    }

    /// The release a check would read, without the version test or the
    /// target lookup (acceptance check against the real endpoint).
    pub fn fetch_release(&self) -> UpdaterResult<Option<RemoteRelease>> {
        let target = updater_os().ok_or(UpdaterError::UnsupportedOs)?;
        let mut last_error = None;
        for endpoint in &self.config.endpoints {
            let url = self.endpoint_url(endpoint, target)?;
            match self
                .client()?
                .get(url)
                .headers(self.headers("application/json")?)
                .send()
            {
                Ok(response) if response.status() == reqwest::StatusCode::NO_CONTENT => {
                    return Ok(None)
                }
                Ok(response) if response.status().is_success() => {
                    let body: serde_json::Value = response
                        .json()
                        .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
                    return parse_manifest(body).map(Some);
                }
                Ok(_) => {}
                Err(error) => last_error = Some(UpdaterError::Reqwest(error.to_string())),
            }
        }
        Err(last_error.unwrap_or(UpdaterError::ReleaseNotFound))
    }

    /// updater.rs lines 608-638.
    pub fn get_urls<'a>(
        &self,
        release: &'a RemoteRelease,
        installer: Option<Installer>,
    ) -> UpdaterResult<(&'a Url, &'a String)> {
        if let Some(target) = &self.config.target {
            return Ok((release.download_url(target)?, release.signature(target)?));
        }
        let os = updater_os().ok_or(UpdaterError::UnsupportedOs)?;
        let targets = fallback_targets(os, self.arch, installer);
        for target in &targets {
            if let (Ok(url), Ok(signature)) =
                (release.download_url(target), release.signature(target))
            {
                return Ok((url, signature));
            }
        }
        Err(UpdaterError::TargetsNotFound(targets))
    }
}

/// `[{os}-{arch}-{installer}, {os}-{arch}]`, updater.rs lines 618-626.
pub fn fallback_targets(os: &str, arch: &str, installer: Option<Installer>) -> Vec<String> {
    let mut targets = Vec::new();
    if let Some(installer) = installer {
        targets.push(format!("{os}-{arch}-{}", installer.name()));
    }
    targets.push(format!("{os}-{arch}"));
    targets
}

fn headers_with_accept(
    headers: &[(String, String)],
    accept: &'static str,
) -> UpdaterResult<reqwest::header::HeaderMap> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT};
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
        let value =
            HeaderValue::from_str(value).map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
        map.insert(name, value);
    }
    if !map.contains_key(ACCEPT) {
        map.insert(ACCEPT, HeaderValue::from_static(accept));
    }
    Ok(map)
}

/// What the install needs from the updater that found the release.
#[derive(Debug, Clone)]
pub struct InstallContext {
    pub app_name: String,
    pub install_mode: WindowsUpdateInstallMode,
    pub installer_args: Vec<OsString>,
    pub current_exe_args: Vec<OsString>,
    pub restart_after_install: bool,
    pub bundle_type: Option<BundleType>,
    pub executable_path: Option<PathBuf>,
    pub pubkey: String,
}

/// The JS `DownloadEvent` (commands.rs lines 121-133): `Started` on the
/// first chunk, then one `Progress` per chunk, then `Finished`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

/// updater.rs lines 640-668: an available update.
#[derive(Debug, Clone)]
pub struct Update {
    pub body: Option<String>,
    pub current_version: String,
    pub version: String,
    pub date: Option<OffsetDateTime>,
    pub target: String,
    pub download_url: Url,
    pub signature: String,
    pub raw_json: serde_json::Value,
    pub timeout: Option<Duration>,
    pub headers: Vec<(String, String)>,
    pub install: InstallContext,
}

/// The side effects of an install: the hook before exit, the installer
/// launch, the exit, the macOS main-thread hop. [`SystemInstaller`] is the
/// real one; tests record calls instead.
pub trait InstallerHost {
    /// `on_before_exit`: the plugin runs `app.cleanup_before_exit()` (hides
    /// the windows on Windows).
    fn before_exit(&self);
    /// `ShellExecuteW(null, "open", file, parameters, null, SW_SHOW)`;
    /// an `Err` when it answers 32 or less.
    fn shell_execute(&self, file: &OsStr, parameters: &OsStr) -> std::io::Result<()>;
    /// `std::process::exit(code)`. The real one never returns.
    fn exit(&self, code: i32);
    /// `run_on_main_thread` (macOS admin prompt). Inline by default.
    fn run_on_main_thread(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        task();
        Ok(())
    }
}

/// The real [`InstallerHost`].
pub struct SystemInstaller {
    pub before_exit: Box<dyn Fn() + Send + Sync>,
}

impl InstallerHost for SystemInstaller {
    fn before_exit(&self) {
        (self.before_exit)();
    }

    #[cfg(windows)]
    fn shell_execute(&self, file: &OsStr, parameters: &OsStr) -> std::io::Result<()> {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;
        let operation = encode_wide("open");
        let file = encode_wide(file);
        let parameters = encode_wide(parameters);
        // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that
        // outlives the call.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                std::ptr::null(),
                SW_SHOW,
            )
        };
        if result as isize <= 32 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn shell_execute(&self, _file: &OsStr, _parameters: &OsStr) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }

    fn exit(&self, code: i32) {
        std::process::exit(code);
    }
}

#[cfg(windows)]
fn encode_wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

impl Update {
    /// The `pub_date` as RFC 3339, as the JS `Update.date` carries it.
    pub fn date_rfc3339(&self) -> Option<String> {
        self.date.and_then(|date| {
            date.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
    }

    /// `Update::download`, updater.rs lines 680-743: the bytes, verified
    /// against the release signature. `on_chunk(len, content_length)` per
    /// chunk read, `on_download_finish` once the body is read.
    pub fn download(
        &self,
        mut on_chunk: impl FnMut(usize, Option<u64>),
        on_download_finish: impl FnOnce(),
    ) -> UpdaterResult<Vec<u8>> {
        let headers = headers_with_accept(&self.headers, "application/octet-stream")?;
        let client = reqwest::blocking::Client::builder()
            .user_agent(UPDATER_USER_AGENT)
            .timeout(self.timeout)
            .build()
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
        let mut response = client
            .get(self.download_url.clone())
            .headers(headers)
            .send()
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
        if !response.status().is_success() {
            return Err(UpdaterError::Network(format!(
                "Download request failed with status: {}",
                response.status()
            )));
        }
        let content_length: Option<u64> = response
            .headers()
            .get("Content-Length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());

        let mut buffer = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let read = response
                .read(&mut chunk)
                .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
            if read == 0 {
                break;
            }
            on_chunk(read, content_length);
            buffer.extend_from_slice(&chunk[..read]);
        }
        on_download_finish();

        verify_signature(&buffer, &self.signature, &self.install.pubkey)?;
        Ok(buffer)
    }

    /// [`Self::download`] reporting [`DownloadEvent`]s, as the JS command.
    pub fn download_with_events(
        &self,
        mut on_event: impl FnMut(DownloadEvent),
    ) -> UpdaterResult<Vec<u8>> {
        let mut first_chunk = true;
        let events = std::cell::RefCell::new(&mut on_event);
        self.download(
            |chunk_length, content_length| {
                let mut on_event = events.borrow_mut();
                if first_chunk {
                    first_chunk = false;
                    on_event(DownloadEvent::Started { content_length });
                }
                on_event(DownloadEvent::Progress { chunk_length });
            },
            || (events.borrow_mut())(DownloadEvent::Finished),
        )
    }

    /// `Update::install`, updater.rs lines 751-753. Windows: launches the
    /// installer then calls `host.exit(0)`. Linux and macOS: replaces the
    /// app in place; the caller relaunches.
    pub fn install(&self, bytes: &[u8], host: &dyn InstallerHost) -> UpdaterResult<()> {
        self.install_inner(bytes, host)
    }

    #[cfg(windows)]
    fn install_inner(&self, bytes: &[u8], host: &dyn InstallerHost) -> UpdaterResult<()> {
        let updater_type = self.extract(bytes)?;
        host.before_exit();
        let file = windows_installer_file(&updater_type, std::env::var("SYSTEMROOT").ok());
        let parameters = self.updater_parameters(&updater_type);
        host.shell_execute(&file, &parameters)
            .map_err(UpdaterError::Io)?;
        host.exit(0);
        Ok(())
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn install_inner(&self, bytes: &[u8], host: &dyn InstallerHost) -> UpdaterResult<()> {
        let executable = self
            .install
            .executable_path
            .clone()
            .ok_or(UpdaterError::FailedToDetermineExtractPath)?;
        // updater.rs lines 379-383: Linux replaces the file itself.
        let extract_path = if cfg!(target_os = "linux") {
            executable
        } else {
            extract_path_from_executable(&executable)?
        };
        let main_thread = |task: Box<dyn FnOnce() + Send>| host.run_on_main_thread(task);
        unix::install_files(
            installer_for_bundle_type(self.install.bundle_type),
            &extract_path,
            bytes,
            &main_thread,
        )
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    fn install_inner(&self, _bytes: &[u8], _host: &dyn InstallerHost) -> UpdaterResult<()> {
        Err(UpdaterError::UnsupportedOs)
    }

    /// The installer command line, updater.rs lines 879-939.
    pub fn updater_parameters(&self, updater_type: &WindowsUpdaterType) -> OsString {
        let context = &self.install;
        let mode = context.install_mode;
        let current_args = context.current_exe_args.get(1..).unwrap_or_default();
        let mut args: Vec<OsString> = Vec::new();
        match updater_type {
            WindowsUpdaterType::Nsis { .. } => {
                args.extend(mode.nsis_args().iter().map(OsString::from));
                args.push("/UPDATE".into());
                if context.restart_after_install {
                    args.extend(
                        mode.nsis_restart_after_install_args()
                            .iter()
                            .map(OsString::from),
                    );
                    args.push("/ARGS".into());
                    args.extend(current_args.iter().map(escape_nsis_current_exe_arg));
                }
                args.extend(context.installer_args.iter().cloned());
            }
            WindowsUpdaterType::Msi { path, .. } => {
                args.push("/i".into());
                args.push(path.as_os_str().to_os_string());
                args.extend(mode.msiexec_args().iter().map(OsString::from));
                args.push("/promptrestart".into());
                args.extend(context.installer_args.iter().cloned());
                if context.restart_after_install {
                    args.extend(
                        mode.msi_restart_after_install_args()
                            .iter()
                            .map(OsString::from),
                    );
                    let escaped = current_args
                        .iter()
                        .map(escape_msi_property_arg)
                        .collect::<Vec<_>>()
                        .join(" ");
                    args.push(format!("LAUNCHAPPARGS=\"{escaped}\"").into());
                }
            }
        }
        join_os(&args, " ")
    }

    /// updater.rs lines 947-954 without the plugin's `zip` feature, off in
    /// the Tauri app too: the release feed serves the installer itself.
    pub fn extract(&self, bytes: &[u8]) -> UpdaterResult<WindowsUpdaterType> {
        self.extract_exe(bytes)
    }

    /// updater.rs lines 956-965: kept on purpose (`keep`), the installer
    /// runs after this process exits.
    fn make_temp_dir(&self) -> UpdaterResult<PathBuf> {
        Ok(tempfile::Builder::new()
            .prefix(&format!(
                "{}-{}-updater-",
                self.install.app_name, self.version
            ))
            .tempdir()?
            .keep())
    }

    /// updater.rs lines 988-998.
    fn extract_exe(&self, bytes: &[u8]) -> UpdaterResult<WindowsUpdaterType> {
        if infer::app::is_exe(bytes) {
            let (path, temp) = self.write_to_temp(bytes, ".exe")?;
            Ok(WindowsUpdaterType::nsis(path, temp))
        } else if infer::archive::is_msi(bytes) {
            let (path, temp) = self.write_to_temp(bytes, ".msi")?;
            Ok(WindowsUpdaterType::msi(path, temp))
        } else {
            Err(UpdaterError::InvalidUpdaterFormat)
        }
    }

    /// updater.rs lines 1000-1021: `{app}-{version}-installer{ext}` in a
    /// kept temp folder; the file itself is deleted when the value drops.
    fn write_to_temp(
        &self,
        bytes: &[u8],
        ext: &str,
    ) -> UpdaterResult<(PathBuf, Option<tempfile::TempPath>)> {
        use std::io::Write;
        let temp_dir = self.make_temp_dir()?;
        let mut temp_file = tempfile::Builder::new()
            .prefix(&format!(
                "{}-{}-installer",
                self.install.app_name, self.version
            ))
            .suffix(ext)
            .rand_bytes(0)
            .tempfile_in(temp_dir)?;
        temp_file.write_all(bytes)?;
        let temp = temp_file.into_temp_path();
        Ok((temp.to_path_buf(), Some(temp)))
    }
}

/// updater.rs lines 786-810. The MSI path is stored quoted, as upstream.
#[derive(Debug)]
pub enum WindowsUpdaterType {
    Nsis {
        path: PathBuf,
        temp: Option<tempfile::TempPath>,
    },
    Msi {
        path: PathBuf,
        temp: Option<tempfile::TempPath>,
    },
}

impl WindowsUpdaterType {
    pub fn nsis(path: PathBuf, temp: Option<tempfile::TempPath>) -> Self {
        Self::Nsis { path, temp }
    }

    pub fn msi(path: PathBuf, temp: Option<tempfile::TempPath>) -> Self {
        let mut quoted = OsString::from("\"");
        quoted.push(path.as_os_str());
        quoted.push("\"");
        Self::Msi {
            path: PathBuf::from(quoted),
            temp,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Nsis { path, .. } | Self::Msi { path, .. } => path,
        }
    }
}

/// updater.rs lines 848-852: the NSIS setup itself, or
/// `%SYSTEMROOT%\System32\msiexec.exe` (`msiexec.exe` without the variable).
pub fn windows_installer_file(
    updater_type: &WindowsUpdaterType,
    system_root: Option<String>,
) -> OsString {
    match updater_type {
        WindowsUpdaterType::Nsis { path, .. } => path.as_os_str().to_os_string(),
        WindowsUpdaterType::Msi { .. } => match system_root {
            Some(root) => OsString::from(format!("{root}\\System32\\msiexec.exe")),
            None => OsString::from("msiexec.exe"),
        },
    }
}

fn join_os(parts: &[OsString], separator: &str) -> OsString {
    let mut joined = OsString::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            joined.push(separator);
        }
        joined.push(part);
    }
    joined
}

/// updater.rs lines 1572-1609: std's Windows argument quoting, plus `/`
/// forcing quotes so NSIS does not read an option. Works on UTF-16 units on
/// Windows and on the lossy text elsewhere.
pub fn escape_nsis_current_exe_arg(arg: impl AsRef<OsStr>) -> OsString {
    let arg = arg.as_ref();
    let quote = arg
        .as_encoded_bytes()
        .iter()
        .any(|c| *c == b' ' || *c == b'\t' || *c == b'/')
        || arg.is_empty();
    let units = os_to_units(arg);
    let mut cmd: Vec<u16> = Vec::with_capacity(units.len() + 2);
    if quote {
        cmd.push('"' as u16);
    }
    let mut backslashes: usize = 0;
    for x in units {
        if x == '\\' as u16 {
            backslashes += 1;
        } else {
            if x == '"' as u16 {
                cmd.extend((0..=backslashes).map(|_| '\\' as u16));
            }
            backslashes = 0;
        }
        cmd.push(x);
    }
    if quote {
        cmd.extend((0..backslashes).map(|_| '\\' as u16));
        cmd.push('"' as u16);
    }
    units_to_os(cmd)
}

#[cfg(windows)]
fn os_to_units(text: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().collect()
}

#[cfg(not(windows))]
fn os_to_units(text: &OsStr) -> Vec<u16> {
    text.to_string_lossy().encode_utf16().collect()
}

#[cfg(windows)]
fn units_to_os(units: Vec<u16>) -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(&units)
}

#[cfg(not(windows))]
fn units_to_os(units: Vec<u16>) -> OsString {
    OsString::from(String::from_utf16_lossy(&units))
}

/// updater.rs lines 1612-1635: one argument inside `LAUNCHAPPARGS="..."`.
pub fn escape_msi_property_arg(arg: impl AsRef<OsStr>) -> String {
    let mut arg = arg.as_ref().to_string_lossy().to_string();
    if arg.is_empty() {
        return "\"\"\"\"".to_string();
    } else if !arg.contains(' ') && !arg.contains('"') {
        return arg;
    }
    if arg.contains('"') {
        arg = arg.replace('"', r#""""""#);
    }
    if arg.starts_with('-') {
        if let Some((a1, a2)) = arg.split_once('=') {
            format!("{a1}=\"\"{a2}\"\"")
        } else {
            format!("\"\"{arg}\"\"")
        }
    } else {
        format!("\"\"{arg}\"\"")
    }
}
