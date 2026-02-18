use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Could not locate Steam installation")]
    RegistryOpen(String),

    #[error("Could not read Steam configuration")]
    RegistryRead(String),

    #[error("Could not write Steam configuration")]
    RegistryWrite(String),

    #[error("Could not read Steam login data")]
    FileRead(String),

    #[error("Could not start Steam")]
    ProcessStart(String),

    #[error("Invalid SteamID64")]
    InvalidSteamId,

    #[error("User data folder not found")]
    UserdataNotFound(String),

    #[error("Could not resolve path")]
    PathResolve(String),

    #[error("Could not open folder")]
    FolderOpen(String),

    #[error("Steam is still running")]
    KillSteamTimeout,
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}
