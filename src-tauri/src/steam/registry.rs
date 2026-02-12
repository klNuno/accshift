use std::path::PathBuf;
use winreg::enums::*;
use winreg::RegKey;

use crate::error::AppError;

pub fn get_steam_path() -> Result<PathBuf, AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey("Software\\Valve\\Steam")
        .map_err(|e| AppError::RegistryOpen(e.to_string()))?;

    let steam_path: String = steam_key
        .get_value("SteamPath")
        .map_err(|e| AppError::RegistryRead(e.to_string()))?;

    Ok(PathBuf::from(steam_path))
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    steam_key
        .set_value("AutoLoginUser", &username)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    steam_key
        .set_value("RememberPassword", &1u32)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    Ok(())
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    steam_key
        .set_value("AutoLoginUser", &"")
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    Ok(())
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey("Software\\Valve\\Steam")
        .map_err(|e| AppError::RegistryOpen(e.to_string()))?;

    let auto_login_user: String = steam_key
        .get_value("AutoLoginUser")
        .unwrap_or_else(|_| String::new());

    Ok(auto_login_user)
}
