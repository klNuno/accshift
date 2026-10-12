//! The software runner: what eframe does for this spike, minus OpenGL.
//!
//! Window and input go through egui-winit exactly as in eframe. egui's meshes
//! are rasterized on the CPU by the vendored `egui_software_backend`, and the
//! frame is blitted with softbuffer (a 32-bit DIB and `BitBlt` on Windows).
//! No GPU driver is loaded: with glow, the NVIDIA OpenGL driver took about
//! 300 ms before the first frame and most of the 100 MB working set.

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
use winit::window::{Window, WindowId};

use crate::{bench, Shell};

pub enum UserEvent {
    /// From egui's repaint callback, possibly on another thread.
    Repaint {
        when: Instant,
        pass: u64,
    },
    AccessKit(accesskit_winit::Event),
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
    };
    event_loop.run_app(&mut runner)
}

struct Live {
    window: Rc<Window>,
    surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
    state: egui_winit::State,
    info: ViewportInfo,
    shell: Shell,
    render: EguiSoftwareRender,
    /// Texture changes not rasterized yet (a minimized window has no pixels).
    textures: egui::TexturesDelta,
    actions: Vec<egui_winit::ActionRequested>,
    full_present: bool,
    shown: bool,
}

struct Runner {
    ctx: egui::Context,
    proxy: EventLoopProxy<UserEvent>,
    viewport: egui::ViewportBuilder,
    init: Option<Init>,
    live: Option<Live>,
    next_repaint: Option<Instant>,
    /// Frame pacing in place of vsync: a blit returns at once, so an egui
    /// animation would otherwise repaint as fast as the CPU allows.
    last_paint: Option<Instant>,
    frame_interval: Duration,
}

impl Runner {
    fn repaint_at(&mut self, when: Instant) {
        let when = match self.last_paint {
            Some(last) => when.max(last + self.frame_interval),
            None => when,
        };
        self.next_repaint = Some(self.next_repaint.map_or(when, |t| t.min(when)));
    }

    fn paint(&mut self, event_loop: &ActiveEventLoop) {
        let ctx = self.ctx.clone();
        let Some(live) = &mut self.live else { return };

        egui_winit::update_viewport_info(&mut live.info, &ctx, &live.window, false);
        let mut raw = live.state.take_egui_input(&live.window);
        raw.viewports.insert(ViewportId::ROOT, live.info.clone());
        live.info.events.clear();
        live.shell.input_hook(&ctx, &mut raw);

        let started = Instant::now();
        self.last_paint = Some(started);
        let shell = &mut live.shell;
        let mut out = ctx.run_ui(raw, |ui| shell.frame(ui));
        let ui_done = Instant::now();
        live.state.handle_platform_output_with_event_loop(
            &live.window,
            event_loop,
            out.platform_output,
        );
        live.textures.append(out.textures_delta);
        let commands = out
            .viewport_output
            .remove(&ViewportId::ROOT)
            .map(|v| v.commands)
            .unwrap_or_default();
        let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
        let tessellated = Instant::now();
        let rasterized;
        let mut damaged_px = 0usize;

        let size = live.window.inner_size();
        let mut screenshot = None;
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            live.surface.resize(w, h).expect("softbuffer resize");
            let mut buffer = live.surface.buffer_mut().expect("softbuffer buffer");
            // A buffer that still holds the last frame only needs the tiles
            // that changed; a new one (first frame, resize) or a repaint the
            // OS asked for gets everything.
            let full = std::mem::take(&mut live.full_present)
                || buffer.age() == 0
                || os_wants_full_paint(&live.window);
            // SAFETY: [u8; 4] has the size of u32 and a weaker alignment.
            let pixels = unsafe {
                std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<[u8; 4]>(), buffer.len())
            };
            let mut target = BufferMutRef::new(pixels, w.get() as usize, h.get() as usize);
            let textures = std::mem::take(&mut live.textures);
            live.render.render_to_canvas(
                w.get() as usize,
                h.get() as usize,
                &prims,
                &textures,
                out.pixels_per_point,
            );
            // Premultiplied BGRA over transparent, and GDI keeps the alpha
            // byte, so the acrylic shows through where egui paints nothing.
            let damage: Vec<softbuffer::Rect> = live
                .render
                .copy_canvas_to_buffer(&mut target, full)
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
            if !live.shown {
                // Like eframe, the window waits for its first frame, but it
                // shows right before that frame's present: a blit into a
                // hidden window is lost, and the repaint the OS asks for when
                // it shows would be swallowed by the next damaged-tiles
                // present, since softbuffer validates the whole window.
                live.shown = true;
                live.window.set_visible(true);
            }
            buffer
                .present_with_damage(&damage)
                .expect("softbuffer present");
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

        if bench::enabled() {
            let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
            let done = Instant::now();
            bench::mark_with(
                "frame",
                &format!(
                    "\"px\":[{},{}],\"damaged\":{},\"ui\":{:.3},\"tessellate\":{:.3},\"raster\":{:.3},\"present\":{:.3}",
                    size.width,
                    size.height,
                    damaged_px,
                    ms(started, ui_done),
                    ms(ui_done, tessellated),
                    ms(tessellated, rasterized),
                    ms(rasterized, done),
                ),
            );
        }

        for action in live.actions.drain(..) {
            let event = match action {
                egui_winit::ActionRequested::Screenshot(user_data) => {
                    let Some(image) = screenshot.clone() else {
                        continue;
                    };
                    egui::Event::Screenshot {
                        viewport_id: ViewportId::ROOT,
                        user_data,
                        image,
                    }
                }
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

        if !live.shown {
            // A zero-sized first frame presented nothing; show anyway.
            live.shown = true;
            live.window.set_visible(true);
        }
        if live
            .info
            .events
            .iter()
            .any(|e| matches!(e, ViewportEvent::Close))
        {
            event_loop.exit();
        }
    }
}

/// Whether Windows has part of the client area marked for repaint: it does
/// when it shows, restores or otherwise invalidates the window, and then it
/// expects all of it back. softbuffer validates the whole window on every
/// present, so a damaged-tiles present would swallow that request and leave
/// stale or blank areas until the next full present.
#[cfg(windows)]
fn os_wants_full_paint(window: &Window) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return false;
    };
    // SAFETY: live HWND owned by this runner; a null rect only asks whether
    // the update region is empty.
    unsafe {
        windows_sys::Win32::Graphics::Gdi::GetUpdateRect(h.hwnd.get() as _, std::ptr::null_mut(), 0)
            != 0
    }
}

#[cfg(not(windows))]
fn os_wants_full_paint(_window: &Window) -> bool {
    false
}

/// The frame as egui's glow painter returns it: premultiplied RGBA.
/// The renderer's view of [`crate::ui::backdrop_filter::Mark`].
fn backdrop_op(
    callback: &egui::PaintCallback,
) -> Option<egui_software_backend::backdrop::BackdropOp> {
    use crate::ui::backdrop_filter::Mark;
    use egui_software_backend::backdrop::BackdropOp;
    Mark::of(callback).map(|mark| match mark {
        Mark::Root => BackdropOp::Root,
        Mark::Begin { sigma, opacity } => BackdropOp::Begin { sigma, opacity },
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

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(init) = self.init.take() else { return };

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
        let context = softbuffer::Context::new(event_loop.owned_display_handle())
            .expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");

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

        // The monitor's refresh rate; the primary one while the window sits
        // off screen.
        if let Some(mhz) = window
            .current_monitor()
            .or_else(|| event_loop.primary_monitor())
            .and_then(|monitor| monitor.refresh_rate_millihertz())
            .filter(|&mhz| mhz > 0)
        {
            self.frame_interval = Duration::from_secs_f64(1000.0 / f64::from(mhz));
        }

        let shell = init(&self.ctx, &window);
        let mut info = ViewportInfo::default();
        egui_winit::update_viewport_info(&mut info, &self.ctx, &window, true);
        self.live = Some(Live {
            window,
            surface,
            state,
            info,
            shell,
            render: EguiSoftwareRender::new(ColorFieldOrder::Bgra)
                .with_glyph_coverage(crate::ui::text::coverage_tables())
                .with_backdrop(Box::new(backdrop_op)),
            textures: Default::default(),
            actions: Vec::new(),
            full_present: true,
            shown: false,
        });
        self.paint(event_loop);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Repaint { when, pass } => {
                // eframe's guard against requests from a pass already painted.
                let current = self.ctx.cumulative_pass_nr_for(ViewportId::ROOT);
                if current == pass || current == pass + 1 {
                    self.repaint_at(when);
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
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
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
        let response = live.state.on_window_event(&live.window, &event);
        if response.repaint {
            self.repaint_at(Instant::now());
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
