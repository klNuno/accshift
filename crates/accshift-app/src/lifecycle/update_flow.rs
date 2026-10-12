//! `src/lib/app/useAppUpdater.svelte.ts` as a state machine: the background
//! check, download and "restart to update" button.
//!
//! No async: each blocking half (check, download, install, relaunch) is an
//! [`UpdateCommand`] the shell runs on a [`crate::jobs::Jobs`] thread
//! ([`run_command`] does it with the real updater), then hands the result
//! back to the matching `on_*` method. Every method answers the outputs in
//! the order the TS code produced them: telemetry, toasts, log lines and the
//! next command.

use super::updater::{InstallerHost, Update, Updater};
use crate::text::Text;

/// `UpdateState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    Downloading,
    Ready,
    Applying,
}

/// `UpdateCheckErrorCode`, plus the three codes of the later stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateErrorCode {
    TargetMissing,
    ManifestInvalid,
    CheckFailed,
    DownloadFailed,
    InstallFailed,
    RelaunchFailed,
}

impl UpdateErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TargetMissing => "update_target_missing",
            Self::ManifestInvalid => "update_manifest_invalid",
            Self::CheckFailed => "check_failed",
            Self::DownloadFailed => "download_failed",
            Self::InstallFailed => "install_failed",
            Self::RelaunchFailed => "relaunch_failed",
        }
    }
}

/// `TARGET_MISSING_RE`.
const TARGET_MISSING: &str = "found in the response `platforms` object";

/// `MANIFEST_INVALID_PATTERNS`, all case-insensitive, lowercased here. The
/// last regex, `/while parsing (major|minor|patch) version number/i`, is its
/// three spellings.
const MANIFEST_INVALID: [&str; 10] = [
    "could not fetch a valid release json",
    "could not be decoded",
    "invalid updater binary format",
    "missing field",
    "expected value at line",
    "invalid type:",
    "trailing characters at line",
    "while parsing major version number",
    "while parsing minor version number",
    "while parsing patch version number",
];

/// `classifyUpdateCheckError`: sorts a failed check by its `Display` text.
/// `None` is the TS `undefined`.
pub fn classify_update_check_error(error: Option<&str>) -> UpdateErrorCode {
    let text = error.unwrap_or("undefined");
    if text.contains(TARGET_MISSING) {
        return UpdateErrorCode::TargetMissing;
    }
    let lower = text.to_lowercase();
    if MANIFEST_INVALID
        .iter()
        .any(|pattern| lower.contains(pattern))
    {
        return UpdateErrorCode::ManifestInvalid;
    }
    UpdateErrorCode::CheckFailed
}

/// `trackUpdate` stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    Available,
    Downloaded,
    Applied,
    Failed,
}

impl UpdateStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Downloaded => "downloaded",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }
}

/// A pending update as the state machine needs it: its version.
pub trait PendingUpdate: Clone {
    fn version(&self) -> String;
}

impl PendingUpdate for Update {
    fn version(&self) -> String {
        self.version.clone()
    }
}

/// The blocking work the shell runs next.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateCommand<P> {
    /// `check()`; answer with [`AppUpdater::on_check_result`].
    Check,
    /// `update.download()`; answer with [`AppUpdater::on_download_result`].
    Download(P),
    /// `beforeRelaunch` (flush the stores and settings), then
    /// `update.install()`; answer with [`AppUpdater::on_install_result`].
    /// On Windows a real install exits the process here.
    Install { update: P, bytes: Vec<u8> },
    /// `relaunch()`; answer only on failure, with
    /// [`AppUpdater::on_relaunch_failed`].
    Relaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Error,
}

/// What the flow asks of the shell, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateOutput<P> {
    /// `trackUpdate(stage, targetVersion, errorCode)`; `None` is `undefined`.
    Track {
        stage: UpdateStage,
        target_version: Option<String>,
        error_code: Option<UpdateErrorCode>,
    },
    Toast(Text),
    /// `console.info` or `console.error` with the error text.
    Log {
        level: LogLevel,
        message: String,
    },
    Run(UpdateCommand<P>),
}

/// `createAppUpdater`.
#[derive(Debug, Clone)]
pub struct AppUpdater<P> {
    state: UpdateState,
    version: String,
    pending: Option<P>,
    bytes: Option<Vec<u8>>,
    check_started: bool,
    /// `import.meta.env.DEV`: the flow never runs in a dev build.
    dev: bool,
}

impl<P: PendingUpdate> AppUpdater<P> {
    /// `dev`: true skips every check, as `import.meta.env.DEV` did. The
    /// egui shell passes `cfg!(debug_assertions)`.
    pub fn new(dev: bool) -> Self {
        Self {
            state: UpdateState::Idle,
            version: String::new(),
            pending: None,
            bytes: None,
            check_started: false,
            dev,
        }
    }

    pub fn state(&self) -> UpdateState {
        self.state
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    /// `ctaLabel`: the titlebar button text, only while ready or applying.
    pub fn cta_label(&self) -> Option<Text> {
        match self.state {
            UpdateState::Ready => Some(Text::key("update.ctaAvailable")),
            UpdateState::Applying => Some(Text::key("update.ctaInstalling")),
            _ => None,
        }
    }

    /// `ctaTitle`.
    pub fn cta_title(&self) -> Text {
        if self.version.is_empty() {
            Text::key("update.restartToApply")
        } else {
            Text::with(
                "update.restartToApplyVersion",
                [("version", self.version.clone())],
            )
        }
    }

    /// `ctaDisabled`.
    pub fn cta_disabled(&self) -> bool {
        self.state == UpdateState::Applying
    }

    /// `startBackgroundUpdateFlow`, up to the `check()` call. App.svelte
    /// calls it 3500 ms after mount ([`super::UPDATE_CHECK_DELAY`]).
    pub fn start(&mut self) -> Vec<UpdateOutput<P>> {
        if self.dev || self.check_started {
            return Vec::new();
        }
        self.check_started = true;
        self.state = UpdateState::Checking;
        vec![UpdateOutput::Run(UpdateCommand::Check)]
    }

    /// The `check()` result, the error as its `Display` text.
    pub fn on_check_result(&mut self, result: Result<Option<P>, String>) -> Vec<UpdateOutput<P>> {
        match result {
            Ok(None) => {
                self.state = UpdateState::Idle;
                Vec::new()
            }
            Ok(Some(update)) => {
                self.version = update.version();
                self.pending = Some(update.clone());
                self.state = UpdateState::Downloading;
                vec![
                    UpdateOutput::Track {
                        stage: UpdateStage::Available,
                        target_version: Some(self.version.clone()),
                        error_code: None,
                    },
                    UpdateOutput::Run(UpdateCommand::Download(update)),
                ]
            }
            Err(error) => {
                let code = classify_update_check_error(Some(&error));
                self.flow_failed(code, error)
            }
        }
    }

    /// The `update.download()` result.
    pub fn on_download_result(&mut self, result: Result<Vec<u8>, String>) -> Vec<UpdateOutput<P>> {
        match result {
            Ok(bytes) => {
                self.bytes = Some(bytes);
                self.state = UpdateState::Ready;
                let toast = if self.version.is_empty() {
                    Text::key("update.readyToast")
                } else {
                    Text::with(
                        "update.readyToastVersion",
                        [("version", self.version.clone())],
                    )
                };
                vec![
                    UpdateOutput::Track {
                        stage: UpdateStage::Downloaded,
                        target_version: Some(self.version.clone()),
                        error_code: None,
                    },
                    UpdateOutput::Toast(toast),
                ]
            }
            Err(error) => self.flow_failed(UpdateErrorCode::DownloadFailed, error),
        }
    }

    /// The catch of `startBackgroundUpdateFlow`.
    fn flow_failed(&mut self, code: UpdateErrorCode, error: String) -> Vec<UpdateOutput<P>> {
        let log = if code == UpdateErrorCode::TargetMissing {
            UpdateOutput::Log {
                level: LogLevel::Info,
                message: format!("Updater: this platform is not in the release manifest: {error}"),
            }
        } else {
            UpdateOutput::Log {
                level: LogLevel::Error,
                message: format!("Updater check/download failed: {error}"),
            }
        };
        let track = UpdateOutput::Track {
            stage: UpdateStage::Failed,
            target_version: self.version_or_undefined(),
            error_code: Some(code),
        };
        self.reset();
        vec![log, track]
    }

    fn version_or_undefined(&self) -> Option<String> {
        (!self.version.is_empty()).then(|| self.version.clone())
    }

    fn reset(&mut self) {
        self.pending = None;
        self.bytes = None;
        self.version.clear();
        self.state = UpdateState::Idle;
        self.check_started = false;
    }

    /// `applyReadyUpdate`, up to `install()`: only from `Ready`.
    pub fn apply(&mut self) -> Vec<UpdateOutput<P>> {
        if self.state != UpdateState::Ready {
            return Vec::new();
        }
        let Some(update) = self.pending.clone() else {
            return Vec::new();
        };
        self.state = UpdateState::Applying;
        let bytes = self.bytes.clone().unwrap_or_default();
        vec![UpdateOutput::Run(UpdateCommand::Install { update, bytes })]
    }

    /// The `beforeRelaunch` plus `install()` result. Success tracks
    /// `applied` before the relaunch, as TS: there is no "after".
    pub fn on_install_result(&mut self, result: Result<(), String>) -> Vec<UpdateOutput<P>> {
        match result {
            Ok(()) => vec![
                UpdateOutput::Track {
                    stage: UpdateStage::Applied,
                    target_version: Some(self.version.clone()),
                    error_code: None,
                },
                UpdateOutput::Run(UpdateCommand::Relaunch),
            ],
            Err(error) => self.apply_failed(UpdateErrorCode::InstallFailed, error),
        }
    }

    /// A `relaunch()` that came back with an error.
    pub fn on_relaunch_failed(&mut self, error: String) -> Vec<UpdateOutput<P>> {
        self.apply_failed(UpdateErrorCode::RelaunchFailed, error)
    }

    /// The catch of `applyReadyUpdate`: track, reset, toast, check again.
    fn apply_failed(&mut self, code: UpdateErrorCode, error: String) -> Vec<UpdateOutput<P>> {
        let mut outputs = vec![
            UpdateOutput::Log {
                level: LogLevel::Error,
                message: format!("Failed to restart for update: {error}"),
            },
            UpdateOutput::Track {
                stage: UpdateStage::Failed,
                target_version: self.version_or_undefined(),
                error_code: Some(code),
            },
        ];
        self.reset();
        outputs.push(UpdateOutput::Toast(Text::key("update.restartFailed")));
        outputs.extend(self.start());
        outputs
    }
}

/// What [`run_command`] hands back, for the matching `on_*` method.
#[derive(Debug)]
pub enum CommandResult {
    /// Boxed: an [`Update`] is far larger than the other variants.
    Checked(Box<Result<Option<Update>, String>>),
    Downloaded(Result<Vec<u8>, String>),
    Installed(Result<(), String>),
    /// Only seen when the relaunch failed: a successful one never returns.
    RelaunchFailed(String),
}

/// The side effects around an install: the flush before it and the
/// relaunch after it.
pub trait ApplyHost: InstallerHost {
    /// `beforeRelaunch` (App.svelte `flushAppState`: the settings flush,
    /// then the pending store saves; the first error wins).
    fn before_relaunch(&self) -> Result<(), String>;
    /// `relaunch()`; the real one does not return.
    fn relaunch(&self) -> Result<(), String>;
}

/// Runs one command with the real updater. Blocking: call it on a job
/// thread.
pub fn run_command(
    updater: &Updater,
    command: UpdateCommand<Update>,
    host: &dyn ApplyHost,
) -> Option<CommandResult> {
    match command {
        UpdateCommand::Check => Some(CommandResult::Checked(Box::new(
            updater.check().map_err(|e| e.to_string()),
        ))),
        UpdateCommand::Download(update) => Some(CommandResult::Downloaded(
            update.download(|_, _| {}, || {}).map_err(|e| e.to_string()),
        )),
        UpdateCommand::Install { update, bytes } => {
            let result = host
                .before_relaunch()
                .and_then(|()| update.install(&bytes, host).map_err(|e| e.to_string()));
            Some(CommandResult::Installed(result))
        }
        UpdateCommand::Relaunch => host.relaunch().err().map(CommandResult::RelaunchFailed),
    }
}
