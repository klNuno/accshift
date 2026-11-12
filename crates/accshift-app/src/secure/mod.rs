//! The security layer of the app: the PIN, the inactivity timer and the
//! secure screen that combines them.
//!
//! - [`pin`]: the PIN checks and session over `accshift_core::pin`.
//! - [`InactivityBlur`]: the inactivity timer.
//! - [`SecureScreen`]: the AFK cover and the PIN lock overlay.
//! - [`ScreenCover`]: what the app hides behind the secure screen and the
//!   streamer overlay.

pub mod inactivity;
pub mod pin;
pub mod screen;

pub use inactivity::InactivityBlur;
pub use screen::{SecureEvent, SecureInputs, SecureScreen, WindowView};

/// What the secure screen and streamer mode take away from the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenCover {
    /// Pointer events off under the PIN.
    pub stage_locked: bool,
    /// The app is blurred.
    pub shell_obscured: bool,
    /// The app is inert: no focus, no click, no key reaches it.
    pub shell_inert: bool,
    /// The inactivity frost is drawn.
    pub frost_visible: bool,
    /// The keyboard scope is `locked`: only the overlay keys.
    pub keys_locked: bool,
    /// The grid is hidden: no avatar priming.
    pub grid_hidden: bool,
    /// Titlebar, panels and onboarding are not drawn.
    pub render_suspended: bool,
}

impl ScreenCover {
    /// `streamer_active` is [`crate::streamer::StreamerModeController::active`],
    /// `settings_open` says the settings panel is open.
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
