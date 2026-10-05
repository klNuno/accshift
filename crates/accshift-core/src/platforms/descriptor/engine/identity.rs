//! Account ids: discovery, the live identity and its sources.

#[allow(unused_imports)]
use super::*;

impl DescriptorService {
    pub(super) fn id_is_valid(&self, candidate: &str) -> bool {
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
    pub(super) fn normalise_id(&self, raw: &str) -> String {
        let trimmed = raw.trim();
        match self.profile() {
            Ok(profile) if profile.identity.format.lowercase => trimmed.to_lowercase(),
            _ => trimmed.to_string(),
        }
    }

    /// The id is joined into snapshot paths, so anything outside the declared
    /// charset is refused before it reaches the filesystem.
    pub(super) fn validate_account_id(&self, id: &str) -> Result<String, String> {
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
    pub(super) fn read_identity(&self, app: &dyn AppContext) -> Option<String> {
        let runtime = self.runtime(app).ok()?;
        self.read_identity_in(&runtime)
    }

    /// The id the launcher currently reports, when it reports one at all.
    ///
    /// Takes a runtime the caller already built, so a poll that touches
    /// several things does not resolve the launcher once per step.
    pub(super) fn read_identity_in(&self, runtime: &Runtime<'_>) -> Option<String> {
        self.read_identity_detail(runtime).map(|found| found.id)
    }

    /// The signed-in account with whatever name came with it. Only a native
    /// hook reports a name; every other source knows the id alone.
    pub(super) fn read_identity_detail(&self, runtime: &Runtime<'_>) -> Option<HookIdentity> {
        let found = match &runtime.profile.identity.source {
            IdentitySource::Registry { root, key, value } => {
                reg::read(*root, key, value).map(bare_identity)
            }
            IdentitySource::LogTail { .. } => {
                self.read_identity_from_log(runtime).map(bare_identity)
            }
            // Nothing readable: the account is whatever we last put there.
            IdentitySource::Synthetic => None,
            IdentitySource::NativeHook { name, paths } => {
                let hook = hooks::hook(name)?;
                // Only paths the descriptor declared, and only after the
                // sandbox has cleared them: a hook cannot widen its own reach.
                let resolved = paths
                    .iter()
                    .filter_map(|(key, template)| {
                        runtime.path(template).ok().map(|path| (key.clone(), path))
                    })
                    .collect();
                hook.identity(&HookContext::new(resolved))
            }
        }?;
        let id = self.normalise_id(&found.id);
        self.id_is_valid(&id).then_some(HookIdentity {
            id,
            display_name: found
                .display_name
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
        })
    }

    /// Reads the id out of the launcher's own log, most recent line first.
    pub(super) fn read_identity_from_log(&self, runtime: &Runtime<'_>) -> Option<String> {
        let IdentitySource::LogTail {
            path, tail_bytes, ..
        } = &runtime.profile.identity.source
        else {
            return None;
        };
        let resolved = runtime.path(path).ok()?;
        #[cfg(test)]
        probe(&self.probes.log_reads);
        let content = read_log_tail(&resolved, *tail_bytes)?;
        self.identity_in(runtime, &content)
    }

    /// The id in `content`, most recent matching line first.
    fn identity_in(&self, runtime: &Runtime<'_>, content: &str) -> Option<String> {
        let IdentitySource::LogTail {
            line_contains,
            prefix,
            near_word,
            ..
        } = &runtime.profile.identity.source
        else {
            return None;
        };
        // The id has one fixed width here: the schema refuses a log source
        // whose format allows a range, because a scan has nothing to match on.
        let width = runtime.profile.identity.format.max_length;
        content
            .lines()
            .rev()
            .filter(|line| line.contains(line_contains.as_str()))
            .find_map(|line| self.extract_id(line, prefix, near_word, width))
    }

    /// Identity lines that begin at or after `offset`.
    ///
    /// An offset that lands mid-line drops that partial line: it was already
    /// on disk at the switch, so its id is the previous account. A file that
    /// ended on a newline keeps every complete line of the suffix. A tail
    /// window that starts past the offset is already entirely new.
    fn read_identity_past(
        &self,
        runtime: &Runtime<'_>,
        path: &Path,
        offset: u64,
        tail_bytes: u64,
    ) -> Option<String> {
        #[cfg(test)]
        probe(&self.probes.log_reads);
        let (window_start, content) = read_log_window(path, offset, tail_bytes)?;
        let content = if window_start == offset && offset > 0 {
            match read_log_byte(path, offset - 1) {
                Some(b'\n' | b'\r') => content,
                _ => {
                    let end = content.find('\n')?;
                    content[end + 1..].to_string()
                }
            }
        } else {
            content
        };
        self.identity_in(runtime, &content)
    }

    /// Pulls an id out of one log line: the text right after `prefix`, else any
    /// id preceded within a few characters by `near_word`.
    ///
    /// Every slice goes through `get`: a log line carries user names and paths,
    /// so slicing at a byte offset would panic on a character boundary.
    pub(super) fn extract_id(
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
    pub(super) fn discovered_ids(
        &self,
        runtime: &Runtime<'_>,
        cfg: &AppConfig,
    ) -> BTreeSet<String> {
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
            let blocked = self.blocked_ids(cfg);
            ids.retain(|id| !blocked.contains(id));
        }
        ids
    }

    pub(super) fn blocked_ids(&self, cfg: &AppConfig) -> HashSet<String> {
        if !self
            .profile()
            .map(|profile| profile.identity.blocklist_on_forget)
            .unwrap_or(false)
        {
            return HashSet::new();
        }
        config_bridge::blocklist_in(cfg, &self.descriptor.id)
            .iter()
            .map(|id| self.normalise_id(id))
            .collect()
    }

    /// Which account is signed in, from whichever source the descriptor names.
    pub(super) fn current_account_id(&self, app: &dyn AppContext) -> Option<String> {
        let runtime = self.runtime(app).ok()?;
        self.current_from(&runtime, &config::load_config(app))
    }

    pub(super) fn current_from(&self, runtime: &Runtime<'_>, cfg: &AppConfig) -> Option<String> {
        match runtime.profile.identity.current {
            CurrentSource::Identity => self.live_identity(runtime, cfg),
            CurrentSource::Config => config_bridge::current_account_in(cfg, &self.descriptor.id)
                .map(|id| self.normalise_id(&id))
                .filter(|id| self.id_is_valid(id)),
        }
    }

    /// The id the launcher reports, unless a switch is still ahead of the log.
    ///
    /// A launcher logs its sign-in some time after it starts, and never when
    /// the restored session fails to sign in. A switch stores the log's
    /// length. Until an identity line appears past that offset, the account
    /// the switch put in place is the better answer. A startup line that only
    /// moves the modification time does not count. A shorter file is a
    /// rewrite, so it is scanned whole. Rows with no stored length still
    /// compare the modification time with the switch.
    pub(super) fn live_identity(&self, runtime: &Runtime<'_>, cfg: &AppConfig) -> Option<String> {
        let IdentitySource::LogTail {
            path, tail_bytes, ..
        } = &runtime.profile.identity.source
        else {
            return self.read_identity_in(runtime);
        };
        let Some(record) = config_bridge::last_switch_in(cfg, &self.descriptor.id) else {
            return self.read_identity_in(runtime);
        };
        let recorded = self.normalise_id(&record.account_id);
        if !self.id_is_valid(&recorded) {
            return self.read_identity_in(runtime);
        }
        let Some(log_len) = record.log_len else {
            let read = self.read_identity_in(runtime);
            let written = runtime
                .path(path)
                .ok()
                .and_then(|log| fs::metadata(log).ok())
                .and_then(|meta| meta.modified().ok())
                .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|since| since.as_millis() as u64);
            return match written {
                Some(written) if written > record.at => read,
                _ => Some(recorded),
            };
        };
        let Some(resolved) = runtime.path(path).ok() else {
            return Some(recorded);
        };
        let Some(len) = fs::metadata(&resolved).ok().map(|meta| meta.len()) else {
            return Some(recorded);
        };
        if len == log_len {
            return Some(recorded);
        }
        if len < log_len {
            return self.read_identity_from_log(runtime).or(Some(recorded));
        }
        self.read_identity_past(runtime, &resolved, log_len, *tail_bytes)
            .or(Some(recorded))
    }

    /// Remembers the account a switch put in place, for [`Self::live_identity`].
    /// Only a log lags behind a switch, so no other platform records one.
    pub(super) fn record_switch(&self, app: &dyn AppContext, account_id: &str) {
        let logged = self
            .profile()
            .map(|profile| matches!(profile.identity.source, IdentitySource::LogTail { .. }))
            .unwrap_or(false);
        if !logged {
            return;
        }
        // Not fatal: without the record the log is trusted, as it always was.
        // Zero when the log is missing: the first line written afterwards is new.
        if let Err(error) = config_bridge::set_last_switch(
            app,
            &self.descriptor.id,
            account_id,
            now_unix_ms(),
            Some(self.log_len_now(app)),
        ) {
            log_platform_error(
                app,
                &format!("{}.switch_account", self.descriptor.id),
                "Could not record the switch",
                error,
            );
        }
    }

    /// Length of the identity log right now. Zero when it cannot be read, so
    /// a later rewrite is scanned instead of sticking on the recorded id.
    fn log_len_now(&self, app: &dyn AppContext) -> u64 {
        let Ok(runtime) = self.runtime(app) else {
            return 0;
        };
        let IdentitySource::LogTail { path, .. } = &runtime.profile.identity.source else {
            return 0;
        };
        runtime
            .path(path)
            .ok()
            .and_then(|log| fs::metadata(log).ok())
            .map(|meta| meta.len())
            .unwrap_or(0)
    }

    pub(super) fn snapshot_root(
        &self,
        app: &dyn AppContext,
        account_id: &str,
    ) -> Result<PathBuf, String> {
        Ok(crate::storage::platform_snapshots_dir(app, &self.descriptor.id)?.join(account_id))
    }
}
