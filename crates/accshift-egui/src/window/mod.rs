//! The main window of a normal run, driven as src-tauri/src/boot.rs drives
//! Tauri's: built hidden where the saved geometry puts it ([`placement`],
//! [`frame`]), shown once boot completes or after the 5 s failsafe, its
//! geometry saved 700 ms after the last move and once more on close, its
//! OS backdrop following the theme (`applyWindowBackdrop`).
//!
//! Scenes and the bench keep their own cloaked window and never come here.

#[cfg(windows)]
pub mod frame;
pub mod placement;
#[cfg(all(debug_assertions, windows))]
pub mod probe;

use accshift_app::lifecycle::boot::{BOOT_FAILSAFE, BOOT_FAILSAFE_MESSAGE, BOOT_FAILSAFE_SOURCE};
use accshift_app::real::RealBackend;
use accshift_core::config;
use accshift_theme::WindowBackdrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// boot.rs `GEOMETRY_SAVE_DEBOUNCE`.
pub const GEOMETRY_SAVE_DEBOUNCE: Duration = Duration::from_millis(700);

/// boot.rs `WindowGeometry`: the inner size and outer origin, logical.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

impl Geometry {
    /// boot.rs `current_geometry`: nothing from a maximized window (it would
    /// store the screen as the restored size) or a minimized one (parked at
    /// -32000).
    pub fn read(
        maximized: bool,
        minimized: bool,
        scale: f64,
        inner: (u32, u32),
        outer: (i32, i32),
    ) -> Option<Self> {
        if maximized || minimized {
            return None;
        }
        Some(Self {
            width: config::logical_from_physical(inner.0 as f64, scale),
            height: config::logical_from_physical(inner.1 as f64, scale),
            x: config::logical_from_physical(outer.0 as f64, scale),
            y: config::logical_from_physical(outer.1 as f64, scale),
            scale,
        })
    }
}

/// boot.rs `PENDING_GEOMETRY` and `GEOMETRY_SAVER_RUNNING`.
#[derive(Default)]
struct Saver {
    pending: Mutex<Option<Geometry>>,
    running: AtomicBool,
}

impl Saver {
    fn take(&self) -> Option<Geometry> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
}

fn save(backend: &RealBackend, g: Geometry) {
    let _ = config::save_window_geometry(
        backend.context().as_ref(),
        g.width,
        g.height,
        Some((g.x, g.y)),
        Some(g.scale),
    );
}

/// The window side of a normal run.
pub struct Host {
    backend: Arc<RealBackend>,
    /// The window's `HWND`, 0 off Windows.
    hwnd: isize,
    started: Instant,
    shown: bool,
    focus: bool,
    saver: Arc<Saver>,
    /// `appliedKey` of backdrop.ts.
    backdrop: Option<String>,
}

impl Host {
    pub fn new(backend: Arc<RealBackend>, hwnd: isize) -> Self {
        Self {
            backend,
            hwnd,
            started: Instant::now(),
            shown: false,
            focus: true,
            saver: Arc::default(),
            backdrop: None,
        }
    }

    /// `applyWindowBackdrop`, when the theme's material changes: acrylic
    /// (tinted for Glass Dark) for the glass themes, every effect cleared
    /// otherwise, Liquid Glass included; DWM kept drawing the acrylic of an
    /// unfocused window (`set_keep_backdrop_active`).
    pub fn apply_backdrop(&mut self, backdrop: &WindowBackdrop) {
        let key = backdrop.key();
        if self.backdrop.as_deref() == Some(key.as_str()) {
            return;
        }
        self.backdrop = Some(key);
        #[cfg(windows)]
        if let Some(hwnd) = std::num::NonZeroIsize::new(self.hwnd) {
            let window = Hwnd(hwnd);
            let acrylic = match backdrop {
                WindowBackdrop::Glass {
                    windows_acrylic: true,
                    acrylic_tint,
                    ..
                } => {
                    let tint = acrylic_tint.map(|[r, g, b, a]| (r, g, b, a));
                    let _ = window_vibrancy::apply_acrylic(&window, tint);
                    true
                }
                _ => {
                    // tauri `clear_effects`.
                    let _ = window_vibrancy::clear_blur(&window);
                    let _ = window_vibrancy::clear_acrylic(&window);
                    let _ = window_vibrancy::clear_mica(&window);
                    false
                }
            };
            frame::keep_backdrop_active(acrylic);
        }
    }

    pub fn backend(&self) -> &Arc<RealBackend> {
        &self.backend
    }

    /// `set_focus` after the show; the probe never takes the focus.
    pub fn focus_on_show(mut self, focus: bool) -> Self {
        self.focus = focus;
        self
    }

    pub fn focuses_on_show(&self) -> bool {
        self.focus
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    /// When the failsafe fires, for the runner's timer.
    pub fn failsafe_at(&self) -> Instant {
        self.started + BOOT_FAILSAFE
    }

    /// True once, when the hidden window shows: boot completed
    /// (`finish_boot` calls `show_main_window`) or the failsafe elapsed
    /// (`spawn_boot_failsafe`, which logs it).
    pub fn take_show(&mut self, now: Instant) -> bool {
        if self.shown {
            return false;
        }
        if self.backend.boot_state().is_completed() {
            self.shown = true;
        } else if now >= self.failsafe_at() {
            self.shown = true;
            let _ = accshift_core::logging::append_app_log(
                self.backend.context(),
                "warn",
                BOOT_FAILSAFE_SOURCE,
                BOOT_FAILSAFE_MESSAGE,
                None,
            );
        }
        self.shown
    }

    /// boot.rs `geometry_save_allowed`: a window moved or closed before
    /// boot completed never got its saved geometry, so saving would replace
    /// the real one with the default.
    fn save_allowed(&self) -> bool {
        self.backend.boot_state().is_completed()
    }

    /// boot.rs `queue_window_geometry_save`, on every move: the last
    /// geometry lands 700 ms after the moves stop, off the UI thread.
    pub fn moved(&self, geometry: Option<Geometry>) {
        if !self.save_allowed() {
            return;
        }
        let Some(geometry) = geometry else {
            return;
        };
        *self.saver.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(geometry);
        if self.saver.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let saver = Arc::clone(&self.saver);
        let backend = Arc::clone(&self.backend);
        std::thread::spawn(move || loop {
            std::thread::sleep(GEOMETRY_SAVE_DEBOUNCE);
            match saver.take() {
                Some(geometry) => save(&backend, geometry),
                None => {
                    saver.running.store(false, Ordering::SeqCst);
                    // A move between the take and the release.
                    let missed = saver
                        .pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .is_some();
                    if missed && !saver.running.swap(true, Ordering::SeqCst) {
                        continue;
                    }
                    break;
                }
            }
        });
    }

    /// boot.rs `spawn_window_geometry_save`, first step of the close: the
    /// save runs on its own thread (the config lock may wait 5 s on the
    /// CLI); the receiver says when it landed. `None`: nothing queued.
    pub fn start_close_save(&self, geometry: Option<Geometry>) -> Option<Receiver<()>> {
        if !self.save_allowed() {
            let _ = accshift_core::logging::append_app_log(
                self.backend.context(),
                "info",
                "backend.window",
                "Skipped window geometry save because boot was not completed",
                None,
            );
            return None;
        }
        let geometry = geometry?;
        // Newer than anything the debounce holds, and never undone by it.
        self.saver.take();
        let backend = Arc::clone(&self.backend);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            save(&backend, geometry);
            let _ = tx.send(());
        });
        Some(rx)
    }
}

/// The main window for window-vibrancy, which takes a window handle.
#[cfg(windows)]
struct Hwnd(std::num::NonZeroIsize);

#[cfg(windows)]
impl raw_window_handle::HasWindowHandle for Hwnd {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = raw_window_handle::RawWindowHandle::Win32(
            raw_window_handle::Win32WindowHandle::new(self.0),
        );
        // SAFETY: the runner keeps the window alive as long as the shell.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximized_or_minimized_windows_save_nothing() {
        assert_eq!(Geometry::read(true, false, 1.0, (800, 600), (0, 0)), None);
        assert_eq!(
            Geometry::read(false, true, 1.0, (800, 600), (-32000, -32000)),
            None
        );
    }

    #[test]
    fn geometry_is_stored_logical() {
        let g = Geometry::read(false, false, 1.25, (1250, 850), (-10, 125)).unwrap();
        assert_eq!((g.width, g.height, g.x, g.y), (1000.0, 680.0, -8.0, 100.0));
    }
}
