//! Per-account snapshots: capture, restore staging and markers.

#[allow(unused_imports)]
use super::*;

impl DescriptorService {
    pub(super) fn save_snapshot(
        &self,
        app: &dyn AppContext,
        account_id: &str,
    ) -> Result<(), String> {
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
            if !live.is_dir() && !item.clear_snapshot_when_source_missing {
                // The launcher has not written this directory yet. Copying
                // nothing over the previous capture would leave the account
                // with an empty snapshot, which restores as a signed-out
                // session.
                continue;
            }
            // On Linux and macOS every encrypted file in there owns a keyring
            // entry, and removing the directory is the only thing that still
            // knows the entry ids. Free them first or they leak, one full
            // capture's worth per switch.
            free_dir_secrets(&dest);
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

    /// The account's snapshot directory, or the error naming the way out when
    /// there is none. Checked before anything is closed or cleared.
    pub(super) fn require_snapshot(
        &self,
        app: &dyn AppContext,
        runtime: &Runtime<'_>,
        account_id: &str,
    ) -> Result<PathBuf, String> {
        let cache_dir = self.snapshot_root(app, account_id)?;
        if cache_dir.exists() {
            return Ok(cache_dir);
        }
        let hint = runtime.profile.setup.missing_snapshot_hint.trim();
        let mut message = format!("No auth snapshot found for account {account_id}.");
        if !hint.is_empty() {
            message.push(' ');
            message.push_str(hint);
        }
        Err(message)
    }

    /// Puts the account's snapshot in place of the live session, or leaves
    /// the live session as it was.
    ///
    /// Several files, folders and registry values are one credential set, so
    /// the restore runs in two phases. Everything is decrypted first, files
    /// and folders next to their live location and registry values into
    /// memory. Only then is the live state swapped, each step journaled, and
    /// a failed step undoes the ones before it.
    ///
    /// An item the snapshot does not hold is one the capture dropped because
    /// the account had none. When the descriptor clears it at capture, the
    /// live one belongs to the outgoing account and is removed.
    pub(super) fn restore_snapshot(
        &self,
        app: &dyn AppContext,
        account_id: &str,
    ) -> Result<(), String> {
        let runtime = self.runtime(app)?;
        let cache_dir = self.require_snapshot(app, &runtime, account_id)?;

        let mut steps = Vec::new();
        if let Err(error) = self.stage_restore(&runtime, &cache_dir, &mut steps) {
            discard_staging(&steps);
            return Err(error);
        }

        let mut journal = Vec::new();
        for step in &steps {
            if let Err(error) = apply_restore_step(step, &mut journal) {
                let mut message = error;
                for failure in roll_back(journal) {
                    message.push_str("; could not undo: ");
                    message.push_str(&failure);
                }
                discard_staging(&steps);
                return Err(message);
            }
        }
        for undo in journal {
            undo.forget_backup();
        }
        Ok(())
    }

    /// The first phase of a restore: decrypts everything, touches nothing
    /// live. Each staged item is pushed as soon as it exists, so a failure
    /// halfway can still clean up what came before it.
    pub(super) fn stage_restore(
        &self,
        runtime: &Runtime<'_>,
        cache_dir: &Path,
        steps: &mut Vec<RestoreStep>,
    ) -> Result<(), String> {
        let state = &runtime.profile.state;
        for item in &state.files {
            let source = cache_dir.join(&item.snapshot);
            let live = runtime.spec_path(&item.live)?;
            if !source.exists() {
                if item.clear_snapshot_when_source_missing && live.exists() {
                    steps.push(RestoreStep::RemoveFile { live });
                }
                continue;
            }
            if let Some(parent) = live.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
            }
            let staging = staging_path(&live);
            let result = decrypted_copy_file(&source, &staging);
            steps.push(RestoreStep::File {
                staging,
                live,
                remove_live_first: item.remove_live_before_restore,
            });
            result?;
        }

        for item in &state.registry_values {
            let source = cache_dir.join(&item.snapshot);
            if !source.exists() {
                if item.clear_snapshot_when_source_missing {
                    steps.push(RestoreStep::RemoveValue { item: item.clone() });
                }
                continue;
            }
            let bytes = read_decrypted_bytes(&source)?;
            let data = String::from_utf8(bytes).map_err(|_| {
                format!(
                    "Snapshot of {} is not text",
                    reg::display(item.root, &item.key, &item.value)
                )
            })?;
            steps.push(RestoreStep::Value {
                item: item.clone(),
                data: data.trim().to_string(),
            });
        }

        for item in &state.directories {
            let source = cache_dir.join(&item.snapshot);
            let live = runtime.spec_path(&item.live)?;
            if !source.exists() {
                if item.clear_snapshot_when_source_missing && live.exists() {
                    steps.push(RestoreStep::RemoveDir { live });
                }
                continue;
            }
            let staging = staging_path(&live);
            let _ = fs::remove_dir_all(&staging);
            let ignored: Vec<&str> = item.ignored_names.iter().map(String::as_str).collect();
            let result = snapshot_crypto::decrypted_copy_dir(
                &source,
                &staging,
                DirCopyOptions {
                    ignored_names: &ignored,
                    follow_symlinks: item.follow_symlinks,
                },
            );
            steps.push(RestoreStep::Dir { staging, live });
            result?;
        }
        Ok(())
    }

    /// Removes the launcher caches keyed to the account that just left. They
    /// are never captured: they hold no credential, only material the launcher
    /// rebuilds, and keeping them across a switch shows the previous account.
    pub(super) fn clear_caches(&self, app: &dyn AppContext) {
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
    #[cfg(all(test, windows))]
    pub(super) fn has_snapshot(&self, app: &dyn AppContext, account_id: &str) -> bool {
        self.snapshot_markers(app, account_id)
            .iter()
            .any(|path| path.exists())
    }

    /// Whether this account has anything worth restoring, under the
    /// platform's snapshots folder resolved once by the caller.
    pub(super) fn has_snapshot_in(&self, snapshots: Option<&Path>, account_id: &str) -> bool {
        snapshots.is_some_and(|dir| {
            self.markers_under(&dir.join(account_id))
                .iter()
                .any(|path| path.exists())
        })
    }

    /// Stricter check, for the moment right after a capture: a marker that
    /// exists but is empty means the launcher was closed before it wrote the
    /// session, and restoring it later would land on the login screen.
    pub(super) fn snapshot_has_content(&self, app: &dyn AppContext, account_id: &str) -> bool {
        self.snapshot_markers(app, account_id)
            .iter()
            .any(|path| path_has_content(path, true))
    }

    /// Whether anything in the descriptor can answer the two questions above.
    /// A descriptor declaring no marker never claims to hold a snapshot.
    pub(super) fn declares_snapshot_marker(&self) -> bool {
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

    pub(super) fn snapshot_markers(&self, app: &dyn AppContext, account_id: &str) -> Vec<PathBuf> {
        match self.snapshot_root(app, account_id) {
            Ok(cache_dir) => self.markers_under(&cache_dir),
            Err(_) => Vec::new(),
        }
    }

    pub(super) fn markers_under(&self, cache_dir: &Path) -> Vec<PathBuf> {
        let Ok(profile) = self.profile() else {
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
    pub(super) fn clear_live_state(&self, app: &dyn AppContext) -> Result<(), String> {
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
    pub(super) fn delete_snapshot(&self, app: &dyn AppContext, account_id: &str) {
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
    pub(super) fn capture_current_account(&self, app: &dyn AppContext) -> Result<(), String> {
        let Some(current_id) = self.current_account_id(app) else {
            return Ok(());
        };
        if !self.capture_worth_running(app) {
            // Nothing live to capture: keeping the snapshot taken while the
            // account was signed in beats replacing it with an empty one.
            return Ok(());
        }
        let Some(target) = self.capture_target(app, current_id) else {
            return Ok(());
        };
        let _ = config_bridge::touch_account(app, &self.descriptor.id, &target, now_unix_ms());
        self.save_snapshot(app, &target)
    }

    /// Which account's snapshot the live session goes into, `None` to skip
    /// the capture.
    ///
    /// A platform tracked through the config only knows which account we
    /// last put in place. When the launcher can also say who is signed in and
    /// names someone else, the user switched inside the launcher: capturing
    /// under the marker would overwrite that account's snapshot with another
    /// account's session. The session goes to the account it belongs to when
    /// that one is tracked, and nowhere otherwise.
    pub(super) fn capture_target(&self, app: &dyn AppContext, marker: String) -> Option<String> {
        let Ok(runtime) = self.runtime(app) else {
            return Some(marker);
        };
        let cfg = config::load_config(app);
        let identity = &runtime.profile.identity;
        if identity.current != CurrentSource::Config
            || matches!(identity.source, IdentitySource::Synthetic)
        {
            return Some(marker);
        }
        let Some(live) = self.live_identity(&runtime, &cfg) else {
            return Some(marker);
        };
        if live == marker {
            return Some(marker);
        }
        let source = format!("{}.capture", self.descriptor.id);
        let details = format!("marker={}; live={}", redact_id(&marker), redact_id(&live));
        let tracked = config_bridge::accounts_in(&cfg, &self.descriptor.id)
            .iter()
            .any(|account| self.normalise_id(&account.account_id) == live);
        if tracked {
            log_platform_info(
                app,
                &source,
                "Live session belongs to another tracked account, captured there",
                details,
            );
            Some(live)
        } else {
            log_platform_info(
                app,
                &source,
                "Live session belongs to an untracked account, capture skipped",
                details,
            );
            None
        }
    }

    /// Whether the descriptor's own gate on capturing is satisfied.
    pub(super) fn capture_worth_running(&self, app: &dyn AppContext) -> bool {
        let Ok(runtime) = self.runtime(app) else {
            return true;
        };
        runtime
            .profile
            .state
            .capture_when
            .iter()
            .all(|condition| self.condition_holds(&runtime, condition, ConditionInput::default()))
    }
}
