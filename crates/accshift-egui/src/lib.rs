//! Spike: the accshift main screen in egui. See `README.md` next to this crate.

pub mod app;
pub mod avatars;
pub mod backdrop;
pub mod bench;
pub mod capture;
pub mod data;
pub mod icons;
#[cfg(feature = "software")]
pub mod soft;
pub mod theme;

pub use app::{AccshiftApp, Options};

#[cfg(all(feature = "software", feature = "glow"))]
compile_error!("pick one renderer: `software` (default) or `glow` with --no-default-features");

/// What both runners drive: the app plus the optional capture script. The
/// window is cleared to transparent so the OS acrylic shows through the 55 %
/// window fill, exactly like the Tauri window.
pub struct Shell {
    pub app: AccshiftApp,
    pub capture: Option<capture::Capture>,
}

impl Shell {
    pub fn input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(capture) = &mut self.capture {
            capture.feed(ctx, raw_input, self.app.avatars_ready());
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        if let Some(capture) = &mut self.capture {
            capture.collect(ui.ctx());
        }
        self.app.ui(ui);
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
