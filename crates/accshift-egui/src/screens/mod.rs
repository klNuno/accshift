//! Screens: what fills the window.

pub mod main_spike;
pub mod primitives;

use std::path::Path;

use crate::app::{AppModel, MockSetup};
use crate::harness::Scene;

pub enum Screen {
    /// Nothing yet: a scene run before its first scene is set up.
    Blank,
    /// The app: shell, title bar and the panel slot.
    App(Box<AppModel>),
    Primitives(primitives::Primitives),
}

impl Screen {
    /// The screen a scene asks for: its `html` fragment, or the app on the
    /// mock with the scene's scenario, OS, theme, locale and seed.
    pub fn for_scene(ctx: &egui::Context, scene: &Scene) -> Result<Screen, String> {
        if let Some((_, html)) = &scene.html {
            return Ok(Screen::Primitives(primitives::Primitives::parse(html)?));
        }
        let data_dir = scene.data_dir();
        let data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir);
        // parity/data sits in crates/accshift-egui: the repo is four up.
        let repo_root = data_dir.ancestors().nth(4).map(Path::to_path_buf);
        let setup = MockSetup {
            scenario: scene.scenario.clone(),
            os: scene.os.clone(),
            theme: scene.theme.clone(),
            locale: scene.locale.clone(),
            seed: scene.seed.clone(),
            data_dir,
            repo_root,
        };
        Ok(Screen::App(Box::new(AppModel::mock(ctx, setup)?)))
    }

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
