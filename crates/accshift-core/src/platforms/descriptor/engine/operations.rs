//! Operations that change state: switch, setup, forget.

#[allow(unused_imports)]
use super::*;

impl DescriptorService {
    pub(super) fn switch(&self, app: &dyn AppContext, account_id: &str) -> Result<(), String> {
        let account_id = self.validate_account_id(account_id)?;
        let source = format!("{}.switch_account", self.descriptor.id);
        log_platform_info(
            app,
            &source,
            &format!("{} switch requested", self.descriptor.short_name),
            format!("target={}", redact_id(&account_id)),
        );

        // Nothing is closed and no marker touched until the target is known
        // to be restorable: failing later leaves the launcher closed and, on
        // a platform we track ourselves, nobody recorded as signed in.
        {
            let runtime = self.runtime(app)?;
            self.require_snapshot(app, &runtime, &account_id)?;
        }

        let mut closed = false;
        let result = match self.switch_steps(app, &account_id, &mut closed) {
            Ok(()) => self.launch(app),
            Err(error) => {
                if closed {
                    self.relaunch_after_failure(app, &source);
                }
                Err(error)
            }
        };
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

    /// Everything a switch does between closing the launcher and starting
    /// it again. `closed` records whether the launcher was closed, so the
    /// caller knows to start it again when a later step fails.
    pub(super) fn switch_steps(
        &self,
        app: &dyn AppContext,
        account_id: &str,
        closed: &mut bool,
    ) -> Result<(), String> {
        let close_first = self.closes_before_capture();
        if close_first {
            // This client holds its session in memory and writes it out as it
            // exits, so capturing it while it runs would store nothing.
            self.quit_and_wait();
            *closed = true;
        }

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

        if !close_first {
            self.quit_and_wait();
            *closed = true;
        }
        self.restore_snapshot(app, account_id)?;
        self.clear_caches(app);
        config_bridge::touch_account(app, &self.descriptor.id, account_id, now_unix_ms())?;
        if uses_config_marker {
            config_bridge::set_current_account(app, &self.descriptor.id, account_id)?;
        }
        self.record_switch(app, account_id);
        Ok(())
    }

    /// Starts the launcher again after an operation closed it and then
    /// failed, so the user is not left without it. The operation's own error
    /// is the one reported; a failed start is only logged.
    pub(super) fn relaunch_after_failure(&self, app: &dyn AppContext, source: &str) {
        if let Err(error) = self.launch(app) {
            log_platform_error(
                app,
                source,
                &format!(
                    "{} relaunch after a failure failed",
                    self.descriptor.short_name
                ),
                error,
            );
        }
    }

    pub(super) fn begin(&self, app: &dyn AppContext) -> Result<SetupStatus, String> {
        let source = format!("{}.begin_account_setup", self.descriptor.id);
        log_platform_info(
            app,
            &source,
            &format!("{} account setup requested", self.descriptor.short_name),
            "",
        );

        let mut closed = false;
        let setup_id = match self.begin_steps(app, &mut closed) {
            Ok(BeginOutcome::Adopted(status)) => return Ok(status),
            Ok(BeginOutcome::AwaitSignIn(setup_id)) => setup_id,
            Err(error) => {
                if closed {
                    self.relaunch_after_failure(app, &source);
                }
                return Err(error);
            }
        };

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

    /// Everything a setup does before the launcher is started on the login
    /// screen. `closed` works as in [`Self::switch_steps`].
    pub(super) fn begin_steps(
        &self,
        app: &dyn AppContext,
        closed: &mut bool,
    ) -> Result<BeginOutcome, String> {
        let was_running = self.is_running();
        let close_first = self.closes_before_capture();
        if close_first {
            self.quit_and_wait();
            *closed = true;
        }

        // Everything that already exists, so the flow can tell the account the
        // user is about to add from the ones that were there before.
        let runtime = self.runtime(app)?;
        let stored: HashSet<String> = config_bridge::accounts(app, &self.descriptor.id)
            .iter()
            .map(|account| self.normalise_id(&account.account_id))
            .filter(|id| !id.is_empty())
            .collect();
        let live = self.read_identity_detail(&runtime);
        let mut known: HashSet<String> = self
            .discovered_ids(&runtime, &config::load_config(app))
            .into_iter()
            .collect();
        known.extend(live.as_ref().map(|found| found.id.clone()));
        known.extend(stored.iter().cloned());

        if runtime.profile.setup.adopt_signed_in {
            if let Some(status) = self.try_adopt(app, live, &stored, was_running)? {
                return Ok(BeginOutcome::Adopted(status));
            }
        }

        self.capture_current_account(app)?;

        let setup_id = format!("{}-setup-{}", self.descriptor.id, Uuid::new_v4());
        self.jobs.insert(
            setup_id.clone(),
            SetupJob {
                known_account_ids: known,
                started_at: now_unix_ms(),
            },
        )?;

        if !close_first {
            self.quit_and_wait();
            *closed = true;
        }
        self.clear_live_state(app)?;
        if self
            .profile()
            .map(|p| p.identity.current == CurrentSource::Config)
            .unwrap_or(false)
        {
            // Nobody is signed in until the flow completes.
            config_bridge::set_current_account(app, &self.descriptor.id, "")?;
        }
        Ok(BeginOutcome::AwaitSignIn(setup_id))
    }

    /// Takes the session that is already signed in as the account being added,
    /// when nothing tracks it yet.
    ///
    /// Without this, adding an account starts by wiping the one the user was
    /// already using, so their first account is signed out just to be listed.
    /// Adopting is refused as soon as a current account is recorded: that
    /// session already belongs to a tracked account, possibly under an id we
    /// minted ourselves, and adopting would list it twice.
    pub(super) fn try_adopt(
        &self,
        app: &dyn AppContext,
        live: Option<HookIdentity>,
        stored: &HashSet<String>,
        was_running: bool,
    ) -> Result<Option<SetupStatus>, String> {
        if self
            .current_account_id(app)
            .is_some_and(|current| !current.is_empty())
        {
            return Ok(None);
        }
        let Some(found) = live else {
            return Ok(None);
        };
        if stored.contains(&found.id) {
            return Ok(None);
        }

        self.save_snapshot(app, &found.id)?;
        if self.declares_snapshot_marker() && !self.snapshot_has_content(app, &found.id) {
            // Nothing was captured, so there is no session to adopt after all.
            // Fall through to the sign-in flow rather than list an account
            // that restores to a login screen.
            self.delete_snapshot(app, &found.id);
            return Ok(None);
        }
        config_bridge::touch_account(app, &self.descriptor.id, &found.id, now_unix_ms())?;
        if self
            .profile()
            .map(|profile| profile.identity.current == CurrentSource::Config)
            .unwrap_or(false)
        {
            config_bridge::set_current_account(app, &self.descriptor.id, &found.id)?;
        }
        let display_name = self.seed_label(app, &found);

        // Put the client back the way we found it: same session, no sign-in.
        if was_running {
            let _ = self.launch(app).inspect_err(|e| {
                log_platform_error(
                    app,
                    &format!("{}.begin_account_setup", self.descriptor.id),
                    &format!("{} relaunch after adopt failed", self.descriptor.short_name),
                    e.clone(),
                );
            });
        }

        log_platform_info(
            app,
            &format!("{}.begin_account_setup", self.descriptor.id),
            &format!("Adopted live {} session", self.descriptor.short_name),
            format!("account={}", redact_id(&found.id)),
        );

        // Terminal status: no job is registered, so the wizard never polls.
        let setup_id = format!("{}-setup-{}", self.descriptor.id, Uuid::new_v4());
        Ok(Some(make_setup_status(
            &setup_id,
            "ready",
            found.id,
            display_name,
            "",
        )))
    }

    /// Names a freshly captured account after whatever the platform calls it,
    /// so the list never opens on a raw id. Returns the name to report.
    pub(super) fn seed_label(&self, app: &dyn AppContext, found: &HookIdentity) -> String {
        match &found.display_name {
            Some(name) => {
                let _ = config_bridge::set_label(app, &self.descriptor.id, &found.id, name);
                name.clone()
            }
            None => {
                let from_id = self
                    .profile()
                    .map(|profile| profile.setup.display_name_from_id)
                    .unwrap_or(false);
                if from_id {
                    found.id.clone()
                } else {
                    String::new()
                }
            }
        }
    }

    pub(super) fn closes_before_capture(&self) -> bool {
        self.profile()
            .map(|profile| profile.close.before_capture)
            .unwrap_or(false)
    }

    pub(super) fn setup_status(
        &self,
        app: &dyn AppContext,
        setup_id: &str,
    ) -> Result<SetupStatus, String> {
        let job = self.jobs.touch(setup_id)?;
        let runtime = self.runtime(app)?;
        let setup = &runtime.profile.setup;

        // The launcher's own marker first, then anything the platform leaves on
        // disk: some write the id where we can read it only after a restart.
        let found = self
            .read_identity_detail(&runtime)
            .filter(|found| !job.known_account_ids.contains(&found.id));
        let new_identity = found.as_ref().map(|found| found.id.clone()).or_else(|| {
            self.discovered_ids(&runtime, &config::load_config(app))
                .into_iter()
                .find(|id| !job.known_account_ids.contains(id))
        });
        let input = ConditionInput {
            new_identity: new_identity.as_deref(),
            started_at: Some(job.started_at),
        };

        let triggered = !setup.trigger.is_empty()
            && setup
                .trigger
                .iter()
                .all(|condition| self.condition_holds(&runtime, condition, input));

        if triggered {
            // A trigger only says the user got through the login screen. The
            // launcher may still hold the session in memory, so it is closed
            // and the conditions re-checked before anything is captured.
            self.quit_and_wait();
            let source = format!("{}.get_setup_status", self.descriptor.id);
            // Every way back to waiting starts the launcher again: the user
            // cannot finish signing in with it closed.
            let keep_waiting = || {
                self.relaunch_after_failure(app, &source);
                make_setup_status(setup_id, "waiting_for_login", "", "", "")
            };

            let still_holds = setup
                .confirm
                .iter()
                .all(|condition| self.condition_holds(&runtime, condition, input));
            if !still_holds {
                return Ok(keep_waiting());
            }

            let synthetic = matches!(runtime.profile.identity.source, IdentitySource::Synthetic);
            let key = if synthetic {
                generate_account_id()
            } else {
                match new_identity {
                    Some(id) => id,
                    // The confirm pass says a session exists but no id came
                    // with it: keep waiting rather than store an unnamed one.
                    None => return Ok(keep_waiting()),
                }
            };

            // An id we minted names nothing once its capture is rejected, and
            // each poll mints a new one: its folder goes with the rejection.
            let reject = |key: &str| {
                if synthetic {
                    self.delete_snapshot(app, key);
                }
            };
            if let Err(error) = self.save_snapshot(app, &key) {
                reject(&key);
                self.relaunch_after_failure(app, &source);
                return Err(error);
            }
            if self.declares_snapshot_marker() && !self.snapshot_has_content(app, &key) {
                // The capture produced nothing worth restoring: the launcher
                // was closed before it wrote the session. Keep waiting rather
                // than hand back an account that restores to a login screen.
                reject(&key);
                return Ok(keep_waiting());
            }
            config_bridge::touch_account(app, &self.descriptor.id, &key, now_unix_ms())?;
            if runtime.profile.identity.current == CurrentSource::Config {
                config_bridge::set_current_account(app, &self.descriptor.id, &key)?;
            }

            self.jobs.remove(setup_id);

            // A hook that read a name off the platform names the account with
            // it, so the list never opens on a raw id. The id is checked first
            // because a synthetic key belongs to no identity that was read.
            let display_name = match found.filter(|found| found.id == key) {
                Some(found) => self.seed_label(app, &found),
                None if setup.display_name_from_id => key.clone(),
                None => String::new(),
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

    pub(super) fn condition_holds(
        &self,
        runtime: &Runtime<'_>,
        condition: &Condition,
        input: ConditionInput<'_>,
    ) -> bool {
        match condition {
            Condition::NewIdentity => input.new_identity.is_some(),
            Condition::IdentityPresent => self.read_identity_in(runtime).is_some(),
            Condition::SinceStart { ms } => match input.started_at {
                Some(started_at) => now_unix_ms().saturating_sub(started_at) >= *ms,
                // Asked outside a setup flow, where there is no start to
                // measure from. Nothing has elapsed, so it does not hold.
                None => false,
            },
            Condition::AnyOf { conditions } => conditions
                .iter()
                .any(|nested| self.condition_holds(runtime, nested, input)),
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

    pub(super) fn forget(&self, app: &dyn AppContext, account_id: &str) -> Result<(), String> {
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
}
