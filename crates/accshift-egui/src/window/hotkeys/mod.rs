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
//! [`keys`] holds the pure translations (virtual keys, portal triggers and
//! keys) and [`session`] the portal's bookkeeping, both tested on every OS. The logic (the table, conflicts, what a shortcut
//! runs) lives in `accshift_app::shortcuts` and the app.

pub mod keys;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native;
#[cfg(target_os = "linux")]
mod portal;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod session;
#[cfg(windows)]
mod windows;

use std::sync::Arc;

use accshift_app::shortcuts::Registrar;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
use accshift_app::shortcuts::{Availability, Combo, RegisterError, ShortcutTarget};

/// What a registrar tells the runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// A shortcut fired, by its registration id, with the compositor's
    /// activation token when it gave one (the Wayland portal): the window
    /// raises itself with it.
    Pressed { id: u32, token: Option<String> },
    /// The registrar's state changed on its own (the portal answered or
    /// found its bus): the app reads it again.
    Changed,
}

impl Signal {
    pub fn pressed(id: u32) -> Self {
        Self::Pressed { id, token: None }
    }
}

/// Where a registrar's signals go: the runner's event loop proxy.
pub type Deliver = Arc<dyn Fn(Signal) + Send + Sync>;

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

/// The Super (Windows) key as winit last reported it.
static SUPER_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The runner's winit modifiers: egui keeps Super on macOS only (as
/// Command), so the capture reads it from here.
pub fn set_super_held(held: bool) {
    SUPER_HELD.store(held, std::sync::atomic::Ordering::Relaxed);
}

/// Whether Super (Windows) is held now: winit's last modifiers, and on
/// Windows the key state itself, which the shell may keep from winit.
pub fn super_held() -> bool {
    let winit = SUPER_HELD.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(windows)]
    {
        winit || windows::super_held()
    }
    #[cfg(not(windows))]
    {
        winit
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
struct Unsupported;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl Registrar for Unsupported {
    fn availability(&self) -> Availability {
        Availability::Unsupported
    }
    fn register(
        &mut self,
        _id: u32,
        _target: &ShortcutTarget,
        _combo: &Combo,
    ) -> Result<(), RegisterError> {
        Err(RegisterError::Failed("unsupported".into()))
    }
    fn unregister(&mut self, _id: u32) {}
}
