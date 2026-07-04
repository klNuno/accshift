use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Could not locate Steam installation: {0}")]
    RegistryOpen(String),

    #[error("Could not read Steam configuration: {0}")]
    RegistryRead(String),

    #[error("Could not write Steam configuration: {0}")]
    RegistryWrite(String),

    #[error("Could not read or write file: {0}")]
    FileRead(String),

    #[error("Could not start process: {0}")]
    ProcessStart(String),

    #[error("Invalid SteamID64")]
    InvalidSteamId,

    #[error("User data folder not found")]
    UserdataNotFound(String),

    #[error("Could not resolve path: {0}")]
    PathResolve(String),

    #[error("Could not open folder: {0}")]
    FolderOpen(String),

    #[error("Steam is still running")]
    KillSteamTimeout,

    #[error("Steam is running as administrator")]
    SteamElevated,

    #[error("Dialog cancelled")]
    Cancelled,

    #[error("{0}")]
    UnsupportedOperatingSystem(String),
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression test: every variant that wraps an OS/detail string must surface it
    // through Display (and therefore through `to_string()` and `From<AppError> for
    // String`, which is what every Tauri command and the diagnostic logger relies on).
    // Losing the detail here means callers only ever see a generic sentence with no
    // way to tell a permission error from a locked file from a missing path.
    #[test]
    fn detail_carrying_variants_include_the_wrapped_string() {
        let detail = "access is denied (os error 5)";

        let cases: Vec<AppError> = vec![
            AppError::RegistryOpen(detail.to_string()),
            AppError::RegistryRead(detail.to_string()),
            AppError::RegistryWrite(detail.to_string()),
            AppError::FileRead(detail.to_string()),
            AppError::ProcessStart(detail.to_string()),
            AppError::PathResolve(detail.to_string()),
            AppError::FolderOpen(detail.to_string()),
        ];

        for case in cases {
            let message = case.to_string();
            assert!(
                message.contains(detail),
                "expected {message:?} to contain the wrapped detail {detail:?}"
            );
        }
    }

    #[test]
    fn file_read_message_is_not_login_specific() {
        // FileRead is reused for non-login file operations (bulk edit reads/writes,
        // game-settings copy staging), so its fixed text must not claim it is about
        // Steam login data.
        let err = AppError::FileRead("permission denied".to_string());
        let message = err.to_string();
        assert!(!message.contains("login"));
        assert!(message.contains("permission denied"));
    }

    #[test]
    fn variants_without_a_detail_string_keep_a_fixed_message() {
        assert_eq!(AppError::InvalidSteamId.to_string(), "Invalid SteamID64");
        assert_eq!(
            AppError::KillSteamTimeout.to_string(),
            "Steam is still running"
        );
        assert_eq!(
            AppError::SteamElevated.to_string(),
            "Steam is running as administrator"
        );
        assert_eq!(AppError::Cancelled.to_string(), "Dialog cancelled");
    }

    #[test]
    fn app_error_converts_into_string_via_display() {
        let err = AppError::RegistryRead("value not found".to_string());
        let as_string: String = err.into();
        assert_eq!(as_string, "Could not read Steam configuration: value not found");
    }
}
