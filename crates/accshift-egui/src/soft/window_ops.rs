//! Window lifecycle operations for [`Runner`]: monitor tracking, surface healing, tray, reveal, toggle, and close.

use std::time::Instant;

use egui_winit::winit::event_loop::ActiveEventLoop;

use crate::window::tray::Tray;

use super::platform::{frame_interval, geometry};
use super::present::Present;
use super::{Runner, UserEvent, HEAL_STEPS};

impl Runner {
    /// The frame interval from the refresh rate of the monitor the window
    /// is on, read again when it moves to another one or on `force` (the
    /// display mode may have changed).
    pub(super) fn follow_monitor(&mut self, force: bool) {
        let Some(live) = &self.live else { return };
        let monitor = live.window.current_monitor();
        if monitor.is_none() || (!force && monitor == self.monitor) {
            return;
        }
        if let Some(interval) = frame_interval(monitor.as_ref()) {
            self.frame_interval = interval;
        }
        self.monitor = monitor;
    }

    /// The window's pixels may be gone ([`super::platform::take_content_lost`], a reveal).
    /// A present right away can land before DWM composes the window again
    /// and be clipped to nothing; softbuffer then validates the window and
    /// Windows asks for nothing more. So full presents follow over a few
    /// seconds, through a new window DC: softbuffer keeps the one it took
    /// when the surface was made.
    pub(super) fn heal(&mut self) {
        let Some(live) = &mut self.live else { return };
        // Before boot or hidden in the tray: the next show presents in full.
        if !live.shown || live.window.is_visible() == Some(false) {
            return;
        }
        let _ = live.canvas.rebuild();
        live.full_present = true;
        let now = Instant::now();
        self.heal_due = HEAL_STEPS.iter().map(|step| now + *step).collect();
        self.repaint_at(now);
    }

    /// Create the tray icon once, on a `--tray` launch or the first
    /// close-to-tray. `None` from [`Tray::new`] leaves `self.tray` unset, and
    /// the caller falls back to a normal close.
    pub(super) fn ensure_tray(&mut self) {
        if self.tray.is_some() {
            return;
        }
        let Some(live) = &self.live else { return };
        if !live.shell.supports_tray() {
            return;
        }
        let (open, quit, tip) = live.shell.tray_labels();
        let proxy = self.proxy.clone();
        self.tray = Tray::new(
            &open,
            &quit,
            &tip,
            Box::new(move |action| {
                let _ = proxy.send_event(UserEvent::Tray(action));
            }),
        );
    }

    /// A close request (the button, Alt+F4, the title bar): hide to the tray
    /// when the app asks for it and a tray exists, else run the close
    /// sequence. Falling back to close keeps the app reachable if the tray
    /// could not be created.
    pub(super) fn request_close(&mut self, event_loop: &ActiveEventLoop) {
        // The session ending quits for real: a hidden app would lose its
        // close sequence.
        let to_tray = !crate::window::session_ending()
            && self
                .live
                .as_ref()
                .is_some_and(|live| live.shell.wants_close_to_tray());
        if to_tray {
            self.ensure_tray();
            if self.tray.is_some() {
                if let Some(live) = &self.live {
                    live.window.set_visible(false);
                }
                return;
            }
        }
        self.close(event_loop);
    }

    /// Show and focus the window again from the tray or a shortcut.
    /// `token`: the compositor's activation token of the press, which a
    /// Wayland window needs to come back (winit cannot raise it there).
    /// False when it could not: Wayland without a token, or without
    /// xdg-activation.
    pub(super) fn reveal(&mut self, token: Option<&str>) -> bool {
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut raised = true;
        if let Some(live) = &mut self.live {
            live.window.set_visible(true);
            if live.window.is_minimized() == Some(true) {
                live.window.set_minimized(false);
            }
            live.window.focus_window();
            #[cfg(target_os = "linux")]
            if crate::window::wayland::is_wayland(&*live.window) {
                raised = token.is_some_and(|token| live.raiser.raise(token));
            }
            #[cfg(not(target_os = "linux"))]
            let _ = token;
            live.shown = true;
        }
        self.heal();
        raised
    }

    /// The show/hide shortcut: a window in front hides, to the tray when the
    /// app uses one (close to tray, a `--tray` launch), else minimized; any
    /// other window comes back to the front. Wayland cannot hide a window
    /// nor tell it is minimized: there it minimizes, and a window without
    /// the focus counts as away; a press without an activation token (KDE)
    /// leaves it, as nothing could bring it back
    /// ([`toggle_plan`](crate::app::shortcuts::toggle_plan)). False
    /// when the window could not be brought or kept within reach.
    pub(super) fn toggle_window(&mut self, token: Option<&str>) -> bool {
        use crate::app::shortcuts::{toggle_plan, TogglePlan};
        let Some(live) = &self.live else { return true };
        let window = &live.window;
        let in_front = live.shown
            && window.is_visible() != Some(false)
            && window.is_minimized() != Some(true)
            && window.has_focus();
        #[cfg(target_os = "linux")]
        let wayland = crate::window::wayland::is_wayland(&**window);
        #[cfg(not(target_os = "linux"))]
        let wayland = false;
        let can_hide = match toggle_plan(in_front, wayland, token.is_some()) {
            TogglePlan::Reveal => return self.reveal(token),
            TogglePlan::Keep => return false,
            TogglePlan::Away { can_hide } => can_hide,
        };
        if can_hide && self.tray.is_none() && live.shell.wants_close_to_tray() {
            self.ensure_tray();
        }
        if let Some(live) = &self.live {
            if can_hide && self.tray.is_some() {
                live.window.set_visible(false);
            } else {
                live.window.set_minimized(true);
            }
        }
        true
    }

    /// The title bar's close button or the system's (Alt+F4, the taskbar):
    /// the shell's close sequence, then the loop ends.
    pub(super) fn close(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(live) = &mut self.live {
            let geometry = geometry(&live.window);
            let window = &live.window;
            live.shell.close(geometry, || window.set_visible(false));
        }
        event_loop.exit();
    }
}
