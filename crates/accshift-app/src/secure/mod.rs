//! The security layer of the app: the PIN, the inactivity timer, the window
//! activity state and the secure screen that combines them.
//!
//! - [`pin`]: `pin.ts` and `pinSession.ts` over `accshift_core::pin`.
//! - The inactivity timer of `useInactivityBlur.svelte.ts` is
//!   [`InactivityBlur`].
//! - The state machine of `useWindowActivity.svelte.ts` is
//!   [`WindowActivity`].
//! - [`SecureScreen`]: `useSecureScreen.svelte.ts` plus the logic of
//!   `AppScreenOverlays.svelte`.
//! - [`ScreenCover`]: what App.svelte hides behind the secure screen and the
//!   streamer overlay.

pub mod inactivity;
pub mod pin;
pub mod screen;
pub mod window_activity;

pub use inactivity::InactivityBlur;
pub use screen::{SecureEvent, SecureInputs, SecureScreen, WindowView};
pub use window_activity::WindowActivity;

/// What the secure screen and streamer mode take away from the app, as
/// App.svelte applies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenCover {
    /// `.app-stage.locked` (line 783): pointer events off under the PIN.
    pub stage_locked: bool,
    /// `.app-shell.obscured` (line 789): the app is blurred.
    pub shell_obscured: bool,
    /// `inert` on `.app-shell` (line 790): no focus, no click, no key reaches
    /// the app.
    pub shell_inert: bool,
    /// `.inactivity-frost.visible` (line 822).
    pub frost_visible: bool,
    /// The keyboard scope is `locked` (line 561): only the overlay keys.
    pub keys_locked: bool,
    /// `isGridHidden` for visible priming (line 426): no avatar priming.
    pub grid_hidden: bool,
    /// Titlebar, panels and onboarding are not drawn (`renderSuspended`,
    /// lines 780, 816, 935, 979).
    pub render_suspended: bool,
}

impl ScreenCover {
    /// `streamer_active` is [`crate::streamer::StreamerModeController::active`],
    /// `settings_open` is `settingsPanel.showSettings`.
    pub fn compute(secure: &SecureScreen, streamer_active: bool, settings_open: bool) -> Self {
        let suspended = secure.render_suspended();
        Self {
            stage_locked: secure.is_pin_locked(),
            shell_obscured: secure.is_obscured() || streamer_active,
            shell_inert: secure.is_pin_locked() || streamer_active,
            frost_visible: !suspended && secure.is_obscured(),
            keys_locked: secure.is_pin_locked() || streamer_active || suspended,
            grid_hidden: settings_open || !secure.window_foreground() || suspended,
            render_suspended: suspended,
        }
    }
}
