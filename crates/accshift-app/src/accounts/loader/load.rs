//! Loading the account list, and the background refresh a load asks for.

use super::*;

impl AccountLoader {
    /// Starts a load. The caller runs `adapter.startup_snapshot` on the job
    /// pool and hands the answer to [`AccountLoader::finish_load`].
    pub fn begin_load(&mut self, hooks: &dyn LoaderHooks, options: LoadOptions) -> LoadTicket {
        self.latest_load_id += 1;
        let ticket = LoadTicket {
            load_id: self.latest_load_id,
            epoch: self.current_account_epoch,
            platform_id: hooks.platform_id().to_string(),
            options,
        };
        self.bump_prime_run();
        self.loading = true;
        self.error = None;
        self.replace_warning_states(hooks.cached_warning_states().unwrap_or_default());
        ticket
    }

    /// Lands a load's snapshot. `visible` is the ids on screen (`None` when
    /// the caller has no such reader); `now_ms` drives the toast cooldowns.
    pub fn finish_load(
        &mut self,
        ticket: &LoadTicket,
        result: Result<StartupSnapshot, BackendError>,
        hooks: &dyn LoaderHooks,
        visible: Option<&[String]>,
        now_ms: f64,
    ) -> LoadOutcome {
        if self.add_reload_load_id == Some(ticket.load_id) {
            self.add_reload_load_id = None;
            self.adding = false;
        }
        let mut outcome = LoadOutcome::default();
        if ticket.load_id != self.latest_load_id {
            return outcome;
        }
        outcome.applied = true;
        let options = ticket.options;
        match result {
            Ok(snapshot) => {
                self.set_accounts(snapshot.accounts);
                if ticket.epoch == self.current_account_epoch {
                    self.current_account = snapshot.current_account;
                }
                let seed = self.resolve_visible(visible);
                self.avatars
                    .seed_for_accounts(&seed, options.force_refresh, hooks);
                if self.accounts.is_empty() {
                    if let Some(text) = hooks.no_accounts_toast() {
                        if now_ms - self.last_no_accounts_toast_at >= LOAD_TOAST_COOLDOWN_MS {
                            outcome.toasts.push(Toast::new(text, ToastKind::Default));
                            self.last_no_accounts_toast_at = now_ms;
                        }
                    }
                } else if options.show_refreshed_toast && !options.silent {
                    let count = self.accounts.len();
                    let key = if count == 1 {
                        "toast.accountsRefreshed.single"
                    } else {
                        "toast.accountsRefreshed.multiple"
                    };
                    outcome.toasts.push(Toast::new(
                        Text::with(key, [("count", count as f64)]),
                        ToastKind::Success,
                    ));
                }
                outcome.after_load = Some(ticket.platform_id.clone());
                outcome.background = Some(BackgroundRefresh {
                    load_id: ticket.load_id,
                    check_bans: options.check_bans,
                    force_refresh: options.force_refresh,
                    silent: options.silent,
                    deferred: options.defer_background,
                });
            }
            Err(error) => {
                let message = error.message;
                self.error = Some(message.clone());
                self.set_accounts(Vec::new());
                self.current_account.clear();
                self.replace_warning_states(BTreeMap::new());
                if let Some(text) = hooks.load_error_toast(&message) {
                    if now_ms - self.last_load_error_toast_at >= LOAD_TOAST_COOLDOWN_MS {
                        outcome.toasts.push(Toast::new(text, ToastKind::Error));
                        self.last_load_error_toast_at = now_ms;
                    }
                }
            }
        }
        self.loading = false;
        outcome
    }

    /// The refresh of the visible accounts, unless a newer load started.
    /// Never deferred again.
    pub fn run_background(
        &mut self,
        background: &BackgroundRefresh,
        visible: Option<&[String]>,
    ) -> Option<RefreshTicket> {
        if background.load_id != self.latest_load_id {
            return None;
        }
        self.refresh_visible_accounts(
            visible,
            background.check_bans,
            background.force_refresh,
            background.silent,
            false,
        )
    }
}
