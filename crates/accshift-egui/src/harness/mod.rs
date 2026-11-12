//! Scene runner: `accshift-egui --scene <scene.json>... --out <dir>`, in a
//! build with the `harness` feature (and in this crate's tests). A release
//! build has none of it: no switch, no runner, no mock screen; the stand-ins
//! below keep the shell's calls compiling and do nothing.
//!
//! Replays the scene files of `parity/scenes` (format in
//! `parity/README.md`). Each scene resizes the hidden window to its viewport
//! and scale, builds its screen, waits until the screen is ready (every
//! avatar uploaded), then injects its steps as egui raw input on the wall
//! clock, like the reference runner does over CDP. A `shot` reads back the
//! frame the renderer presented and writes `<out>/<id>/<shot>.png`.
//!
//! Failures (a scene the egui side cannot render yet, a shot of the wrong
//! size, a write error) are printed and counted; the process exits non-zero
//! when any happened ([`failures`]).

use std::path::PathBuf;

#[cfg(any(test, feature = "harness"))]
mod scenes;
#[cfg(any(test, feature = "harness"))]
pub use scenes::*;

#[cfg(not(any(test, feature = "harness")))]
mod off;
#[cfg(not(any(test, feature = "harness")))]
pub use off::*;

/// `--scene <scene.json>... --out <dir>` from the command line: the scene
/// files and the output folder. Empty without the `harness` feature, so a
/// release build reads no scene switch.
pub fn scene_args(args: impl IntoIterator<Item = String>) -> (Vec<PathBuf>, Option<PathBuf>) {
    let mut files = Vec::new();
    let mut out = None;
    if !cfg!(feature = "harness") {
        return (files, out);
    }
    let mut args = args.into_iter();
    let mut in_scenes = false;
    while let Some(arg) = args.next() {
        if arg == "--scene" {
            in_scenes = true;
        } else if arg == "--out" {
            out = args.next().map(PathBuf::from);
            in_scenes = false;
        } else if in_scenes {
            files.push(PathBuf::from(arg));
        }
    }
    (files, out)
}

#[cfg(test)]
mod switch_tests {
    use super::*;

    fn args() -> Vec<String> {
        ["--scene", "a.json", "b.json", "--out", "shots", "--tray"]
            .map(String::from)
            .to_vec()
    }

    #[cfg(not(feature = "harness"))]
    #[test]
    fn a_release_build_reads_no_scene_switch() {
        assert_eq!(scene_args(args()), (Vec::new(), None));
    }

    #[cfg(feature = "harness")]
    #[test]
    fn a_harness_build_reads_the_scene_switch() {
        let (files, out) = scene_args(args());
        assert_eq!(files, [PathBuf::from("a.json"), PathBuf::from("b.json")]);
        assert_eq!(out, Some(PathBuf::from("shots")));
    }
}
