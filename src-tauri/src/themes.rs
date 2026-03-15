use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub color_scheme: String,
    pub tokens: serde_json::Value,
}

fn themes_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
        .join("themes")
}

fn is_safe_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn list_custom_themes(app: &tauri::AppHandle) -> Result<Vec<CustomTheme>, String> {
    let dir = themes_dir(app);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("Could not read themes directory: {e}"))?;
    let mut themes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let theme: CustomTheme = match serde_json::from_str(&data) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if !is_safe_theme_id(&theme.id) {
            continue;
        }
        themes.push(theme);
    }
    themes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(themes)
}

pub fn save_custom_theme(app: &tauri::AppHandle, theme: &CustomTheme) -> Result<(), String> {
    if !is_safe_theme_id(&theme.id) {
        return Err(
            "Invalid theme ID: only alphanumeric characters, hyphens, and underscores are allowed"
                .to_string(),
        );
    }
    let dir = themes_dir(app);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create themes directory: {e}"))?;
    let path = dir.join(format!("{}.json", theme.id));
    let json = serde_json::to_string_pretty(&theme)
        .map_err(|e| format!("Could not serialize theme: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Could not write theme file: {e}"))?;
    Ok(())
}

pub fn delete_custom_theme(app: &tauri::AppHandle, theme_id: &str) -> Result<(), String> {
    if !is_safe_theme_id(theme_id) {
        return Err("Invalid theme ID".to_string());
    }
    let path = themes_dir(app).join(format!("{}.json", theme_id));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Could not delete theme file: {e}"))?;
    }
    Ok(())
}
