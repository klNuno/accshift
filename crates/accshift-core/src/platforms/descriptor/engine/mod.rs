//! The generic engine: one implementation of [`PlatformService`] that executes
//! whatever a descriptor says.
//!
//! Everything the hand-written platform modules had in common lives here once:
//! resolve the launcher, read the account id, copy the live session into an
//! encrypted per-account snapshot, copy one back, close the launcher, start it
//! again. What differs between platforms is data, not code.
//!
//! Every path this file touches comes from [`Runtime::path`], which resolves a
//! template and checks it against the descriptor's roots. There is no other
//! way to obtain one, so a step added later cannot skip the sandbox.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::config::{self, AppConfig};
use crate::error::PlatformError;
use crate::platforms::setup_jobs::{SetupJobs, DEFAULT_SETUP_TTL_MS};
use crate::platforms::{
    log_platform_error, log_platform_info, make_setup_status, now_unix_ms, redact_id,
    PlatformService, SetupStatus,
};
use crate::snapshot_crypto::{
    self, decrypted_copy_file, delete_encrypted_file_secret, encrypted_copy_file, free_dir_secrets,
    read_decrypted_bytes, write_encrypted_bytes, DirCopyOptions,
};
use crate::{AppContext, AppCtx};

use super::config_bridge;
use super::hooks::{self, HookContext, HookIdentity};
use super::paths::{PathResolver, Sandbox};
use super::plan::{DryRunPlan, PlanAction, PlanStep, PlanTargetKind};
use super::reg;
use super::schema::{
    Condition, CurrentSource, Descriptor, Discovery, EntryKind, Executable, ExecutableCandidate,
    IdentitySource, OsProfile, PathSpec, PathTemplate, RegistryItem, INSTALL_DIR,
};

mod dry_run;
mod executable;
mod fs_probe;
mod identity;
mod operations;
mod reads;
mod restore;
mod snapshot;

#[allow(unused_imports)]
use self::fs_probe::*;
#[allow(unused_imports)]
use self::restore::*;

/// Where a descriptor came from. Shipped descriptors are read-only; a user
/// descriptor lives in the data folder and can be edited or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptorOrigin {
    Embedded,
    User(PathBuf),
}

/// One account as the frontend already expects it, for every descriptor-driven
/// platform. The field names match what the hand-written modules serialized,
/// so no adapter changes when a platform is converted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DescriptorAccount {
    account_id: String,
    label: String,
    last_used_at: Option<u64>,
    snapshot_saved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DescriptorStartupSnapshot {
    accounts: Vec<DescriptorAccount>,
    current_account: String,
}

/// Where the first half of a setup flow left off.
enum BeginOutcome {
    /// The signed-in session was adopted: nothing left to do.
    Adopted(SetupStatus),
    /// The live session is cleared and a job registered under this id.
    AwaitSignIn(String),
}

/// Setup jobs remember which accounts existed when the flow started, so a
/// "new" account can be told from the one already signed in.
#[derive(Clone, Default)]
struct SetupJob {
    known_account_ids: HashSet<String>,
    /// When the flow began, for the conditions that hold only after a delay.
    started_at: u64,
}

pub struct DescriptorService {
    descriptor: Descriptor,
    origin: DescriptorOrigin,
    jobs: SetupJobs<SetupJob>,
    /// Overrides the environment templates resolve against. Used by tests and
    /// by a dry run asked to reason about a machine other than this one.
    env_override: Option<Vec<(String, String)>>,
    #[cfg(test)]
    probes: TestProbes,
}

/// Counts of the things a test cannot see from outside.
#[cfg(test)]
#[derive(Default)]
struct TestProbes {
    /// Starts the engine asked for. Tests declare no launch step, so this is
    /// the only trace a start leaves.
    launches: std::sync::atomic::AtomicUsize,
    runtimes: std::sync::atomic::AtomicUsize,
    log_reads: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
fn probe(counter: &std::sync::atomic::AtomicUsize) {
    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

impl DescriptorService {
    pub fn new(descriptor: Descriptor, origin: DescriptorOrigin) -> Self {
        // SetupJobs labels its errors with a `&'static str` because platforms
        // hold it in a static. A descriptor's name is only known at run time,
        // and services live as long as the process, so leaking the label once
        // per service is the honest cost of keeping those messages readable.
        let label: &'static str = Box::leak(descriptor.short_name.clone().into_boxed_str());
        Self {
            descriptor,
            origin,
            jobs: SetupJobs::new(label, DEFAULT_SETUP_TTL_MS),
            env_override: None,
            #[cfg(test)]
            probes: TestProbes::default(),
        }
    }

    /// Resolves templates against `env` instead of this process's environment.
    pub fn with_environment<K: Into<String>, V: Into<String>>(
        mut self,
        env: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.env_override = Some(env.into_iter().map(|(k, v)| (k.into(), v.into())).collect());
        self
    }

    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    pub fn origin(&self) -> &DescriptorOrigin {
        &self.origin
    }

    pub fn id(&self) -> &str {
        &self.descriptor.id
    }

    // -----------------------------------------------------------------------
    // Runtime assembly
    // -----------------------------------------------------------------------

    fn profile(&self) -> Result<&OsProfile, String> {
        self.descriptor.current_profile().ok_or_else(|| {
            format!(
                "{} is not supported on this operating system",
                self.descriptor.name
            )
        })
    }

    fn base_resolver(&self) -> PathResolver {
        match &self.env_override {
            Some(env) => PathResolver::from_env(env.iter().map(|(k, v)| (k.clone(), v.clone()))),
            None => PathResolver::from_process_env(),
        }
    }

    /// Builds the resolver and the sandbox for one operation.
    ///
    /// The install directory is only looked up when a template actually asks
    /// for it: resolving the executable reads the config and hits the disk,
    /// and most operations never need it.
    fn runtime(&self, app: &dyn AppContext) -> Result<Runtime<'_>, String> {
        self.build_runtime(|| config_bridge::path_override(app, &self.descriptor.id))
    }

    /// [`Self::runtime`] for a caller that already holds the config: the
    /// user's path override is asked for only when a template needs it.
    fn build_runtime(&self, path_override: impl FnOnce() -> String) -> Result<Runtime<'_>, String> {
        #[cfg(test)]
        probe(&self.probes.runtimes);
        let profile = self.profile()?;
        let mut resolver = self.base_resolver();
        if profile_uses_install_dir(profile) {
            if let Ok(exe) = self.locate_executable(&path_override()) {
                if let Some(dir) = exe.parent() {
                    resolver = resolver.with_install_dir(dir);
                }
            }
        }
        // A root that does not resolve here stops the operation. Every path the
        // steps below build is checked against these, so carrying on with a
        // half-built sandbox would mean carrying on with no sandbox.
        let sandbox = Sandbox::new(&profile.roots, &resolver).map_err(|e| e.to_string())?;
        Ok(Runtime {
            profile,
            resolver,
            sandbox,
        })
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

/// What one read of the account list works from. See
/// [`DescriptorService::read_view`].
struct ReadView<'a> {
    runtime: Runtime<'a>,
    cfg: AppConfig,
    /// The account signed in now, from whichever source the descriptor names.
    current: Option<String>,
    snapshots: Option<PathBuf>,
}

/// One operation's resolved view of a descriptor.
struct Runtime<'a> {
    profile: &'a OsProfile,
    resolver: PathResolver,
    sandbox: Sandbox,
}

impl Runtime<'_> {
    /// Resolves a state path and refuses it if it falls outside the roots.
    fn path(&self, template: &PathTemplate) -> Result<PathBuf, PlatformError> {
        let resolved = self.resolver.resolve(template)?;
        self.sandbox.ensure_allowed(&resolved)?;
        Ok(resolved)
    }

    /// The candidate that exists, or the first one that resolves.
    ///
    /// Falling back to the first is what makes a file the launcher has not
    /// written yet land where it expects to find it, instead of failing.
    fn spec_path(&self, spec: &PathSpec) -> Result<PathBuf, PlatformError> {
        let mut fallback: Option<PathBuf> = None;
        let mut failure: Option<PlatformError> = None;
        for template in spec.candidates() {
            match self.path(template) {
                Ok(resolved) => {
                    if resolved.exists() {
                        return Ok(resolved);
                    }
                    if fallback.is_none() {
                        fallback = Some(resolved);
                    }
                }
                Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
            }
        }
        fallback.ok_or_else(|| {
            failure.unwrap_or_else(|| PlatformError::other("No path candidate to resolve"))
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn generate_account_id() -> String {
    Uuid::new_v4().simple().to_string()
}

/// How far back from an id a log line is searched for the word that says the
/// id belongs to an account, rather than being any other identifier on the line.
const NEAR_WORD_WINDOW: usize = 10;

/// True when any template in the profile asks for `${installDir}`.
///
/// Resolving it costs a config read and a walk of the install locations, so it
/// only happens for a descriptor that actually mentions it.
fn profile_uses_install_dir(profile: &OsProfile) -> bool {
    let mut placeholders: Vec<String> = profile
        .roots
        .files
        .iter()
        .chain(profile.detect.path_exists.iter())
        .chain(profile.state.caches.iter())
        .flat_map(|template| template.placeholders())
        .collect();
    for item in &profile.state.files {
        placeholders.extend(item.live.placeholders());
    }
    for item in &profile.state.directories {
        placeholders.extend(item.live.placeholders());
    }
    for entry in &profile.identity.discovery {
        let Discovery::DirectoryEntries { path, .. } = entry;
        placeholders.extend(path.placeholders());
    }
    match &profile.identity.source {
        IdentitySource::LogTail { path, .. } => placeholders.extend(path.placeholders()),
        IdentitySource::NativeHook { paths, .. } => {
            for path in paths.values() {
                placeholders.extend(path.placeholders());
            }
        }
        _ => {}
    }
    for condition in profile
        .setup
        .trigger
        .iter()
        .chain(profile.setup.confirm.iter())
        .chain(profile.state.capture_when.iter())
    {
        collect_condition_placeholders(condition, &mut placeholders);
    }
    placeholders.iter().any(|name| name == INSTALL_DIR)
}

fn collect_condition_placeholders(condition: &Condition, out: &mut Vec<String>) {
    match condition {
        Condition::NewIdentity | Condition::IdentityPresent | Condition::SinceStart { .. } => {}
        Condition::AnyOf { conditions } => {
            for nested in conditions {
                collect_condition_placeholders(nested, out);
            }
        }
        Condition::PathNonEmpty { path, .. } | Condition::PathFresh { path, .. } => {
            out.extend(path.placeholders())
        }
    }
}

/// A candidate may name the binary itself, the directory holding it, or a
/// directory one of the declared probes hangs off. Launchers shipping
/// per-architecture binaries need the last form and nothing else.
///
/// A candidate naming a file must name `fileName`: the preview and the
/// process checks go by that name, so any other binary would run unseen.
fn locate_binary(base: &Path, executable: &Executable) -> Option<PathBuf> {
    if base.is_file() {
        let named = base
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(&executable.file_name));
        return named.then(|| base.to_path_buf());
    }
    for probe in &executable.relative_probes {
        let candidate = base.join(probe).join(&executable.file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let candidate = base.join(&executable.file_name);
    candidate.is_file().then_some(candidate)
}

/// What a condition is allowed to know beyond the machine itself.
#[derive(Clone, Copy, Default)]
struct ConditionInput<'a> {
    /// The account the setup flow is watching for, once one shows up.
    new_identity: Option<&'a str>,
    /// When the setup flow started, absent outside one.
    started_at: Option<u64>,
}

/// An id with no name attached, which is every source but a native hook.
fn bare_identity(id: String) -> HookIdentity {
    HookIdentity {
        id,
        display_name: None,
    }
}

/// Where a decrypted file waits until every one of its siblings has landed.
fn staging_path(live: &Path) -> PathBuf {
    let mut name = live.file_name().unwrap_or_default().to_os_string();
    name.push(".accshift-restore-tmp");
    live.with_file_name(name)
}
// ---------------------------------------------------------------------------
// PlatformService
// ---------------------------------------------------------------------------

impl PlatformService for DescriptorService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let accounts = self.accounts_in(&self.read_view(&app)?);
        serde_json::to_value(accounts).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let view = self.read_view(&app)?;
        let snapshot = DescriptorStartupSnapshot {
            accounts: self.accounts_in(&view),
            current_account: view.current.clone().unwrap_or_default(),
        };
        serde_json::to_value(snapshot).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_current_account(&self, app: AppCtx) -> Result<String, PlatformError> {
        Ok(self.current_account_id(&app).unwrap_or_default())
    }

    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        _params: Value,
    ) -> Result<(), PlatformError> {
        self.switch(&app, account_id).map_err(Into::into)
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError> {
        self.forget(&app, account_id).map_err(Into::into)
    }

    fn begin_setup(&self, app: AppCtx, _params: Value) -> Result<SetupStatus, PlatformError> {
        self.begin(&app).map_err(Into::into)
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError> {
        self.setup_status(&app, setup_id).map_err(Into::into)
    }

    fn cancel_setup(&self, _app: AppCtx, setup_id: &str) -> Result<(), PlatformError> {
        self.jobs.cancel(setup_id).map_err(Into::into)
    }

    fn get_path(&self, app: AppCtx) -> Result<String, PlatformError> {
        let override_path = config_bridge::path_override(&app, &self.descriptor.id);
        if !override_path.is_empty() {
            return Ok(override_path);
        }
        self.resolve_executable(&app)
            .map(|p| p.to_string_lossy().to_string())
            .map_err(Into::into)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), PlatformError> {
        config_bridge::set_path_override(&app, &self.descriptor.id, path).map_err(Into::into)
    }

    fn select_path(&self) -> Result<String, PlatformError> {
        let profile = self.profile()?;
        let executable = profile
            .executable
            .as_ref()
            .ok_or_else(|| PlatformError::other("Path management not supported"))?;
        crate::os::select_file(
            &format!("Select {} executable", self.descriptor.name),
            &executable.select_filter,
        )
        .map_err(|e| PlatformError::other(e.to_string()))
    }

    fn is_installed(&self, app: AppCtx) -> bool {
        let Ok(profile) = self.profile() else {
            return false;
        };
        if profile.detect.executable_resolves {
            if let Ok(exe) = self.resolve_executable(&app) {
                if exe.exists() {
                    return true;
                }
            }
        }
        let resolver = self.base_resolver();
        profile
            .detect
            .path_exists
            .iter()
            .filter_map(|template| resolver.resolve(template).ok())
            .any(|path| path.exists())
    }

    fn set_account_label(
        &self,
        app: AppCtx,
        account_id: &str,
        label: &str,
    ) -> Result<(), PlatformError> {
        let account_id = self.validate_account_id(account_id)?;
        config_bridge::set_label(&app, &self.descriptor.id, &account_id, label).map_err(Into::into)
    }

    fn supports_dry_run(&self) -> bool {
        true
    }

    fn dry_run(&self, app: AppCtx, account_id: &str) -> Result<Value, PlatformError> {
        let plan = self.plan_switch(&app, account_id)?;
        serde_json::to_value(plan).map_err(|e| PlatformError::other(e.to_string()))
    }
}

#[cfg(test)]
mod tests;
