//! The accshift UI in egui. See `README.md` next to this crate.

pub mod app;
pub mod avatars;
pub mod backdrop;
pub mod bench;
pub mod data;
pub mod harness;
pub mod icons;
pub mod screens;
#[cfg(feature = "software")]
pub mod soft;
pub mod theme;
pub mod ui;

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
}

impl Shell {
    pub fn new(screen: Screen, scenes: Option<harness::SceneRun>) -> Self {
        Self {
            screen,
            scenes,
            held: Vec::new(),
        }
    }

    pub fn input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(run) = &mut self.scenes {
            run.feed(ctx, raw_input, self.screen.ready());
        }
        if sequence_input(&mut self.held, &mut raw_input.events) {
            ctx.request_repaint();
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if let Some(run) = &mut self.scenes {
            run.collect(ui.ctx());
            if let Some(scene) = run.take_build() {
                match Screen::for_scene(ui.ctx(), &scene) {
                    Ok(screen) => self.screen = screen,
                    Err(why) => {
                        self.screen = Screen::Blank;
                        run.skip(&why);
                    }
                }
            }
        }
        self.screen.ui(ui);
    }
}

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
        self.input_hook(ctx, raw_input);
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
