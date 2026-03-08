// Keep this to hide the extra console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::webview::PageLoadEvent;
use tauri::Manager;

mod app_runtime;
mod commands;
mod config;
mod error;
mod fs_utils;
mod logging;
mod os;
mod platforms;
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
        .manage(app_runtime::BootState::default())
        .manage(client)
        .setup(|app| {
            let setup_handle = app.handle().clone();
            let _ = logging::begin_log_session(&setup_handle);
            logging::install_panic_hook(setup_handle.clone());
            let _ = logging::append_app_log(
                &setup_handle,
                "info",
                "backend.startup",
                "App setup started",
                None,
            );

            let (start_width, start_height) = config::load_window_size(app.handle())
                .unwrap_or((config::DEFAULT_WINDOW_WIDTH, config::DEFAULT_WINDOW_HEIGHT));

            let navigation_log_handle = setup_handle.clone();
            let page_load_log_handle = setup_handle.clone();
            let mut window_builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Accshift")
            .inner_size(start_width, start_height)
            .min_inner_size(config::MIN_WINDOW_WIDTH, config::MIN_WINDOW_HEIGHT)
            .visible(false)
            .background_color(tauri::webview::Color(9, 9, 11, 255))
            .center()
            .decorations(false)
            .resizable(true)
            .on_navigation(move |url| {
                // Only allow app URLs in production. In dev, allow local Vite URLs only.
                let scheme = url.scheme();
                let host = url.host_str();
                let allowed = if scheme == "tauri" {
                    true
                } else if matches!(scheme, "http" | "https")
                    && matches!(host, Some("tauri.localhost"))
                {
                    true
                } else if cfg!(debug_assertions) && matches!(scheme, "http" | "https") {
                    matches!(host, Some("localhost" | "127.0.0.1"))
                } else {
                    false
                };

                let _ = logging::append_app_log(
                    &navigation_log_handle,
                    if allowed { "info" } else { "warn" },
                    "backend.webview.navigation",
                    if allowed {
                        "Navigation allowed"
                    } else {
                        "Navigation blocked"
                    },
                    Some(&url.to_string()),
                );

                allowed
            })
            .on_page_load(move |_window, payload| {
                let event = match payload.event() {
                    PageLoadEvent::Started => "Page load started",
                    PageLoadEvent::Finished => "Page load finished",
                };
                let url = payload.url().to_string();
                let _ = logging::append_app_log(
                    &page_load_log_handle,
                    "info",
                    "backend.webview.page_load",
                    event,
                    Some(&url),
                );
            });

            if let Some(icon) = app.default_window_icon() {
                window_builder = window_builder.icon(icon.clone())?;
            }

            let win = window_builder.build()?;
            let _ = logging::append_app_log(
                &setup_handle,
                "info",
                "backend.window",
                "Main window created",
                Some("label=main"),
            );
            let app_handle = app.handle().clone();
            let win_for_events = win.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    let boot_state = app_handle.state::<app_runtime::BootState>();
                    if !boot_state.is_completed() {
                        let _ = logging::append_app_log(
                            &app_handle,
                            "info",
                            "backend.window",
                            "Skipped window size save because boot was not completed",
                            None,
                        );
                        return;
                    }
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

            let fallback_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(5000));

                let boot_state = fallback_handle.state::<app_runtime::BootState>();
                if boot_state.is_completed() {
                    return;
                }

                let _ = logging::append_app_log(
                    &fallback_handle,
                    "warn",
                    "backend.boot-failsafe",
                    "Rust failsafe triggered after 5000ms; forcing main window visibility",
                    None,
                );
                let _ = app_runtime::show_main_window(&fallback_handle);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::log_app_event,
            commands::get_log_file_path,
            commands::finish_boot,
            commands::get_runtime_os,
            commands::get_steam_accounts,
            commands::get_startup_snapshot,
            commands::get_current_account,
            commands::switch_account,
            commands::switch_account_mode,
            commands::add_account,
            commands::begin_steam_account_setup,
            commands::get_steam_account_setup_status,
            commands::cancel_steam_account_setup,
            commands::forget_account,
            commands::open_userdata,
            commands::clear_steam_integrated_browser_cache,
            commands::copy_game_settings,
            commands::get_copyable_games,
            commands::get_profile_info,
            commands::get_player_bans,
            commands::has_api_key,
            commands::set_api_key,
            commands::get_steam_path,
            commands::set_steam_path,
            commands::select_steam_path,
            commands::open_steam_api_key_page,
            commands::minimize_window,
            commands::toggle_maximize_window,
            commands::close_window,
            commands::get_riot_profiles,
            commands::get_riot_startup_snapshot,
            commands::get_current_riot_profile,
            commands::begin_riot_profile_setup,
            commands::get_riot_profile_setup_status,
            commands::cancel_riot_profile_setup,
            commands::capture_riot_profile,
            commands::switch_riot_profile,
            commands::forget_riot_profile,
            commands::get_riot_path,
            commands::set_riot_path,
            commands::select_riot_path,
            commands::get_battle_net_accounts,
            commands::get_battle_net_startup_snapshot,
            commands::get_current_battle_net_account,
            commands::switch_battle_net_account,
            commands::begin_battle_net_account_setup,
            commands::get_battle_net_account_setup_status,
            commands::cancel_battle_net_account_setup,
            commands::forget_battle_net_account,
            commands::get_battle_net_path,
            commands::set_battle_net_path,
            commands::select_battle_net_path,
            commands::copy_battle_net_game_settings,
            commands::get_battle_net_copyable_games,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
