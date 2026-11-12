//! Switching account, and the profile fetch after a switch.

use super::*;

impl AccountLoader {
    /// Starts a switch: `None` when a switch already runs or there is no
    /// adapter. The caller runs the switch on the job pool.
    pub fn begin_switch(&mut self, account: &PlatformAccount) -> Option<SwitchTicket> {
        let platform_id = self.adapter_id.clone()?;
        if self.switching {
            return None;
        }
        self.latest_switch_id += 1;
        let previous_account_id = self.current_account_id().map(str::to_string);
        self.switching = true;
        self.switching_account_id = Some(account.id.clone());
        self.error = None;
        Some(SwitchTicket {
            switch_id: self.latest_switch_id,
            account: account.clone(),
            previous_account_id,
            platform_id,
        })
    }

    /// Lands a switch.
    pub fn finish_switch(
        &mut self,
        ticket: &SwitchTicket,
        result: Result<(), BackendError>,
        hooks: &dyn LoaderHooks,
    ) -> SwitchOutcome {
        let mut outcome = SwitchOutcome::default();
        if ticket.switch_id != self.latest_switch_id {
            return outcome;
        }
        let account = &ticket.account;
        match result {
            Ok(()) => {
                outcome.succeeded = true;
                // A load in flight read the current account before this
                // switch landed: the epoch keeps it from clobbering ours.
                self.current_account_epoch += 1;
                self.current_account = account.id.clone();
                if ticket.platform_id == "steam" {
                    if let Some(previous) = &ticket.previous_account_id {
                        if previous != &account.id {
                            outcome.cs2_check = Some(previous.clone());
                        }
                    }
                }
                if hooks.has_profile_info() {
                    self.avatars.update_state(
                        &account.id,
                        AvatarPatch {
                            refreshing: Some(true),
                            ..AvatarPatch::default()
                        },
                    );
                    outcome.fetch_profile = Some(account.clone());
                }
            }
            Err(error) => {
                // Refused for want of the PIN: the lock screen is up, nothing
                // failed that a toast could explain.
                if !error.is_pin_locked() {
                    let message = error.message;
                    self.error = Some(message.clone());
                    let text = hooks
                        .switch_error_toast(&message)
                        .unwrap_or_else(|| Text::key("toast.switchFailed"));
                    outcome.toasts.push(Toast::new(text, ToastKind::Error));
                    if let Some(cached) = hooks.cached_warning_states() {
                        self.replace_warning_states(cached);
                    }
                }
            }
        }
        self.switching = false;
        self.switching_account_id = None;
        outcome
    }

    /// The profile fetch after a switch. `None` is a failed fetch.
    pub fn finish_switch_profile(
        &mut self,
        account: &PlatformAccount,
        profile: Option<PlatformProfileInfo>,
    ) {
        // The account may have been removed while this fetch was in flight.
        if self.account(&account.id).is_none() {
            return;
        }
        let mut accounts = std::mem::take(&mut self.accounts);
        let changed = self
            .avatars
            .apply_profile_update(account, profile.as_ref(), &mut accounts);
        self.accounts = accounts;
        if changed {
            self.accounts_version += 1;
        }
    }
}
