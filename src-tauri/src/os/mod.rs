use crate::error::AppError;
use std::path::{Path, PathBuf};

#[cfg(not(target_os = "windows"))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "windows"))]
use unsupported as imp;
#[cfg(target_os = "windows")]
use windows as imp;

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    imp::encrypt_secret(secret)
}

pub fn decrypt_secret(secret: &str) -> Result<String, AppError> {
    imp::decrypt_secret(secret)
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    imp::steam_installation_path()
}

pub fn steam_executable_name() -> &'static str {
    imp::steam_executable_name()
}

pub fn steam_process_name() -> &'static str {
    imp::steam_process_name()
}

pub fn steam_web_helper_process_name() -> &'static str {
    imp::steam_web_helper_process_name()
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    imp::get_auto_login_user()
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    imp::set_auto_login_user(username)
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    imp::clear_auto_login_user()
}

pub fn is_process_running(process_name: &str) -> bool {
    imp::is_process_running(process_name)
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    imp::kill_process(process_name)
}

pub fn kill_and_relaunch_steam_elevated(
    steam_path: &Path,
    launch_options: &[String],
) -> Result<(), AppError> {
    imp::kill_and_relaunch_steam_elevated(steam_path, launch_options)
}

pub fn launch_steam(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    imp::launch_steam(steam_path, run_as_admin, launch_options)
}

pub fn open_folder(path: &Path) -> Result<(), AppError> {
    imp::open_folder(path)
}

pub fn select_folder(title: &str) -> Result<String, AppError> {
    imp::select_folder(title)
}

pub fn select_file(title: &str, filter: &str) -> Result<String, AppError> {
    imp::select_file(title, filter)
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    imp::open_url(url)
}
