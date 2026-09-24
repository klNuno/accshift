//! Custom theme commands.

#[allow(unused_imports)]
use super::*;

#[tauri::command(async)]
pub fn list_custom_themes(
    app_handle: tauri::AppHandle,
) -> Result<Vec<crate::themes::CustomTheme>, String> {
    crate::themes::list_custom_themes(&ctx(&app_handle))
}

#[tauri::command(async)]
pub fn save_custom_theme(
    app_handle: tauri::AppHandle,
    theme: crate::themes::CustomTheme,
) -> Result<(), String> {
    crate::themes::save_custom_theme(&ctx(&app_handle), &theme)
}

#[tauri::command(async)]
pub fn delete_custom_theme(app_handle: tauri::AppHandle, theme_id: String) -> Result<(), String> {
    crate::themes::delete_custom_theme(&ctx(&app_handle), &theme_id)
}
