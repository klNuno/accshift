//! Safe actions: a task runs and a failure becomes a toast of the error
//! string, except a PIN refusal: the lock screen is already up, so no toast
//! goes on top. A task returns a [`TaskOutcome`], the effects the UI applies
//! on its thread in order.

use crate::error::BackendError;
use crate::stores::DefaultGame;
use crate::text::Text;

/// One thing a finished task asks the UI to do.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskEffect {
    /// Show a toast.
    Toast(Text),
    /// Reload the accounts.
    RefreshAccounts,
    /// Remove the account from the grid.
    RemoveAccount(String),
    /// Store the account's default game in the client stores.
    SetDefaultGame { steam_id: String, game: DefaultGame },
    /// Clear the account's default game in the client stores.
    ClearDefaultGame { steam_id: String },
    /// Bring the lock screen up.
    PinLocked,
}

/// What a task did, as effects in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TaskOutcome {
    pub effects: Vec<TaskEffect>,
}

impl TaskOutcome {
    pub fn push(&mut self, effect: TaskEffect) {
        self.effects.push(effect);
    }

    /// The toast texts, for tests and logs.
    pub fn toasts(&self) -> Vec<&Text> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                TaskEffect::Toast(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    /// A failed task: a toast with the error message, nothing for a PIN
    /// refusal.
    pub fn fail(&mut self, error: &BackendError) {
        if error.is_pin_locked() {
            return;
        }
        self.push(TaskEffect::Toast(Text::raw(error.message.clone())));
    }
}

/// Runs `task` as a safe action: its `Ok` effects are already in `outcome`,
/// an `Err` adds the failure toast.
pub fn run_safe(
    outcome: &mut TaskOutcome,
    task: impl FnOnce(&mut TaskOutcome) -> Result<(), BackendError>,
) {
    if let Err(error) = task(outcome) {
        outcome.fail(&error);
    }
}
