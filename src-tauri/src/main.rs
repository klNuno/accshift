// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod error;
mod steam;

fn main() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("failed to create HTTP client");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(client)
        .invoke_handler(tauri::generate_handler![
            commands::get_steam_accounts,
            commands::get_current_account,
            commands::switch_account,
            commands::switch_account_mode,
            commands::add_account,
            commands::open_userdata,
            commands::get_profile_info,
            commands::get_player_bans,
            commands::get_api_key,
            commands::set_api_key,
            commands::minimize_window,
            commands::close_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
