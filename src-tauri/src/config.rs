use serde::{Deserialize, Serialize};
use std::fs;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub steam_api_key: String,
    #[serde(default)]
    pub steam_path_override: String,
    #[serde(default)]
    pub window_width: Option<f64>,
    #[serde(default)]
    pub window_height: Option<f64>,
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

pub fn load_window_size(app_handle: &tauri::AppHandle) -> Option<(f64, f64)> {
    let cfg = load_config(app_handle);
    let width = cfg.window_width?;
    let height = cfg.window_height?;
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        Some((width, height))
    } else {
        None
    }
}

pub fn save_window_size(app_handle: &tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Ok(());
    }

    let mut cfg = load_config(app_handle);
    cfg.window_width = Some(width);
    cfg.window_height = Some(height);
    save_config(app_handle, &cfg)
}
