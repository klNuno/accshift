//! Account loading, profiles and avatars: what turns backend data into the
//! accounts a grid shows.
//!
//! - [`loader::AccountLoader`]: the accounts of the active tab, the current
//!   one, switch, add, removal, warnings and the guards that keep stale
//!   results out.
//! - [`avatars`]: per account avatar state, and the image fetch and cache
//!   that hands bytes or files to the UI.
//! - [`profile_cache`]: the persisted Steam and Roblox profile caches with
//!   expiry, retries and in-flight sharing.
//! - [`priming::VisiblePriming`]: profile priming of the accounts on screen.
//! - [`refresh`]: the settings "refresh now" runs.
//! - [`loading::AccountLoading`]: drives the loader through [`crate::jobs`],
//!   keeps what launcher detection found and turns finished jobs into
//!   [`LoadingEvent`]s.
//!
//! Each async operation is a `begin_*` that returns a ticket, the blocking
//! work run on the job pool, and a `finish_*` that applies the result on the
//! UI thread. Every ticket carries a generation counter, compared on finish
//! so a stale result is dropped.

pub mod avatars;
pub mod loader;
pub mod loading;
pub mod priming;
pub mod profile_cache;
pub mod refresh;

use crate::platforms::AddFlowStatus;
use crate::text::Text;

/// A toast's look: info, success, warning or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastKind {
    /// No `type` given.
    #[default]
    Default,
    Success,
    Info,
    Error,
}

/// A toast the account code asks the UI to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub text: Text,
    pub kind: ToastKind,
}

impl Toast {
    pub fn new(text: Text, kind: ToastKind) -> Self {
        Self { text, kind }
    }
}

/// What [`loading::AccountLoading::poll`] reports to the shell.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadingEvent {
    Toast(Toast),
    /// A toast that stays until [`LoadingEvent::DismissToast`] with its text.
    StickyToast(Toast),
    /// Remove the sticky toast showing this text.
    DismissToast(Text),

    /// A load of `platform_id` landed and its folder sync ran: refresh the
    /// navigation items and the grid padding.
    AccountsLoaded {
        platform_id: String,
    },
    /// The platform started its own setup: start the add flow.
    AddFlowStarted {
        platform_id: String,
        status: AddFlowStatus,
    },
    /// Steam: re-check the account just left on the CS2 bridge.
    Cs2Check {
        steam_id: String,
    },
    /// Minimize the window after a switch, when the setting asks.
    MinimizeWindow,
    /// A switch finished; `succeeded` is whether it switched.
    SwitchFinished {
        account_id: String,
        succeeded: bool,
    },
}
