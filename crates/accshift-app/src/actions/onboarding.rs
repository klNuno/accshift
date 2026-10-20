//! The first-run telemetry onboarding and the fake account grid its feature
//! tour draws over. Ported from `src/lib/app/useOnboardingTour.svelte.ts`
//! (state and steps; the dialog itself is drawn by the egui crate).
//!
//! A new install has no accounts, which is when the tour needs cards to point
//! at: three mock accounts replace the workspace for the duration and the
//! previous tab comes back afterwards.

use super::AppEffect;
use crate::backend::Backend;
use crate::folders::ItemRef;
use crate::jobs::{Jobs, Pending, Poll};
use crate::platforms::PlatformAccount;
use crate::text::Text;

/// Ids of the fake accounts, prefixed so nothing takes one for a real id.
pub const MOCK_IDS: [&str; 3] = ["__tour_mock_1", "__tour_mock_2", "__tour_mock_3"];
/// The tour walks through Steam's card features.
pub const TOUR_PLATFORM: &str = "steam";

/// A fake account: its display name stays a key, resolved at draw time.
#[derive(Debug, Clone, PartialEq)]
pub struct MockAccount {
    pub account: PlatformAccount,
    /// `onboarding.features.mockAccount` with `{number}`.
    pub display_name: Text,
}

/// `mockAccounts`.
pub fn mock_accounts() -> Vec<MockAccount> {
    MOCK_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let number = index + 1;
            MockAccount {
                account: PlatformAccount {
                    id: (*id).to_string(),
                    display_name: String::new(),
                    username: format!("account_{number}"),
                    last_login_at_sec: None,
                },
                display_name: Text::with(
                    "onboarding.features.mockAccount",
                    [("number", number as f64)],
                ),
            }
        })
        .collect()
}

/// `mockItems`.
pub fn mock_items() -> Vec<ItemRef> {
    MOCK_IDS.iter().map(|id| ItemRef::account(*id)).collect()
}

/// `createOnboardingTour`.
#[derive(Default)]
pub struct OnboardingTour {
    open: bool,
    mock_active: bool,
    previous_tab: Option<String>,
    state_read: Option<Pending<bool>>,
}

impl OnboardingTour {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&self) -> bool {
        self.open
    }

    /// True while the workspace shows the tour's fake accounts.
    pub fn mock_active(&self) -> bool {
        self.mock_active
    }

    /// `openOnboarding`.
    pub fn open_onboarding(&mut self) {
        self.open = true;
    }

    fn activate_mock(&mut self, active_tab: &str) -> Vec<AppEffect> {
        self.previous_tab = Some(active_tab.to_string());
        let mut effects = Vec::new();
        if active_tab != TOUR_PLATFORM {
            effects.push(AppEffect::SetActiveTab(TOUR_PLATFORM.to_string()));
        }
        self.mock_active = true;
        effects
    }

    fn deactivate_mock(&mut self, active_tab: &str) -> Vec<AppEffect> {
        self.mock_active = false;
        let mut effects = Vec::new();
        if let Some(previous) = self.previous_tab.take() {
            if !previous.is_empty() && previous != active_tab {
                effects.push(AppEffect::SetActiveTab(previous));
            }
        }
        effects
    }

    /// `setMockActive`.
    pub fn set_mock_active(&mut self, active: bool, active_tab: &str) -> Vec<AppEffect> {
        if active {
            self.activate_mock(active_tab)
        } else {
            self.deactivate_mock(active_tab)
        }
    }

    /// `close`.
    pub fn close(&mut self, active_tab: &str) -> Vec<AppEffect> {
        self.open = false;
        self.deactivate_mock(active_tab)
    }

    /// `openIfNeverCompleted`, started: reads `telemetry_get_state` on the
    /// pool. A failed read shows nothing.
    pub fn start_open_if_never_completed(&mut self, jobs: &Jobs) {
        self.state_read = Some(jobs.run(|backend: &dyn Backend| {
            backend
                .telemetry_get_state()
                .map(|state| !state.onboarding_completed)
                .unwrap_or(false)
        }));
    }

    /// Applies the read once it lands: true when the onboarding opened.
    pub fn poll(&mut self) -> bool {
        let Some(pending) = self.state_read.as_mut() else {
            return false;
        };
        match pending.poll() {
            Poll::Waiting => false,
            Poll::Ready(show) => {
                self.state_read = None;
                if show {
                    self.open_onboarding();
                }
                show
            }
            Poll::Gone => {
                self.state_read = None;
                false
            }
        }
    }
}
