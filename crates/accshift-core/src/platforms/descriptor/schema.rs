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

/// Schema version this build understands. A descriptor declaring anything else
/// is refused rather than interpreted with the wrong meaning.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Platforms allowed to name a `nativeHook`. The escape hatch exists for the
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
    /// Name of a compiled-in step this descriptor delegates to. Restricted to
    /// [`NATIVE_HOOK_ALLOWLIST`].
    #[serde(default)]
    pub native_hook: Option<String>,
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
}

/// Where the account id comes from and what it is allowed to look like.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub source: IdentitySource,
    pub format: IdFormat,
    /// Which account counts as "currently signed in".
    pub current: CurrentSource,
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
    /// Discovery needs code the descriptor cannot express.
    NativeHook { name: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdFormat {
    pub charset: Charset,
    pub max_length: usize,
}

/// Account ids are joined into snapshot paths, so the charset is a path
/// traversal guard first and a sanity check second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Charset {
    Digits,
    Hex,
    Alphanumeric,
}

impl Charset {
    pub fn accepts(&self, value: &str) -> bool {
        value.chars().all(|c| match self {
            Charset::Digits => c.is_ascii_digit(),
            Charset::Hex => c.is_ascii_hexdigit(),
            Charset::Alphanumeric => c.is_ascii_alphanumeric(),
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
}

impl State {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.directories.is_empty() && self.registry_values.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileItem {
    /// Live location on disk.
    pub live: PathTemplate,
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
    pub live: PathTemplate,
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
}

impl Default for Close {
    fn default() -> Self {
        Self {
            processes: Vec::new(),
            timeout_ms: default_quit_timeout_ms(),
            settle_ms: default_settle_ms(),
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
    /// Start the process in the directory holding the binary. Some launchers
    /// resolve their own resources relative to it.
    #[serde(default = "default_true")]
    pub working_directory_is_install_dir: bool,
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
        path: PathTemplate,
        #[serde(default)]
        recursive: bool,
    },
    /// A non-empty file written within the window. A stale mtime means the
    /// launcher never flushed the new session.
    PathFresh { path: PathTemplate, window_ms: u64 },
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Path templates
// ---------------------------------------------------------------------------

/// A location written with `${...}` placeholders, resolved at run time.
///
/// `${installDir}` is the directory holding the launcher binary; every other
/// name is an environment variable. Both separators are accepted and
/// normalised for the running OS.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PathTemplate(String);

impl PathTemplate {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Placeholder names used by this template, in order of appearance.
    pub fn placeholders(&self) -> Vec<String> {
        let mut names = Vec::new();
        let bytes = self.0.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'$' && bytes[i + 1] == b'{' {
                if let Some(end) = self.0[i + 2..].find('}') {
                    names.push(self.0[i + 2..i + 2 + end].to_string());
                    i = i + 2 + end + 1;
                    continue;
                }
            }
            i += 1;
        }
        names
    }

    /// Checks the template can be resolved at all: balanced placeholders,
    /// usable names, and no way to climb out of the sandbox.
    pub fn validate(&self, source: &str, field: &str) -> Result<(), DescriptorError> {
        if self.0.trim().is_empty() {
            return Err(DescriptorError::new(
                source,
                field,
                "expected a non-empty path template, found an empty string",
            ));
        }

        let mut rest = self.0.as_str();
        while let Some(start) = rest.find("${") {
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                return Err(DescriptorError::new(
                    source,
                    field,
                    format!(
                        "expected every `${{` to be closed by `}}`, found `{}`",
                        self.0
                    ),
                ));
            };
            let name = &after[..end];
            if name.is_empty() {
                return Err(DescriptorError::new(
                    source,
                    field,
                    "expected a name inside `${}`, found an empty placeholder",
                ));
            }
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')'))
            {
                return Err(DescriptorError::new(
                    source,
                    field,
                    format!(
                        "expected a placeholder name of letters, digits, `_`, `(` or `)`, found `{name}`"
                    ),
                ));
            }
            rest = &after[end + 1..];
        }

        for part in self.0.split(['/', '\\']) {
            if part == ".." {
                return Err(DescriptorError::new(
                    source,
                    field,
                    format!(
                        "expected a path that stays inside its roots, found a `..` segment in `{}`",
                        self.0
                    ),
                ));
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

impl Descriptor {
    /// Parse and validate in one step, so no unvalidated descriptor exists.
    pub fn parse(source: &str, json: &str) -> Result<Self, DescriptorError> {
        let descriptor: Descriptor = serde_json::from_str(json).map_err(|e| {
            DescriptorError::new(
                source,
                format!("line {} column {}", e.line(), e.column()),
                format!("could not be read: {e}"),
            )
        })?;
        descriptor.validate(source)?;
        Ok(descriptor)
    }

    /// The profile for the OS this build runs on, if the platform supports it.
    pub fn current_profile(&self) -> Option<&OsProfile> {
        Os::current().and_then(|os| self.os.get(&os))
    }

    pub fn validate(&self, source: &str) -> Result<(), DescriptorError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(DescriptorError::new(
                source,
                "schemaVersion",
                format!(
                    "expected {CURRENT_SCHEMA_VERSION}, found {}",
                    self.schema_version
                ),
            ));
        }
        validate_id(source, "id", &self.id)?;
        for (field, value) in [("name", &self.name), ("shortName", &self.short_name)] {
            if value.trim().is_empty() {
                return Err(DescriptorError::new(
                    source,
                    field,
                    "expected a non-empty display name, found an empty string",
                ));
            }
        }
        if self.os.is_empty() {
            return Err(DescriptorError::new(
                source,
                "os",
                "expected at least one of `windows`, `macos` or `linux`, found none",
            ));
        }
        for (os, profile) in &self.os {
            profile.validate(source, &format!("os.{}", os.as_str()), &self.id)?;
        }
        Ok(())
    }
}

fn validate_id(source: &str, field: &str, id: &str) -> Result<(), DescriptorError> {
    let ok = !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!("expected 1 to 32 lowercase letters, digits or `-`, found `{id}`"),
        ))
    }
}

impl OsProfile {
    fn validate(
        &self,
        source: &str,
        field: &str,
        platform_id: &str,
    ) -> Result<(), DescriptorError> {
        if let Some(hook) = &self.native_hook {
            if !NATIVE_HOOK_ALLOWLIST.contains(&platform_id) {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.nativeHook"),
                    format!(
                        "expected no native hook: only {} may name one, found `{hook}` on `{platform_id}`",
                        NATIVE_HOOK_ALLOWLIST.join(", ")
                    ),
                ));
            }
            if hook.trim().is_empty() {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.nativeHook"),
                    "expected a hook name, found an empty string",
                ));
            }
        }

        for (index, root) in self.roots.files.iter().enumerate() {
            root.validate(source, &format!("{field}.roots.files[{index}]"))?;
        }
        for (index, root) in self.roots.registry.iter().enumerate() {
            validate_registry_key(
                source,
                &format!("{field}.roots.registry[{index}].key"),
                &root.key,
            )?;
        }

        if !self.detect.executable_resolves && self.detect.path_exists.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.detect"),
                "expected `executableResolves` or at least one `pathExists` entry, found neither",
            ));
        }
        for (index, path) in self.detect.path_exists.iter().enumerate() {
            path.validate(source, &format!("{field}.detect.pathExists[{index}]"))?;
        }

        match &self.executable {
            Some(executable) => executable.validate(source, &format!("{field}.executable"))?,
            None => {
                if self.detect.executable_resolves {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.executable"),
                        "expected an executable block, found none while `detect.executableResolves` is set",
                    ));
                }
                if self.launch.is_some() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.executable"),
                        "expected an executable block, found none while `launch` is set",
                    ));
                }
            }
        }

        self.identity
            .validate(source, &format!("{field}.identity"), &self.roots)?;
        self.validate_state(source, field)?;

        for (index, process) in self.close.processes.iter().enumerate() {
            if process.trim().is_empty() || process.contains(['/', '\\']) {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.close.processes[{index}]"),
                    format!("expected a bare process name, found `{process}`"),
                ));
            }
        }

        for (index, condition) in self
            .setup
            .trigger
            .iter()
            .chain(self.setup.confirm.iter())
            .enumerate()
        {
            condition.validate(source, &format!("{field}.setup[{index}]"), &self.roots)?;
        }

        if !self.state.is_empty() && self.close.processes.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.close.processes"),
                "expected the processes to close before session files are replaced, found none",
            ));
        }

        Ok(())
    }

    fn validate_state(&self, source: &str, field: &str) -> Result<(), DescriptorError> {
        let mut snapshot_names: Vec<&str> = Vec::new();

        for (index, item) in self.state.files.iter().enumerate() {
            let at = format!("{field}.state.files[{index}]");
            item.live.validate(source, &format!("{at}.live"))?;
            validate_in_file_roots(source, &format!("{at}.live"), &item.live, &self.roots)?;
            validate_snapshot_name(source, &format!("{at}.snapshot"), &item.snapshot)?;
            snapshot_names.push(&item.snapshot);
        }
        for (index, item) in self.state.directories.iter().enumerate() {
            let at = format!("{field}.state.directories[{index}]");
            item.live.validate(source, &format!("{at}.live"))?;
            validate_in_file_roots(source, &format!("{at}.live"), &item.live, &self.roots)?;
            validate_snapshot_name(source, &format!("{at}.snapshot"), &item.snapshot)?;
            snapshot_names.push(&item.snapshot);
        }
        for (index, item) in self.state.registry_values.iter().enumerate() {
            let at = format!("{field}.state.registryValues[{index}]");
            validate_registry_key(source, &format!("{at}.key"), &item.key)?;
            validate_in_registry_roots(
                source,
                &format!("{at}.key"),
                item.root,
                &item.key,
                &self.roots,
            )?;
            if item.value.trim().is_empty() {
                return Err(DescriptorError::new(
                    source,
                    format!("{at}.value"),
                    "expected a registry value name, found an empty string",
                ));
            }
            validate_snapshot_name(source, &format!("{at}.snapshot"), &item.snapshot)?;
            snapshot_names.push(&item.snapshot);
        }

        // Two entries writing the same snapshot name would silently overwrite
        // each other, and the loser would restore the winner's bytes.
        for (index, name) in snapshot_names.iter().enumerate() {
            if snapshot_names[..index]
                .iter()
                .any(|earlier| earlier.eq_ignore_ascii_case(name))
            {
                return Err(DescriptorError::new(
                    source,
                    format!("{field}.state"),
                    format!("expected every `snapshot` name to be unique, found `{name}` twice"),
                ));
            }
        }

        Ok(())
    }
}

impl Executable {
    fn validate(&self, source: &str, field: &str) -> Result<(), DescriptorError> {
        if self.file_name.trim().is_empty() || self.file_name.contains(['/', '\\']) {
            return Err(DescriptorError::new(
                source,
                format!("{field}.fileName"),
                format!("expected a bare binary name, found `{}`", self.file_name),
            ));
        }
        if self.candidates.is_empty() {
            return Err(DescriptorError::new(
                source,
                format!("{field}.candidates"),
                "expected at least one place to look for the binary, found none",
            ));
        }
        for (index, candidate) in self.candidates.iter().enumerate() {
            let at = format!("{field}.candidates[{index}]");
            match candidate {
                ExecutableCandidate::Path { template } => {
                    template.validate(source, &format!("{at}.template"))?;
                    if template.placeholders().iter().any(|p| p == INSTALL_DIR) {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.template"),
                            "expected a template that does not use `${installDir}`: the install directory is what this candidate resolves",
                        ));
                    }
                }
                ExecutableCandidate::Registry { key, value, .. } => {
                    validate_registry_key(source, &format!("{at}.key"), key)?;
                    if value.trim().is_empty() {
                        return Err(DescriptorError::new(
                            source,
                            format!("{at}.value"),
                            "expected a registry value name, found an empty string",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Identity {
    fn validate(&self, source: &str, field: &str, roots: &Roots) -> Result<(), DescriptorError> {
        if self.format.max_length == 0 || self.format.max_length > 256 {
            return Err(DescriptorError::new(
                source,
                format!("{field}.format.maxLength"),
                format!("expected 1 to 256, found {}", self.format.max_length),
            ));
        }
        match &self.source {
            IdentitySource::Registry { root, key, value } => {
                validate_registry_key(source, &format!("{field}.source.key"), key)?;
                validate_in_registry_roots(
                    source,
                    &format!("{field}.source.key"),
                    *root,
                    key,
                    roots,
                )?;
                if value.trim().is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.source.value"),
                        "expected a registry value name, found an empty string",
                    ));
                }
            }
            IdentitySource::Synthetic => {
                if self.current == CurrentSource::Identity {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.current"),
                        "expected `config`: a synthetic id has no live source to read the current account from, found `identity`",
                    ));
                }
            }
            IdentitySource::NativeHook { name } => {
                if name.trim().is_empty() {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.source.name"),
                        "expected a hook name, found an empty string",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Condition {
    fn validate(&self, source: &str, field: &str, roots: &Roots) -> Result<(), DescriptorError> {
        match self {
            Condition::NewIdentity | Condition::IdentityPresent => Ok(()),
            Condition::PathNonEmpty { path, .. } => {
                path.validate(source, &format!("{field}.path"))?;
                validate_in_file_roots(source, &format!("{field}.path"), path, roots)
            }
            Condition::PathFresh { path, window_ms } => {
                path.validate(source, &format!("{field}.path"))?;
                validate_in_file_roots(source, &format!("{field}.path"), path, roots)?;
                if *window_ms == 0 {
                    return Err(DescriptorError::new(
                        source,
                        format!("{field}.windowMs"),
                        "expected a freshness window above zero, found 0",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Name of the placeholder standing for the launcher's install directory.
pub const INSTALL_DIR: &str = "installDir";

fn validate_snapshot_name(source: &str, field: &str, name: &str) -> Result<(), DescriptorError> {
    let ok = !name.trim().is_empty()
        && name == name.trim()
        && !name.contains(['/', '\\'])
        && name != "."
        && name != "..";
    if ok {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!("expected a bare file or directory name, found `{name}`"),
        ))
    }
}

fn validate_registry_key(source: &str, field: &str, key: &str) -> Result<(), DescriptorError> {
    let ok = !key.trim().is_empty()
        && !key.contains('/')
        && !key.split('\\').any(|part| part == ".." || part.is_empty());
    if ok {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!("expected a backslash-separated registry key, found `{key}`"),
        ))
    }
}

/// The sandbox check the loader can make ahead of time: a template whose
/// literal text does not start with a declared root can never resolve inside
/// one, whatever the environment holds.
fn validate_in_file_roots(
    source: &str,
    field: &str,
    path: &PathTemplate,
    roots: &Roots,
) -> Result<(), DescriptorError> {
    if roots.files.is_empty() {
        return Err(DescriptorError::new(
            source,
            field,
            "expected at least one entry in `roots.files`, found none while the descriptor reads or writes files",
        ));
    }
    let candidate = normalise_template_text(path.as_str());
    let covered = roots.files.iter().any(|root| {
        let root_text = normalise_template_text(root.as_str());
        candidate == root_text
            || candidate.starts_with(&format!("{}/", root_text.trim_end_matches('/')))
    });
    if covered {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!(
                "expected a path under one of the declared roots ({}), found `{}`",
                roots
                    .files
                    .iter()
                    .map(|r| r.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                path.as_str()
            ),
        ))
    }
}

fn validate_in_registry_roots(
    source: &str,
    field: &str,
    hive: RegistryHive,
    key: &str,
    roots: &Roots,
) -> Result<(), DescriptorError> {
    let covered = roots.registry.iter().any(|root| {
        root.root == hive
            && (key.eq_ignore_ascii_case(&root.key)
                || key.to_ascii_lowercase().starts_with(&format!(
                    "{}\\",
                    root.key.trim_end_matches('\\').to_ascii_lowercase()
                )))
    });
    if covered {
        Ok(())
    } else {
        Err(DescriptorError::new(
            source,
            field,
            format!(
                "expected a key under one of the declared registry roots ({}), found `{}\\{key}`",
                roots
                    .registry
                    .iter()
                    .map(|r| format!("{}\\{}", r.root.as_str(), r.key))
                    .collect::<Vec<_>>()
                    .join(", "),
                hive.as_str()
            ),
        ))
    }
}

/// Compare templates on their text, separators and case folded, so a root
/// written with `\` covers a path written with `/`.
fn normalise_template_text(text: &str) -> String {
    text.replace('\\', "/").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
      "id": "demo",
      "schemaVersion": 1,
      "name": "Demo Launcher",
      "shortName": "Demo",
      "os": {
        "windows": {
          "roots": {
            "files": ["${LOCALAPPDATA}/Demo"],
            "registry": [{ "root": "HKCU", "key": "Software\\Demo" }]
          },
          "detect": { "executableResolves": true },
          "executable": {
            "fileName": "Demo.exe",
            "candidates": [{ "kind": "path", "template": "${ProgramFiles}/Demo/Demo.exe" }]
          },
          "identity": {
            "source": { "kind": "registry", "root": "HKCU", "key": "Software\\Demo", "value": "userId" },
            "format": { "charset": "digits", "maxLength": 32 },
            "current": "identity"
          },
          "state": {
            "files": [{ "live": "${LOCALAPPDATA}/Demo/session.json", "snapshot": "session.json" }]
          },
          "close": { "processes": ["Demo.exe"] },
          "launch": {}
        }
      }
    }"#;

    fn with_windows(
        mutate: impl Fn(&mut serde_json::Value),
    ) -> Result<Descriptor, DescriptorError> {
        let mut value: serde_json::Value = serde_json::from_str(MINIMAL).unwrap();
        mutate(&mut value);
        Descriptor::parse("test", &value.to_string())
    }

    #[test]
    fn minimal_descriptor_is_accepted() {
        let descriptor = Descriptor::parse("test", MINIMAL).unwrap();
        assert_eq!(descriptor.id, "demo");
        assert!(descriptor.os.contains_key(&Os::Windows));
    }

    #[test]
    fn wrong_schema_version_names_the_field_and_the_expectation() {
        let err = with_windows(|v| v["schemaVersion"] = serde_json::json!(2)).unwrap_err();
        assert_eq!(err.field, "schemaVersion");
        assert_eq!(err.problem, "expected 1, found 2");
        assert_eq!(
            err.to_string(),
            "Invalid platform descriptor test: field `schemaVersion` expected 1, found 2"
        );
    }

    #[test]
    fn unknown_field_is_refused_rather_than_ignored() {
        // A typo in a key would otherwise quietly drop the step it describes.
        let err =
            with_windows(|v| v["os"]["windows"]["lunch"] = serde_json::json!({})).unwrap_err();
        assert!(
            err.problem.contains("unknown field `lunch`"),
            "{}",
            err.problem
        );
    }

    #[test]
    fn state_path_outside_the_declared_roots_is_refused() {
        let err = with_windows(|v| {
            v["os"]["windows"]["state"]["files"][0]["live"] =
                serde_json::json!("${APPDATA}/Elsewhere/session.json");
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.state.files[0].live");
        assert!(err.problem.contains("declared roots"), "{}", err.problem);
    }

    #[test]
    fn parent_segments_are_refused_in_templates() {
        let err = with_windows(|v| {
            v["os"]["windows"]["roots"]["files"] = serde_json::json!(["${LOCALAPPDATA}"]);
            v["os"]["windows"]["state"]["files"][0]["live"] =
                serde_json::json!("${LOCALAPPDATA}/../Roaming/session.json");
        })
        .unwrap_err();
        assert!(err.problem.contains("`..`"), "{}", err.problem);
    }

    #[test]
    fn registry_value_outside_the_declared_roots_is_refused() {
        let err = with_windows(|v| {
            v["os"]["windows"]["state"]["registryValues"] = serde_json::json!([{
                "root": "HKLM",
                "key": "SOFTWARE\\Elsewhere",
                "value": "token",
                "snapshot": "token.txt"
            }]);
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.state.registryValues[0].key");
        assert!(
            err.problem.contains("declared registry roots"),
            "{}",
            err.problem
        );
    }

    #[test]
    fn duplicate_snapshot_names_are_refused() {
        let err = with_windows(|v| {
            v["os"]["windows"]["state"]["directories"] = serde_json::json!([{
                "live": "${LOCALAPPDATA}/Demo/cache",
                "snapshot": "session.json"
            }]);
        })
        .unwrap_err();
        assert!(err.problem.contains("twice"), "{}", err.problem);
    }

    #[test]
    fn detect_with_no_condition_is_refused() {
        let err = with_windows(|v| {
            v["os"]["windows"]["detect"] = serde_json::json!({});
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.detect");
    }

    #[test]
    fn native_hook_is_refused_outside_the_allowlist() {
        let err = with_windows(|v| {
            v["os"]["windows"]["nativeHook"] = serde_json::json!("scan_leveldb");
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.nativeHook");
        assert!(err.problem.contains("riot, discord"), "{}", err.problem);
    }

    #[test]
    fn native_hook_is_accepted_for_the_two_platforms_entitled_to_one() {
        let descriptor = with_windows(|v| {
            v["id"] = serde_json::json!("discord");
            v["os"]["windows"]["nativeHook"] = serde_json::json!("scan_leveldb");
        })
        .unwrap();
        assert_eq!(
            descriptor.os[&Os::Windows].native_hook.as_deref(),
            Some("scan_leveldb")
        );
    }

    #[test]
    fn synthetic_identity_cannot_claim_a_live_current_account() {
        let err = with_windows(|v| {
            v["os"]["windows"]["identity"]["source"] = serde_json::json!({ "kind": "synthetic" });
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.identity.current");
    }

    #[test]
    fn state_without_a_process_to_close_is_refused() {
        // Replacing session files under a running launcher loses them when it
        // writes its own copy back on exit.
        let err = with_windows(|v| {
            v["os"]["windows"]["close"] = serde_json::json!({ "processes": [] });
        })
        .unwrap_err();
        assert_eq!(err.field, "os.windows.close.processes");
    }

    #[test]
    fn unbalanced_placeholder_is_refused() {
        let err = with_windows(|v| {
            v["os"]["windows"]["detect"] =
                serde_json::json!({ "pathExists": ["${LOCALAPPDATA/Demo"] });
        })
        .unwrap_err();
        assert!(err.problem.contains("closed by"), "{}", err.problem);
    }

    #[test]
    fn placeholders_are_listed_in_order() {
        let template = PathTemplate::new("${LOCALAPPDATA}/Demo/${installDir}/x");
        assert_eq!(template.placeholders(), vec!["LOCALAPPDATA", "installDir"]);
    }

    #[test]
    fn charset_guards_the_account_id() {
        assert!(Charset::Digits.accepts("12345"));
        assert!(!Charset::Digits.accepts("12a45"));
        assert!(Charset::Alphanumeric.accepts("a3f0c2d1"));
        assert!(!Charset::Alphanumeric.accepts("a3f0-c2d1"));
        assert!(!Charset::Alphanumeric.accepts("../evil"));
    }

    #[test]
    fn os_profiles_may_be_partial_without_breaking_the_descriptor() {
        // A platform present on Windows only is not a broken descriptor.
        let descriptor = Descriptor::parse("test", MINIMAL).unwrap();
        assert!(!descriptor.os.contains_key(&Os::Linux));
        assert_eq!(descriptor.os.len(), 1);
    }
}
