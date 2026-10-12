//! The accshift UI in egui. See `README.md` next to this crate.

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

pub use screens::main_spike::{AccshiftApp, Options};
pub use screens::Screen;

#[cfg(all(feature = "software", feature = "glow"))]
compile_error!("pick one renderer: `software` (default) or `glow` with --no-default-features");

/// What both runners drive: the current screen plus the optional scene run.
/// The window is cleared to transparent so the OS acrylic shows through the
/// 55 % window fill, exactly like the Tauri window.
pub struct Shell {
    pub screen: Screen,
    pub scenes: Option<harness::SceneRun>,
}

impl Shell {
    pub fn input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(run) = &mut self.scenes {
            run.feed(ctx, raw_input, self.screen.ready());
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
