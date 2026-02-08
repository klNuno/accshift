// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SteamAccount {
    pub steam_id: String,
    pub account_name: String,
    pub persona_name: String,
}

/// Get the Steam installation path from the registry
fn get_steam_path() -> Result<PathBuf, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey("Software\\Valve\\Steam")
        .map_err(|e| format!("Failed to open Steam registry key: {}", e))?;

    let steam_path: String = steam_key
        .get_value("SteamPath")
        .map_err(|e| format!("Failed to read SteamPath: {}", e))?;

    Ok(PathBuf::from(steam_path))
}

/// Parse the VDF (Valve Data Format) file - simple key-value parser
fn parse_vdf(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current_id: Option<String> = None;
    let mut current_account: HashMap<String, String> = HashMap::new();
    let mut depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "{" {
            depth += 1;
            continue;
        }

        if trimmed == "}" {
            depth -= 1;
            if depth == 1 && current_id.is_some() {
                accounts.insert(current_id.take().unwrap(), current_account.clone());
                current_account.clear();
            }
            continue;
        }

        let parts: Vec<&str> = trimmed.split('"').collect();

        if parts.len() >= 4 {
            let key = parts[1];
            let value = parts[3];

            if depth == 2 && current_id.is_some() {
                current_account.insert(key.to_lowercase(), value.to_string());
            }
        } else if parts.len() >= 2 {
            let key = parts[1];

            if depth == 1 && !key.is_empty() && key.chars().all(|c| c.is_ascii_digit()) {
                current_id = Some(key.to_string());
            }
        }
    }

    accounts
}

/// Fetch avatar URL from Steam Community XML profile (async)
async fn fetch_avatar_url(steam_id: &str) -> Option<String> {
    let url = format!("https://steamcommunity.com/profiles/{}/?xml=1", steam_id);

    let response = reqwest::get(&url).await.ok()?;
    let body = response.text().await.ok()?;

    // Parse the avatarFull tag from XML
    if let Some(start) = body.find("<avatarFull><![CDATA[") {
        let start = start + 21;
        if let Some(end) = body[start..].find("]]></avatarFull>") {
            return Some(body[start..start + end].to_string());
        }
    }

    None
}

/// Convert SteamID64 to account ID (lower 32 bits)
fn steam_id_to_account_id(steam_id64: &str) -> Option<u32> {
    let id: u64 = steam_id64.parse().ok()?;
    Some((id & 0xFFFFFFFF) as u32)
}

/// Get all Steam accounts from loginusers.vdf
#[tauri::command]
fn get_steam_accounts() -> Result<Vec<SteamAccount>, String> {
    let steam_path = get_steam_path()?;
    let loginusers_path = steam_path.join("config").join("loginusers.vdf");

    let content = fs::read_to_string(&loginusers_path)
        .map_err(|e| format!("Failed to read loginusers.vdf: {}", e))?;

    let parsed = parse_vdf(&content);

    let mut accounts: Vec<SteamAccount> = parsed
        .into_iter()
        .map(|(steam_id, data)| SteamAccount {
            steam_id,
            account_name: data.get("accountname").cloned().unwrap_or_default(),
            persona_name: data.get("personaname").cloned().unwrap_or_default(),
        })
        .collect();

    accounts.sort_by(|a, b| a.account_name.cmp(&b.account_name));

    Ok(accounts)
}

/// Fetch avatar for a single account (async - non-blocking)
#[tauri::command]
async fn get_avatar(steam_id: String) -> Option<String> {
    fetch_avatar_url(&steam_id).await
}

/// Get the currently logged in Steam account
#[tauri::command]
fn get_current_account() -> Result<String, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey("Software\\Valve\\Steam")
        .map_err(|e| format!("Failed to open Steam registry key: {}", e))?;

    let auto_login_user: String = steam_key
        .get_value("AutoLoginUser")
        .unwrap_or_else(|_| String::new());

    Ok(auto_login_user)
}

/// Switch to a different Steam account
#[tauri::command]
fn switch_account(username: String) -> Result<(), String> {
    let steam_path = get_steam_path()?;
    let steam_exe = steam_path.join("steam.exe");

    // Close Steam
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "steam.exe"])
        .output();

    std::thread::sleep(std::time::Duration::from_millis(1500));

    // Update registry
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| format!("Failed to open Steam registry key for writing: {}", e))?;

    steam_key
        .set_value("AutoLoginUser", &username)
        .map_err(|e| format!("Failed to set AutoLoginUser: {}", e))?;

    steam_key
        .set_value("RememberPassword", &1u32)
        .map_err(|e| format!("Failed to set RememberPassword: {}", e))?;

    // Restart Steam
    Command::new(&steam_exe)
        .spawn()
        .map_err(|e| format!("Failed to start Steam: {}", e))?;

    Ok(())
}

/// Launch Steam login dialog to add a new account
#[tauri::command]
fn add_account() -> Result<(), String> {
    let steam_path = get_steam_path()?;
    let steam_exe = steam_path.join("steam.exe");

    // Close Steam
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "steam.exe"])
        .output();

    std::thread::sleep(std::time::Duration::from_millis(1500));

    // Clear AutoLoginUser to force login prompt
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| format!("Failed to open Steam registry key for writing: {}", e))?;

    steam_key
        .set_value("AutoLoginUser", &"")
        .map_err(|e| format!("Failed to clear AutoLoginUser: {}", e))?;

    // Launch Steam (will show login dialog)
    Command::new(&steam_exe)
        .spawn()
        .map_err(|e| format!("Failed to start Steam: {}", e))?;

    Ok(())
}

/// Set PersonaState in localconfig.vdf for a given account
fn set_persona_state(steam_path: &PathBuf, account_id: u32, state: &str) {
    let config_path = steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf");

    if let Ok(content) = fs::read_to_string(&config_path) {
        let mut result = String::new();
        let mut found = false;

        for line in content.lines() {
            if !found && line.contains("\"PersonaState\"") {
                if let Some(pos) = line.rfind('"') {
                    if let Some(start) = line[..pos].rfind('"') {
                        let mut new_line = line[..start + 1].to_string();
                        new_line.push_str(state);
                        new_line.push_str(&line[pos..]);
                        result.push_str(&new_line);
                        result.push('\n');
                        found = true;
                        continue;
                    }
                }
            }
            result.push_str(line);
            result.push('\n');
        }

        if found {
            let _ = fs::write(&config_path, result);
        }
    }
}

/// Switch to account and launch Steam in a specific mode (online/invisible)
#[tauri::command]
fn switch_account_mode(username: String, steam_id: String, mode: String) -> Result<(), String> {
    let steam_path = get_steam_path()?;
    let steam_exe = steam_path.join("steam.exe");

    // Close Steam
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "steam.exe"])
        .output();

    std::thread::sleep(std::time::Duration::from_millis(1500));

    // Update registry
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| format!("Failed to open Steam registry key for writing: {}", e))?;

    steam_key
        .set_value("AutoLoginUser", &username)
        .map_err(|e| format!("Failed to set AutoLoginUser: {}", e))?;

    steam_key
        .set_value("RememberPassword", &1u32)
        .map_err(|e| format!("Failed to set RememberPassword: {}", e))?;

    // Set PersonaState in localconfig.vdf (1 = online, 7 = invisible)
    if let Some(account_id) = steam_id_to_account_id(&steam_id) {
        let state = match mode.as_str() {
            "invisible" => "7",
            _ => "1",
        };
        set_persona_state(&steam_path, account_id, state);
    }

    // Launch Steam
    Command::new(&steam_exe)
        .spawn()
        .map_err(|e| format!("Failed to start Steam: {}", e))?;

    Ok(())
}

/// Open the userdata folder for a specific account in file explorer
#[tauri::command]
fn open_userdata(steam_id: String) -> Result<(), String> {
    let steam_path = get_steam_path()?;
    let account_id = steam_id_to_account_id(&steam_id)
        .ok_or_else(|| "Invalid SteamID64".to_string())?;

    let userdata_path = steam_path.join("userdata").join(account_id.to_string());

    if !userdata_path.exists() {
        return Err(format!("Userdata folder not found: {}", userdata_path.display()));
    }

    // Canonicalize to get a proper Windows backslash path
    let canonical = userdata_path.canonicalize()
        .map_err(|e| format!("Failed to resolve path: {}", e))?;

    Command::new("explorer")
        .arg(canonical)
        .spawn()
        .map_err(|e| format!("Failed to open folder: {}", e))?;

    Ok(())
}

/// Window controls
#[tauri::command]
fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
fn close_window(window: tauri::Window) {
    let _ = window.close();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_steam_accounts,
            get_current_account,
            switch_account,
            switch_account_mode,
            get_avatar,
            add_account,
            open_userdata,
            minimize_window,
            close_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
