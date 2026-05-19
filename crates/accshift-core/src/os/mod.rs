use crate::error::AppError;
use std::path::{Path, PathBuf};

mod common;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod secrets;
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(target_os = "macos")]
use macos as imp;
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
use unsupported as imp;
#[cfg(target_os = "windows")]
use windows as imp;

// ---------------------------------------------------------------------------
// Cross-platform primitives (sysinfo + open crates)
// ---------------------------------------------------------------------------

pub fn is_process_running(process_name: &str) -> bool {
    common::is_process_running(process_name)
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    common::kill_process(process_name)
}

pub fn wait_for_process_exit(process_name: &str, timeout_ms: u32) -> bool {
    common::wait_for_process_exit(process_name, timeout_ms)
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    common::open_url(url)
}

pub fn open_folder(path: &Path) -> Result<(), AppError> {
    common::open_folder(path)
}

// ---------------------------------------------------------------------------
// Platform-specific primitives (Windows-only for now)
// ---------------------------------------------------------------------------

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    imp::encrypt_secret(secret)
}

pub fn decrypt_secret(secret: &str) -> Result<String, AppError> {
    imp::decrypt_secret(secret)
}

pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    imp::encrypt_bytes(data)
}

pub fn decrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    imp::decrypt_bytes(data)
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

pub fn steam_htmlcache_path() -> Result<PathBuf, AppError> {
    imp::steam_htmlcache_path()
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

pub fn select_folder(title: &str) -> Result<String, AppError> {
    imp::select_folder(title)
}

pub fn select_file(title: &str, filter: &str) -> Result<String, AppError> {
    imp::select_file(title, filter)
}
