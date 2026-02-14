use serde::{Deserialize, Serialize};
use std::fs;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub steam_api_key: String,
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let path = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
        .join("config.json");

    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(app_handle: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let dir = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");

    fs::create_dir_all(&dir).map_err(|e| format!("Could not create config directory: {}", e))?;

    let path = dir.join("config.json");
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Could not serialize config: {}", e))?;

    fs::write(&path, json).map_err(|e| format!("Could not write config file: {}", e))?;

    Ok(())
}
