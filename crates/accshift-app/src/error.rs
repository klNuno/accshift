//! The one error type every backend call returns.

use accshift_core::error::PlatformError;
use serde::Serialize;

/// A failed backend call, carrying the exact message the webview receives
/// today: Tauri sends a rejected command to the frontend as a bare string
/// (`PlatformError` serializes as its message, `Result<_, String>` as the
/// string), and the UI shows that string. Keeping only the message keeps the
/// egui side showing the same text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    pub message: String,
}

impl BackendError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The refusal the mock answers for a guarded command it does not fake.
    /// Same text as `mockInvoke` in `src/lib/mock/runtime.ts`.
    pub fn mock_refused(command: &str) -> Self {
        Self::new(format!("mock mode: {command} is not available"))
    }

    /// What the real backend answers until its job fills a command in.
    pub fn not_ported(command: &str) -> Self {
        Self::new(format!("{command} is not ported to the egui backend yet"))
    }

    /// True for the PIN refusal (`isPinLockedError` in the frontend).
    pub fn is_pin_locked(&self) -> bool {
        self.message.starts_with("pin_locked:")
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

/// Serialized as the bare message, like the rejection the webview gets.
impl Serialize for BackendError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.message)
    }
}

impl From<PlatformError> for BackendError {
    fn from(error: PlatformError) -> Self {
        Self::new(error.message)
    }
}

impl From<String> for BackendError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for BackendError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

pub type BackendResult<T> = Result<T, BackendError>;
