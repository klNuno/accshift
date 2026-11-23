//! A guard for commands a test holds at the mock.

use std::sync::Arc;

use accshift_app::mock::MockBackend;

/// Commands held at the mock for one test, released when it drops. A failed
/// assertion then unwinds through the release: otherwise a worker stays parked
/// at the hold and `Jobs::drop`, joining it, hangs the suite.
pub(super) struct HeldCommands {
    mock: Arc<MockBackend>,
    commands: Vec<&'static str>,
}

impl HeldCommands {
    /// Declare it after the app so it drops first.
    pub(super) fn new(mock: &Arc<MockBackend>) -> Self {
        Self {
            mock: mock.clone(),
            commands: Vec::new(),
        }
    }

    pub(super) fn hold(&mut self, command: &'static str) {
        self.commands.push(command);
        assert!(self.mock.hold(command), "{command} could not be held");
    }

    pub(super) fn release(&mut self, command: &str) {
        self.commands.retain(|held| *held != command);
        self.mock.release(command);
    }
}

impl Drop for HeldCommands {
    fn drop(&mut self) {
        for command in self.commands.drain(..) {
            self.mock.release(command);
        }
    }
}
