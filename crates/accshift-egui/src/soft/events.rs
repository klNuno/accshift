//! Winit [`ApplicationHandler`] implementation for [`Runner`].

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{ViewportId, ViewportInfo};
use egui_software_backend::{ColorFieldOrder, EguiSoftwareRender};
use egui_winit::accesskit_winit;
use egui_winit::winit;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::bench;
use crate::window::hotkeys::Signal;
use crate::window::tray::TrayAction;

#[cfg(target_os = "linux")]
use super::platform::edge_press;
use super::platform::{
    context_menu_key, frame_interval, geometry, hwnd, is_shortcut_press, take_content_lost,
};
use super::present::{backdrop_op, Canvas, Faults};
use super::{Live, Runner, UserEvent};

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

        let mut shell = init(&self.ctx, &window);
        let hotkeys = Mutex::new(self.proxy.clone());
        shell.set_hotkey_deliver(Arc::new(move |signal| {
            if let Ok(proxy) = hotkeys.lock() {
                let _ = proxy.send_event(UserEvent::Hotkey(signal));
            }
        }));
        let mut info = ViewportInfo::default();
        egui_winit::update_viewport_info(&mut info, &self.ctx, &window, true);
        #[cfg(target_os = "linux")]
        let raiser =
            crate::window::wayland::Raiser::new(window.clone(), event_loop.owned_display_handle());
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
            overlay_render: EguiSoftwareRender::new(ColorFieldOrder::Bgra)
                .with_glyph_coverage(crate::ui::text::coverage_tables())
                .with_backdrop(Box::new(backdrop_op)),
            overlay_textures: Default::default(),
            overlay_shown: false,
            actions: Vec::new(),
            full_present: true,
            stage_zoom: 1.0,
            stage: Vec::new(),
            overlay_backdrop: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            held: winit::keyboard::ModifiersState::empty(),
            #[cfg(target_os = "linux")]
            raiser,
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
                TrayAction::Open => {
                    self.reveal(None);
                }
                TrayAction::Quit => self.close(event_loop),
            },
            UserEvent::Hotkey(Signal::Changed) => {
                let Some(live) = &mut self.live else { return };
                live.shell.hotkeys_changed(&self.ctx);
                self.repaint_at(Instant::now());
            }
            UserEvent::Hotkey(Signal::Pressed { id, token }) => {
                use crate::app::shortcuts::WindowOp;
                let Some(live) = &mut self.live else { return };
                let raised = match live.shell.hotkey(&self.ctx, id) {
                    WindowOp::None => true,
                    WindowOp::Reveal => self.reveal(token.as_deref()),
                    WindowOp::Toggle => self.toggle_window(token.as_deref()),
                };
                if !raised {
                    if let Some(live) = &mut self.live {
                        live.shell.hotkey_cannot_raise(&self.ctx);
                    }
                }
                self.repaint_at(Instant::now());
            }
            UserEvent::Quit => self.close(event_loop),
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
        // A listening shortcut row reads the press as winit gives it, before
        // egui-winit rewrites it ([`crate::window::keystroke`]).
        if let WindowEvent::KeyboardInput {
            event: key,
            is_synthetic,
            ..
        } = &event
        {
            if is_shortcut_press(key.state.is_pressed(), key.repeat, *is_synthetic)
                && live.shell.capturing_shortcut()
            {
                let stroke = crate::window::keystroke::stroke(
                    key,
                    live.held,
                    crate::window::hotkeys::super_at_press(),
                );
                live.shell.shortcut_stroke(&self.ctx, stroke);
            }
        }
        let response = live.state.on_window_event(&live.window, &event);
        let menu_key = context_menu_key(live, &event);
        if response.repaint || menu_key {
            self.repaint_at(Instant::now());
        }
    }

    /// The loop also ends without a close request: the macOS Quit menu,
    /// Cmd+Q and the Dock's Quit. The close sequence runs here then; after a
    /// normal close it already ran, and [`crate::Shell::close`] runs once.
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
