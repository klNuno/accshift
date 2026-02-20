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
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(client)
        .setup(|app| {
            let (start_width, start_height) =
                config::load_window_size(app.handle()).unwrap_or((900.0, 450.0));

            let mut window_builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Accshift")
            .inner_size(start_width, start_height)
            .min_inner_size(400.0, 300.0)
            .center()
            .decorations(false)
            .resizable(true)
            .on_navigation(|url| {
                // Only allow app URLs to prevent WebView2 back/forward navigation.
                let scheme = url.scheme();
                scheme == "tauri" || scheme == "http" || scheme == "https"
            });

            if let Some(icon) = app.default_window_icon() {
                window_builder = window_builder.icon(icon.clone())?;
            }

            let win = window_builder.build()?;
            let app_handle = app.handle().clone();
            let win_for_events = win.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    if matches!(win_for_events.is_maximized(), Ok(true)) {
                        return;
                    }
                    if let Ok(size) = win_for_events.inner_size() {
                        let _ = config::save_window_size(
                            &app_handle,
                            f64::from(size.width),
                            f64::from(size.height),
                        );
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_steam_accounts,
            commands::get_startup_snapshot,
            commands::get_current_account,
            commands::switch_account,
            commands::switch_account_mode,
            commands::add_account,
            commands::forget_account,
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
            commands::toggle_maximize_window,
            commands::close_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
