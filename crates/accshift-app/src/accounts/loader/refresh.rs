//! Profile and warning refreshes of some accounts.

use super::*;

impl AccountLoader {
    /// Refreshes the accounts on screen.
    pub fn refresh_visible_accounts(
        &mut self,
        visible: Option<&[String]>,
        check_bans: bool,
        force_refresh: bool,
        silent: bool,
        deferred: bool,
    ) -> Option<RefreshTicket> {
        let targets = self.resolve_visible(visible);
        self.refresh_accounts(targets, check_bans, force_refresh, silent, deferred)
    }

    /// Refreshes the given accounts.
    pub fn refresh_account_ids(
        &mut self,
        account_ids: &[String],
        check_bans: bool,
        force_refresh: bool,
        silent: bool,
        deferred: bool,
    ) -> Option<RefreshTicket> {
        let targets = resolve_accounts_by_ids(&self.accounts, account_ids);
        self.refresh_accounts(targets, check_bans, force_refresh, silent, deferred)
    }

    /// `None` when there is nothing to refresh or no adapter. Starts a prime
    /// run.
    fn refresh_accounts(
        &mut self,
        targets: Vec<PlatformAccount>,
        check_bans: bool,
        force_refresh: bool,
        silent: bool,
        deferred: bool,
    ) -> Option<RefreshTicket> {
        if targets.is_empty() {
            return None;
        }
        let platform_id = self.adapter_id.clone()?;
        let run_id = self.new_prime_run();
        Some(RefreshTicket {
            run_id,
            platform_id,
            targets,
            check_bans,
            force_refresh,
            silent,
            deferred,
        })
    }

    /// Seeds the avatars and says what to fetch. `None` when the run was
    /// superseded.
    pub fn run_refresh(
        &mut self,
        ticket: RefreshTicket,
        hooks: &dyn LoaderHooks,
    ) -> Option<RefreshWork> {
        if !self.should_continue(&ticket) {
            return None;
        }
        let profiles = self
            .avatars
            .seed_for_accounts(&ticket.targets, ticket.force_refresh, hooks);
        let warnings = ticket.check_bans && hooks.has_warnings();
        Some(RefreshWork {
            ticket,
            profiles,
            warnings,
        })
    }

    /// The profile half of a run, landed. False when superseded.
    pub fn finish_refresh_profiles(
        &mut self,
        ticket: &RefreshTicket,
        results: ProfileResults,
    ) -> bool {
        let applied = self.should_continue(ticket);
        if applied {
            let mut accounts = std::mem::take(&mut self.accounts);
            let changed = match &results {
                ProfileResults::Batch {
                    accounts: targets,
                    profiles,
                } => self
                    .avatars
                    .apply_batch_profile_updates(targets, profiles, &mut accounts),
                ProfileResults::Each(list) => {
                    let mut changed = false;
                    for (account, profile) in list {
                        changed |= self.avatars.apply_profile_update(
                            account,
                            profile.as_ref(),
                            &mut accounts,
                        );
                    }
                    changed
                }
            };
            self.accounts = accounts;
            if changed {
                self.accounts_version += 1;
            }
        }
        self.prune_orphaned_avatar_states();
        applied
    }

    /// The warning half of a run, landed. False when superseded.
    pub fn finish_refresh_warnings(
        &mut self,
        ticket: &RefreshTicket,
        warnings: BTreeMap<String, WarningPresentation>,
    ) -> bool {
        let applied = self.should_continue(ticket);
        if applied {
            self.queue_warning_states(warnings);
        }
        self.prune_orphaned_avatar_states();
        applied
    }

    /// Seeds the avatars of the accounts on screen; how many there are.
    pub fn prepare_visible_accounts(
        &mut self,
        visible: Option<&[String]>,
        force_refresh: bool,
        hooks: &dyn LoaderHooks,
    ) -> usize {
        let targets = self.resolve_visible(visible);
        self.avatars
            .seed_for_accounts(&targets, force_refresh, hooks);
        targets.len()
    }

    /// Seeds the avatars of the given accounts; how many there are.
    pub fn prepare_account_ids(
        &mut self,
        account_ids: &[String],
        force_refresh: bool,
        hooks: &dyn LoaderHooks,
    ) -> usize {
        let targets = resolve_accounts_by_ids(&self.accounts, account_ids);
        self.avatars
            .seed_for_accounts(&targets, force_refresh, hooks);
        targets.len()
    }
}
