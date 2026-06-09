// Keep this to hide the extra console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use tauri::webview::PageLoadEvent;
use tauri::Manager;

mod app_runtime;
mod commands;
mod commands_telemetry;
mod tauri_context;
mod telemetry_runtime;

// Re-export core modules at the crate root so `crate::foo` keeps working
// across the split (commands.rs and app_runtime.rs still use `crate::`).
pub(crate) use accshift_core::{config, logging, os, platforms, storage, telemetry, themes};
pub(crate) use tauri_context::ctx;

fn main() {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Fatal: failed to create HTTP client: {e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_runtime::BootState::default())
        .manage(client)
        .setup(|app| {
            let setup_handle = app.handle().clone();
            let setup_ctx = ctx(&setup_handle);
            let _ = logging::begin_log_session(&setup_ctx);
            logging::install_panic_hook(setup_ctx.clone());

            // Telemetry: build the worker, share the handle with commands.
            app.manage(telemetry_runtime::TelemetryState::new(&setup_ctx));
            let _ = logging::append_app_log(
                &setup_ctx,
                "info",
                "backend.startup",
                "App setup started",
                None,
            );

            let (start_width, start_height) = config::load_window_size(&setup_ctx)
                .unwrap_or((config::DEFAULT_WINDOW_WIDTH, config::DEFAULT_WINDOW_HEIGHT));

            let navigation_log_ctx = setup_ctx.clone();
            let page_load_log_ctx = setup_ctx.clone();
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
                    &navigation_log_ctx,
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
                    &page_load_log_ctx,
                    "info",
                    "backend.webview.page_load",
                    event,
                    Some(&url),
                );
            });

            #[cfg(target_os = "macos")]
            {
                // Native traffic lights float over our custom titlebar. WKWebView
                // injects env(safe-area-inset-top) so the CSS header aligns with
                // the system-reserved zone for free.
                window_builder = window_builder
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true);
            }
            #[cfg(not(target_os = "macos"))]
            {
                window_builder = window_builder.decorations(false);
            }

            if let Some(icon) = app.default_window_icon() {
                window_builder = window_builder.icon(icon.clone())?;
            }

            let win = window_builder.build()?;
            let _ = logging::append_app_log(
                &setup_ctx,
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
                    // Session ended + flush telemetry before the process exits.
                    let tstate = app_handle.state::<telemetry_runtime::TelemetryState>();
                    let duration_ms = tstate
                        .app_start
                        .elapsed()
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64;
                    tstate
                        .handle
                        .track(telemetry::Event::SessionEnded { duration_ms });
                    tstate.shutdown();

                    if !boot_state.is_completed() {
                        let _ = logging::append_app_log(
                            &ctx(&app_handle),
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
                            &ctx(&app_handle),
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
                    &ctx(&fallback_handle),
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
            commands::migrate_legacy_config,
            commands::load_client_storage_snapshot,
            commands::save_client_storage_store,
            commands::get_storage_manifest,
            // Generic platform commands
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
            commands::platform_set_account_label,
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
            commands::steam_switch_account_and_launch_game,
            commands::steam_get_profile_info,
            commands::steam_get_player_bans,
            commands::steam_copy_game_settings,
            commands::steam_get_copyable_games,
            commands::steam_open_userdata,
            commands::steam_clear_browser_cache,
            commands::steam_bulk_edit,
            commands::steam_get_account_games,
            // Riot-specific (Windows-only)
            #[cfg(windows)]
            commands::riot_capture_profile,
            // Roblox-specific (Windows-only)
            #[cfg(windows)]
            commands::roblox_add_account_by_cookie,
            #[cfg(windows)]
            commands::roblox_get_profile_info,
            // Theme
            commands::list_custom_themes,
            commands::save_custom_theme,
            commands::delete_custom_theme,
            // Telemetry
            commands_telemetry::telemetry_get_state,
            commands_telemetry::telemetry_set_mode_a,
            commands_telemetry::telemetry_set_mode_b,
            commands_telemetry::telemetry_complete_onboarding,
            commands_telemetry::telemetry_export,
            commands_telemetry::telemetry_upload_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
