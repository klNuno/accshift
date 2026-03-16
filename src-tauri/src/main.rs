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
mod themes;

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
            .transparent(true)
            .background_color(tauri::webview::Color(0, 0, 0, 0))
            .center()
            .decorations(false)
            .resizable(true)
            .on_navigation(move |url| {
                // Only allow app URLs in production. In dev, allow local Vite URLs only.
                let scheme = url.scheme();
                let host = url.host_str();
                let is_http = matches!(scheme, "http" | "https");
                let allowed = scheme == "tauri"
                    || (is_http && matches!(host, Some("tauri.localhost")))
                    || (cfg!(debug_assertions)
                        && is_http
                        && matches!(host, Some("localhost" | "127.0.0.1")));

                let _ = logging::append_app_log(
                    &navigation_log_handle,
                    if allowed { "info" } else { "warn" },
                    "backend.webview.navigation",
                    if allowed {
                        "Navigation allowed"
                    } else {
                        "Navigation blocked"
                    },
                    Some(url.as_ref()),
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
            // Core
            commands::log_app_event,
            commands::finish_boot,
            commands::get_runtime_os,
            // Generic platform commands
            commands::platform_get_capabilities,
            commands::platform_get_accounts,
            commands::platform_get_startup_snapshot,
            commands::platform_get_current_account,
            commands::platform_switch_account,
            commands::platform_forget_account,
            commands::platform_begin_setup,
            commands::platform_get_setup_status,
            commands::platform_cancel_setup,
            commands::platform_get_path,
            commands::platform_set_path,
            commands::platform_select_path,
            // Utility
            commands::open_url,
            // Window
            commands::minimize_window,
            commands::toggle_maximize_window,
            commands::close_window,
            // Steam-specific
            commands::steam_set_api_key,
            commands::steam_has_api_key,
            commands::steam_open_api_key_page,
            commands::steam_switch_account_mode,
            commands::steam_get_profile_info,
            commands::steam_get_player_bans,
            commands::steam_copy_game_settings,
            commands::steam_get_copyable_games,
            commands::steam_open_userdata,
            commands::steam_clear_browser_cache,
            commands::steam_bulk_edit,
            commands::steam_get_account_games,
            // Ubisoft-specific
            commands::ubisoft_set_account_label,
            // Riot-specific
            commands::riot_capture_profile,
            // Roblox-specific
            commands::roblox_add_account_by_cookie,
            commands::roblox_get_profile_info,
            // Theme
            commands::list_custom_themes,
            commands::save_custom_theme,
            commands::delete_custom_theme,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
