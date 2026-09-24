//! Descriptor schema: serde types plus the validation pass that runs before a
//! descriptor is ever executed.
//!
//! Every rule here exists so a broken descriptor fails at load with the field
//! that is wrong and what was expected, instead of a mid-switch panic that
//! leaves a launcher signed out. Unknown fields are rejected too: a typo in a
//! key name would otherwise silently disable the step it was meant to describe.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use super::hooks;

mod template;
mod validate;

pub use self::template::{PathSpec, PathTemplate};
pub use self::validate::INSTALL_DIR;

/// Schema version this build understands. A descriptor declaring anything else
/// is refused rather than interpreted with the wrong meaning.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Platforms allowed to name a native hook. The escape hatch exists for the
/// two clients whose identity discovery cannot be expressed as data (Riot's
/// local HTTPS API, Discord's leveldb scan). Opening it wider would turn the
/// descriptors back into code.
pub const NATIVE_HOOK_ALLOWLIST: &[&str] = &["riot", "discord"];

/// A descriptor that could not be accepted, naming the offending field and
/// what was expected of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptorError {
    /// Where the descriptor came from: a file path, or `embedded:<id>`.
    pub source: String,
    /// Dotted path of the offending field, e.g. `os.windows.state.files[0].live`.
    pub field: String,
    /// What was expected, and what was found instead.
    pub problem: String,
}

impl DescriptorError {
    pub fn new(
        source: impl Into<String>,
        field: impl Into<String>,
        problem: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            field: field.into(),
            problem: problem.into(),
        }
    }
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid platform descriptor {}: field `{}` {}",
            self.source, self.field, self.problem
        )
    }
}

impl std::error::Error for DescriptorError {}

/// Operating systems a descriptor can carry a profile for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

impl Os {
    /// The OS this build runs on, or `None` on a target no descriptor targets.
    pub fn current() -> Option<Self> {
        #[cfg(windows)]
        {
            Some(Os::Windows)
        }
        #[cfg(target_os = "macos")]
        {
            Some(Os::Macos)
        }
        #[cfg(target_os = "linux")]
        {
            Some(Os::Linux)
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            None
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Macos => "macos",
            Os::Linux => "linux",
        }
    }
}

/// One platform, described end to end for every OS it supports.
///
/// A platform may be present on one OS and absent on another without the
/// descriptor being broken: an OS with no profile simply has no service.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    /// Canonical platform id, the key everything else in the app uses.
    pub id: String,
    pub schema_version: u32,
    /// Display name, and the one used in user-facing messages about the
    /// launcher itself ("Could not locate GOG Galaxy executable").
    pub name: String,
    /// Short name used in account-id error messages ("Invalid GOG account ID").
    /// The CLI classifies exit codes off those, so it is spelled out rather
    /// than derived from `name`.
    pub short_name: String,
    pub os: BTreeMap<Os, OsProfile>,
}

/// Everything needed to run one platform on one OS.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OsProfile {
    /// Filesystem and registry areas this descriptor is allowed to touch.
    pub roots: Roots,
    pub detect: Detect,
    #[serde(default)]
    pub executable: Option<Executable>,
    pub identity: Identity,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub close: Close,
    #[serde(default)]
    pub launch: Option<Launch>,
    #[serde(default)]
    pub setup: Setup,
}

/// The sandbox. Every path a descriptor reads or writes as state must sit
/// under one of these, and every registry value under one of these keys.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Roots {
    #[serde(default)]
    pub files: Vec<PathTemplate>,
    #[serde(default)]
    pub registry: Vec<RegistryRoot>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryRoot {
    pub root: RegistryHive,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum RegistryHive {
    #[serde(rename = "HKCU")]
    CurrentUser,
    #[serde(rename = "HKLM")]
    LocalMachine,
}

impl RegistryHive {
    pub fn as_str(&self) -> &'static str {
        match self {
            RegistryHive::CurrentUser => "HKCU",
            RegistryHive::LocalMachine => "HKLM",
        }
    }
}

/// How to tell whether the launcher is present on this machine.
///
/// Any satisfied condition means installed. An empty `Detect` is refused: a
/// platform that can never report itself installed is a mistake, not a choice.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Detect {
    #[serde(default)]
    pub executable_resolves: bool,
    #[serde(default)]
    pub path_exists: Vec<PathTemplate>,
}

/// How to find the launcher binary. Candidates are tried in order; the
/// user's path override, when set, always wins.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Executable {
    /// Binary name, appended when a candidate resolves to a directory.
    pub file_name: String,
    #[serde(default)]
    pub candidates: Vec<ExecutableCandidate>,
    /// Sub-directories tried inside a candidate that resolved to a directory,
    /// before the directory itself. Launchers shipping per-architecture
    /// binaries (`Binaries/Win64`, `Binaries/Win32`) need this and nothing else.
    #[serde(default)]
    pub relative_probes: Vec<String>,
    /// Filter shown by the "select executable" dialog.
    #[serde(default = "default_exe_filter")]
    pub select_filter: String,
}

fn default_exe_filter() -> String {
    "Executable files (*.exe)|*.exe|All files (*.*)|*.*".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// `rename_all` only renames the variants: without `rename_all_fields` the
// fields inside a variant keep their snake_case Rust names and a camelCase
// descriptor is rejected at load.
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExecutableCandidate {
    /// A literal location, resolved through [`PathTemplate`].
    Path { template: PathTemplate },
    /// A registry value holding either the install directory or the binary.
    Registry {
        root: RegistryHive,
        key: String,
        value: String,
    },
    /// The install directory recorded by an uninstall entry, found by its
    /// display name. Launchers that register no path of their own still
    /// register one here.
    UninstallEntry {
        display_name: String,
        #[serde(default = "default_install_location")]
        value: String,
    },
}

fn default_install_location() -> String {
    "InstallLocation".to_string()
}

/// Where the account id comes from and what it is allowed to look like.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub source: IdentitySource,
    pub format: IdFormat,
    /// Which account counts as "currently signed in".
    pub current: CurrentSource,
    /// Extra places accounts leave a trace, so accounts added outside accshift
    /// still show up. The signed-in account comes from `source`; these only
    /// widen the list.
    #[serde(default)]
    pub discovery: Vec<Discovery>,
    /// Remember forgotten ids so `discovery` cannot resurrect them. Only
    /// meaningful with a `discovery` entry, and only for platforms whose config
    /// section carries the list.
    #[serde(default)]
    pub blocklist_on_forget: bool,
}

/// A place account ids can be enumerated from.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Discovery {
    /// Entry names of a directory, each one an account id once the declared
    /// prefixes and the extension are stripped.
    DirectoryEntries {
        path: PathTemplate,
        #[serde(default)]
        entries: EntryKind,
        #[serde(default)]
        strip_prefixes: Vec<String>,
        #[serde(default)]
        strip_extension: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    #[default]
    Any,
    Directories,
    Files,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum IdentitySource {
    /// The launcher writes the id where we can read it.
    Registry {
        root: RegistryHive,
        key: String,
        value: String,
    },
    /// The launcher exposes no id: we mint one when the account is captured.
    Synthetic,
    /// The id sits in a log the launcher appends to. The tail is read with
    /// shared access, most recent line first, since launchers keep the file
    /// open and the log can be megabytes long.
    LogTail {
        path: PathTemplate,
        #[serde(default = "default_tail_bytes")]
        tail_bytes: u64,
        /// Only lines holding this text are considered.
        line_contains: String,
        /// Text the id follows directly, tried first.
        #[serde(default)]
        prefix: String,
        /// Fallback: any id in the line preceded within ten characters by this
        /// word. Empty disables the fallback.
        #[serde(default)]
        near_word: String,
    },
    /// Discovery needs code the descriptor cannot express. `name` picks one of
    /// the compiled hooks; `paths` supplies the locations it works on, so the
    /// hook stays inside the sandbox instead of building paths of its own.
    NativeHook {
        name: String,
        #[serde(default)]
        paths: BTreeMap<String, PathTemplate>,
    },
}

fn default_tail_bytes() -> u64 {
    64 * 1024
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdFormat {
    pub charset: Charset,
    pub max_length: usize,
    /// Shortest acceptable id. Ids of a fixed width set this to `maxLength`,
    /// which is what makes a truncated id a rejection instead of a new account.
    #[serde(default = "default_min_length")]
    pub min_length: usize,
    /// Fold the id to lowercase before it is compared or stored, for launchers
    /// that write the same id in either case.
    #[serde(default)]
    pub lowercase: bool,
    /// Replaces "Invalid <shortName> account ID: <id>" when the platform's own
    /// wording is load-bearing. The CLI maps exit codes off these strings.
    #[serde(default)]
    pub invalid_message: String,
}

fn default_min_length() -> usize {
    1
}

/// Account ids are joined into snapshot paths, so the charset is a path
/// traversal guard first and a sanity check second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Charset {
    Digits,
    Hex,
    Alphanumeric,
    /// The canonical 8-4-4-4-12 form, dashes included.
    Uuid,
}

impl Charset {
    /// An id of `length` characters this charset accepts.
    ///
    /// Used to plan a switch for a platform that has no accounts yet, so the
    /// preview can show which files and keys a real switch would touch without
    /// the user having to sign in first. A UUID ignores `length`, since only
    /// the canonical 8-4-4-4-12 form is accepted.
    pub fn sample(&self, length: usize) -> String {
        match self {
            Charset::Uuid => "00000000-0000-0000-0000-000000000000".to_string(),
            Charset::Digits => "0".repeat(length),
            Charset::Hex | Charset::Alphanumeric => "a".repeat(length),
        }
    }

    pub fn accepts(&self, value: &str) -> bool {
        if let Charset::Uuid = self {
            return value.len() == 36
                && value.chars().enumerate().all(|(index, c)| match index {
                    8 | 13 | 18 | 23 => c == '-',
                    _ => c.is_ascii_hexdigit(),
                });
        }
        value.chars().all(|c| match self {
            Charset::Digits => c.is_ascii_digit(),
            Charset::Hex => c.is_ascii_hexdigit(),
            Charset::Alphanumeric => c.is_ascii_alphanumeric(),
            Charset::Uuid => unreachable!("handled above"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CurrentSource {
    /// Read live from the identity source on every call.
    Identity,
    /// Remembered by us, because the launcher keeps no readable marker.
    Config,
}

/// The per-account material captured, restored and cleared.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    #[serde(default)]
    pub files: Vec<FileItem>,
    #[serde(default)]
    pub directories: Vec<DirItem>,
    #[serde(default)]
    pub registry_values: Vec<RegistryItem>,
    /// Directories wiped once the incoming session is in place, and never
    /// captured. A launcher cache keyed to the outgoing account makes the next
    /// sign-in show the wrong name, or fail outright.
    #[serde(default)]
    pub caches: Vec<PathTemplate>,
    /// Conditions that must all hold for a capture to run at all. A session the
    /// user signed out of by hand would otherwise be captured as an empty
    /// snapshot, overwriting the good one taken while they were signed in.
    #[serde(default)]
    pub capture_when: Vec<Condition>,
}

impl State {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.directories.is_empty()
            && self.registry_values.is_empty()
            && self.caches.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileItem {
    /// Live location on disk.
    pub live: PathSpec,
    /// File name inside the account's snapshot directory.
    pub snapshot: String,
    /// Deleted when a setup flow clears the live session.
    #[serde(default)]
    pub clear_on_setup: bool,
    /// Its presence in a snapshot means the account has one.
    #[serde(default)]
    pub snapshot_marker: bool,
    /// Delete the live file before writing it back. Needed for files the OS
    /// marks hidden or system, which cannot be truncated in place on Windows.
    #[serde(default)]
    pub remove_live_before_restore: bool,
    /// Drop a stale snapshot when the live file is gone at capture time, so a
    /// later restore cannot resurrect another account's file.
    #[serde(default = "default_true")]
    pub clear_snapshot_when_source_missing: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirItem {
    pub live: PathSpec,
    /// Directory name inside the account's snapshot directory.
    pub snapshot: String,
    #[serde(default)]
    pub clear_on_setup: bool,
    #[serde(default)]
    pub snapshot_marker: bool,
    /// Entry names skipped at every depth (lock files and the like).
    #[serde(default)]
    pub ignored_names: Vec<String>,
    #[serde(default)]
    pub follow_symlinks: bool,
    /// Drop a stale snapshot when the live directory is gone at capture time,
    /// so a later restore cannot resurrect another account's session.
    ///
    /// Left off where a missing directory means the launcher has not written it
    /// yet rather than the account signing out: the capture would otherwise
    /// throw away the only copy the user has, and an empty copy is worse than a
    /// slightly old one.
    #[serde(default = "default_true")]
    pub clear_snapshot_when_source_missing: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryItem {
    pub root: RegistryHive,
    pub key: String,
    pub value: String,
    /// File name inside the account's snapshot directory holding the captured
    /// value, encrypted like every other snapshot file.
    pub snapshot: String,
    #[serde(default)]
    pub clear_on_setup: bool,
    #[serde(default)]
    pub snapshot_marker: bool,
    /// Drop a stale snapshot when the value is gone at capture time. Left off
    /// where the value is the account id itself: a transient read failure would
    /// otherwise erase the only copy of it.
    #[serde(default = "default_true")]
    pub clear_snapshot_when_source_missing: bool,
}

/// How the launcher is shut down before its files are touched.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Close {
    #[serde(default)]
    pub processes: Vec<String>,
    /// How long to wait for each process to exit.
    #[serde(default = "default_quit_timeout_ms")]
    pub timeout_ms: u32,
    /// Extra wait after the last exit, so exit-time flushes land.
    #[serde(default = "default_settle_ms")]
    pub settle_ms: u64,
    /// Close the launcher before the outgoing account is captured, instead of
    /// after. Clients that keep their session in memory and only write it out
    /// on exit are captured empty otherwise.
    #[serde(default)]
    pub before_capture: bool,
}

impl Default for Close {
    fn default() -> Self {
        Self {
            processes: Vec::new(),
            timeout_ms: default_quit_timeout_ms(),
            settle_ms: default_settle_ms(),
            before_capture: false,
        }
    }
}

fn default_quit_timeout_ms() -> u32 {
    8000
}

fn default_settle_ms() -> u64 {
    500
}

// No `Default` on purpose: `workingDirectoryIsInstallDir` defaults to true
// through serde, and a derived Default would silently disagree with the JSON.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Launch {
    #[serde(default)]
    pub args: Vec<String>,
    /// Pass `args` only when the resolved binary carries this name, empty to
    /// always pass them. A launcher reached through an updater stub needs a
    /// hand-off argument the real client does not understand, and the user's
    /// path override may well point straight at the real client.
    #[serde(default)]
    pub args_only_for: String,
    /// Start the process in the directory holding the binary. Some launchers
    /// resolve their own resources relative to it.
    #[serde(default = "default_true")]
    pub working_directory_is_install_dir: bool,
}

impl Launch {
    /// The arguments to pass to the binary that was actually resolved.
    pub fn args_for(&self, executable: &std::path::Path) -> &[String] {
        if self.args_only_for.is_empty() {
            return &self.args;
        }
        let matches = executable
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(&self.args_only_for));
        if matches {
            &self.args
        } else {
            &[]
        }
    }
}

/// The "sign in to add an account" flow.
///
/// `trigger` is polled while the user signs in. Once every trigger holds, the
/// launcher is closed so it flushes, and `confirm` is re-checked before
/// anything is captured. A failed confirm keeps the flow waiting instead of
/// storing a snapshot with no session in it.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Setup {
    #[serde(default)]
    pub trigger: Vec<Condition>,
    #[serde(default)]
    pub confirm: Vec<Condition>,
    /// Report the account id as its display name. Platforms that expose no
    /// readable name leave this off and let the user label the account.
    #[serde(default)]
    pub display_name_from_id: bool,
    /// Appended to "No auth snapshot found for account X." so the message says
    /// what the user should do about it, which differs per platform: some
    /// accounts appear by signing in, others only through this flow.
    #[serde(default)]
    pub missing_snapshot_hint: String,
    /// When the flow starts on a session nothing tracks yet, take that session
    /// as the new account instead of wiping it and asking the user to sign in
    /// again. Without this, adding a first account signs the user out of the
    /// one they were already using.
    #[serde(default)]
    pub adopt_signed_in: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Condition {
    /// The identity source reports an id that was not already known.
    NewIdentity,
    /// The identity source reports any id at all.
    IdentityPresent,
    /// A file with content, or a directory holding one somewhere below it.
    PathNonEmpty {
        path: PathSpec,
        #[serde(default)]
        recursive: bool,
    },
    /// A non-empty file written within the window. A stale mtime means the
    /// launcher never flushed the new session.
    PathFresh { path: PathSpec, window_ms: u64 },
    /// Holds as soon as one of the nested conditions does. Launchers that write
    /// a paired credential set may refresh either half on a sign-in.
    AnyOf { conditions: Vec<Condition> },
    /// Holds once the flow has been running this long. Clients that write their
    /// session store the instant they open need it: without a floor, the write
    /// that happens at launch reads as a sign-in.
    SinceStart { ms: u64 },
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests;
