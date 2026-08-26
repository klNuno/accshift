//! The startup steps, one named function each.
//!
//! These all used to live inline in `main`, which made the boot order a
//! 280-line closure: window build, webview tweaks, close handler, deep links
//! and two background threads, with the interesting branches four levels deep.
//! Nothing here changes what happens or in what order; `main` now reads as the
//! list of steps and each step is skimmable on its own.

use crate::{app_runtime, config, ctx, logging, telemetry, telemetry_runtime};
use accshift_core::AppCtx;
use std::sync::mpsc::Receiver;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewWindow};

/// Shared HTTP client. A process that cannot build one cannot reach Steam, so
/// there is nothing useful left to boot into.
pub(crate) fn build_http_client() -> reqwest::Client {
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            eprintln!("Fatal: failed to create HTTP client: {e}");
            std::process::exit(1);
        }
    }
}

/// True for a URL the webview is allowed to navigate to: the app's own scheme
/// in every build, plus the local Vite server in a dev build.
fn navigation_allowed(url: &tauri::Url) -> bool {
    let scheme = url.scheme();
    let host = url.host_str();
    let is_http = matches!(scheme, "http" | "https");
    scheme == "tauri"
        || (is_http && matches!(host, Some("tauri.localhost")))
        || (cfg!(debug_assertions) && is_http && matches!(host, Some("localhost" | "127.0.0.1")))
}

/// Build the main window: last saved size, frameless and transparent, with the
/// navigation guard and the page-load log wired in.
///
/// It is built hidden. Boot completion (or the failsafe below) shows it.
pub(crate) fn build_main_window(
    app: &tauri::App,
    setup_ctx: &AppCtx,
) -> Result<WebviewWindow, Box<dyn std::error::Error>> {
    let (start_width, start_height) = config::load_window_size(setup_ctx)
        .unwrap_or((config::DEFAULT_WINDOW_WIDTH, config::DEFAULT_WINDOW_HEIGHT));

    let navigation_log_ctx = setup_ctx.clone();
    let page_load_log_ctx = setup_ctx.clone();
    #[cfg_attr(target_os = "macos", allow(unused_mut))]
    let mut window_builder =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("Accshift")
            .inner_size(start_width, start_height)
            .min_inner_size(config::MIN_WINDOW_WIDTH, config::MIN_WINDOW_HEIGHT)
            .visible(false)
            .transparent(true)
            .background_color(tauri::webview::Color(0, 0, 0, 0))
            .center()
            .resizable(true)
            .on_navigation(move |url| {
                let allowed = navigation_allowed(url);
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
        setup_ctx,
        "info",
        "backend.window",
        "Main window created",
        Some("label=main"),
    );
    Ok(win)
}

/// Turn off Edge's form autofill.
///
/// It pops "saved information" suggestions over plain text inputs (Steam launch
/// options, for one). Nothing in the app wants browser-managed form data.
#[cfg(windows)]
pub(crate) fn disable_webview_autofill(win: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
    use windows_core::Interface;
    let _ = win.with_webview(|webview| unsafe {
        let settings = webview
            .controller()
            .CoreWebView2()
            .and_then(|core| core.Settings());
        if let Ok(settings) = settings {
            if let Ok(settings) = settings.cast::<ICoreWebView2Settings4>() {
                let _ = settings.SetIsGeneralAutofillEnabled(false);
                let _ = settings.SetIsPasswordAutosaveEnabled(false);
            }
        }
    });
}

#[cfg(not(windows))]
pub(crate) fn disable_webview_autofill(_win: &WebviewWindow) {}

/// Persist the window size on its own thread, and hand back the channel that
/// says when the write landed. `None` means nothing was queued.
///
/// The save is a read-modify-write that takes the cross-process config lock,
/// which can wait up to 5s while the CLI holds it. Running it inline would
/// freeze the UI thread for that whole stretch.
///
/// Three reasons to skip it, and the first is not cosmetic: a window closed
/// before boot completed never got its saved size applied, so saving now would
/// overwrite the real one with the default.
fn spawn_window_size_save(app_handle: &AppHandle, win: &WebviewWindow) -> Option<Receiver<()>> {
    if !app_handle.state::<app_runtime::BootState>().is_completed() {
        let _ = logging::append_app_log(
            &ctx(app_handle),
            "info",
            "backend.window",
            "Skipped window size save because boot was not completed",
            None,
        );
        return None;
    }
    if matches!(win.is_maximized(), Ok(true)) {
        return None;
    }
    let size = win.inner_size().ok()?;

    let save_handle = app_handle.clone();
    let width = f64::from(size.width);
    let height = f64::from(size.height);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = config::save_window_size(&ctx(&save_handle), width, height);
        let _ = tx.send(());
    });
    Some(rx)
}

/// What runs when the user closes the window: queue the size save, hide, end
/// the telemetry session, then wait out the size save.
///
/// This handler runs on the UI thread, so the hide comes before the telemetry
/// flush: anything slow ahead of it shows up as a frozen window rather than a
/// closed app.
pub(crate) fn install_close_handler(app_handle: AppHandle, win: &WebviewWindow) {
    let win_for_events = win.clone();
    win.on_window_event(move |event| {
        if !matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            return;
        }
        let size_save_wait = spawn_window_size_save(&app_handle, &win_for_events);

        let _ = win_for_events.hide();

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

        // Give the size save the same bound save_config's own cross-process
        // lock uses, so it either lands before we exit or is abandoned
        // deliberately rather than silently.
        if let Some(rx) = size_save_wait {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        }
    });
}

/// Claim the `accshift://` scheme and handle the URLs it delivers.
///
/// The installer registers the scheme system-wide; the registration here covers
/// dev runs and portable builds (HKCU on Windows, `.desktop` on Linux). macOS
/// only supports Info.plist registration, handled by the bundle.
pub(crate) fn wire_deep_links(app: &tauri::App, setup_ctx: &AppCtx) {
    use tauri_plugin_deep_link::DeepLinkExt;

    #[cfg(any(windows, target_os = "linux"))]
    if let Err(reason) = app.deep_link().register_all() {
        let _ = logging::append_app_log(
            setup_ctx,
            "warn",
            "backend.deep-link",
            "Failed to register accshift:// scheme",
            Some(&reason.to_string()),
        );
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = setup_ctx;

    let focus_handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        let urls = event
            .urls()
            .iter()
            .map(|url| url.as_str().to_owned())
            .collect::<Vec<_>>()
            .join(" ");
        let _ = logging::append_app_log(
            &ctx(&focus_handle),
            "info",
            "backend.deep-link",
            "Deep link received",
            Some(&urls),
        );
        // Usage counter only: the URL (which carries account ids) is never
        // part of the event.
        focus_handle
            .state::<telemetry_runtime::TelemetryState>()
            .handle
            .track(telemetry::Event::DeepLinkUsed);
        // Don't force the window visible mid-boot: a deep link can be the
        // launch trigger, and boot completion shows it anyway.
        if focus_handle
            .state::<app_runtime::BootState>()
            .is_completed()
        {
            let _ = app_runtime::show_main_window(&focus_handle);
        }
    });
}

/// Re-encrypt any snapshot still stored as plaintext, once per launch.
///
/// Snapshots captured before encryption shipped only get encrypted when the
/// account is captured again, and a dormant account never is. Off the boot path
/// on purpose: on Linux and macOS every upgraded file costs a keyring round
/// trip.
pub(crate) fn spawn_snapshot_upgrade(upgrade_ctx: AppCtx) {
    std::thread::spawn(move || {
        let mut failures: Vec<String> = Vec::new();
        let stats = accshift_core::snapshot_crypto::upgrade_legacy_plaintext_snapshots(
            &upgrade_ctx,
            &mut |message, detail| failures.push(format!("{message} ({detail})")),
        );
        if !stats.touched_anything() {
            return;
        }
        let level = if stats.failed > 0 { "warn" } else { "info" };
        let _ = logging::append_app_log(
            &upgrade_ctx,
            level,
            "backend.snapshot-upgrade",
            &format!(
                "Re-encrypted {} legacy plaintext snapshot file(s), {} failed",
                stats.upgraded, stats.failed
            ),
            (!failures.is_empty())
                .then(|| failures.join("; "))
                .as_deref(),
        );
    });
}

/// Show the window anyway if the frontend never reports boot done.
pub(crate) fn spawn_boot_failsafe(fallback_handle: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(5000));

        if fallback_handle
            .state::<app_runtime::BootState>()
            .is_completed()
        {
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
}
