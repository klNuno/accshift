//! The runner: window and input through egui-winit, egui's meshes
//! rasterized on the CPU by the vendored `egui_software_backend`, and the
//! frame blitted with softbuffer (a 32-bit DIB and `BitBlt` on Windows).
//! No GPU driver is loaded: an OpenGL driver took about 300 ms before the
//! first frame and most of the 100 MB working set.

mod events;
mod paint;
mod platform;
mod present;
mod window_ops;

#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::time::{Duration, Instant};

use egui::ViewportInfo;
use egui_software_backend::EguiSoftwareRender;
use egui_winit::accesskit_winit;
use egui_winit::winit;
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::monitor::MonitorHandle;
use winit::window::Window;

use crate::window::tray::{Tray, TrayAction};
use crate::{bench, Shell};

use present::{Canvas, Faults};

pub enum UserEvent {
    /// From egui's repaint callback, possibly on another thread.
    Repaint {
        when: Instant,
        pass: u64,
    },
    AccessKit(accesskit_winit::Event),
    /// The vblank the last frame armed ([`crate::vblank`]).
    VBlank,
    /// The tray icon was clicked or its menu chosen ([`crate::window::tray`]).
    Tray(TrayAction),
    /// A global shortcut fired, or its registrar changed
    /// ([`crate::window::hotkeys`]).
    Hotkey(crate::window::hotkeys::Signal),
    /// A Unix termination signal (`SIGTERM` / `SIGINT`) asked the app to quit
    /// cleanly ([`crate::window::signal`]).
    Quit,
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::AccessKit(event)
    }
}

/// Builds the app once the window exists, before the first frame.
pub type Init = Box<dyn FnOnce(&egui::Context, &Window) -> Shell>;

pub fn run(
    viewport: egui::ViewportBuilder,
    init: Init,
) -> Result<(), winit::error::EventLoopError> {
    bench::mark("run");
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    bench::mark("event_loop");
    let _signals = crate::window::signal::install({
        let proxy = event_loop.create_proxy();
        move || {
            let _ = proxy.send_event(UserEvent::Quit);
        }
    });
    let mut runner = Runner {
        ctx: egui::Context::default(),
        proxy: event_loop.create_proxy(),
        viewport,
        init: Some(init),
        live: None,
        next_repaint: None,
        last_paint: None,
        frame_interval: Duration::from_secs_f64(1.0 / 60.0),
        vblank: None,
        armed: false,
        tray: None,
        heal_due: Vec::new(),
        monitor: None,
    };
    event_loop.run_app(&mut runner)
}

/// When the full presents of [`Runner::heal`] run, after its signal.
const HEAL_STEPS: [Duration; 5] = [
    Duration::ZERO,
    Duration::from_millis(150),
    Duration::from_millis(500),
    Duration::from_millis(1500),
    Duration::from_millis(4000),
];

/// The shortest gap between the frames of a window hidden in the tray.
/// They still run the app's jobs and timers, but nothing animates where no
/// one looks, as a hidden WebView throttles its timers.
const HIDDEN_FRAME_INTERVAL: Duration = Duration::from_millis(250);

struct Live {
    window: Rc<Window>,
    canvas: Canvas,
    faults: Faults,
    /// The pointer in window pixels, for the resize edges.
    #[cfg(target_os = "linux")]
    pointer: Option<(f64, f64)>,
    state: egui_winit::State,
    info: ViewportInfo,
    shell: Shell,
    render: EguiSoftwareRender,
    /// Texture changes not rasterized yet (a minimized window has no pixels).
    textures: egui::TexturesDelta,
    /// Unscaled overlay pass renderer (onboarding, AFK, streamer, PIN lock).
    overlay_render: EguiSoftwareRender,
    overlay_textures: egui::TexturesDelta,
    /// Whether the overlay pass drew onto the window buffer last frame.
    overlay_shown: bool,
    actions: Vec<egui_winit::ActionRequested>,
    full_present: bool,
    /// The UI zoom of the last frame, and the stage it rendered at that
    /// zoom before the resample ([`crate::zoom`]).
    stage_zoom: f32,
    stage: Vec<[u8; 4]>,
    /// The stage's root pixels scaled into the window, an overlay's blur
    /// source while one shows on a zoomed stage.
    overlay_backdrop: Vec<[u8; 4]>,
    /// The held modifiers, for the keys egui-winit drops.
    modifiers: egui::Modifiers,
    /// winit's held modifiers, for a shortcut capture's presses.
    held: winit::keyboard::ModifiersState,
    /// Raises the window with a Wayland activation token.
    #[cfg(target_os = "linux")]
    raiser: crate::window::wayland::Raiser,
    shown: bool,
    /// The bench's scripted wheel and keys, from the first shown frame.
    script: Option<bench::Script>,
}

impl Live {
    /// Hidden after it first showed: closed to the tray. Before that the
    /// window waits for boot, and its frames must not wait.
    fn hidden(&self) -> bool {
        self.shown && self.window.is_visible() == Some(false)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        // Deltas of frames a minimized or closing window never drew.
        self.textures.clear();
        self.overlay_textures.clear();
    }
}

struct Runner {
    ctx: egui::Context,
    proxy: EventLoopProxy<UserEvent>,
    viewport: egui::ViewportBuilder,
    init: Option<Init>,
    live: Option<Live>,
    next_repaint: Option<Instant>,
    /// Frame pacing: a blit returns at once, so repaints would otherwise
    /// run as fast as the CPU allows. While an animation runs the next frame
    /// waits for the vblank instead, this timer at twice the interval only
    /// covering a vblank that never comes.
    last_paint: Option<Instant>,
    frame_interval: Duration,
    vblank: Option<crate::vblank::Clock>,
    /// The last frame asked for the next one on the vblank.
    armed: bool,
    /// The notification-area icon, created on a `--tray` launch or the first
    /// close-to-tray, then kept for the rest of the run.
    tray: Option<Tray>,
    /// Full presents still owed by [`Self::heal`], soonest first.
    heal_due: Vec<Instant>,
    /// The monitor `frame_interval` was read from.
    monitor: Option<MonitorHandle>,
}
