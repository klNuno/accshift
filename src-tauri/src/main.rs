// Keep this to hide the extra console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use tauri::Manager;

mod app_runtime;
mod boot;
mod commands;
mod commands_diagnostics;
mod commands_telemetry;
mod tauri_context;
mod telemetry_runtime;

// Re-export core modules at the crate root so `crate::foo` keeps working
// across the split (commands.rs and app_runtime.rs still use `crate::`).
pub(crate) use accshift_core::{config, logging, os, platforms, storage, telemetry, themes};
pub(crate) use tauri_context::ctx;

fn main() {
    // WebKitGTK's DMABUF renderer is broken on the NVIDIA proprietary driver
    // (white window, severe rendering lag). Opt out only on those machines;
    // an explicit user-set value always wins.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
        && std::path::Path::new("/proc/driver/nvidia").exists()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let app_start = std::time::Instant::now();

    let client = boot::build_http_client();

    let builder = tauri::Builder::default();

    // Must stay the first plugin: it short-circuits duplicate processes and,
    // via its `deep-link` feature, forwards accshift:// URLs from the second
    // instance's argv to this one as deep-link events.
    //
    // Skipped in a dev build carrying the MCP bridge: that session shares the
    // production identifier, so the guard would refuse to start whenever an
    // installed Accshift is already running. Both instances still write the
    // same loginusers.vdf and the same Steam registry keys, so never run a
    // switch in both at once.
    #[cfg(not(all(debug_assertions, feature = "mcp-bridge")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        let _ = logging::append_app_log(
            &ctx(app),
            "info",
            "backend.single-instance",
            "Second instance launch redirected to the running app",
            None,
        );
        // Don't force the window visible mid-boot: boot completion shows
        // it anyway, mirroring the deep-link handler's guard below.
        if app.state::<app_runtime::BootState>().is_completed() {
            let _ = app_runtime::show_main_window(app);
        }
    }));

    let builder = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

    // Bound to loopback: the plugin's own default is
    // 0.0.0.0, which would put an unauthenticated WebSocket on the LAN. That
    // socket answers execute_js, and JS in the webview reaches the IPC commands
    // that read Steam sessions and the keyring.
    #[cfg(all(debug_assertions, feature = "mcp-bridge"))]
    let builder = builder.plugin(
        tauri_plugin_mcp_bridge::Builder::new()
            .bind_address("127.0.0.1")
            .build(),
    );

    builder
        .manage(app_runtime::BootState::default())
        .manage(client)
        .setup(move |app| {
            let setup_handle = app.handle().clone();
            let setup_ctx = ctx(&setup_handle);
            let _ = logging::begin_log_session(&setup_ctx);
            logging::install_panic_hook(setup_ctx.clone());

            let _ = logging::append_app_log(
                &setup_ctx,
                "info",
                "backend.startup",
                "App setup started",
                None,
            );

            let win = boot::build_main_window(app, &setup_ctx)?;
            boot::disable_webview_autofill(&win);

            // Telemetry: build the worker, share the handle with commands.
            // After the window build on purpose: the webview is the slow part
            // of startup, let it begin initializing as early as possible.
            app.manage(telemetry_runtime::TelemetryState::new(
                &setup_ctx, app_start,
            ));

            boot::install_window_event_handlers(app.handle().clone(), &win);
            boot::wire_deep_links(app, &setup_ctx);
            boot::spawn_snapshot_upgrade(setup_ctx.clone());
            boot::spawn_boot_failsafe(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Core
            commands::log_app_event,
            commands::finish_boot,
            commands::get_boot_payload,
            commands::get_runtime_os,
            commands::detect_streaming_software,
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
            commands::platform_detect_installed,
            commands::platform_set_account_label,
            commands::platform_dry_run,
            // Platforms the user added themselves, from a descriptor file
            commands::reload_user_platforms,
            commands::descriptor_select_file,
            commands::descriptor_preview_file,
            commands::descriptor_install_file,
            commands::descriptor_remove,
            commands::open_descriptors_folder,
            // Utility
            commands::open_url,
            commands::open_logs_folder,
            commands_diagnostics::diagnostics,
            // Window
            commands::minimize_window,
            commands::toggle_maximize_window,
            commands::close_window,
            commands::set_keep_backdrop_active,
            commands::get_desktop_wallpaper,
            // Steam-specific
            commands::steam_set_api_key,
            commands::steam_has_api_key,
            commands::steam_open_api_key_page,
            commands::steam_switch_account_and_launch_game,
            commands::steam_get_profile_info,
            commands::steam_get_profile_infos,
            commands::steam_get_player_bans,
            commands::steam_copy_game_settings,
            commands::steam_get_copyable_games,
            commands::steam_open_userdata,
            commands::steam_clear_browser_cache,
            commands::steam_bulk_edit,
            commands::steam_get_account_games,
            commands::cs2_bridge_get_settings,
            commands::cs2_bridge_set_settings,
            commands::cs2_bridge_fetch,
            commands::cs2_bridge_check,
            commands::cs2_bridge_test,
            // Riot-specific (Windows-only)
            #[cfg(windows)]
            commands::riot_capture_profile,
            // Roblox-specific (Windows-only)
            #[cfg(windows)]
            commands::roblox_add_account_by_cookie,
            #[cfg(windows)]
            commands::roblox_get_profile_info,
            #[cfg(windows)]
            commands::roblox_check_sessions,
            // Theme
            commands::list_custom_themes,
            commands::save_custom_theme,
            commands::delete_custom_theme,
            // Telemetry
            commands_telemetry::telemetry_get_state,
            commands_telemetry::telemetry_set_mode_a,
            commands_telemetry::telemetry_set_mode_b,
            commands_telemetry::telemetry_retry_forget,
            commands_telemetry::telemetry_complete_onboarding,
            commands_telemetry::telemetry_track_persona_switch,
            commands_telemetry::telemetry_track_account_added,
            commands_telemetry::telemetry_track_account_add_started,
            commands_telemetry::telemetry_track_account_add_cancelled,
            commands_telemetry::telemetry_track_operation_failed,
            commands_telemetry::telemetry_track_update,
            commands_telemetry::telemetry_track_settings_snapshot,
            commands_telemetry::telemetry_track_streamer_mode,
            commands_telemetry::telemetry_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
