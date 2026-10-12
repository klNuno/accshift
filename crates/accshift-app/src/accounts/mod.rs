//! Account loading, profiles and avatars: what turns backend data into the
//! accounts a grid shows.
//!
//! - [`loader::AccountLoader`] (`useAccountLoader.svelte.ts`): the accounts
//!   of the active tab, the current one, switch, add, removal, warnings and
//!   the guards that keep stale results out.
//! - [`avatars`] (`useAvatarLoader.svelte.ts`): per account avatar state, and
//!   the image fetch and cache that hands bytes or files to the UI.
//! - [`profile_cache`] (`shared/profileCache.ts` and the Steam and Roblox
//!   `profileCache.ts`): the persisted profile caches with expiry, retries
//!   and in-flight sharing.
//! - [`priming::VisiblePriming`] (`useVisiblePriming.svelte.ts`).
//! - [`refresh`] (`useProfileRefresh.ts`): the settings "refresh now" runs.
//! - [`loading::AccountLoading`] (`useAccountLoading.svelte.ts` and
//!   `detectedPlatforms.svelte.ts`): drives the loader through [`crate::jobs`]
//!   and turns finished jobs into [`LoadingEvent`]s.
//!
//! Svelte state becomes plain fields; each async TS function becomes a
//! `begin_*` that returns a ticket, the blocking work run on the job pool,
//! and a `finish_*` that applies the result on the UI thread. Every ticket
//! carries the generation counter TS compares after each `await`.

pub mod avatars;
pub mod loader;
pub mod loading;
pub mod priming;
pub mod profile_cache;
pub mod refresh;

use crate::platforms::AddFlowStatus;
use crate::text::Text;

/// `addToast(..., { type })`.
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
    /// A load of `platform_id` landed and its folder sync ran: refresh the
    /// navigation items and the grid padding (`onAfterLoad`).
    AccountsLoaded {
        platform_id: String,
    },
    /// `addFlow.start(platformId, status)`.
    AddFlowStarted {
        platform_id: String,
        status: AddFlowStatus,
    },
    /// Steam: re-check the account just left on the CS2 bridge.
    Cs2Check {
        steam_id: String,
    },
    /// `invoke("minimize_window")` after a switch, when the setting asks.
    MinimizeWindow,
    /// A switch finished; `succeeded` is what `handleAccountSwitch` returns.
    SwitchFinished {
        account_id: String,
        succeeded: bool,
    },
}
