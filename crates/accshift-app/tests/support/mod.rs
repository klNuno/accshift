//! Shared helpers of the platform model tests.
#![allow(dead_code)]

pub mod scripted;

use accshift_app::mock::MockBackend;
use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

pub fn mock(scenario: &str) -> MockBackend {
    let root = repo_root();
    MockBackend::load(&MockBackend::data_dir(&root), scenario, Some(&root)).expect("scenario")
}

pub fn scripted(scenario: &str) -> scripted::Scripted {
    scripted::Scripted::new(mock(scenario))
}
