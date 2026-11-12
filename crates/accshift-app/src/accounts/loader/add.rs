//! The add-account flow.

use super::*;

impl AccountLoader {
    /// Starts an add: `None` when an add already runs or there is no
    /// adapter.
    pub fn begin_add(&mut self) -> Option<AddTicket> {
        if self.adding {
            return None;
        }
        let platform_id = self.adapter_id.clone()?;
        self.adding = true;
        self.current_account.clear();
        Some(AddTicket {
            platform_id,
            epoch: self.add_epoch,
        })
    }

    /// Lands an add. `platform_name` is the registry name for the "client
    /// missing" toast; `reload_after_add` is the adapter's flag.
    pub fn finish_add(
        &mut self,
        ticket: &AddTicket,
        result: Result<AddAccountResult, BackendError>,
        platform_name: &str,
        reload_after_add: bool,
        hooks: &dyn LoaderHooks,
    ) -> AddOutcome {
        if ticket.epoch != self.add_epoch
            || self.adapter_id.as_deref() != Some(ticket.platform_id.as_str())
        {
            return AddOutcome::Done;
        }
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                let message = error.message;
                self.error = Some(message.clone());
                self.adding = false;
                let text = if is_client_missing(&message) {
                    Text::with(
                        "toast.addAccountClientMissing",
                        [("platform", platform_name)],
                    )
                } else {
                    Text::key("toast.addAccountFailed")
                };
                return AddOutcome::Failed(Toast::new(text, ToastKind::Error));
            }
        };
        if let Some(status) = result.setup_status {
            self.adding = false;
            return AddOutcome::SetupStarted(status);
        }
        if reload_after_add {
            let ticket = self.begin_load(
                hooks,
                LoadOptions {
                    silent: true,
                    ..LoadOptions::default()
                },
            );
            self.add_reload_load_id = Some(ticket.load_id);
            return AddOutcome::Reload(ticket);
        }
        self.adding = false;
        AddOutcome::Done
    }
}
