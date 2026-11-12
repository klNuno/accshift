//! The main window of a normal run: built hidden where the saved geometry
//! puts it ([`placement`],
//! [`frame`]), shown once boot completes or after the 5 s failsafe, its
//! geometry saved 700 ms after the last move and once more on close, its
//! OS backdrop following the theme, and the second
//! launches the single instance hands over ([`Host::second_launches`]).
//!
//! Scenes and the bench keep their own cloaked window and never come here.

pub mod color;
pub mod edges;
#[cfg(windows)]
pub mod frame;
pub mod placement;
#[cfg(all(debug_assertions, windows))]
pub mod probe;
pub mod tray;

use accshift_app::lifecycle::boot::{BOOT_FAILSAFE, BOOT_FAILSAFE_MESSAGE, BOOT_FAILSAFE_SOURCE};
use accshift_app::lifecycle::deep_link;
use accshift_app::lifecycle::single_instance::SingleInstance;
use accshift_app::real::RealBackend;
use accshift_core::config;
use accshift_theme::WindowBackdrop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long after the last move the geometry is saved.
pub const GEOMETRY_SAVE_DEBOUNCE: Duration = Duration::from_millis(700);

/// The window's inner size and outer origin, logical.
/// The origin is `None` where the platform hides it (Wayland): the size is
/// still saved and the stored origin kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    pub position: Option<(f64, f64)>,
    pub scale: f64,
}

impl Geometry {
    /// Nothing from a maximized window (it would
    /// store the screen as the restored size) or a minimized one (parked at
    /// -32000).
    pub fn read(
        maximized: bool,
        minimized: bool,
        scale: f64,
        inner: (u32, u32),
        outer: Option<(i32, i32)>,
    ) -> Option<Self> {
        if maximized || minimized {
            return None;
        }
        let logical = |v: i32| config::logical_from_physical(v as f64, scale);
        Some(Self {
            width: config::logical_from_physical(inner.0 as f64, scale),
            height: config::logical_from_physical(inner.1 as f64, scale),
            position: outer.map(|(x, y)| (logical(x), logical(y))),
            scale,
        })
    }
}

/// The geometry waiting to be saved, and whether a save thread runs.
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
        g.position,
        Some(g.scale),
    );
}

/// The main window's frame. Windows and Linux: borderless, the app draws
/// its caption buttons (and [`frame`] the Windows edges, [`edges`] the
/// Linux ones). macOS: the native traffic lights over the app's title bar,
/// the content drawn under a transparent title bar with no title.
pub fn chrome(builder: egui::ViewportBuilder, macos: bool) -> egui::ViewportBuilder {
    if macos {
        builder
            .with_decorations(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        builder.with_decorations(false)
    }
}

/// Off Windows, the saved origin (or the centre of the primary monitor)
/// applied to the built, hidden window; [`frame`] does it on Windows.
/// Wayland neither reports nor takes a window position: nothing happens.
#[cfg(all(feature = "software", not(windows)))]
pub fn restore_position(window: &egui_winit::winit::window::Window, saved: &config::SavedWindow) {
    use egui_winit::winit::dpi::PhysicalPosition;
    if window.outer_position().is_err() {
        return;
    }
    let primary = window.primary_monitor();
    let monitors: Vec<placement::Monitor> = window
        .available_monitors()
        .map(|m| {
            let (at, size) = (m.position(), m.size());
            // winit reports no work area: the whole monitor stands in.
            let rect = placement::Rect::at(at.x, at.y, size.width as i32, size.height as i32);
            placement::Monitor {
                rect,
                work: rect,
                dpi: (m.scale_factor() * 96.0).round() as u32,
                primary: primary.as_ref() == Some(&m),
            }
        })
        .collect();
    let outer = window.outer_size();
    let outer = (outer.width as i32, outer.height as i32);
    if let Some((x, y)) = placement::origin(saved, &monitors, outer) {
        window.set_outer_position(PhysicalPosition::new(x, y));
    }
}

/// Windows is ending the session: a close request quits instead of hiding
/// to the tray ([`frame`] turns `WM_ENDSESSION` into one).
pub fn session_ending() -> bool {
    #[cfg(windows)]
    return frame::session_ending();
    #[cfg(not(windows))]
    false
}

/// The pointer is over the maximize button Windows owns for Snap Layouts:
/// the title bar draws it hovered (`titlebar:maximize-hover`).
pub fn maximize_hover() -> bool {
    #[cfg(windows)]
    return frame::maximize_hover();
    #[cfg(not(windows))]
    false
}

/// The window side of a normal run.
pub struct Host {
    backend: Arc<RealBackend>,
    /// The window's `HWND`.
    #[cfg(windows)]
    hwnd: isize,
    started: Instant,
    shown: bool,
    focus: bool,
    /// `--tray`: the window never auto-shows, it waits in the tray for the
    /// user to open it. Boot still runs and marks [`Host::take_show`] done so
    /// the failsafe repaint loop stops.
    start_hidden: bool,
    saver: Arc<Saver>,
    /// The key of the backdrop last applied.
    backdrop: Option<String>,
    /// The single-instance service, held for the whole run: dropping it is
    /// its release.
    single: Option<SingleInstance>,
    /// This launch's own `accshift://` argument, until the app takes it.
    launch_url: Option<String>,
}

impl Host {
    pub fn new(backend: Arc<RealBackend>, hwnd: isize) -> Self {
        #[cfg(not(windows))]
        let _ = hwnd;
        Self {
            backend,
            #[cfg(windows)]
            hwnd,
            started: Instant::now(),
            shown: false,
            focus: true,
            start_hidden: false,
            saver: Arc::default(),
            backdrop: None,
            single: None,
            launch_url: None,
        }
    }

    /// The arguments this process started with. A lone `accshift://` one is
    /// a link clicked while the app was closed: it is kept for
    /// [`Self::take_launch_url`].
    pub fn with_launch_args(mut self, args: &[String]) -> Self {
        self.launch_url =
            deep_link::handle_cli_arguments(args, deep_link::SCHEMES).map(|url| url.to_string());
        self
    }

    /// The link this launch was started with, once: logged and counted like
    /// one a second launch hands over. The window needs no showing, a normal
    /// launch reveals itself.
    pub fn take_launch_url(&mut self) -> Option<String> {
        let url = self.launch_url.take()?;
        if let Ok(parsed) = url.parse() {
            let effects = deep_link::on_open_url(std::slice::from_ref(&parsed), false);
            self.log(
                "backend.deep-link",
                "Deep link received",
                Some(&effects.log_details),
            );
            if effects.track {
                self.backend.track_deep_link();
            }
        }
        Some(url)
    }

    /// The service [`single_instance::acquire`] answered for this run.
    ///
    /// [`single_instance::acquire`]: accshift_app::lifecycle::single_instance::acquire
    pub fn with_single_instance(mut self, single: Option<SingleInstance>) -> Self {
        self.single = single;
        self
    }

    /// Each launch handed over by the single instance since the last frame.
    /// A lone `accshift://` argument is a deep link: the log line, the usage
    /// counter, the window shown; then the redirect is logged and the window
    /// shown, both only once boot completed. Returns the deep-link URLs.
    pub fn second_launches(&self, ctx: &egui::Context) -> Vec<String> {
        let Some(single) = &self.single else {
            return Vec::new();
        };
        let mut urls = Vec::new();
        while let Some(second) = single.try_recv() {
            let completed = self.backend.boot_state().is_completed();
            let launch = deep_link::second_launch(&second.args, completed);
            if let Some(url) = launch.url {
                let effects = deep_link::on_open_url(std::slice::from_ref(&url), completed);
                self.log(
                    "backend.deep-link",
                    "Deep link received",
                    Some(&effects.log_details),
                );
                if effects.track {
                    self.backend.track_deep_link();
                }
                if effects.show_window {
                    self.show_main_window(ctx);
                }
                urls.push(url.to_string());
            }
            self.log(launch.log_source, launch.log_message, None);
            if launch.show_window {
                self.show_main_window(ctx);
            }
        }
        urls
    }

    /// `app_runtime::show_main_window`: show, then `set_focus`. The probe's
    /// window is already up, cloaked, and never takes the focus.
    fn show_main_window(&self, ctx: &egui::Context) {
        if self.focus {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn log(&self, source: &str, message: &str, details: Option<&str>) {
        let _ = accshift_core::logging::append_app_log(
            self.backend.context(),
            "info",
            source,
            message,
            details,
        );
    }

    /// When the theme's material changes: acrylic
    /// (tinted for Glass Dark) for the glass themes, every effect cleared
    /// otherwise, Liquid Glass included; DWM kept drawing the acrylic of an
    /// unfocused window.
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
                    // Every effect cleared.
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

    /// `--tray`: start in the tray, the window hidden until the user opens it.
    pub fn start_hidden(mut self, hidden: bool) -> Self {
        self.start_hidden = hidden;
        self
    }

    /// Whether the window should stay hidden on boot (the `--tray` launch).
    pub fn starts_hidden(&self) -> bool {
        self.start_hidden
    }

    /// A `--tray` launch where no tray icon could be made (no tray off
    /// Windows): the window shows as on a normal launch, else nothing would
    /// reach the app.
    pub fn no_tray(&mut self) {
        self.start_hidden = false;
    }

    /// Stops answering second launches. The close sequence calls it before
    /// the app's jobs drain, so a launch during a slow shutdown starts a new
    /// app instead of being handed to this one.
    pub fn release_single_instance(&mut self) {
        self.single = None;
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

    /// A window moved or closed before
    /// boot completed never got its saved geometry, so saving would replace
    /// the real one with the default.
    fn save_allowed(&self) -> bool {
        self.backend.boot_state().is_completed()
    }

    /// On every move: the last
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

    /// First step of the close: the
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
    fn macos_keeps_the_native_buttons_over_the_title_bar() {
        let mac = chrome(egui::ViewportBuilder::default(), true);
        assert_eq!(mac.decorations, Some(true));
        assert_eq!(mac.fullsize_content_view, Some(true));
        assert_eq!(mac.titlebar_shown, Some(false));
        assert_eq!(mac.title_shown, Some(false));
        let other = chrome(egui::ViewportBuilder::default(), false);
        assert_eq!(other.decorations, Some(false));
        assert_eq!(other.fullsize_content_view, None);
    }

    #[test]
    fn maximized_or_minimized_windows_save_nothing() {
        assert_eq!(
            Geometry::read(true, false, 1.0, (800, 600), Some((0, 0))),
            None
        );
        assert_eq!(
            Geometry::read(false, true, 1.0, (800, 600), Some((-32000, -32000))),
            None
        );
    }

    #[test]
    fn geometry_is_stored_logical() {
        let g = Geometry::read(false, false, 1.25, (1250, 850), Some((-10, 125))).unwrap();
        assert_eq!(
            (g.width, g.height, g.position),
            (1000.0, 680.0, Some((-8.0, 100.0)))
        );
    }

    #[test]
    fn a_hidden_origin_still_saves_the_size() {
        // Wayland: no outer position, the size alone is kept.
        let g = Geometry::read(false, false, 2.0, (1800, 1000), None).unwrap();
        assert_eq!((g.width, g.height, g.position), (900.0, 500.0, None));
    }
}
