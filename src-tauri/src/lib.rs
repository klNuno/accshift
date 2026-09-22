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

/// Entry point of the Windows release launcher (launcher.rs), which has
/// already started WebView2 by the time this DLL is loaded. The suffix moves
/// with the handoff layout, so an exe and a DLL from different builds refuse
/// to start rather than misread each other.
///
/// # Safety
/// `handoff` is null or comes from `wry::webview2_prestart`, on this thread.
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "C" fn accshift_run_v1(handoff: *const wry::PrestartHandoff) -> i32 {
    if let Some(handoff) = handoff.as_ref() {
        wry::webview2_prestart_adopt(handoff);
    }
    run();
    0
}

pub fn run() {
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
    let mut phases = boot::StartupPhases {
        pre_main_us: boot::pre_main_us(),
        main_entry_ts_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
        ..Default::default()
    };

    let client_start = std::time::Instant::now();
    let client = boot::build_http_client();
    phases.http_client_us = client_start.elapsed().as_micros() as u64;

    let plugins_start = std::time::Instant::now();
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

    phases.plugins_us = plugins_start.elapsed().as_micros() as u64;

    builder
        .manage(app_runtime::BootState::default())
        .manage(client)
        .setup(move |app| {
            let setup_handle = app.handle().clone();
            let setup_ctx = ctx(&setup_handle);
            let step = std::time::Instant::now();
            // Rotating the chain is a dozen filesystem calls in %APPDATA%, and
            // it used to run before the webview was even asked for. The webview
            // build that follows takes ~260 ms with nothing else on this
            // thread, so the rotation goes there instead. It takes the sink
            // lock as its first act and every other writer is a whole webview
            // build away, so nothing can slip into the file being rotated. The
            // session banner and "App setup started" are emitted from the same
            // thread to keep them in the order a reader expects.
            let log_ctx = setup_ctx.clone();
            let open_session = |ctx: &accshift_core::AppCtx| {
                let _ = logging::begin_log_session(ctx);
                let _ = logging::append_app_log(
                    ctx,
                    "info",
                    "backend.startup",
                    "App setup started",
                    None,
                );
            };
            let log_session = std::thread::Builder::new()
                .name("log-session".into())
                .spawn(move || open_session(&log_ctx));
            if let Err(error) = &log_session {
                // No thread, no session: rotate here rather than lose the file.
                eprintln!("log session thread failed to start: {error}");
                open_session(&setup_ctx);
            }
            phases.log_session_us = step.elapsed().as_micros() as u64;

            let step = std::time::Instant::now();
            logging::install_panic_hook(setup_ctx.clone());
            phases.panic_hook_us = step.elapsed().as_micros() as u64;

            let win = boot::build_main_window(app, &setup_ctx, &mut phases)?;
            // Finished long ago behind the webview build. Joining here only
            // makes it explicit that everything logged below comes after the
            // rotation, never into the file it was moving aside.
            if let Ok(handle) = log_session {
                let _ = handle.join();
            }

            let step = std::time::Instant::now();
            boot::disable_webview_autofill(&win);
            phases.autofill_us = step.elapsed().as_micros() as u64;

            // Telemetry: build the worker, share the handle with commands.
            // After the window build on purpose: the webview is the slow part
            // of startup, let it begin initializing as early as possible.
            let step = std::time::Instant::now();
            app.manage(telemetry_runtime::TelemetryState::new(
                &setup_ctx, app_start,
            ));
            phases.telemetry_us = step.elapsed().as_micros() as u64;

            let step = std::time::Instant::now();
            boot::install_window_event_handlers(app.handle().clone(), &win);
            phases.close_handler_us = step.elapsed().as_micros() as u64;

            let step = std::time::Instant::now();
            boot::wire_deep_links(app, &setup_ctx);
            phases.deep_link_us = step.elapsed().as_micros() as u64;

            let step = std::time::Instant::now();
            boot::spawn_snapshot_upgrade(setup_ctx.clone());
            boot::spawn_boot_failsafe(app.handle().clone());
            phases.threads_us = step.elapsed().as_micros() as u64;

            boot::emit_startup_profile(&setup_ctx, &phases, app_start.elapsed());

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
            commands::set_maximize_button_rect,
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
