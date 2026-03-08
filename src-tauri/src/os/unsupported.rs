use crate::error::AppError;
use std::path::{Path, PathBuf};

fn unsupported(feature: &str) -> AppError {
    AppError::UnsupportedOperatingSystem(format!(
        "{feature} is not supported on this operating system"
    ))
}

pub fn encrypt_secret(_secret: &str) -> Result<String, AppError> {
    Err(unsupported("Secret storage"))
}

pub fn decrypt_secret(_secret: &str) -> Result<String, AppError> {
    Err(unsupported("Secret storage"))
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    Err(unsupported("Steam installation discovery"))
}

pub fn steam_executable_name() -> &'static str {
    "steam"
}

pub fn steam_process_name() -> &'static str {
    "steam"
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    Err(unsupported("Steam auto-login lookup"))
}

pub fn set_auto_login_user(_username: &str) -> Result<(), AppError> {
    Err(unsupported("Steam auto-login write"))
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    Err(unsupported("Steam auto-login write"))
}

pub fn is_process_running(_process_name: &str) -> bool {
    false
}

pub fn kill_process(_process_name: &str) -> Result<(), AppError> {
    Err(unsupported("Process management"))
}

pub fn launch_steam(
    _steam_path: &Path,
    _run_as_admin: bool,
    _launch_options: &[String],
) -> Result<(), AppError> {
    Err(unsupported("Steam launch"))
}

pub fn open_folder(_path: &Path) -> Result<(), AppError> {
    Err(unsupported("Folder opening"))
}

pub fn select_folder(_title: &str) -> Result<String, AppError> {
    Err(unsupported("Folder picker"))
}

pub fn select_file(_title: &str, _filter: &str) -> Result<String, AppError> {
    Err(unsupported("File picker"))
}

pub fn open_url(_url: &str) -> Result<(), AppError> {
    Err(unsupported("URL opening"))
}
