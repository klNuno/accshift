use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Failed to open Steam registry key: {0}")]
    RegistryOpen(String),

    #[error("Failed to read SteamPath: {0}")]
    RegistryRead(String),

    #[error("Failed to write registry: {0}")]
    RegistryWrite(String),

    #[error("Failed to read file: {0}")]
    FileRead(String),

    #[error("Failed to start Steam: {0}")]
    ProcessStart(String),

    #[error("Invalid SteamID64")]
    InvalidSteamId,

    #[error("Userdata folder not found: {0}")]
    UserdataNotFound(String),

    #[error("Failed to resolve path: {0}")]
    PathResolve(String),

    #[error("Failed to open folder: {0}")]
    FolderOpen(String),
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}
