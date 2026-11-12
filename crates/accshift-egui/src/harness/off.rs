//! What the shell and `main` call on the scene runner, in a build without
//! it: no scene ever loads, so no run has anything to do.

use std::path::{Path, PathBuf};

use crate::screens::Screen;

/// A scene file: none can exist in this build.
pub enum Scene {}

/// The scene run `main` hands the shell: always empty here.
pub struct SceneRun;

impl SceneRun {
    pub fn new(scenes: Vec<Scene>, _out: PathBuf) -> Self {
        drop(scenes);
        SceneRun
    }

    pub fn take_build(&mut self) -> Option<Scene> {
        None
    }

    pub fn take_links(&mut self) -> Vec<String> {
        Vec::new()
    }

    pub fn holds_zoom(&self) -> bool {
        false
    }

    pub fn skip(&mut self, _why: &str) {}

    pub fn feed(&mut self, _ctx: &egui::Context, _raw: &mut egui::RawInput, _ready: bool) {}

    pub fn collect(&mut self) {}
}

pub fn load(path: &Path) -> Result<Scene, String> {
    Err(format!(
        "{}: this build has no scene runner (feature `harness`)",
        path.display()
    ))
}

pub fn failures() -> usize {
    0
}

impl Screen {
    pub fn for_scene(_ctx: &egui::Context, scene: &Scene) -> Result<Screen, String> {
        match *scene {}
    }
}
