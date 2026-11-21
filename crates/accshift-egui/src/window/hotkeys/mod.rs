//! The OS layer of the global shortcuts: a [`Registrar`] per platform, each
//! delivering a pressed shortcut's id through the runner's event loop.
//!
//! - Windows ([`windows`]): `RegisterHotKey` on a message-only window; the
//!   UI thread's message loop (winit's) dispatches its `WM_HOTKEY`, so it
//!   fires while the window sits hidden in the tray.
//! - macOS and X11 ([`native`]): the `global-hotkey` crate.
//! - Wayland ([`portal`]): the `org.freedesktop.portal.GlobalShortcuts`
//!   portal over D-Bus; without it, [`Availability::Unsupported`].
//!
//! [`keys`] holds the pure translations (virtual keys, portal triggers),
//! tested on every OS. The logic (the table, conflicts, what a shortcut
//! runs) lives in `accshift_app::shortcuts` and the app.

pub mod keys;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(windows)]
mod windows;

use std::sync::Arc;

use accshift_app::shortcuts::Registrar;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
use accshift_app::shortcuts::{Availability, Combo, RegisterError};

/// Where a pressed shortcut's id goes: the runner's event loop proxy.
pub type Deliver = Arc<dyn Fn(u32) + Send + Sync>;

/// The registrar of this OS and session. On the UI thread.
pub fn registrar(deliver: Deliver) -> Box<dyn Registrar> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsRegistrar::new(deliver))
    }
    #[cfg(target_os = "linux")]
    {
        if portal::wayland_session() {
            return Box::new(portal::PortalRegistrar::new(deliver));
        }
        Box::new(native::NativeRegistrar::new(deliver))
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(native::NativeRegistrar::new(deliver))
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = deliver;
        Box::new(Unsupported)
    }
}

/// Whether the Windows key is held now: egui reports it on macOS only (as
/// Command), so a capture asks the OS on Windows. Always false elsewhere.
pub fn super_held() -> bool {
    #[cfg(windows)]
    {
        windows::super_held()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
struct Unsupported;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl Registrar for Unsupported {
    fn availability(&self) -> Availability {
        Availability::Unsupported
    }
    fn register(&mut self, _id: u32, _combo: &Combo) -> Result<(), RegisterError> {
        Err(RegisterError::Failed("unsupported".into()))
    }
    fn unregister(&mut self, _id: u32) {}
}
