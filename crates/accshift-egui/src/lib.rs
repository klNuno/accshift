//! The accshift UI in egui. See `README.md` next to this crate.

pub mod app;
pub mod avatars;
pub mod backdrop;
pub mod bench;
pub mod clipboard;
pub mod data;
pub mod harness;
pub mod icons;
pub mod screens;
#[cfg(feature = "software")]
pub mod soft;
pub mod theme;
pub mod ui;
pub mod vblank;
pub mod window;
pub mod zoom;

pub use app::{Action, AppModel};
pub use screens::Screen;

#[cfg(all(feature = "software", feature = "glow"))]
compile_error!("pick one renderer: `software` (default) or `glow` with --no-default-features");

/// What both runners drive: the current screen plus the optional scene run.
/// The window is cleared to transparent so the OS backdrop shows through the
/// theme's window fill, like the Tauri window.
pub struct Shell {
    pub screen: Screen,
    pub scenes: Option<harness::SceneRun>,
    /// Input held for the next frame by [`sequence_input`].
    pub held: Vec<egui::Event>,
    /// The window of a normal run; scenes and the bench have none.
    pub window: Option<window::Host>,
}

impl Shell {
    pub fn new(screen: Screen, scenes: Option<harness::SceneRun>) -> Self {
        Self {
            screen,
            scenes,
            held: Vec::new(),
            window: None,
        }
    }

    pub fn with_window(mut self, host: window::Host) -> Self {
        self.window = Some(host);
        self
    }

    /// Whether the window reveals itself on boot. False for a `--tray`
    /// launch: it waits in the tray until the user opens it. Scenes and the
    /// bench have no host and always reveal (cloaked).
    pub fn auto_reveal(&self) -> bool {
        self.window
            .as_ref()
            .is_none_or(|host| !host.starts_hidden())
    }

    /// A normal run that owns a window (and therefore a possible tray).
    pub fn supports_tray(&self) -> bool {
        self.window.is_some()
    }

    /// A `--tray` launch: the tray icon must exist from the start so the
    /// hidden app is reachable.
    pub fn starts_in_tray(&self) -> bool {
        self.window
            .as_ref()
            .is_some_and(|host| host.starts_hidden())
    }

    /// The close button hides to the tray instead of quitting: only a booted
    /// normal run with `closeToTray` on.
    pub fn wants_close_to_tray(&self) -> bool {
        self.window.is_some() && matches!(&self.screen, Screen::App(app) if app.close_to_tray())
    }

    /// The tray menu labels and tooltip, in the app's locale.
    pub fn tray_labels(&self) -> (String, String, String) {
        use accshift_app::i18n::{translate, Locale};
        let locale = match &self.screen {
            Screen::App(app) => app.locale(),
            _ => Locale::En,
        };
        (
            translate(locale, "tray.open"),
            translate(locale, "tray.quit"),
            "Accshift".to_string(),
        )
    }

    /// Whether the hidden window shows with this frame: at once without a
    /// [`window::Host`] (scenes keep theirs cloaked), else when the host
    /// says so, once.
    pub fn take_show(&mut self, now: std::time::Instant) -> bool {
        self.window.as_mut().is_none_or(|host| host.take_show(now))
    }

    /// boot.rs `install_window_event_handlers`, `Moved`. True when the
    /// frame must be painted again: the Liquid Glass wallpaper follows the
    /// window (`onMoved` then `realign`).
    pub fn window_moved(&self, geometry: Option<window::Geometry>) -> bool {
        if let Some(host) = &self.window {
            host.moved(geometry);
        }
        matches!(&self.screen, Screen::App(app) if app.wallpaper_shown())
    }

    /// The close sequence of a normal run (`SHUTDOWN_ORDER`): the geometry
    /// save starts, the window hides, the telemetry session ends, the save
    /// gets its 5 s, then every write the UI still holds is flushed. The
    /// runner exits after.
    pub fn close(&mut self, geometry: Option<window::Geometry>, hide: impl FnOnce()) {
        let Some(host) = &self.window else {
            return;
        };
        let wait = host.start_close_save(geometry);
        hide();
        host.backend().end_telemetry_session();
        if let Some(rx) = wait {
            let _ = rx.recv_timeout(accshift_app::lifecycle::GEOMETRY_SAVE_WAIT);
        }
        if let Screen::App(app) = &mut self.screen {
            if let Err(error) = app.flush_app_state() {
                let _ = accshift_core::logging::append_app_log(
                    host.backend().context(),
                    "error",
                    "frontend.app",
                    &accshift_app::lifecycle::boot::close_flush_error(&error),
                    None,
                );
            }
        }
    }

    /// The UI zoom the software runner applies to this frame: the stage
    /// lays out `1 / zoom` as large and is scaled back up. 1 while a scene
    /// is being set up, and whenever egui's zoom factor carries the zoom
    /// ([`zoom::upscaled`]).
    pub fn stage_zoom(&self, ctx: &egui::Context) -> f32 {
        if zoom::upscaled(ctx) && !self.scenes.as_ref().is_some_and(|run| run.holds_zoom()) {
            zoom::ui(ctx)
        } else {
            1.0
        }
    }

    /// `stage` is the zoom [`Self::stage_zoom`] gave for this frame: the
    /// window's events, the scene's included, move into the stage's points.
    pub fn input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput, stage: f32) {
        if let Some(run) = &mut self.scenes {
            run.feed(ctx, raw_input, self.screen.ready());
        }
        zoom::stage_input(raw_input, stage);
        if sequence_input(&mut self.held, &mut raw_input.events) {
            ctx.request_repaint();
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if let Some(run) = &mut self.scenes {
            run.collect();
            if let Some(scene) = run.take_build() {
                // The new app publishes its own UI zoom once booted.
                zoom::clear_ui(ui.ctx());
                match Screen::for_scene(ui.ctx(), &scene) {
                    Ok(screen) => self.screen = screen,
                    Err(why) => {
                        self.screen = Screen::Blank;
                        run.skip(&why);
                    }
                }
            }
        }
        let launched = self
            .window
            .as_ref()
            .map(|host| host.second_launches(ui.ctx()))
            .unwrap_or_default();
        if let (false, Screen::App(app)) = (launched.is_empty(), &mut self.screen) {
            app.open_urls(ui.ctx(), &launched);
        }
        if let (Some(run), Screen::App(app)) = (&mut self.scenes, &mut self.screen) {
            let links = run.take_links();
            if !links.is_empty() {
                app.open_urls(ui.ctx(), &links);
            }
        }
        self.screen.ui(ui);
        if let (Some(host), Screen::App(app)) = (&mut self.window, &self.screen) {
            if let Some(backdrop) = app.window_backdrop() {
                host.apply_backdrop(backdrop);
            }
        }
        // Snap Layouts: where the maximize button was drawn, in physical
        // client pixels (`set_maximize_button_rect`).
        #[cfg(windows)]
        if let (Some(_), Screen::App(app)) = (&self.window, &self.screen) {
            let scale = (ui.ctx().pixels_per_point() * self.stage_zoom(ui.ctx())) as f64;
            window::frame::set_maximize_button(app.maximize_button_rect().map(|r| {
                [
                    r.min.x as f64 * scale,
                    r.min.y as f64 * scale,
                    r.width() as f64 * scale,
                    r.height() as f64 * scale,
                ]
            }));
        }
        if let Some(host) = &self.window {
            // The failsafe's timer: a delayed repaint lasts one pass.
            let left = host
                .failsafe_at()
                .saturating_duration_since(std::time::Instant::now());
            if !host.shown() {
                ui.ctx().request_repaint_after(left);
            }
        }
        // A scene being set up holds the window's own factor.
        if !self.scenes.as_ref().is_some_and(|run| run.holds_zoom()) {
            zoom::apply(ui.ctx());
        }
    }
}

/// The context menu key (`e.key == "ContextMenu"`). egui has no such key,
/// so the software runner and the scenes send it as F35, which no Windows
/// keyboard reports (winit stops at F24 there). eframe hands the app no
/// window event, so the glow runner never sees it.
pub const CONTEXT_MENU_KEY: egui::Key = egui::Key::F35;

/// Chromium dispatches input events one at a time; egui hands a frame its
/// whole batch. A pointer press makes the focused field give up focus
/// (`SurrenderFocusOn::Presses`) in its `interact`, before it reads the
/// frame's keys, so text typed just before a click in the same frame would
/// be lost. A press that follows key or text events waits for the next
/// frame, with the input after it; other events (a screenshot, a focus
/// change) stay. Returns whether something was held.
pub fn sequence_input(held: &mut Vec<egui::Event>, events: &mut Vec<egui::Event>) -> bool {
    use egui::Event;
    let input = |e: &Event| {
        matches!(
            e,
            Event::Key { .. }
                | Event::Text(_)
                | Event::Paste(_)
                | Event::Copy
                | Event::Cut
                | Event::PointerMoved(_)
                | Event::MouseMoved(_)
                | Event::PointerButton { .. }
                | Event::PointerGone
                | Event::MouseWheel { .. }
                | Event::Zoom(_)
                | Event::Ime(_)
        )
    };
    let mut all = std::mem::take(held);
    all.append(events);
    let mut typed = false;
    let split = all.iter().position(|e| {
        if matches!(e, Event::PointerButton { pressed: true, .. }) && typed {
            return true;
        }
        typed |= matches!(
            e,
            Event::Key { .. } | Event::Text(_) | Event::Paste(_) | Event::Ime(_)
        );
        false
    });
    if let Some(i) = split {
        for e in all.split_off(i) {
            if input(&e) {
                held.push(e);
            } else {
                all.push(e);
            }
        }
    }
    *events = all;
    !held.is_empty()
}

#[cfg(feature = "glow")]
impl eframe::App for Shell {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.input_hook(ctx, raw_input, 1.0);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod tests {
    use egui::{pos2, Event, Key, Modifiers, PointerButton};

    use super::sequence_input;

    fn key(k: Key) -> Event {
        Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    fn press(pressed: bool) -> Event {
        Event::PointerButton {
            pos: pos2(1.0, 1.0),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn a_press_after_typing_waits_for_the_next_frame() {
        let mut held = Vec::new();
        let mut events = vec![
            key(Key::Backspace),
            Event::PointerMoved(pos2(1.0, 1.0)),
            press(true),
            Event::WindowFocused(true),
        ];
        assert!(sequence_input(&mut held, &mut events));
        assert_eq!(
            events,
            vec![
                key(Key::Backspace),
                Event::PointerMoved(pos2(1.0, 1.0)),
                Event::WindowFocused(true),
            ]
        );
        assert_eq!(held, vec![press(true)]);
        let mut next = vec![press(false)];
        assert!(!sequence_input(&mut held, &mut next));
        assert_eq!(next, vec![press(true), press(false)]);
        // A press with no typing before it goes through at once.
        let mut events = vec![press(true), key(Key::A)];
        assert!(!sequence_input(&mut held, &mut events));
        assert_eq!(events.len(), 2);
    }
}
