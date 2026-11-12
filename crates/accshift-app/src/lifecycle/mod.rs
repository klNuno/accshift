//! The app lifecycle the egui shell drives: the updater, single instance,
//! deep links, process control, launch at sign-in and the boot helpers.
//!
//! - [`updater`]: fetch, verify and install a signed update;
//!   [`update_flow`]: the update state machine on top of it.
//! - [`single_instance`]: one running app; a second launch hands its
//!   arguments to the first. The lock names match the ones older installs
//!   used, so both builds see each other.
//! - [`deep_link`]: `accshift://` scheme registration, parsing and handling.
//! - [`process`]: exit and relaunch.
//! - [`autostart`]: the `HKCU\...\Run` value for launch at sign-in into the
//!   tray.
//! - [`app_shell`]: shell init and the external refresh.
//! - [`boot`]: boot marks, boot locale, log redaction, the telemetry calls
//!   and the close flush message; [`ui_scale`]: the UI scale and the
//!   backdrop decisions.

pub mod app_shell;
pub mod autostart;
pub mod boot;
pub mod deep_link;
pub mod process;
pub mod single_instance;
pub mod ui_scale;
pub mod update_flow;
pub mod updater;

use std::time::Duration;

/// The first update check starts this long after the window opens.
pub const UPDATE_CHECK_DELAY: Duration = Duration::from_millis(3500);
/// On close, the window geometry save gets this long before the process
/// goes on without it.
pub const GEOMETRY_SAVE_WAIT: Duration = Duration::from_secs(5);
