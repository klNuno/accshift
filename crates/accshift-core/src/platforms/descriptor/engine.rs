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
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

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
use super::paths::{PathResolver, Sandbox};
use super::plan::{DryRunPlan, PlanAction, PlanStep, PlanTargetKind};
use super::reg;
use super::schema::{
    Condition, CurrentSource, Descriptor, DirItem, Discovery, EntryKind, Executable,
    ExecutableCandidate, IdentitySource, OsProfile, PathSpec, PathTemplate, INSTALL_DIR,
};

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

/// Setup jobs remember which accounts existed when the flow started, so a
/// "new" account can be told from the one already signed in.
#[derive(Clone, Default)]
struct SetupJob {
    known_account_ids: HashSet<String>,
}

pub struct DescriptorService {
    descriptor: Descriptor,
    origin: DescriptorOrigin,
    jobs: SetupJobs<SetupJob>,
    /// Overrides the environment templates resolve against. Used by tests and
    /// by a dry run asked to reason about a machine other than this one.
    env_override: Option<Vec<(String, String)>>,
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
        let profile = self.profile()?;
        let mut resolver = self.base_resolver();
        if profile_uses_install_dir(profile) {
            if let Ok(exe) = self.resolve_executable(app) {
                if let Some(dir) = exe.parent() {
                    resolver = resolver.with_install_dir(dir);
                }
            }
        }
        let sandbox = Sandbox::new(&profile.roots, &resolver);
        Ok(Runtime {
            profile,
            resolver,
            sandbox,
        })
    }

    // -----------------------------------------------------------------------
    // Executable
    // -----------------------------------------------------------------------

    /// Finds the launcher binary: the user's override first, then the places
    /// the descriptor lists, in order.
    ///
    /// The binary itself is outside the sandbox by design. The roots bound
    /// where per-account session data is read and written; the launcher lives
    /// in Program Files and the user may point at it by hand.
    fn resolve_executable(&self, app: &dyn AppContext) -> Result<PathBuf, String> {
        let profile = self.profile()?;
        let executable = profile
            .executable
            .as_ref()
            .ok_or_else(|| "Path management not supported".to_string())?;

        let override_path = config_bridge::path_override(app, &self.descriptor.id);
        if !override_path.is_empty() {
            if let Some(found) = locate_binary(Path::new(&override_path), executable) {
                return Ok(found);
            }
        }

        let resolver = self.base_resolver();
        for candidate in &executable.candidates {
            let base = match candidate {
                ExecutableCandidate::Path { template } => match resolver.resolve(template) {
                    Ok(path) => path,
                    // A candidate naming a variable this machine does not have
                    // is not an error, it is a candidate that does not apply.
                    Err(_) => continue,
                },
                ExecutableCandidate::Registry { root, key, value } => {
                    let Some(raw) = reg::read(*root, key, value) else {
                        continue;
                    };
                    PathBuf::from(raw.trim().trim_end_matches(['\\', '/']))
                }
                ExecutableCandidate::UninstallEntry {
                    display_name,
                    value,
                } => {
                    let Some(raw) = reg::uninstall_entry(display_name, value) else {
                        continue;
                    };
                    PathBuf::from(raw.trim().trim_end_matches(['\\', '/']))
                }
            };
            if let Some(found) = locate_binary(&base, executable) {
                return Ok(found);
            }
        }

        Err(format!(
            "Could not locate {} executable",
            self.descriptor.name
        ))
    }

    fn launch(&self, app: &dyn AppContext) -> Result<(), String> {
        let profile = self.profile()?;
        let Some(launch) = profile.launch.as_ref() else {
            return Ok(());
        };
        let executable = self.resolve_executable(app)?;
        let mut command = Command::new(&executable);
        if launch.working_directory_is_install_dir {
            if let Some(install_dir) = executable.parent() {
                command.current_dir(install_dir);
            }
        }
        command.args(&launch.args);
        command.spawn().map_err(|e| {
            format!(
                "Could not launch {} {}: {e}",
                self.descriptor.name,
                executable.display()
            )
        })?;
        Ok(())
    }

    fn process_names(&self) -> Vec<String> {
        self.profile()
            .map(|profile| profile.close.processes.clone())
            .unwrap_or_default()
    }

    fn is_running(&self) -> bool {
        let names = self.process_names();
        if names.is_empty() {
            return false;
        }
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        crate::os::any_process_running(&refs)
    }

    /// Closes the launcher and waits for it to actually exit, so nothing races
    /// its exit-time flush of the session files to disk.
    fn quit_and_wait(&self) {
        let Ok(profile) = self.profile() else {
            return;
        };
        if profile.close.processes.is_empty() {
            return;
        }
        let refs: Vec<&str> = profile.close.processes.iter().map(String::as_str).collect();
        crate::os::quit_processes_and_wait(
            &refs,
            profile.close.timeout_ms,
            Duration::from_millis(profile.close.settle_ms),
        );
    }

    // -----------------------------------------------------------------------
    // Account ids
    // -----------------------------------------------------------------------

    fn id_is_valid(&self, candidate: &str) -> bool {
        let Ok(profile) = self.profile() else {
            return false;
        };
        let format = &profile.identity.format;
        !candidate.is_empty()
            && candidate.len() >= format.min_length
            && candidate.len() <= format.max_length
            && format.charset.accepts(candidate)
    }

    /// The one spelling of an id the rest of the engine works with. Launchers
    /// that write the same id in either case would otherwise get two snapshot
    /// directories and two config entries for one account.
    fn normalise_id(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        match self.profile() {
            Ok(profile) if profile.identity.format.lowercase => trimmed.to_lowercase(),
            _ => trimmed.to_string(),
        }
    }

    /// The id is joined into snapshot paths, so anything outside the declared
    /// charset is refused before it reaches the filesystem.
    fn validate_account_id(&self, id: &str) -> Result<String, String> {
        let candidate = self.normalise_id(id);
        // A platform whose wording the CLI classifies exit codes on keeps its
        // own message for both cases rather than the two generic ones.
        let declared = self
            .profile()
            .ok()
            .map(|profile| profile.identity.format.invalid_message.trim().to_string())
            .filter(|message| !message.is_empty());
        if candidate.is_empty() {
            return Err(declared
                .unwrap_or_else(|| format!("Empty {} account ID", self.descriptor.short_name)));
        }
        if !self.id_is_valid(&candidate) {
            return Err(declared.unwrap_or_else(|| {
                format!(
                    "Invalid {} account ID: {candidate}",
                    self.descriptor.short_name
                )
            }));
        }
        Ok(candidate)
    }

    /// The id the launcher currently reports, resolving the launcher first.
    ///
    /// Every caller in the engine already holds a runtime and goes through
    /// [`Self::read_identity_in`]; this is the shorthand the tests read with.
    #[cfg(test)]
    fn read_identity(&self, app: &dyn AppContext) -> Option<String> {
        let runtime = self.runtime(app).ok()?;
        self.read_identity_in(&runtime)
    }

    /// The id the launcher currently reports, when it reports one at all.
    ///
    /// Takes a runtime the caller already built, so a poll that touches
    /// several things does not resolve the launcher once per step.
    fn read_identity_in(&self, runtime: &Runtime<'_>) -> Option<String> {
        let raw = match &runtime.profile.identity.source {
            IdentitySource::Registry { root, key, value } => reg::read(*root, key, value),
            IdentitySource::LogTail { .. } => self.read_identity_from_log(runtime),
            // Nothing readable: the account is whatever we last put there.
            IdentitySource::Synthetic => None,
            // Reserved for the two platforms whose discovery needs code.
            IdentitySource::NativeHook { .. } => None,
        }?;
        let id = self.normalise_id(&raw);
        self.id_is_valid(&id).then_some(id)
    }

    /// Reads the id out of the launcher's own log, most recent line first.
    fn read_identity_from_log(&self, runtime: &Runtime<'_>) -> Option<String> {
        let IdentitySource::LogTail {
            path,
            tail_bytes,
            line_contains,
            prefix,
            near_word,
        } = &runtime.profile.identity.source
        else {
            return None;
        };
        let resolved = runtime.path(path).ok()?;
        let content = read_log_tail(&resolved, *tail_bytes)?;
        // The id has one fixed width here: the schema refuses a log source
        // whose format allows a range, because a scan has nothing to match on.
        let width = runtime.profile.identity.format.max_length;
        content
            .lines()
            .rev()
            .filter(|line| line.contains(line_contains.as_str()))
            .find_map(|line| self.extract_id(line, prefix, near_word, width))
    }

    /// Pulls an id out of one log line: the text right after `prefix`, else any
    /// id preceded within a few characters by `near_word`.
    ///
    /// Every slice goes through `get`: a log line carries user names and paths,
    /// so slicing at a byte offset would panic on a character boundary.
    fn extract_id(
        &self,
        line: &str,
        prefix: &str,
        near_word: &str,
        width: usize,
    ) -> Option<String> {
        if !prefix.is_empty() {
            if let Some(position) = line.find(prefix) {
                let start = position + prefix.len();
                if let Some(candidate) = line.get(start..start + width) {
                    if self.id_is_valid(candidate) {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
        if !near_word.is_empty() && line.len() >= width {
            for start in 0..=line.len() - width {
                let Some(candidate) = line.get(start..start + width) else {
                    continue;
                };
                if self.id_is_valid(candidate)
                    && line
                        .get(start.saturating_sub(NEAR_WORD_WINDOW)..start)
                        .is_some_and(|context| context.contains(near_word))
                {
                    return Some(candidate.to_string());
                }
            }
        }
        None
    }

    /// Ids the platform leaves lying around outside its own session files, so
    /// an account added without accshift still shows up.
    ///
    /// Forgotten ids are filtered out here: a blocklist that discovery ignored
    /// would put the account straight back on the next poll.
    fn discovered_ids(&self, app: &dyn AppContext, runtime: &Runtime<'_>) -> BTreeSet<String> {
        let identity = &runtime.profile.identity;
        let mut ids = BTreeSet::new();
        for entry in &identity.discovery {
            let Discovery::DirectoryEntries {
                path,
                entries,
                strip_prefixes,
                strip_extension,
            } = entry;
            let Ok(dir) = runtime.path(path) else {
                continue;
            };
            let Ok(listing) = fs::read_dir(&dir) else {
                continue;
            };
            for item in listing.flatten() {
                let keep = match entries {
                    EntryKind::Any => true,
                    EntryKind::Directories => item.path().is_dir(),
                    EntryKind::Files => item.path().is_file(),
                };
                if !keep {
                    continue;
                }
                let Some(name) = item.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let mut candidate = name.as_str();
                for prefix in strip_prefixes {
                    if let Some(stripped) = candidate.strip_prefix(prefix.as_str()) {
                        candidate = stripped;
                        break;
                    }
                }
                if *strip_extension {
                    candidate = candidate.split('.').next().unwrap_or(candidate);
                }
                let id = self.normalise_id(candidate);
                if self.id_is_valid(&id) {
                    ids.insert(id);
                }
            }
        }
        if identity.blocklist_on_forget {
            let blocked = self.blocked_ids(app);
            ids.retain(|id| !blocked.contains(id));
        }
        ids
    }

    fn blocked_ids(&self, app: &dyn AppContext) -> HashSet<String> {
        if !self
            .profile()
            .map(|profile| profile.identity.blocklist_on_forget)
            .unwrap_or(false)
        {
            return HashSet::new();
        }
        config_bridge::blocklist(app, &self.descriptor.id)
            .iter()
            .map(|id| self.normalise_id(id))
            .collect()
    }

    /// Which account is signed in, from whichever source the descriptor names.
    fn current_account_id(&self, app: &dyn AppContext) -> Option<String> {
        let runtime = self.runtime(app).ok()?;
        self.current_account_id_in(app, &runtime)
    }

    fn current_account_id_in(&self, app: &dyn AppContext, runtime: &Runtime<'_>) -> Option<String> {
        match runtime.profile.identity.current {
            CurrentSource::Identity => self.read_identity_in(runtime),
            CurrentSource::Config => config_bridge::current_account(app, &self.descriptor.id)
                .map(|id| self.normalise_id(&id))
                .filter(|id| self.id_is_valid(id)),
        }
    }

    fn snapshot_root(&self, app: &dyn AppContext, account_id: &str) -> Result<PathBuf, String> {
        Ok(crate::storage::platform_snapshots_dir(app, &self.descriptor.id)?.join(account_id))
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    fn save_snapshot(&self, app: &dyn AppContext, account_id: &str) -> Result<(), String> {
        let runtime = self.runtime(app)?;
        let cache_dir = self.snapshot_root(app, account_id)?;
        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Could not create auth cache dir: {e}"))?;

        for item in &runtime.profile.state.files {
            let live = runtime.spec_path(&item.live)?;
            let dest = cache_dir.join(&item.snapshot);
            if live.is_file() {
                delete_encrypted_file_secret(&dest);
                encrypted_copy_file(&live, &dest)?;
            } else if item.clear_snapshot_when_source_missing {
                // Nothing live to capture: drop the stale snapshot so a later
                // restore cannot resurrect another account's file.
                delete_encrypted_file_secret(&dest);
                let _ = fs::remove_file(&dest);
            }
        }

        for item in &runtime.profile.state.registry_values {
            let dest = cache_dir.join(&item.snapshot);
            match reg::read(item.root, &item.key, &item.value) {
                Some(value) => {
                    delete_encrypted_file_secret(&dest);
                    if let Err(e) = write_encrypted_bytes(&dest, value.as_bytes()) {
                        // Non-fatal: the session files still carry the account,
                        // and failing the whole switch over one value would
                        // strand the user worse than the missing value does.
                        log_platform_error(
                            app,
                            &format!("{}.save_snapshot", self.descriptor.id),
                            "Could not encrypt registry value for snapshot",
                            e,
                        );
                    }
                }
                None if item.clear_snapshot_when_source_missing => {
                    // Freeing the secret has to happen with the file, never
                    // before deciding to keep it: a kept snapshot whose key is
                    // gone can no longer be decrypted.
                    delete_encrypted_file_secret(&dest);
                    let _ = fs::remove_file(&dest);
                }
                None => {}
            }
        }

        for item in &runtime.profile.state.directories {
            let live = runtime.spec_path(&item.live)?;
            let dest = cache_dir.join(&item.snapshot);
            let _ = fs::remove_dir_all(&dest);
            let ignored: Vec<&str> = item.ignored_names.iter().map(String::as_str).collect();
            snapshot_crypto::encrypted_copy_dir(
                &live,
                &dest,
                DirCopyOptions {
                    ignored_names: &ignored,
                    follow_symlinks: item.follow_symlinks,
                },
            )?;
        }

        Ok(())
    }

    fn restore_snapshot(&self, app: &dyn AppContext, account_id: &str) -> Result<(), String> {
        let runtime = self.runtime(app)?;
        let cache_dir = self.snapshot_root(app, account_id)?;

        if !cache_dir.exists() {
            let hint = runtime.profile.setup.missing_snapshot_hint.trim();
            let mut message = format!("No auth snapshot found for account {account_id}.");
            if !hint.is_empty() {
                message.push(' ');
                message.push_str(hint);
            }
            return Err(message);
        }

        // Every file is decrypted next to its destination first and only moved
        // into place once they have all landed. Several session files are one
        // credential set: a failure halfway through would otherwise leave the
        // incoming account's half next to the outgoing account's.
        let mut staged: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
        for item in &runtime.profile.state.files {
            let source = cache_dir.join(&item.snapshot);
            if !source.exists() {
                continue;
            }
            let live = runtime.spec_path(&item.live)?;
            if let Some(parent) = live.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
            }
            let staging = staging_path(&live);
            if let Err(error) = decrypted_copy_file(&source, &staging) {
                for (path, _, _) in &staged {
                    let _ = fs::remove_file(path);
                }
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
            staged.push((staging, live, item.remove_live_before_restore));
        }
        for (staging, live, remove_live_first) in &staged {
            if *remove_live_first {
                // Files the OS marks hidden or system cannot be replaced in
                // place on Windows, so the live copy goes first.
                let _ = fs::remove_file(live);
            }
            if fs::rename(staging, live).is_err() {
                // Cross-volume rename or a lingering lock: copy the already
                // decrypted staging file instead.
                fs::copy(staging, live)
                    .map_err(|e| format!("Could not finalize {}: {e}", live.display()))?;
                let _ = fs::remove_file(staging);
            }
        }

        for item in &runtime.profile.state.registry_values {
            let source = cache_dir.join(&item.snapshot);
            if !source.exists() {
                continue;
            }
            if let Ok(bytes) = read_decrypted_bytes(&source) {
                if let Ok(text) = String::from_utf8(bytes) {
                    let _ = reg::write(item.root, &item.key, &item.value, text.trim());
                }
            }
        }

        for item in &runtime.profile.state.directories {
            let source = cache_dir.join(&item.snapshot);
            let live = runtime.spec_path(&item.live)?;
            restore_dir_snapshot(&source, &live, item)?;
        }

        Ok(())
    }

    /// Removes the launcher caches keyed to the account that just left. They
    /// are never captured: they hold no credential, only material the launcher
    /// rebuilds, and keeping them across a switch shows the previous account.
    fn clear_caches(&self, app: &dyn AppContext) {
        let Ok(profile) = self.profile() else {
            return;
        };
        if profile.state.caches.is_empty() {
            return;
        }
        let Ok(runtime) = self.runtime(app) else {
            return;
        };
        for template in &profile.state.caches {
            if let Ok(path) = runtime.path(template) {
                let _ = fs::remove_dir_all(&path);
            }
        }
    }

    /// Whether this account has anything worth restoring.
    fn has_snapshot(&self, app: &dyn AppContext, account_id: &str) -> bool {
        self.snapshot_markers(app, account_id)
            .iter()
            .any(|path| path.exists())
    }

    /// Stricter check, for the moment right after a capture: a marker that
    /// exists but is empty means the launcher was closed before it wrote the
    /// session, and restoring it later would land on the login screen.
    fn snapshot_has_content(&self, app: &dyn AppContext, account_id: &str) -> bool {
        self.snapshot_markers(app, account_id)
            .iter()
            .any(|path| path_has_content(path, true))
    }

    /// Whether anything in the descriptor can answer the two questions above.
    /// A descriptor declaring no marker never claims to hold a snapshot.
    fn declares_snapshot_marker(&self) -> bool {
        self.profile()
            .map(|profile| {
                profile.state.files.iter().any(|i| i.snapshot_marker)
                    || profile.state.directories.iter().any(|i| i.snapshot_marker)
                    || profile
                        .state
                        .registry_values
                        .iter()
                        .any(|i| i.snapshot_marker)
            })
            .unwrap_or(false)
    }

    fn snapshot_markers(&self, app: &dyn AppContext, account_id: &str) -> Vec<PathBuf> {
        let Ok(profile) = self.profile() else {
            return Vec::new();
        };
        let Ok(cache_dir) = self.snapshot_root(app, account_id) else {
            return Vec::new();
        };
        let files = profile
            .state
            .files
            .iter()
            .filter(|i| i.snapshot_marker)
            .map(|i| &i.snapshot);
        let dirs = profile
            .state
            .directories
            .iter()
            .filter(|i| i.snapshot_marker)
            .map(|i| &i.snapshot);
        let values = profile
            .state
            .registry_values
            .iter()
            .filter(|i| i.snapshot_marker)
            .map(|i| &i.snapshot);
        files
            .chain(dirs)
            .chain(values)
            .map(|name| cache_dir.join(name))
            .collect()
    }

    /// Clears the live session so a fresh sign-in starts from the login screen.
    /// Only the setup path calls this; a switch restores over the live state
    /// instead.
    fn clear_live_state(&self, app: &dyn AppContext) -> Result<(), String> {
        let runtime = self.runtime(app)?;
        for item in &runtime.profile.state.files {
            if item.clear_on_setup {
                let live = runtime.spec_path(&item.live)?;
                let _ = fs::remove_file(&live);
            }
        }
        for item in &runtime.profile.state.registry_values {
            if item.clear_on_setup {
                reg::delete(item.root, &item.key, &item.value);
            }
        }
        for item in &runtime.profile.state.directories {
            if item.clear_on_setup {
                let live = runtime.spec_path(&item.live)?;
                let _ = fs::remove_dir_all(&live);
            }
        }
        for template in &runtime.profile.state.caches {
            if let Ok(path) = runtime.path(template) {
                let _ = fs::remove_dir_all(&path);
            }
        }
        Ok(())
    }

    /// Frees the keyring entries the snapshot files point at, then removes the
    /// account's snapshot directory.
    fn delete_snapshot(&self, app: &dyn AppContext, account_id: &str) {
        let Ok(profile) = self.profile() else {
            return;
        };
        let Ok(cache_dir) = self.snapshot_root(app, account_id) else {
            return;
        };
        for item in &profile.state.files {
            delete_encrypted_file_secret(&cache_dir.join(&item.snapshot));
        }
        for item in &profile.state.registry_values {
            delete_encrypted_file_secret(&cache_dir.join(&item.snapshot));
        }
        for item in &profile.state.directories {
            free_dir_secrets(&cache_dir.join(&item.snapshot));
        }
        let _ = fs::remove_dir_all(&cache_dir);
    }

    /// Records usage of the signed-in account and refreshes its snapshot
    /// before the live session is replaced.
    ///
    /// Returns `Err` when an account IS signed in but could not be captured,
    /// so the caller aborts before killing the launcher: proceeding would
    /// strand that account signed out with no backup.
    fn capture_current_account(&self, app: &dyn AppContext) -> Result<(), String> {
        let Some(current_id) = self.current_account_id(app) else {
            return Ok(());
        };
        let _ = config_bridge::touch_account(app, &self.descriptor.id, &current_id, now_unix_ms());
        self.save_snapshot(app, &current_id)
    }

    // -----------------------------------------------------------------------
    // Reads
    // -----------------------------------------------------------------------

    fn read_accounts(&self, app: &dyn AppContext) -> Result<Vec<DescriptorAccount>, String> {
        let runtime = self.runtime(app)?;
        let blocked = self.blocked_ids(app);
        let mut discovered: HashSet<String> = self
            .discovered_ids(app, &runtime)
            .into_iter()
            .collect::<HashSet<_>>();
        // The account signed in right now counts as discovered, unless it is
        // one the user forgot and has not used since.
        if let Some(id) = self
            .read_identity_in(&runtime)
            .filter(|id| !blocked.contains(id))
        {
            discovered.insert(id);
        }
        let stored = config_bridge::accounts(app, &self.descriptor.id);

        let mut seen = HashSet::new();
        let mut accounts = Vec::new();

        // Config first: it carries the labels and the display order. Ids are
        // normalised on the way out, so a config written before the platform
        // declared a spelling still matches what discovery reports.
        for account in &stored {
            let id = self.normalise_id(&account.account_id);
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            accounts.push(DescriptorAccount {
                snapshot_saved: self.has_snapshot(app, &id),
                account_id: id,
                label: account.label.clone(),
                last_used_at: account.last_used_at,
            });
        }

        // An account signed in outside accshift is real even with no config
        // entry, so it is listed too.
        for id in &discovered {
            if !seen.insert(id.clone()) {
                continue;
            }
            accounts.push(DescriptorAccount {
                account_id: id.clone(),
                label: String::new(),
                last_used_at: None,
                snapshot_saved: self.has_snapshot(app, id),
            });
        }

        let stored_ids: HashSet<String> = stored
            .iter()
            .map(|a| self.normalise_id(&a.account_id))
            .collect();
        accounts.retain(|a| {
            discovered.contains(&a.account_id)
                || stored_ids.contains(&a.account_id)
                || a.snapshot_saved
        });

        Ok(accounts)
    }

    // -----------------------------------------------------------------------
    // Operations
    // -----------------------------------------------------------------------

    fn switch(&self, app: &dyn AppContext, account_id: &str) -> Result<(), String> {
        let account_id = self.validate_account_id(account_id)?;
        let source = format!("{}.switch_account", self.descriptor.id);
        log_platform_info(
            app,
            &source,
            &format!("{} switch requested", self.descriptor.short_name),
            format!("target={}", redact_id(&account_id)),
        );

        // Snapshot the outgoing account first. Aborting here is the point:
        // going further would overwrite its live session with the target's.
        self.capture_current_account(app)?;

        let uses_config_marker = self
            .profile()
            .map(|p| p.identity.current == CurrentSource::Config)
            .unwrap_or(false);
        if uses_config_marker {
            // Clear the marker before touching live files: a restore that
            // fails midway leaves a mix of two accounts, which must not be
            // captured into either snapshot on a later switch.
            config_bridge::set_current_account(app, &self.descriptor.id, "")?;
        }

        self.quit_and_wait();
        self.restore_snapshot(app, &account_id)?;
        self.clear_caches(app);
        config_bridge::touch_account(app, &self.descriptor.id, &account_id, now_unix_ms())?;
        if uses_config_marker {
            config_bridge::set_current_account(app, &self.descriptor.id, &account_id)?;
        }

        let result = self.launch(app);
        match &result {
            Ok(()) => log_platform_info(
                app,
                &source,
                &format!("{} switch completed", self.descriptor.short_name),
                format!("target={}", redact_id(&account_id)),
            ),
            Err(error) => log_platform_error(
                app,
                &source,
                &format!("{} switch failed", self.descriptor.short_name),
                format!("target={}; error={error}", redact_id(&account_id)),
            ),
        }
        result
    }

    fn begin(&self, app: &dyn AppContext) -> Result<SetupStatus, String> {
        let source = format!("{}.begin_account_setup", self.descriptor.id);
        log_platform_info(
            app,
            &source,
            &format!("{} account setup requested", self.descriptor.short_name),
            "",
        );

        self.capture_current_account(app)?;

        // Everything that already exists, so the flow can tell the account the
        // user is about to add from the ones that were there before.
        let runtime = self.runtime(app)?;
        let mut known: HashSet<String> = self.discovered_ids(app, &runtime).into_iter().collect();
        known.extend(self.read_identity_in(&runtime));
        for account in config_bridge::accounts(app, &self.descriptor.id) {
            let id = self.normalise_id(&account.account_id);
            if !id.is_empty() {
                known.insert(id);
            }
        }

        let setup_id = format!("{}-setup-{}", self.descriptor.id, Uuid::new_v4());
        self.jobs.insert(
            setup_id.clone(),
            SetupJob {
                known_account_ids: known,
            },
        )?;

        self.quit_and_wait();
        self.clear_live_state(app)?;
        if self
            .profile()
            .map(|p| p.identity.current == CurrentSource::Config)
            .unwrap_or(false)
        {
            // Nobody is signed in until the flow completes.
            config_bridge::set_current_account(app, &self.descriptor.id, "")?;
        }

        self.launch(app).inspect_err(|e| {
            log_platform_error(
                app,
                &source,
                &format!("{} setup launch failed", self.descriptor.short_name),
                e.clone(),
            );
        })?;

        Ok(make_setup_status(
            &setup_id,
            "waiting_for_client",
            "",
            "",
            "",
        ))
    }

    fn setup_status(&self, app: &dyn AppContext, setup_id: &str) -> Result<SetupStatus, String> {
        let job = self.jobs.touch(setup_id)?;
        let runtime = self.runtime(app)?;
        let setup = &runtime.profile.setup;

        // The launcher's own marker first, then anything the platform leaves on
        // disk: some write the id where we can read it only after a restart.
        let new_identity = self
            .read_identity_in(&runtime)
            .filter(|id| !job.known_account_ids.contains(id))
            .or_else(|| {
                self.discovered_ids(app, &runtime)
                    .into_iter()
                    .find(|id| !job.known_account_ids.contains(id))
            });

        let triggered = !setup.trigger.is_empty()
            && setup.trigger.iter().all(|condition| {
                self.condition_holds(&runtime, condition, new_identity.as_deref())
            });

        if triggered {
            // A trigger only says the user got through the login screen. The
            // launcher may still hold the session in memory, so it is closed
            // and the conditions re-checked before anything is captured.
            self.quit_and_wait();

            let still_holds = setup.confirm.iter().all(|condition| {
                self.condition_holds(&runtime, condition, new_identity.as_deref())
            });
            if !still_holds {
                return Ok(make_setup_status(setup_id, "waiting_for_login", "", "", ""));
            }

            let key = match &runtime.profile.identity.source {
                IdentitySource::Synthetic => generate_account_id(),
                _ => match new_identity {
                    Some(id) => id,
                    // The confirm pass says a session exists but no id came
                    // with it: keep waiting rather than store an unnamed one.
                    None => {
                        return Ok(make_setup_status(setup_id, "waiting_for_login", "", "", ""))
                    }
                },
            };

            self.save_snapshot(app, &key)?;
            if self.declares_snapshot_marker() && !self.snapshot_has_content(app, &key) {
                // The capture produced nothing worth restoring: the launcher
                // was closed before it wrote the session. Keep waiting rather
                // than hand back an account that restores to a login screen.
                return Ok(make_setup_status(setup_id, "waiting_for_login", "", "", ""));
            }
            config_bridge::touch_account(app, &self.descriptor.id, &key, now_unix_ms())?;
            if runtime.profile.identity.current == CurrentSource::Config {
                config_bridge::set_current_account(app, &self.descriptor.id, &key)?;
            }

            self.jobs.remove(setup_id);

            let display_name = if setup.display_name_from_id {
                key.clone()
            } else {
                String::new()
            };
            return Ok(make_setup_status(setup_id, "ready", key, display_name, ""));
        }

        if self.is_running() {
            return Ok(make_setup_status(setup_id, "waiting_for_login", "", "", ""));
        }
        Ok(make_setup_status(
            setup_id,
            "waiting_for_client",
            "",
            "",
            "",
        ))
    }

    fn condition_holds(
        &self,
        runtime: &Runtime<'_>,
        condition: &Condition,
        new_identity: Option<&str>,
    ) -> bool {
        match condition {
            Condition::NewIdentity => new_identity.is_some(),
            Condition::IdentityPresent => self.read_identity_in(runtime).is_some(),
            Condition::AnyOf { conditions } => conditions
                .iter()
                .any(|nested| self.condition_holds(runtime, nested, new_identity)),
            Condition::PathNonEmpty { path, recursive } => match runtime.spec_path(path) {
                Ok(resolved) => path_has_content(&resolved, *recursive),
                Err(_) => false,
            },
            Condition::PathFresh { path, window_ms } => match runtime.spec_path(path) {
                Ok(resolved) => file_is_fresh(&resolved, *window_ms),
                Err(_) => false,
            },
        }
    }

    fn forget(&self, app: &dyn AppContext, account_id: &str) -> Result<(), String> {
        let account_id = self.validate_account_id(account_id)?;
        config_bridge::remove_account(app, &self.descriptor.id, &account_id)?;
        if self
            .profile()
            .map(|profile| profile.identity.blocklist_on_forget)
            .unwrap_or(false)
        {
            // The account is still on disk, so without this the next listing
            // discovers it again and forgetting appears not to work.
            config_bridge::block_account(app, &self.descriptor.id, &account_id)?;
        }
        // Only touch the filesystem for a well-formed id: it is joined into
        // the snapshot path.
        if self.id_is_valid(&account_id) {
            self.delete_snapshot(app, &account_id);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Dry run
    // -----------------------------------------------------------------------

    /// Describes the switch this descriptor would perform, writing nothing.
    pub fn plan_switch(
        &self,
        app: &dyn AppContext,
        account_id: &str,
    ) -> Result<DryRunPlan, String> {
        let account_id = self.validate_account_id(account_id)?;
        let runtime = self.runtime(app)?;
        let cache_dir = self.snapshot_root(app, &account_id)?;

        let mut plan = DryRunPlan::new(&self.descriptor.id, "switch", &account_id).with_roots(
            runtime
                .sandbox
                .roots()
                .iter()
                .map(|root| root.display().to_string()),
        );

        if !cache_dir.exists() {
            plan.warn(format!(
                "No snapshot stored for account {account_id}: the switch would fail here."
            ));
        }

        match self.current_account_id(app) {
            Some(current) => {
                let current_dir = self.snapshot_root(app, &current)?;
                self.plan_capture(&runtime, &mut plan, &current_dir);
            }
            None => plan.warn(
                "No account is signed in, so nothing would be captured before the switch."
                    .to_string(),
            ),
        }

        for name in &runtime.profile.close.processes {
            plan.simple_step(PlanAction::Close, PlanTargetKind::Process, name, "");
        }

        self.plan_restore(&runtime, &mut plan, &cache_dir);

        match self.resolve_executable(app) {
            Ok(exe) => plan.simple_step(
                PlanAction::Launch,
                PlanTargetKind::Executable,
                exe.display().to_string(),
                "",
            ),
            Err(e) => plan.warn(e),
        }

        Ok(plan)
    }

    fn plan_capture(&self, runtime: &Runtime<'_>, plan: &mut DryRunPlan, cache_dir: &Path) {
        for item in &runtime.profile.state.files {
            match runtime.spec_path(&item.live) {
                Ok(live) => plan.path_step(
                    PlanAction::Capture,
                    PlanTargetKind::File,
                    &live,
                    &cache_dir.join(&item.snapshot),
                    if live.is_file() { "" } else { "not present" },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
        for item in &runtime.profile.state.registry_values {
            let present = reg::read(item.root, &item.key, &item.value).is_some();
            plan.push(PlanStep {
                action: PlanAction::Capture,
                kind: PlanTargetKind::RegistryValue,
                target: reg::display(item.root, &item.key, &item.value),
                snapshot: cache_dir.join(&item.snapshot).display().to_string(),
                note: if present {
                    String::new()
                } else {
                    "not set".into()
                },
            });
        }
        for item in &runtime.profile.state.directories {
            match runtime.spec_path(&item.live) {
                Ok(live) => plan.path_step(
                    PlanAction::Capture,
                    PlanTargetKind::Directory,
                    &live,
                    &cache_dir.join(&item.snapshot),
                    if live.is_dir() { "" } else { "not present" },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
    }

    fn plan_restore(&self, runtime: &Runtime<'_>, plan: &mut DryRunPlan, cache_dir: &Path) {
        for item in &runtime.profile.state.files {
            let snapshot = cache_dir.join(&item.snapshot);
            match runtime.spec_path(&item.live) {
                Ok(live) => plan.path_step(
                    PlanAction::Restore,
                    PlanTargetKind::File,
                    &live,
                    &snapshot,
                    if snapshot.exists() {
                        ""
                    } else {
                        "no snapshot, skipped"
                    },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
        for item in &runtime.profile.state.registry_values {
            let snapshot = cache_dir.join(&item.snapshot);
            plan.push(PlanStep {
                action: PlanAction::Restore,
                kind: PlanTargetKind::RegistryValue,
                target: reg::display(item.root, &item.key, &item.value),
                snapshot: snapshot.display().to_string(),
                note: if snapshot.exists() {
                    String::new()
                } else {
                    "no snapshot, skipped".into()
                },
            });
        }
        for item in &runtime.profile.state.directories {
            let snapshot = cache_dir.join(&item.snapshot);
            match runtime.spec_path(&item.live) {
                Ok(live) => plan.path_step(
                    PlanAction::Restore,
                    PlanTargetKind::Directory,
                    &live,
                    &snapshot,
                    if snapshot.exists() {
                        ""
                    } else {
                        "no snapshot, skipped"
                    },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
        for template in &runtime.profile.state.caches {
            match runtime.path(template) {
                Ok(path) => plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::Directory,
                    path.display().to_string(),
                    if path.is_dir() { "" } else { "not present" },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

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
    if let IdentitySource::LogTail { path, .. } = &profile.identity.source {
        placeholders.extend(path.placeholders());
    }
    for condition in profile
        .setup
        .trigger
        .iter()
        .chain(profile.setup.confirm.iter())
    {
        collect_condition_placeholders(condition, &mut placeholders);
    }
    placeholders.iter().any(|name| name == INSTALL_DIR)
}

fn collect_condition_placeholders(condition: &Condition, out: &mut Vec<String>) {
    match condition {
        Condition::NewIdentity | Condition::IdentityPresent => {}
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
fn locate_binary(base: &Path, executable: &Executable) -> Option<PathBuf> {
    if base.is_file() {
        return Some(base.to_path_buf());
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

/// Where a decrypted file waits until every one of its siblings has landed.
fn staging_path(live: &Path) -> PathBuf {
    let mut name = live.file_name().unwrap_or_default().to_os_string();
    name.push(".accshift-restore-tmp");
    live.with_file_name(name)
}

/// Reads the last `tail_bytes` of a file the launcher keeps open.
///
/// Shared access is the point on Windows: the launcher holds its log open, and
/// an ordinary open would simply fail. The tail is decoded lossily so a cut
/// through a multi-byte character cannot fail the read, and only the end is
/// read because the log runs to megabytes and the sign-in sits at the bottom.
fn read_log_tail(path: &Path, tail_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    #[cfg(windows)]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
            .open(path)
            .ok()?
    };
    #[cfg(not(windows))]
    let mut file = fs::File::open(path).ok()?;

    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(tail_bytes);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buffer = Vec::with_capacity(tail_bytes.min(len) as usize);
    file.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// True when a file exists, is non-empty, and was written within `window_ms`.
/// A stale timestamp means the launcher never flushed the new session, so
/// capturing it would store the previous account's material.
fn file_is_fresh(path: &Path, window_ms: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.len() == 0 {
        return false;
    }
    let Ok(modified) = meta.modified() else {
        return true;
    };
    let Ok(elapsed) = modified.elapsed() else {
        return true;
    };
    (elapsed.as_millis() as u64) <= window_ms
}

/// A non-empty file, or (when `recursive`) a directory holding one anywhere
/// below it.
fn path_has_content(path: &Path, recursive: bool) -> bool {
    if path.is_dir() {
        return recursive && dir_has_nonempty_file(path);
    }
    fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
}

fn dir_has_nonempty_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_nonempty_file(&path) {
                return true;
            }
        } else if fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// Restores a session directory from its encrypted snapshot.
///
/// The decrypted copy is staged next to the live directory and swapped in, so
/// a failure partway through never leaves the live directory holding a mix of
/// the outgoing and incoming account's files. A missing snapshot is a no-op.
fn restore_dir_snapshot(
    snapshot_dir: &Path,
    live_dir: &Path,
    item: &DirItem,
) -> Result<(), String> {
    if !snapshot_dir.exists() {
        return Ok(());
    }
    let staging = staging_path(live_dir);
    let _ = fs::remove_dir_all(&staging);

    let ignored: Vec<&str> = item.ignored_names.iter().map(String::as_str).collect();
    snapshot_crypto::decrypted_copy_dir(
        snapshot_dir,
        &staging,
        DirCopyOptions {
            ignored_names: &ignored,
            follow_symlinks: item.follow_symlinks,
        },
    )?;

    if live_dir.exists() {
        fs::remove_dir_all(live_dir)
            .map_err(|e| format!("Could not clear {}: {e}", live_dir.display()))?;
    }
    if let Some(parent) = live_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    match fs::rename(&staging, live_dir) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-volume rename or a lingering lock: copy the already
            // decrypted staging tree instead, then drop the staging dir.
            crate::fs_utils::copy_dir_recursive(&staging, live_dir, &[])?;
            let _ = fs::remove_dir_all(&staging);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// PlatformService
// ---------------------------------------------------------------------------

impl PlatformService for DescriptorService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let accounts = self.read_accounts(&app)?;
        serde_json::to_value(accounts).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let snapshot = DescriptorStartupSnapshot {
            accounts: self.read_accounts(&app)?,
            current_account: self.current_account_id(&app).unwrap_or_default(),
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

    fn dry_run(&self, app: AppCtx, account_id: &str) -> Result<Value, PlatformError> {
        let plan = self.plan_switch(&app, account_id)?;
        serde_json::to_value(plan).map_err(|e| PlatformError::other(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct TempCtx {
        root: PathBuf,
    }

    impl AppContext for TempCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-descriptor-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A platform whose whole world is two directories under the scratch root:
    /// no launcher, no registry, nothing installed.
    fn fixture(live_root: &Path) -> String {
        let live = live_root.display().to_string().replace('\\', "/");
        format!(
            r#"{{
              "id": "gog",
              "schemaVersion": 1,
              "name": "Test Launcher",
              "shortName": "Test",
              "os": {{
                "windows": {{
                  "roots": {{ "files": ["{live}"] }},
                  "detect": {{ "pathExists": ["{live}"] }},
                  "identity": {{
                    "source": {{ "kind": "synthetic" }},
                    "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                    "current": "config"
                  }},
                  "state": {{
                    "files": [
                      {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true, "clearOnSetup": true }}
                    ],
                    "directories": [
                      {{ "live": "{live}/auth", "snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true }}
                    ]
                  }},
                  "close": {{ "processes": ["nothing-here.exe"] }},
                  "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
                }},
                "linux": {{
                  "roots": {{ "files": ["{live}"] }},
                  "detect": {{ "pathExists": ["{live}"] }},
                  "identity": {{
                    "source": {{ "kind": "synthetic" }},
                    "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                    "current": "config"
                  }},
                  "state": {{
                    "files": [
                      {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true, "clearOnSetup": true }}
                    ],
                    "directories": [
                      {{ "live": "{live}/auth", "snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true }}
                    ]
                  }},
                  "close": {{ "processes": ["nothing-here"] }},
                  "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
                }},
                "macos": {{
                  "roots": {{ "files": ["{live}"] }},
                  "detect": {{ "pathExists": ["{live}"] }},
                  "identity": {{
                    "source": {{ "kind": "synthetic" }},
                    "format": {{ "charset": "alphanumeric", "maxLength": 64 }},
                    "current": "config"
                  }},
                  "state": {{
                    "files": [
                      {{ "live": "{live}/session.json", "snapshot": "session.json", "snapshotMarker": true, "clearOnSetup": true }}
                    ],
                    "directories": [
                      {{ "live": "{live}/auth", "snapshot": "auth", "snapshotMarker": true, "clearOnSetup": true }}
                    ]
                  }},
                  "close": {{ "processes": ["nothing-here"] }},
                  "setup": {{ "missingSnapshotHint": "Add this account through setup first." }}
                }}
              }}
            }}"#
        )
    }

    /// The config cache and the poisoned-local flag are process-global, so any
    /// test that reaches config through the engine takes the same lock as
    /// `config`'s own tests instead of clearing state under them.
    fn config_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::config::config_io_test_mutex()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn service(live_root: &Path) -> DescriptorService {
        let descriptor = Descriptor::parse("test", &fixture(live_root)).unwrap();
        DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
    }

    fn seed_live_session(live_root: &Path, marker: &[u8]) {
        fs::create_dir_all(live_root.join("auth").join("nested")).unwrap();
        fs::write(live_root.join("session.json"), marker).unwrap();
        fs::write(
            live_root.join("auth").join("nested").join("token.bin"),
            marker,
        )
        .unwrap();
    }

    #[test]
    fn account_ids_are_checked_against_the_declared_charset() {
        let root = scratch("id-validation");
        let service = service(&root.join("live"));

        assert_eq!(
            service.validate_account_id("").unwrap_err(),
            "Empty Test account ID"
        );
        assert_eq!(
            service.validate_account_id("../../evil").unwrap_err(),
            "Invalid Test account ID: ../../evil"
        );
        assert_eq!(
            service.validate_account_id("  a3f0c2d1  ").unwrap(),
            "a3f0c2d1"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_reads_the_declared_paths_rather_than_a_launcher() {
        let _config = config_guard();
        let root = scratch("detect");
        let live = root.join("live");
        let ctx: AppCtx = Arc::new(TempCtx { root: root.clone() });
        let service = service(&live);

        assert!(!service.is_installed(ctx.clone()));
        fs::create_dir_all(&live).unwrap();
        assert!(service.is_installed(ctx));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restoring_without_a_snapshot_names_the_account_and_the_way_out() {
        let _config = config_guard();
        let root = scratch("missing-snapshot");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&root.join("live"));

        let err = service.restore_snapshot(&ctx, "a3f0c2d1").unwrap_err();
        assert_eq!(
            err,
            "No auth snapshot found for account a3f0c2d1. Add this account through setup first."
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_live_path_outside_the_roots_never_reaches_the_engine() {
        let root = scratch("sandbox");
        let live = root.join("live");
        let live_text = live.display().to_string().replace('\\', "/");

        // Same descriptor, but the session file now sits next to the declared
        // root instead of inside it. Validation refuses it at load, so the
        // engine is never handed a descriptor that could escape.
        let json = fixture(&live).replace(
            &format!("{live_text}/session.json"),
            &format!("{live_text}-elsewhere/session.json"),
        );
        let err = Descriptor::parse("test", &json).unwrap_err();
        assert!(err.problem.contains("declared roots"), "{err}");
        assert!(err.field.ends_with("state.files[0].live"), "{err}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_lists_every_file_and_folder_without_writing_any() {
        let _config = config_guard();
        let root = scratch("dry-run");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        seed_live_session(&live, b"live-session");

        let service = service(&live);
        let plan = service.plan_switch(&ctx, "a3f0c2d1").unwrap();

        assert_eq!(plan.platform_id, "gog");
        assert!(!plan.applied);
        assert!(plan
            .steps
            .iter()
            .any(|s| s.action == PlanAction::Restore && s.target.ends_with("session.json")));
        assert!(plan
            .steps
            .iter()
            .any(|s| s.action == PlanAction::Close && s.target.contains("nothing-here")));
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("No snapshot stored for account a3f0c2d1")));

        // Nothing was captured: the account's snapshot directory is still absent.
        assert!(!service.snapshot_root(&ctx, "a3f0c2d1").unwrap().exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn dry_run_reports_the_roots_it_would_stay_inside() {
        let _config = config_guard();
        let root = scratch("dry-run-roots");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(&live).unwrap();

        let plan = service(&live).plan_switch(&ctx, "a3f0c2d1").unwrap();
        assert_eq!(plan.roots.len(), 1);
        assert!(plan.roots[0].to_lowercase().contains("live"));
        let _ = fs::remove_dir_all(&root);
    }

    // The tests below capture a snapshot, which encrypts through the OS
    // backend. That is DPAPI on Windows and always available; elsewhere it is
    // the login keyring, which a headless build has no way to reach. Windows
    // is also the only OS any shipped descriptor targets today.
    #[cfg(windows)]
    #[test]
    fn capture_then_restore_brings_a_session_back_byte_for_byte() {
        let _config = config_guard();
        let root = scratch("round-trip");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        seed_live_session(&live, b"account-one");
        service.save_snapshot(&ctx, "aaaa1111").unwrap();
        assert!(service.has_snapshot(&ctx, "aaaa1111"));

        // A second account signs in and overwrites the live session.
        seed_live_session(&live, b"account-two");
        service.save_snapshot(&ctx, "bbbb2222").unwrap();

        service.restore_snapshot(&ctx, "aaaa1111").unwrap();
        assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-one");
        assert_eq!(
            fs::read(live.join("auth").join("nested").join("token.bin")).unwrap(),
            b"account-one"
        );

        service.restore_snapshot(&ctx, "bbbb2222").unwrap();
        assert_eq!(fs::read(live.join("session.json")).unwrap(), b"account-two");
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn a_snapshot_never_holds_the_session_in_plaintext() {
        let _config = config_guard();
        let root = scratch("encrypted");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        seed_live_session(&live, b"super-secret-token");
        service.save_snapshot(&ctx, "aaaa1111").unwrap();

        let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
        let stored = fs::read(snapshot.join("session.json")).unwrap();
        assert!(stored.starts_with(crate::snapshot_crypto::ENCRYPTED_HEADER));
        assert!(!stored.windows(18).any(|w| w == b"super-secret-token"));
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn capture_drops_a_stale_snapshot_when_the_live_file_is_gone() {
        // Otherwise a later restore resurrects the previous account's file.
        let _config = config_guard();
        let root = scratch("stale");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        seed_live_session(&live, b"first");
        service.save_snapshot(&ctx, "aaaa1111").unwrap();

        fs::remove_file(live.join("session.json")).unwrap();
        service.save_snapshot(&ctx, "aaaa1111").unwrap();

        let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
        assert!(!snapshot.join("session.json").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clearing_the_live_state_removes_exactly_what_setup_declares() {
        let _config = config_guard();
        let root = scratch("clear");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        seed_live_session(&live, b"session");
        fs::write(live.join("keep-me.txt"), b"preferences").unwrap();

        service.clear_live_state(&ctx).unwrap();

        assert!(!live.join("session.json").exists());
        assert!(!live.join("auth").exists());
        assert!(live.join("keep-me.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn forgetting_an_account_removes_its_snapshot_directory() {
        let _config = config_guard();
        let root = scratch("forget");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        seed_live_session(&live, b"session");
        service.save_snapshot(&ctx, "aaaa1111").unwrap();
        let snapshot = service.snapshot_root(&ctx, "aaaa1111").unwrap();
        assert!(snapshot.exists());

        service.forget(&ctx, "aaaa1111").unwrap();
        assert!(!snapshot.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn restore_leaves_no_staging_directory_behind() {
        let _config = config_guard();
        let root = scratch("staging");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        seed_live_session(&live, b"session");
        service.save_snapshot(&ctx, "aaaa1111").unwrap();
        service.restore_snapshot(&ctx, "aaaa1111").unwrap();

        assert!(!live.join("auth.accshift-restore-tmp").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn accounts_list_what_the_config_holds_plus_anything_with_a_snapshot() {
        let _config = config_guard();
        let root = scratch("accounts");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        let service = service(&live);

        assert!(service.read_accounts(&ctx).unwrap().is_empty());

        seed_live_session(&live, b"session");
        service.save_snapshot(&ctx, "aaaa1111").unwrap();
        config_bridge::touch_account(&ctx, "gog", "aaaa1111", 1234).unwrap();

        let accounts = service.read_accounts(&ctx).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_id, "aaaa1111");
        assert!(accounts[0].snapshot_saved);
        assert_eq!(accounts[0].last_used_at, Some(1234));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn freshness_rejects_missing_empty_and_stale_files() {
        let root = scratch("freshness");
        assert!(!file_is_fresh(&root.join("missing"), 60_000));

        let empty = root.join("empty");
        fs::write(&empty, b"").unwrap();
        assert!(!file_is_fresh(&empty, 60_000));

        let recent = root.join("recent");
        fs::write(&recent, b"data").unwrap();
        assert!(file_is_fresh(&recent, 60_000));

        // Same file, written ten minutes ago: the launcher never flushed the
        // new session, so the account it describes is the previous one.
        let stale = root.join("stale");
        fs::write(&stale, b"data").unwrap();
        let handle = fs::OpenOptions::new().write(true).open(&stale).unwrap();
        handle
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(600))
            .unwrap();
        assert!(!file_is_fresh(&stale, 60_000));
        let _ = fs::remove_dir_all(&root);
    }

    /// A platform that reads its account id out of a log, finds accounts by
    /// listing a directory, and keeps its session file in one of two places.
    /// Modelled on Ubisoft, whose id is `ubisoft` so the config bridge reaches
    /// the section that holds the forget blocklist.
    fn log_fixture(live_root: &Path) -> String {
        let live = live_root.display().to_string().replace('\\', "/");
        format!(
            r#"{{
              "id": "ubisoft",
              "schemaVersion": 1,
              "name": "Test Connect",
              "shortName": "Test",
              "os": {{
                "windows": {{ {profile} }},
                "linux": {{ {profile} }},
                "macos": {{ {profile} }}
              }}
            }}"#,
            profile = log_profile(&live)
        )
    }

    fn log_profile(live: &str) -> String {
        format!(
            r#"
              "roots": {{ "files": ["{live}"] }},
              "detect": {{ "pathExists": ["{live}"] }},
              "identity": {{
                "source": {{
                  "kind": "logTail",
                  "path": "{live}/logs/launcher_log.txt",
                  "lineContains": "AccountStartupUser.cpp",
                  "prefix": "User: ",
                  "nearWord": "User"
                }},
                "format": {{
                  "charset": "uuid",
                  "maxLength": 36,
                  "minLength": 36,
                  "lowercase": true,
                  "invalidMessage": "Invalid Test account UUID"
                }},
                "current": "identity",
                "discovery": [
                  {{ "kind": "directoryEntries", "path": "{live}/savegames", "entries": "directories" }}
                ],
                "blocklistOnForget": true
              }},
              "state": {{
                "files": [
                  {{ "live": "{live}/user.dat", "snapshot": "user.dat", "snapshotMarker": true }},
                  {{
                    "live": ["{live}/Config/Windows/settings.ini", "{live}/Config/WindowsEditor/settings.ini"],
                    "snapshot": "settings.ini"
                  }}
                ]
              }},
              "close": {{ "processes": ["nothing-here.exe"] }},
              "setup": {{ "missingSnapshotHint": "Sign in to this account once first." }}
            "#
        )
    }

    fn log_service(live_root: &Path) -> DescriptorService {
        let descriptor = Descriptor::parse("test", &log_fixture(live_root)).unwrap();
        DescriptorService::new(descriptor, DescriptorOrigin::Embedded)
    }

    const UUID_ONE: &str = "a9da419c-1234-5678-9abc-def012345678";
    const UUID_TWO: &str = "deadbeef-0000-1111-2222-333344445555";

    #[test]
    fn the_account_id_comes_from_the_last_matching_line_of_the_log() {
        let _config = config_guard();
        let root = scratch("log-tail");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(live.join("logs")).unwrap();
        fs::write(
            live.join("logs").join("launcher_log.txt"),
            format!(
                "[00:00] AccountStartupUser.cpp - User: {UUID_ONE} logged in\n\
                 [00:01] Something.cpp - User: 00000000-0000-0000-0000-000000000000\n\
                 [00:02] AccountStartupUser.cpp - User: {UUID_TWO} logged in\n"
            ),
        )
        .unwrap();

        let service = log_service(&live);
        assert_eq!(service.read_identity(&ctx).as_deref(), Some(UUID_TWO));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_id_written_in_the_launchers_own_case_reads_back_as_one_account() {
        let _config = config_guard();
        let root = scratch("log-case");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(live.join("logs")).unwrap();
        fs::write(
            live.join("logs").join("launcher_log.txt"),
            format!(
                "AccountStartupUser.cpp - User={} done\n",
                UUID_ONE.to_uppercase()
            ),
        )
        .unwrap();

        // No `User: ` prefix on that line: the nearby-word fallback finds it,
        // and `lowercase` folds it to the one spelling the engine stores.
        let service = log_service(&live);
        assert_eq!(service.read_identity(&ctx).as_deref(), Some(UUID_ONE));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_platform_keeps_its_own_wording_for_a_bad_account_id() {
        let root = scratch("id-wording");
        let service = log_service(&root.join("live"));
        assert_eq!(
            service.validate_account_id("nope").unwrap_err(),
            "Invalid Test account UUID"
        );
        assert_eq!(
            service.validate_account_id("").unwrap_err(),
            "Invalid Test account UUID"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn accounts_added_outside_the_app_are_discovered_and_stay_forgotten_once_forgotten() {
        let _config = config_guard();
        let root = scratch("discovery");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(live.join("savegames").join(UUID_ONE)).unwrap();
        fs::create_dir_all(live.join("savegames").join(UUID_TWO)).unwrap();
        // A file, not a directory, and not an id: neither may be listed.
        fs::write(live.join("savegames").join("readme.txt"), b"x").unwrap();

        let service = log_service(&live);
        let mut ids: Vec<String> = service
            .read_accounts(&ctx)
            .unwrap()
            .into_iter()
            .map(|a| a.account_id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec![UUID_ONE.to_string(), UUID_TWO.to_string()]);

        // The directory is still on disk, so without the blocklist the next
        // listing would put the account straight back.
        service.forget(&ctx, UUID_ONE).unwrap();
        let after: Vec<String> = service
            .read_accounts(&ctx)
            .unwrap()
            .into_iter()
            .map(|a| a.account_id)
            .collect();
        assert_eq!(after, vec![UUID_TWO.to_string()]);

        // Using it again is a clear statement that it should come back.
        config_bridge::touch_account(&ctx, "ubisoft", UUID_ONE, 1234).unwrap();
        assert_eq!(service.read_accounts(&ctx).unwrap().len(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_with_several_candidates_uses_the_one_that_exists() {
        let _config = config_guard();
        let root = scratch("first-existing");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(live.join("Config").join("WindowsEditor")).unwrap();
        fs::write(
            live.join("Config")
                .join("WindowsEditor")
                .join("settings.ini"),
            b"editor",
        )
        .unwrap();

        let plan = log_service(&live).plan_switch(&ctx, UUID_ONE).unwrap();
        let restored: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == PlanAction::Restore)
            .map(|s| s.target.as_str())
            .collect();
        assert!(
            restored.iter().any(|t| t.contains("WindowsEditor")),
            "{restored:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_with_several_candidates_falls_back_to_the_first_when_none_exist() {
        // Otherwise a file the launcher has not written yet would have nowhere
        // to be restored to.
        let _config = config_guard();
        let root = scratch("first-listed");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(&live).unwrap();

        let plan = log_service(&live).plan_switch(&ctx, UUID_ONE).unwrap();
        let restored: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.action == PlanAction::Restore)
            .map(|s| s.target.as_str())
            .collect();
        assert!(
            restored
                .iter()
                .any(|t| t.contains("Config") && !t.contains("WindowsEditor")),
            "{restored:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_restore_leaves_every_live_file_as_it_was() {
        // The session files are one credential set: a half-applied restore
        // would leave the incoming account's file next to the outgoing one's.
        let _config = config_guard();
        let root = scratch("staged-restore");
        let live = root.join("live");
        let ctx = TempCtx { root: root.clone() };
        fs::create_dir_all(live.join("Config").join("Windows")).unwrap();
        fs::write(live.join("user.dat"), b"incoming").unwrap();
        fs::write(
            live.join("Config").join("Windows").join("settings.ini"),
            b"incoming",
        )
        .unwrap();

        let service = log_service(&live);
        service.save_snapshot(&ctx, UUID_ONE).unwrap();

        // The second snapshot file is replaced by a directory, so decrypting it
        // fails after the first one has already been staged.
        let snapshot = service.snapshot_root(&ctx, UUID_ONE).unwrap();
        fs::remove_file(snapshot.join("settings.ini")).unwrap();
        fs::create_dir_all(snapshot.join("settings.ini")).unwrap();

        fs::write(live.join("user.dat"), b"outgoing").unwrap();
        fs::write(
            live.join("Config").join("Windows").join("settings.ini"),
            b"outgoing",
        )
        .unwrap();

        assert!(service.restore_snapshot(&ctx, UUID_ONE).is_err());
        assert_eq!(fs::read(live.join("user.dat")).unwrap(), b"outgoing");
        assert_eq!(
            fs::read(live.join("Config").join("Windows").join("settings.ini")).unwrap(),
            b"outgoing"
        );
        assert!(!live.join("user.dat.accshift-restore-tmp").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn non_empty_check_walks_into_subdirectories_only_when_asked() {
        let root = scratch("non-empty");
        let dir = root.join("auth");
        fs::create_dir_all(dir.join("nested")).unwrap();
        assert!(!path_has_content(&dir, true));

        fs::write(dir.join("nested").join("token.bin"), b"x").unwrap();
        assert!(path_has_content(&dir, true));
        assert!(!path_has_content(&dir, false));
        let _ = fs::remove_dir_all(&root);
    }
}
