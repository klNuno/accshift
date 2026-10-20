//! Safe actions, ported from `src/lib/shared/contextMenu/actions.ts`.
//!
//! `createSafeContextAction` runs a task and turns a failure into a toast of
//! the error string, except a PIN refusal: the lock screen is already up, so
//! no toast goes on top. Here a task returns a [`TaskOutcome`], the effects the
//! UI applies on its thread in order.

use crate::error::BackendError;
use crate::stores::DefaultGame;
use crate::text::Text;

/// One thing a finished task asks the UI to do.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskEffect {
    /// `callbacks.showToast(text)`.
    Toast(Text),
    /// `callbacks.refreshAccounts()`.
    RefreshAccounts,
    /// `callbacks.removeAccount(id)`.
    RemoveAccount(String),
    /// `setDefaultGame(steamId, game)` on the client stores.
    SetDefaultGame { steam_id: String, game: DefaultGame },
    /// `clearDefaultGame(steamId)` on the client stores.
    ClearDefaultGame { steam_id: String },
    /// `reportPinLockedError`: bring the lock screen up.
    PinLocked,
}

/// What a task did, as effects in the order TS performs them.
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

    /// The `catch` of `createSafeContextAction`: a toast with the error
    /// message (`String(error)`), nothing for a PIN refusal.
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
