// Keep this to hide the extra console window in Windows release builds.
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
        .setup(|app| {
            let _win = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Accshift")
            .inner_size(900.0, 450.0)
            .min_inner_size(400.0, 300.0)
            .center()
            .decorations(false)
            .resizable(true)
            .on_navigation(|url| {
                // Only allow app URLs to prevent WebView2 back/forward navigation.
                let scheme = url.scheme();
                scheme == "tauri" || scheme == "http" || scheme == "https"
            })
            .build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_steam_accounts,
            commands::get_current_account,
            commands::switch_account,
            commands::switch_account_mode,
            commands::add_account,
            commands::open_userdata,
            commands::copy_game_settings,
            commands::get_copyable_games,
            commands::get_profile_info,
            commands::get_player_bans,
            commands::get_api_key,
            commands::set_api_key,
            commands::get_steam_path,
            commands::set_steam_path,
            commands::select_steam_path,
            commands::minimize_window,
            commands::close_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
