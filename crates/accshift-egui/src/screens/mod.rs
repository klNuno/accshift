//! Screens: what fills the window.

pub mod persona_wizard;
pub mod personas;
pub mod primitives;
pub mod settings;
pub mod workspace;

use crate::app::AppModel;

pub enum Screen {
    /// Nothing yet: a scene run before its first scene is set up.
    Blank,
    /// The app: shell, title bar and the panel slot.
    App(Box<AppModel>),
    Primitives(primitives::Primitives),
}

impl Screen {
    /// Ready to be photographed: booted, entrance over, every asset uploaded.
    pub fn ready(&self) -> bool {
        match self {
            Screen::Blank => false,
            Screen::App(app) => app.ready(),
            Screen::Primitives(p) => p.ready(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        match self {
            Screen::Blank => {}
            Screen::App(app) => app.frame(ui),
            Screen::Primitives(p) => p.ui(ui),
        }
    }
}
