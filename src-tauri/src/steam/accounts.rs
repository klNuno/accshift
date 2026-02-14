use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;

use crate::error::AppError;
use super::registry::{get_steam_path, set_auto_login_user, clear_auto_login_user};
use super::vdf::{parse_vdf, set_persona_state};

const MAX_KILL_WAIT_MS: u64 = 5000;
const KILL_POLL_INTERVAL_MS: u64 = 500;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SteamAccount {
    pub steam_id: String,
    pub account_name: String,
    pub persona_name: String,
}

fn steam_id_to_account_id(steam_id64: &str) -> Option<u32> {
    let id: u64 = steam_id64.parse().ok()?;
    Some((id & 0xFFFFFFFF) as u32)
}

fn is_steam_running() -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.to_lowercase().contains("steam.exe")
        }
        Err(_) => false,
    }
}

fn kill_steam() -> Result<(), AppError> {
    if !is_steam_running() {
        return Ok(());
    }

    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "steam.exe"])
        .output();

    let max_polls = MAX_KILL_WAIT_MS / KILL_POLL_INTERVAL_MS;
    for _ in 0..max_polls {
        std::thread::sleep(std::time::Duration::from_millis(KILL_POLL_INTERVAL_MS));
        if !is_steam_running() {
            return Ok(());
        }
    }

    Err(AppError::KillSteamTimeout)
}

fn launch_steam(steam_path: &std::path::PathBuf) -> Result<(), AppError> {
    let steam_exe = steam_path.join("steam.exe");
    Command::new(&steam_exe)
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

pub fn get_accounts() -> Result<Vec<SteamAccount>, AppError> {
    let steam_path = get_steam_path()?;
    let loginusers_path = steam_path.join("config").join("loginusers.vdf");

    let content = fs::read_to_string(&loginusers_path)
        .map_err(|e| AppError::FileRead(e.to_string()))?;

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

pub fn switch_account(username: &str) -> Result<(), AppError> {
    let steam_path = get_steam_path()?;
    kill_steam()?;
    set_auto_login_user(username)?;
    launch_steam(&steam_path)
}

pub fn add_account() -> Result<(), AppError> {
    let steam_path = get_steam_path()?;
    kill_steam()?;
    clear_auto_login_user()?;
    launch_steam(&steam_path)
}

pub fn switch_account_mode(username: &str, steam_id: &str, mode: &str) -> Result<(), AppError> {
    let steam_path = get_steam_path()?;
    kill_steam()?;
    set_auto_login_user(username)?;

    if let Some(account_id) = steam_id_to_account_id(steam_id) {
        let state = match mode {
            "invisible" => "7",
            _ => "1",
        };
        set_persona_state(&steam_path, account_id, state);
    }

    launch_steam(&steam_path)
}

pub fn open_userdata(steam_id: &str) -> Result<(), AppError> {
    let steam_path = get_steam_path()?;
    let account_id = steam_id_to_account_id(steam_id)
        .ok_or(AppError::InvalidSteamId)?;

    let userdata_path = steam_path.join("userdata").join(account_id.to_string());

    if !userdata_path.exists() {
        return Err(AppError::UserdataNotFound(userdata_path.display().to_string()));
    }

    let canonical = userdata_path.canonicalize()
        .map_err(|e| AppError::PathResolve(e.to_string()))?;

    Command::new("explorer")
        .arg(canonical)
        .spawn()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;

    Ok(())
}
