//! Screens: what fills the window.

pub mod main_spike;
pub mod primitives;

use crate::harness::Scene;

pub enum Screen {
    /// Nothing yet: a scene run before its first scene is set up.
    Blank,
    Main(Box<main_spike::AccshiftApp>),
    Primitives(primitives::Primitives),
}

impl Screen {
    /// The screen a scene asks for, or why the egui side cannot draw it yet.
    pub fn for_scene(ctx: &egui::Context, scene: &Scene) -> Result<Screen, String> {
        if let Some((_, html)) = &scene.html {
            return Ok(Screen::Primitives(primitives::Primitives::parse(html)?));
        }
        // The spike only knows its own demo data and Glass Dark; the mock
        // data in parity/data/ is wired by a later phase.
        let supported = scene.scenario == "demo"
            && scene.theme.as_deref().is_none_or(|t| t == "glass-dark")
            && scene.locale.as_deref().is_none_or(|l| l == "en")
            && scene.os.as_deref().is_none_or(|o| o == "windows")
            && scene.seed.is_none();
        if !supported {
            return Err(format!(
                "the spike screen only draws scenario demo, theme glass-dark, locale en, os windows, no seed (got scenario {}, theme {:?}, locale {:?}, os {:?}, seed {})",
                scene.scenario,
                scene.theme,
                scene.locale,
                scene.os,
                if scene.seed.is_some() { "set" } else { "none" },
            ));
        }
        Ok(Screen::Main(Box::new(main_spike::AccshiftApp::new(
            ctx,
            main_spike::Options {
                desktop_backdrop: true,
            },
        ))))
    }

    /// Ready to be photographed: every asset uploaded.
    pub fn ready(&self) -> bool {
        match self {
            Screen::Blank => false,
            Screen::Main(app) => app.avatars_ready(),
            Screen::Primitives(p) => p.ready(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        match self {
            Screen::Blank => {}
            Screen::Main(app) => app.ui(ui),
            Screen::Primitives(p) => p.ui(ui),
        }
    }
}
