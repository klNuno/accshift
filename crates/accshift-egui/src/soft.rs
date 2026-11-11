//! The runner: window and input through egui-winit, egui's meshes
//! rasterized on the CPU by the vendored `egui_software_backend`, and the
//! frame blitted with softbuffer (a 32-bit DIB and `BitBlt` on Windows).
//! No GPU driver is loaded: an OpenGL driver took about 300 ms before the
//! first frame and most of the 100 MB working set.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{ViewportEvent, ViewportId, ViewportInfo};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use egui_winit::accesskit_winit;
use egui_winit::winit;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{
    ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy, OwnedDisplayHandle,
};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowId};

use crate::window::tray::{Tray, TrayAction};
use crate::{bench, Shell};

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

/// The softbuffer surface, with what it takes to build it again: after a
/// failure ([`Faults`]) and in [`Runner::heal`].
struct Canvas {
    window: Rc<Window>,
    context: softbuffer::Context<OwnedDisplayHandle>,
    surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
}

/// The presenter steps a frame can fail at, so the failure path is tested
/// without a window.
trait Present {
    type Buffer<'a>
    where
        Self: 'a;
    fn rebuild(&mut self) -> Result<(), String>;
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), String>;
    fn buffer(&mut self) -> Result<Self::Buffer<'_>, String>;
}

impl Present for Canvas {
    type Buffer<'a> = softbuffer::Buffer<'a, OwnedDisplayHandle, Rc<Window>>;

    fn rebuild(&mut self) -> Result<(), String> {
        self.surface = softbuffer::Surface::new(&self.context, self.window.clone())
            .map_err(|err| format!("surface: {err}"))?;
        Ok(())
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), String> {
        self.surface
            .resize(width, height)
            .map_err(|err| format!("resize: {err}"))
    }

    fn buffer(&mut self) -> Result<Self::Buffer<'_>, String> {
        self.surface
            .buffer_mut()
            .map_err(|err| format!("buffer: {err}"))
    }
}

/// Presenter failures in a row (a lost X11 connection, a window buffer
/// that could not be allocated, a lost Wayland buffer). The frame is
/// skipped, never the app: the next one rebuilds the surface, after a delay
/// that grows with the failures, and only the first ones are logged.
#[derive(Default)]
struct Faults {
    in_a_row: u32,
}

/// Failures in a row that reach the log.
const LOGGED_FAULTS: u32 = 3;

impl Faults {
    fn fail(&mut self) {
        self.in_a_row = self.in_a_row.saturating_add(1);
    }

    fn logged(&self) -> bool {
        (1..=LOGGED_FAULTS).contains(&self.in_a_row)
    }

    fn retry_after(&self) -> Duration {
        Duration::from_millis(16)
            .saturating_mul(1 << self.in_a_row.min(6))
            .min(Duration::from_secs(1))
    }

    fn succeeded(&mut self) {
        self.in_a_row = 0;
    }
}

/// The buffer for a `width` by `height` frame, the surface rebuilt first
/// when the last frame failed. A failure is counted in `faults`.
fn frame_buffer<'a, P: Present>(
    presenter: &'a mut P,
    faults: &mut Faults,
    width: NonZeroU32,
    height: NonZeroU32,
) -> Result<P::Buffer<'a>, String> {
    let ready = if faults.in_a_row > 0 {
        presenter.rebuild()
    } else {
        Ok(())
    }
    .and_then(|()| presenter.resize(width, height));
    match ready.and_then(|()| presenter.buffer()) {
        Ok(buffer) => Ok(buffer),
        Err(err) => {
            faults.fail();
            Err(err)
        }
    }
}

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
    actions: Vec<egui_winit::ActionRequested>,
    full_present: bool,
    /// The UI zoom of the last frame, and the stage it rendered at that
    /// zoom before the resample ([`crate::zoom`]).
    stage_zoom: f32,
    stage: Vec<[u8; 4]>,
    /// The held modifiers, for the keys egui-winit drops.
    modifiers: egui::Modifiers,
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

impl Runner {
    fn repaint_at(&mut self, when: Instant) {
        let interval = if self.live.as_ref().is_some_and(Live::hidden) {
            HIDDEN_FRAME_INTERVAL
        } else if self.armed {
            self.frame_interval * 2
        } else {
            self.frame_interval
        };
        let when = match self.last_paint {
            Some(last) => when.max(last + interval),
            None => when,
        };
        self.next_repaint = Some(self.next_repaint.map_or(when, |t| t.min(when)));
    }

    fn paint(&mut self, event_loop: &ActiveEventLoop) {
        let ctx = self.ctx.clone();
        let now = Instant::now();
        let healing = self.heal_due.first().is_some_and(|due| *due <= now);
        self.heal_due.retain(|due| *due > now);
        let Some(live) = &mut self.live else { return };

        egui_winit::update_viewport_info(&mut live.info, &ctx, &live.window, false);
        let mut raw = live.state.take_egui_input(&live.window);
        raw.viewports.insert(ViewportId::ROOT, live.info.clone());
        live.info.events.clear();
        let zoom = live.shell.stage_zoom(&ctx);
        let zoomed = (zoom - 1.0).abs() > 1e-6;
        if (zoom - live.stage_zoom).abs() > 1e-6 {
            live.stage_zoom = zoom;
            live.full_present = true;
        }
        let script_due = live.script.as_mut().and_then(|script| {
            let sf = live.window.scale_factor() as f32;
            let size = live.window.inner_size();
            script.feed(
                &mut raw,
                egui::pos2(size.width as f32, size.height as f32) / (2.0 * sf),
            )
        });
        live.shell.input_hook(&ctx, &mut raw, zoom);
        let mut retry = None;

        let started = Instant::now();
        self.last_paint = Some(started);
        let shell = &mut live.shell;
        crate::zoom::begin_frame(&ctx, zoom);
        let mut out = ctx.run_ui(raw, |ui| {
            shell.frame(ui);
            if let Some(due) = script_due {
                ui.ctx().request_repaint_after(due);
            }
        });
        let ui_done = Instant::now();
        #[cfg(target_os = "linux")]
        if let Some(edge) = resize_edge(live) {
            out.platform_output.cursor_icon = edge.cursor();
        }
        if let Some(ime) = &mut out.platform_output.ime {
            // Back from the stage's points to the window's.
            ime.rect = ime.rect * zoom;
            ime.cursor_rect = ime.cursor_rect * zoom;
        }
        live.state.handle_platform_output_with_event_loop(
            &live.window,
            event_loop,
            out.platform_output,
        );
        if (live.shell.stage_zoom(&ctx) - zoom).abs() > 1e-6 {
            // The app changed its zoom: the next frame lays out again.
            ctx.request_repaint();
        }
        live.textures.append(out.textures_delta);
        let viewport_output = out.viewport_output.remove(&ViewportId::ROOT);
        // An animation or a scroll asked for the next frame right away.
        let continuous = viewport_output
            .as_ref()
            .is_some_and(|v| v.repaint_delay.is_zero());
        let commands = viewport_output.map(|v| v.commands).unwrap_or_default();
        let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
        let tessellated = Instant::now();
        let rasterized;
        let mut damaged_px = 0usize;

        let size = live.window.inner_size();
        // A normal run's window waits for boot (or the failsafe); a scene's
        // shows with its first frame, cloaked.
        let show = !live.shown && live.shell.take_show(Instant::now());
        // Hidden in the tray, a frame only runs the UI, as when minimized.
        let hidden = live.hidden();
        let mut screenshot = None;
        let mut fault = None;
        let frame = match (
            NonZeroU32::new(size.width),
            NonZeroU32::new(size.height),
            hidden,
        ) {
            (Some(w), Some(h), false) => {
                match frame_buffer(&mut live.canvas, &mut live.faults, w, h) {
                    Ok(buffer) => Some((w, h, buffer)),
                    Err(err) => {
                        fault = Some(err);
                        None
                    }
                }
            }
            _ => None,
        };
        if let Some((w, h, mut buffer)) = frame {
            // A buffer that still holds the last frame only needs the tiles
            // that changed; a new one (first frame, resize) or a repaint the
            // OS asked for gets everything.
            let full = std::mem::take(&mut live.full_present)
                || healing
                || show
                || buffer.age() == 0
                || os_wants_full_paint(&live.window);
            // SAFETY: [u8; 4] has the size of u32 and a weaker alignment.
            let pixels = unsafe {
                std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<[u8; 4]>(), buffer.len())
            };
            let window_px = [w.get() as usize, h.get() as usize];
            let canvas_px = if zoomed {
                crate::zoom::stage_size(window_px, zoom)
            } else {
                window_px
            };
            let mut textures = std::mem::take(&mut live.textures);
            live.render.render_to_canvas(
                canvas_px[0],
                canvas_px[1],
                &prims,
                &textures,
                out.pixels_per_point,
            );
            // Applied: epaint asserts a dropped delta was handled.
            textures.clear();
            // Premultiplied BGRA over transparent, and GDI keeps the alpha
            // byte, so the acrylic shows through where egui paints nothing.
            let tiles = if zoomed {
                // The whole stage, then the compositor's scale into the
                // window; the window presents in full.
                live.stage.resize(canvas_px[0] * canvas_px[1], [0; 4]);
                let mut stage = BufferMutRef::new(&mut live.stage, canvas_px[0], canvas_px[1]);
                live.render.copy_canvas_to_buffer(&mut stage, true);
                crate::zoom::resample(&live.stage, canvas_px, pixels, window_px, zoom);
                vec![[0, 0, window_px[0], window_px[1]]]
            } else {
                let mut target = BufferMutRef::new(pixels, window_px[0], window_px[1]);
                live.render.copy_canvas_to_buffer(&mut target, full)
            };
            // Windows may have invalidated the window while this frame
            // rendered. The present validates all of it, so it carries all
            // of it; checked last to leave no frame in between.
            let tiles = if !full && !zoomed && os_wants_full_paint(&live.window) {
                let mut target = BufferMutRef::new(pixels, window_px[0], window_px[1]);
                live.render.copy_canvas_to_buffer(&mut target, true)
            } else {
                tiles
            };
            let damage: Vec<softbuffer::Rect> = tiles
                .into_iter()
                .filter_map(|[x, y, width, height]| {
                    Some(softbuffer::Rect {
                        x: x as u32,
                        y: y as u32,
                        width: NonZeroU32::new(width as u32)?,
                        height: NonZeroU32::new(height as u32)?,
                    })
                })
                .collect();
            rasterized = Instant::now();

            egui_winit::process_viewport_commands(
                &ctx,
                &mut live.info,
                commands,
                &live.window,
                &mut live.actions,
            );
            if live
                .actions
                .iter()
                .any(|a| matches!(a, egui_winit::ActionRequested::Screenshot(_)))
            {
                screenshot = Some(read_pixels(pixels, [w.get() as usize, h.get() as usize]));
            }
            damaged_px = damage
                .iter()
                .map(|r| r.width.get() as usize * r.height.get() as usize)
                .sum();
            if show {
                // The window shows right before a full present: a blit into
                // a hidden window is lost, and the repaint the OS asks for
                // when it shows would be swallowed by the next damaged-tiles
                // present, since softbuffer validates the whole window.
                live.shown = true;
                live.script = bench::Script::from_env();
                // A `--tray` launch never reveals here: it waits in the tray.
                if live.shell.auto_reveal() {
                    show_window(
                        &live.window,
                        live.shell
                            .window
                            .as_ref()
                            .is_some_and(|h| h.focuses_on_show()),
                    );
                }
            }
            match buffer.present_with_damage(&damage) {
                Ok(()) => live.faults.succeeded(),
                Err(err) => {
                    live.faults.fail();
                    fault = Some(format!("present: {err}"));
                }
            }
        } else {
            rasterized = Instant::now();
            egui_winit::process_viewport_commands(
                &ctx,
                &mut live.info,
                commands,
                &live.window,
                &mut live.actions,
            );
        }
        if let Some(err) = fault {
            // The frame is lost: the next one draws everything again.
            live.full_present = true;
            if live.faults.logged() {
                live.shell
                    .log_window_error("Skipped a frame the presenter failed to draw", &err);
            }
            retry = Some(Instant::now() + live.faults.retry_after());
        }

        if bench::enabled() {
            let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
            let done = Instant::now();
            // What asked for this frame, `file:line` of each request.
            let why: Vec<String> = ctx
                .repaint_causes()
                .iter()
                .take(4)
                .map(|c| {
                    format!(
                        "{}:{}",
                        c.file.rsplit(['/', '\\']).next().unwrap_or(""),
                        c.line
                    )
                })
                .collect();
            bench::mark_with(
                "frame",
                &format!(
                    "\"spans\":{},\"why\":{:?},\"px\":[{},{}],\"damaged\":{},\"ui\":{:.3},\"tessellate\":{:.3},\"raster\":{:.3},\"present\":{:.3},\"held\":{:?}",
                    bench::take_spans(),
                    why.join(" "),
                    size.width,
                    size.height,
                    damaged_px,
                    ms(started, ui_done),
                    ms(ui_done, tessellated),
                    ms(tessellated, rasterized),
                    ms(rasterized, done),
                    live.render.held_bytes(),
                ),
            );
        }

        for action in live.actions.drain(..) {
            let event = match action {
                egui_winit::ActionRequested::Screenshot(callback) => {
                    if let Some(image) = screenshot.clone() {
                        callback.complete(image);
                    }
                    continue;
                }
                // Nothing here sends `RequestPaintWhileHidden`.
                egui_winit::ActionRequested::PaintWhileHidden => continue,
                egui_winit::ActionRequested::Cut => egui::Event::Cut,
                egui_winit::ActionRequested::Copy => egui::Event::Copy,
                egui_winit::ActionRequested::Paste => match live.state.clipboard_text() {
                    Some(text) => egui::Event::Paste(text),
                    None => continue,
                },
            };
            live.state.egui_input_mut().events.push(event);
            ctx.request_repaint();
        }

        if show && !live.shown {
            // A zero-sized frame presented nothing; show anyway.
            live.shown = true;
            if live.shell.auto_reveal() {
                show_window(
                    &live.window,
                    live.shell
                        .window
                        .as_ref()
                        .is_some_and(|h| h.focuses_on_show()),
                );
            }
        }
        // An animation's next frame starts on the vblank that shows this one.
        self.armed =
            continuous && live.shown && !hidden && self.vblank.as_ref().is_some_and(|c| c.arm());
        if live
            .info
            .events
            .iter()
            .any(|e| matches!(e, ViewportEvent::Close))
        {
            self.request_close(event_loop);
        }
        if let Some(&next) = self.heal_due.first() {
            self.repaint_at(next);
        }
        if let Some(at) = retry {
            self.repaint_at(at);
        }
    }

    /// The frame interval from the refresh rate of the monitor the window
    /// is on, read again when it moves to another one or on `force` (the
    /// display mode may have changed).
    fn follow_monitor(&mut self, force: bool) {
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

    /// The window's pixels may be gone ([`take_content_lost`], a reveal).
    /// A present right away can land before DWM composes the window again
    /// and be clipped to nothing; softbuffer then validates the window and
    /// Windows asks for nothing more. So full presents follow over a few
    /// seconds, through a new window DC: softbuffer keeps the one it took
    /// when the surface was made.
    fn heal(&mut self) {
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
    fn ensure_tray(&mut self) {
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
    fn request_close(&mut self, event_loop: &ActiveEventLoop) {
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

    /// Show and focus the window again from the tray.
    fn reveal(&mut self) {
        if let Some(live) = &mut self.live {
            live.window.set_visible(true);
            live.window.focus_window();
            live.shown = true;
        }
        self.heal();
    }

    /// The title bar's close button or the system's (Alt+F4, the taskbar):
    /// the shell's close sequence, then the loop ends.
    fn close(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(live) = &mut self.live {
            let geometry = geometry(&live.window);
            let window = &live.window;
            live.shell.close(geometry, || window.set_visible(false));
        }
        event_loop.exit();
    }
}

/// `show_main_window`: show, then `set_focus` for a normal run.
fn show_window(window: &Window, focus: bool) {
    window.set_visible(true);
    bench::mark("visible");
    if focus {
        window.focus_window();
    }
    bench::mark("shown");
}

/// boot.rs `current_geometry` on the winit window: inner size and outer
/// origin, which the frame makes tao's. Wayland has no origin to give: the
/// size goes alone.
fn geometry(window: &Window) -> Option<crate::window::Geometry> {
    let outer = window.outer_position().ok();
    let inner = window.inner_size();
    crate::window::Geometry::read(
        window.is_maximized(),
        window.is_minimized().unwrap_or(false),
        window.scale_factor(),
        (inner.width, inner.height),
        outer.map(|outer| (outer.x, outer.y)),
    )
}

/// One frame at the refresh rate of `monitor`.
fn frame_interval(monitor: Option<&MonitorHandle>) -> Option<Duration> {
    let mhz = monitor?.refresh_rate_millihertz().filter(|&mhz| mhz > 0)?;
    Some(Duration::from_secs_f64(1000.0 / f64::from(mhz)))
}

/// The resize edge under the pointer, when the window can be resized.
#[cfg(target_os = "linux")]
fn resize_edge(live: &Live) -> Option<crate::window::edges::Edge> {
    use crate::window::edges;
    let window = &live.window;
    if window.is_maximized() || window.fullscreen().is_some() || !window.is_resizable() {
        return None;
    }
    let size = window.inner_size();
    edges::hit(
        live.pointer?,
        (f64::from(size.width), f64::from(size.height)),
        edges::BAND * window.scale_factor(),
    )
}

/// Tracks the pointer for the resize edges and hands a press on one to the
/// window manager's interactive resize. True when egui must not see the
/// event.
#[cfg(target_os = "linux")]
fn edge_press(live: &mut Live, event: &WindowEvent) -> bool {
    use crate::window::edges::Edge;
    use winit::event::{ElementState, MouseButton};
    use winit::window::ResizeDirection;
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            live.pointer = Some((position.x, position.y));
        }
        WindowEvent::CursorLeft { .. } => live.pointer = None,
        WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: MouseButton::Left,
            ..
        } => {
            let Some(edge) = resize_edge(live) else {
                return false;
            };
            let direction = match edge {
                Edge::North => ResizeDirection::North,
                Edge::South => ResizeDirection::South,
                Edge::East => ResizeDirection::East,
                Edge::West => ResizeDirection::West,
                Edge::NorthEast => ResizeDirection::NorthEast,
                Edge::NorthWest => ResizeDirection::NorthWest,
                Edge::SouthEast => ResizeDirection::SouthEast,
                Edge::SouthWest => ResizeDirection::SouthWest,
            };
            return live.window.drag_resize_window(direction).is_ok();
        }
        _ => {}
    }
    false
}

/// Whether Windows has part of the client area marked for repaint: it does
/// when it shows, restores or otherwise invalidates the window, and then it
/// expects all of it back. softbuffer validates the whole window on every
/// present, so a damaged-tiles present would swallow that request and leave
/// stale or blank areas until the next full present.
#[cfg(windows)]
fn os_wants_full_paint(window: &Window) -> bool {
    let Some(hwnd) = hwnd(window) else {
        return false;
    };
    // SAFETY: live HWND owned by this runner; a null rect only asks whether
    // the update region is empty.
    unsafe {
        windows_sys::Win32::Graphics::Gdi::GetUpdateRect(hwnd as _, std::ptr::null_mut(), 0) != 0
    }
}

fn hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
fn os_wants_full_paint(_window: &Window) -> bool {
    false
}

/// [`crate::window::frame::take_content_lost`].
#[cfg(windows)]
fn take_content_lost() -> bool {
    crate::window::frame::take_content_lost()
}

#[cfg(not(windows))]
fn take_content_lost() -> bool {
    false
}

/// The renderer's view of [`crate::ui::backdrop_filter::Mark`].
fn backdrop_op(
    callback: &egui::PaintCallback,
) -> Option<egui_software_backend::backdrop::BackdropOp> {
    use crate::ui::backdrop_filter::Mark;
    use egui_software_backend::backdrop::BackdropOp;
    Mark::of(callback).map(|mark| match mark {
        Mark::Root => BackdropOp::Root,
        Mark::Begin {
            sigma,
            opacity,
            saturate,
            radius,
            ppp,
        } => BackdropOp::Begin {
            sigma,
            opacity,
            saturate,
            // The callback's rect, moved by any visual transform since.
            clip: radius.map(|r| egui_software_backend::backdrop::BackdropClip {
                min: [callback.rect.min.x * ppp, callback.rect.min.y * ppp],
                max: [callback.rect.max.x * ppp, callback.rect.max.y * ppp],
                radius: r * ppp,
            }),
        },
        Mark::Group { opacity } => BackdropOp::Group { opacity },
        Mark::End => BackdropOp::End,
        Mark::Filter {
            sigma,
            saturate,
            scale,
        } => BackdropOp::Filter {
            sigma,
            saturate,
            scale,
        },
    })
}

fn read_pixels(bgra: &[[u8; 4]], size: [usize; 2]) -> Arc<egui::ColorImage> {
    let pixels = bgra
        .iter()
        .map(|&[b, g, r, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a))
        .collect();
    Arc::new(egui::ColorImage::new(size, pixels))
}

/// egui-winit drops the context menu key, egui having none: it goes in as
/// [`crate::CONTEXT_MENU_KEY`].
fn context_menu_key(live: &mut Live, event: &WindowEvent) -> bool {
    use winit::keyboard::{Key, NamedKey};
    let key = match event {
        WindowEvent::ModifiersChanged(held) => {
            // As egui-winit reads them.
            let held = held.state();
            let mac = cfg!(target_os = "macos");
            live.modifiers = egui::Modifiers {
                alt: held.alt_key(),
                ctrl: held.control_key(),
                shift: held.shift_key(),
                mac_cmd: mac && held.super_key(),
                command: if mac {
                    held.super_key()
                } else {
                    held.control_key()
                },
            };
            return false;
        }
        WindowEvent::KeyboardInput { event: key, .. } => key,
        _ => return false,
    };
    if key.logical_key != Key::Named(NamedKey::ContextMenu) {
        return false;
    }
    let modifiers = live.modifiers;
    live.state.egui_input_mut().events.push(egui::Event::Key {
        key: crate::CONTEXT_MENU_KEY,
        physical_key: Some(crate::CONTEXT_MENU_KEY),
        pressed: key.state.is_pressed(),
        repeat: key.repeat,
        modifiers,
    });
    true
}

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(init) = self.init.take() else { return };
        bench::mark("resumed");

        let builder = self.viewport.clone().with_visible(false);
        let window = match egui_winit::create_window(&self.ctx, event_loop, &builder) {
            Ok(window) => Rc::new(window),
            Err(err) => {
                eprintln!("window creation failed: {err}");
                event_loop.exit();
                return;
            }
        };
        bench::mark("window");
        // No presenter, no app: a message and a clean exit, not a panic.
        let context = match softbuffer::Context::new(event_loop.owned_display_handle()) {
            Ok(context) => context,
            Err(err) => {
                eprintln!("presenter setup failed: {err}");
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(surface) => surface,
            Err(err) => {
                eprintln!("presenter setup failed: {err}");
                event_loop.exit();
                return;
            }
        };

        let mut state = egui_winit::State::new(
            self.ctx.clone(),
            ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            window.theme(),
            None,
        );
        bench::mark("surface_input");
        // AccessKit wants its adapter before the window is first shown.
        state.init_accesskit(event_loop, &window, self.proxy.clone());
        bench::mark("accesskit");

        let proxy = Mutex::new(self.proxy.clone());
        self.ctx.set_request_repaint_callback(move |info| {
            if let Ok(proxy) = proxy.lock() {
                let _ = proxy.send_event(UserEvent::Repaint {
                    when: Instant::now() + info.delay,
                    pass: info.current_cumulative_pass_nr,
                });
            }
        });

        self.vblank = hwnd(&window).and_then(|hwnd| {
            let proxy = self.proxy.clone();
            crate::vblank::Clock::new(hwnd, move || {
                let _ = proxy.send_event(UserEvent::VBlank);
            })
        });
        // The monitor's refresh rate; the primary one while the window sits
        // off screen.
        self.monitor = window
            .current_monitor()
            .or_else(|| event_loop.primary_monitor());
        if let Some(interval) = frame_interval(self.monitor.as_ref()) {
            self.frame_interval = interval;
        }

        let shell = init(&self.ctx, &window);
        let mut info = ViewportInfo::default();
        egui_winit::update_viewport_info(&mut info, &self.ctx, &window, true);
        self.live = Some(Live {
            canvas: Canvas {
                window: window.clone(),
                context,
                surface,
            },
            window,
            faults: Faults::default(),
            #[cfg(target_os = "linux")]
            pointer: None,
            state,
            info,
            shell,
            render: EguiSoftwareRender::new(ColorFieldOrder::Bgra)
                .with_glyph_coverage(crate::ui::text::coverage_tables())
                .with_backdrop(Box::new(backdrop_op)),
            textures: Default::default(),
            actions: Vec::new(),
            full_present: true,
            stage_zoom: 1.0,
            stage: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            shown: false,
            script: None,
        });
        // A `--tray` launch needs the icon up front: the window never shows
        // on its own, so the tray is the only way to reach the app.
        if self.live.as_ref().is_some_and(|l| l.shell.starts_in_tray()) {
            self.ensure_tray();
            if self.tray.is_none() {
                // No tray here (none off Windows, or it failed): the window
                // shows on boot, else nothing would reach the app.
                if let Some(live) = &mut self.live {
                    live.shell.no_tray();
                }
            }
        }
        self.paint(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Repaint { when, pass } => {
                // Drops requests from a pass already painted.
                let current = self.ctx.cumulative_pass_nr_for(ViewportId::ROOT);
                if current == pass || current == pass + 1 {
                    self.repaint_at(when);
                }
            }
            UserEvent::VBlank => {
                if std::mem::take(&mut self.armed) && self.next_repaint.is_some() {
                    self.next_repaint = Some(Instant::now());
                }
            }
            UserEvent::AccessKit(event) => {
                let Some(live) = &mut self.live else { return };
                match event.window_event {
                    accesskit_winit::WindowEvent::InitialTreeRequested => {
                        self.ctx.enable_accesskit();
                        self.repaint_at(Instant::now());
                    }
                    accesskit_winit::WindowEvent::ActionRequested(request) => {
                        live.state.on_accesskit_action_request(request);
                        self.repaint_at(Instant::now());
                    }
                    accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                        self.ctx.disable_accesskit();
                    }
                }
            }
            UserEvent::Tray(action) => match action {
                TrayAction::Open => self.reveal(),
                TrayAction::Quit => self.close(event_loop),
            },
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.request_close(event_loop);
                return;
            }
            // A resize alone is saved too: on macOS the window may end
            // through Cmd+Q, and Wayland never reports a move.
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if let Some(live) = &self.live {
                    if live.shell.window_moved(geometry(&live.window)) {
                        self.ctx.request_repaint();
                    }
                }
                self.follow_monitor(false);
            }
            WindowEvent::ScaleFactorChanged { .. } => self.follow_monitor(true),
            // Back in front: put back any tile Windows dropped while the
            // window was away, then egui gets the event.
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                if let Some(live) = &mut self.live {
                    live.full_present = true;
                }
                self.repaint_at(Instant::now());
            }
            WindowEvent::RedrawRequested => {
                // Only the OS asks for this (the runner paints directly), and
                // it wants the whole client area back.
                if let Some(live) = &mut self.live {
                    live.full_present = true;
                }
                self.next_repaint = None;
                self.paint(event_loop);
                return;
            }
            _ => {}
        }
        let Some(live) = &mut self.live else { return };
        #[cfg(target_os = "linux")]
        if edge_press(live, &event) {
            return;
        }
        let response = live.state.on_window_event(&live.window, &event);
        let menu_key = context_menu_key(live, &event);
        if response.repaint || menu_key {
            self.repaint_at(Instant::now());
        }
    }

    /// The loop also ends without a close request: the macOS Quit menu,
    /// Cmd+Q and the Dock's Quit. The close sequence runs here then; after a
    /// normal close it already ran, and [`Shell::close`] runs once.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(live) = &mut self.live {
            let geometry = geometry(&live.window);
            let window = &live.window;
            live.shell.close(geometry, || window.set_visible(false));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if take_content_lost() {
            self.heal();
        }
        match self.next_repaint {
            Some(when) if when <= Instant::now() => {
                self.next_repaint = None;
                self.paint(event_loop);
                // The frame may have asked for another one right away.
                match self.next_repaint {
                    Some(when) => event_loop.set_control_flow(ControlFlow::WaitUntil(when)),
                    None => event_loop.set_control_flow(ControlFlow::Wait),
                }
            }
            Some(when) => event_loop.set_control_flow(ControlFlow::WaitUntil(when)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A presenter whose buffer fails a given number of times.
    struct Flaky {
        failures: u32,
        rebuilt: u32,
    }

    impl Present for Flaky {
        type Buffer<'a> = ();

        fn rebuild(&mut self) -> Result<(), String> {
            self.rebuilt += 1;
            Ok(())
        }

        fn resize(&mut self, _: NonZeroU32, _: NonZeroU32) -> Result<(), String> {
            Ok(())
        }

        fn buffer(&mut self) -> Result<(), String> {
            if self.failures == 0 {
                return Ok(());
            }
            self.failures -= 1;
            Err("buffer: lost".into())
        }
    }

    #[test]
    fn a_failed_frame_is_skipped_and_the_next_rebuilds_the_surface() {
        let size = NonZeroU32::new(8).unwrap();
        let mut presenter = Flaky {
            failures: 1,
            rebuilt: 0,
        };
        let mut faults = Faults::default();
        assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_err());
        assert!(faults.logged());
        assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_ok());
        assert_eq!(presenter.rebuilt, 1);
        faults.succeeded();
        // A healthy frame does not rebuild.
        assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_ok());
        assert_eq!(presenter.rebuilt, 1);
    }

    #[test]
    fn repeated_failures_back_off_and_stop_logging() {
        let mut faults = Faults::default();
        let mut last = Duration::ZERO;
        for _ in 0..LOGGED_FAULTS {
            faults.fail();
            assert!(faults.logged());
            assert!(faults.retry_after() > last);
            last = faults.retry_after();
        }
        for _ in 0..20 {
            faults.fail();
        }
        assert!(!faults.logged());
        assert_eq!(faults.retry_after(), Duration::from_secs(1));
    }
}
