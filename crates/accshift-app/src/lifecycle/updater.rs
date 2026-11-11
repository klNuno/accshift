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

use super::process;

use base64::Engine;
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

/// The NSIS setup arguments of `installMode: "passive"` with a restart
/// after install (config.rs lines 41-58), the only mode accshift sets.
const NSIS_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];

/// updater.rs lines 47-69.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installer {
    AppImage,
    Deb,
    Rpm,
    App,
    Nsis,
}

impl Installer {
    pub fn name(self) -> &'static str {
        match self {
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::App => "app",
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

/// updater.rs lines 1502-1512. No MSI is built: an MSI marker asks for the
/// NSIS setup, the only Windows installer the release ships.
pub fn installer_for_bundle_type(bundle: Option<BundleType>) -> Option<Installer> {
    Some(match bundle? {
        BundleType::Deb => Installer::Deb,
        BundleType::Rpm => Installer::Rpm,
        BundleType::AppImage => Installer::AppImage,
        BundleType::Msi | BundleType::Nsis => Installer::Nsis,
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

/// What `UpdaterBuilder` collects (updater.rs lines 150-405), reduced to
/// what accshift sets: no headers, no custom target, no endpoint templates,
/// no installer arguments, no downgrades.
#[derive(Clone)]
pub struct UpdaterConfig {
    pub pubkey: String,
    pub endpoints: Vec<Url>,
    pub current_version: Version,
    /// `productName`: the prefix of the installer temp folder.
    pub app_name: String,
    /// `None`: no timeout, as the async client the plugin uses.
    pub timeout: Option<Duration>,
    /// What the NSIS setup passes the relaunched app after `/ARGS`
    /// ([`process::relaunch_args`]).
    pub relaunch_args: Vec<OsString>,
    /// `bundle_type()`, read once.
    pub bundle_type: Option<BundleType>,
    /// The binary the update replaces ([`process::current_binary`]).
    pub executable_path: Option<PathBuf>,
}

impl UpdaterConfig {
    /// The app's settings: tauri.conf.json, this process's arguments and
    /// bundle marker. Built at startup, which also pins the binary path a
    /// relaunch starts ([`process::current_binary`]).
    pub fn accshift() -> Self {
        let args: Vec<OsString> = std::env::args_os().collect();
        Self {
            pubkey: PUBKEY.to_string(),
            endpoints: ENDPOINTS
                .iter()
                .map(|endpoint| Url::parse(endpoint).expect("tauri.conf.json endpoint parses"))
                .collect(),
            current_version: Version::parse(APP_VERSION).expect("APP_VERSION is semver"),
            app_name: PRODUCT_NAME.to_string(),
            timeout: None,
            relaunch_args: process::relaunch_args(&args),
            bundle_type: bundle_type(),
            executable_path: process::current_binary().ok(),
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

    fn client(&self) -> UpdaterResult<reqwest::blocking::Client> {
        accshift_core::http::blocking_client_builder()
            .user_agent(UPDATER_USER_AGENT)
            .timeout(self.config.timeout)
            .build()
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))
    }

    /// `Updater::check`, updater.rs lines 432-606. `Ok(None)`: no update
    /// (204, or the release is not newer). rustls-platform-verifier finds
    /// the system CA store itself: no `SSL_CERT_*` variable is set here,
    /// which would race the `getenv` of other threads.
    pub fn check(&self) -> UpdaterResult<Option<Update>> {
        let headers = accept_header("application/json");
        let target = updater_os().ok_or(UpdaterError::UnsupportedOs)?.to_string();

        let mut remote_release: Option<RemoteRelease> = None;
        let mut raw_json: Option<serde_json::Value> = None;
        let mut last_error: Option<UpdaterError> = None;
        for endpoint in &self.config.endpoints {
            let response = self
                .client()?
                .get(endpoint.clone())
                .headers(headers.clone())
                .send();
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

        // Resolved before the version test, as upstream (lines 581-582): a
        // manifest without this platform fails even when nothing is newer.
        let installer = installer_for_bundle_type(self.config.bundle_type);
        let (download_url, signature) = self.get_urls(&release, installer)?;

        if release.version <= self.config.current_version {
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
            install: InstallContext {
                app_name: self.config.app_name.clone(),
                relaunch_args: self.config.relaunch_args.clone(),
                bundle_type: self.config.bundle_type,
                executable_path: self.config.executable_path.clone(),
                pubkey: self.config.pubkey.clone(),
            },
        }))
    }

    /// updater.rs lines 608-638.
    pub fn get_urls<'a>(
        &self,
        release: &'a RemoteRelease,
        installer: Option<Installer>,
    ) -> UpdaterResult<(&'a Url, &'a String)> {
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

/// `[{os}-{arch}-{installer}, {os}-{arch}]`, updater.rs lines 618-626. A
/// deb or rpm install only takes its own key: the plain Linux key is the
/// AppImage, which the package install refuses after the whole download.
pub fn fallback_targets(os: &str, arch: &str, installer: Option<Installer>) -> Vec<String> {
    let mut targets = Vec::new();
    if let Some(installer) = installer {
        targets.push(format!("{os}-{arch}-{}", installer.name()));
        if matches!(installer, Installer::Deb | Installer::Rpm) {
            return targets;
        }
    }
    targets.push(format!("{os}-{arch}"));
    targets
}

fn accept_header(accept: &'static str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};
    let mut map = HeaderMap::new();
    map.insert(ACCEPT, HeaderValue::from_static(accept));
    map
}

/// What the install needs from the updater that found the release.
#[derive(Debug, Clone)]
pub struct InstallContext {
    pub app_name: String,
    pub relaunch_args: Vec<OsString>,
    pub bundle_type: Option<BundleType>,
    pub executable_path: Option<PathBuf>,
    pub pubkey: String,
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
    /// `run_on_main_thread`: the macOS admin prompt (NSAppleScript) must run
    /// on the UI thread. Inline by default; the egui shell posts it to its
    /// event loop.
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
    /// `Update::download`, updater.rs lines 680-743: the bytes, verified
    /// against the release signature.
    pub fn download(&self) -> UpdaterResult<Vec<u8>> {
        let client = accshift_core::http::blocking_client_builder()
            .user_agent(UPDATER_USER_AGENT)
            .timeout(None)
            .build()
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
        let mut response = client
            .get(self.download_url.clone())
            .headers(accept_header("application/octet-stream"))
            .send()
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;
        if !response.status().is_success() {
            return Err(UpdaterError::Network(format!(
                "Download request failed with status: {}",
                response.status()
            )));
        }
        let mut buffer = Vec::new();
        response
            .read_to_end(&mut buffer)
            .map_err(|e| UpdaterError::Reqwest(e.to_string()))?;

        verify_signature(&buffer, &self.signature, &self.install.pubkey)?;
        Ok(buffer)
    }

    /// `Update::install`, updater.rs lines 751-753. Windows: launches the
    /// installer then calls `host.exit(0)`. Linux and macOS: replaces the
    /// app in place; the caller relaunches.
    pub fn install(&self, bytes: &[u8], host: &dyn InstallerHost) -> UpdaterResult<()> {
        self.install_inner(bytes, host)
    }

    #[cfg(windows)]
    fn install_inner(&self, bytes: &[u8], host: &dyn InstallerHost) -> UpdaterResult<()> {
        let setup = self.extract(bytes)?;
        host.before_exit();
        host.shell_execute(setup.path.as_os_str(), &self.updater_parameters())
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

    /// The NSIS command line, updater.rs lines 879-939: `/P /UPDATE /R`,
    /// then `/ARGS` and the relaunch arguments.
    pub fn updater_parameters(&self) -> OsString {
        let mut args: Vec<OsString> = NSIS_ARGS.iter().map(OsString::from).collect();
        args.push("/ARGS".into());
        args.extend(
            self.install
                .relaunch_args
                .iter()
                .map(escape_nsis_current_exe_arg),
        );
        join_os(&args, " ")
    }

    /// updater.rs lines 947-998 without the plugin's `zip` feature, off in
    /// the Tauri app too: the release feed serves the NSIS setup itself.
    pub fn extract(&self, bytes: &[u8]) -> UpdaterResult<NsisSetup> {
        if !infer::app::is_exe(bytes) {
            return Err(UpdaterError::InvalidUpdaterFormat);
        }
        let (path, temp) = self.write_to_temp(bytes, ".exe")?;
        Ok(NsisSetup { path, temp })
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

/// The NSIS setup on disk, updater.rs lines 786-810.
#[derive(Debug)]
pub struct NsisSetup {
    pub path: PathBuf,
    /// Deletes the setup when dropped; a real install exits before that.
    pub temp: Option<tempfile::TempPath>,
}

/// The last step of the macOS install, once the running `.app` (`target`)
/// was moved to `backup`: move `new_app` in. On failure the backup goes
/// back to `target`, so a failed rename never leaves the user without the
/// app; the error that stopped the install is returned either way.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn move_new_app_into_place(
    new_app: &Path,
    target: &Path,
    backup: &Path,
) -> std::io::Result<()> {
    let Err(error) = std::fs::rename(new_app, target) else {
        return Ok(());
    };
    if !target.exists() {
        std::fs::rename(backup, target)?;
    }
    Err(error)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_move_into_place_puts_the_old_app_back() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Accshift.app");
        let backup = dir.path().join("backup");
        std::fs::create_dir(&backup).unwrap();
        std::fs::write(backup.join("old"), b"1.0.5").unwrap();

        // The new app is gone (another volume, a cleaner): the rename fails.
        let error =
            move_new_app_into_place(&dir.path().join("missing"), &target, &backup).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(std::fs::read(target.join("old")).unwrap(), b"1.0.5");
        assert!(!backup.exists());

        let new_app = dir.path().join("new");
        std::fs::create_dir(&new_app).unwrap();
        std::fs::write(new_app.join("new"), b"1.1.0").unwrap();
        std::fs::remove_dir_all(&target).unwrap();
        move_new_app_into_place(&new_app, &target, &backup).unwrap();
        assert_eq!(std::fs::read(target.join("new")).unwrap(), b"1.1.0");
    }
}
