//! The one error type every backend call returns.

use accshift_core::error::PlatformError;
use serde::Serialize;

/// A failed backend call: only its message, which the UI shows as is
/// (`PlatformError` becomes its message, a `String` error the string).
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
    pub fn mock_refused(command: &str) -> Self {
        Self::new(format!("mock mode: {command} is not available"))
    }

    /// True for the refusal of a command while the PIN session is locked.
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

/// Serialized as the bare message.
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
