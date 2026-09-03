//! The startup steps, one named function each.
//!
//! These all used to live inline in `main`, which made the boot order a
//! 280-line closure: window build, webview tweaks, close handler, deep links
//! and two background threads, with the interesting branches four levels deep.
//! Nothing here changes what happens or in what order; `main` now reads as the
//! list of steps and each step is skimmable on its own.

use crate::{app_runtime, config, ctx, logging, telemetry, telemetry_runtime};
use accshift_core::AppCtx;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, Monitor, WebviewWindow};

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

/// Build the main window: last saved size and placement, frameless and
/// transparent, with the navigation guard and the page-load log wired in.
///
/// Both saved values are logical pixels, which is the unit the builder takes.
///
/// It is built hidden. Boot completion (or the failsafe below) shows it.
pub(crate) fn build_main_window(
    app: &tauri::App,
    setup_ctx: &AppCtx,
) -> Result<WebviewWindow, Box<dyn std::error::Error>> {
    let (start_width, start_height) = config::load_window_size(setup_ctx)
        .unwrap_or((config::DEFAULT_WINDOW_WIDTH, config::DEFAULT_WINDOW_HEIGHT));
    let saved_position = config::load_window_position(setup_ctx);

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

    // First launch, or a config with no placement in it, still opens centered.
    window_builder = match saved_position {
        Some((x, y)) => window_builder.position(x, y),
        None => window_builder.center(),
    };

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

    // The monitor the window was saved on may be unplugged, or the desktop
    // rearranged. The window is still hidden here, so recentering it costs no
    // visible jump.
    if saved_position.is_some() && !window_sits_on_a_monitor(&win) {
        let _ = win.center();
        let _ = logging::append_app_log(
            setup_ctx,
            "info",
            "backend.window",
            "Saved window position is off every monitor; centered instead",
            None,
        );
    }

    let _ = logging::append_app_log(
        setup_ctx,
        "info",
        "backend.window",
        "Main window created",
        Some("label=main"),
    );
    Ok(win)
}

/// True when the window overlaps the work area of at least one attached
/// monitor. Everything here is physical pixels, which is what both the window
/// and the monitor report, so no scale factor is involved.
///
/// A window whose monitor list cannot be read is left where it is: with no
/// monitors to compare against there is no evidence it sits anywhere wrong.
fn window_sits_on_a_monitor(win: &WebviewWindow) -> bool {
    let (Ok(position), Ok(size), Ok(monitors)) = (
        win.outer_position(),
        win.outer_size(),
        win.available_monitors(),
    ) else {
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    let window = Rect::at(
        f64::from(position.x),
        f64::from(position.y),
        f64::from(size.width),
        f64::from(size.height),
    );
    monitors
        .iter()
        .any(|monitor| window.overlaps(&work_area_rect(monitor)))
}

fn work_area_rect(monitor: &Monitor) -> Rect {
    let area = monitor.work_area();
    Rect::at(
        f64::from(area.position.x),
        f64::from(area.position.y),
        f64::from(area.size.width),
        f64::from(area.size.height),
    )
}

/// Screen rectangle in physical pixels, origin top left.
#[derive(Clone, Copy, Debug)]
struct Rect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Rect {
    fn at(left: f64, top: f64, width: f64, height: f64) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
        }
    }

    /// True when the two rectangles share any area. Touching edges do not
    /// count: a window whose right edge is exactly a monitor's left edge shows
    /// nothing on it.
    fn overlaps(&self, other: &Self) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }
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

/// Size and placement of the main window, in the logical pixels the config
/// stores and the window builder consumes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowGeometry {
    width: f64,
    height: f64,
    x: f64,
    y: f64,
}

/// Last geometry a move event reported, waiting to be written.
static PENDING_GEOMETRY: Mutex<Option<WindowGeometry>> = Mutex::new(None);
/// Whether a thread is already draining `PENDING_GEOMETRY`.
static GEOMETRY_SAVER_RUNNING: AtomicBool = AtomicBool::new(false);
/// Quiet time after the last move event before the write goes out. A window
/// drag emits dozens of events per second and each save takes the
/// cross-process config lock, so only the last one is worth writing.
const GEOMETRY_SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(700);

/// Read the window's live geometry, converted to logical pixels once.
///
/// This is the bug fix for the launch-over-launch growth: `inner_size` and
/// `outer_position` are physical, the builder is logical, so on a 125% display
/// storing the raw numbers multiplied the window by 1.25 every launch.
///
/// `None` when nothing worth saving is on screen: a maximized window would
/// store the screen size as the restored size, a minimized one reports a
/// parking position far off every monitor (-32000 on Windows), and a window
/// whose size or position cannot be read has nothing to offer.
fn current_geometry(win: &WebviewWindow) -> Option<WindowGeometry> {
    if matches!(win.is_maximized(), Ok(true)) || matches!(win.is_minimized(), Ok(true)) {
        return None;
    }
    let scale = win.scale_factor().ok()?;
    let size = win.inner_size().ok()?;
    let position = win.outer_position().ok()?;
    let size = size.to_logical::<f64>(scale);
    let position = position.to_logical::<f64>(scale);
    Some(WindowGeometry {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
    })
}

/// Whether saving is allowed at all. A window closed or moved before boot
/// completed never got its saved geometry applied, so saving now would
/// overwrite the real one with the default.
fn geometry_save_allowed(app_handle: &AppHandle) -> bool {
    app_handle.state::<app_runtime::BootState>().is_completed()
}

/// Persist the window geometry on its own thread, and hand back the channel
/// that says when the write landed. `None` means nothing was queued.
///
/// The save is a read-modify-write that takes the cross-process config lock,
/// which can wait up to 5s while the CLI holds it. Running it inline would
/// freeze the UI thread for that whole stretch.
fn spawn_window_geometry_save(app_handle: &AppHandle, win: &WebviewWindow) -> Option<Receiver<()>> {
    if !geometry_save_allowed(app_handle) {
        let _ = logging::append_app_log(
            &ctx(app_handle),
            "info",
            "backend.window",
            "Skipped window geometry save because boot was not completed",
            None,
        );
        return None;
    }
    let geometry = current_geometry(win)?;
    // This write is newer than anything the debounce still holds, and it must
    // not be undone by a thread waking up after it.
    take_pending_geometry();

    let save_handle = app_handle.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        save_geometry(&save_handle, geometry);
        let _ = tx.send(());
    });
    Some(rx)
}

fn save_geometry(app_handle: &AppHandle, geometry: WindowGeometry) {
    let _ = config::save_window_geometry(
        &ctx(app_handle),
        geometry.width,
        geometry.height,
        Some((geometry.x, geometry.y)),
    );
}

fn take_pending_geometry() -> Option<WindowGeometry> {
    PENDING_GEOMETRY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// Queue a debounced geometry save. Called from the move handler, so it does
/// no IO of its own: it stamps the value and lets a background thread write the
/// last one once the drag stops.
fn queue_window_geometry_save(app_handle: &AppHandle, win: &WebviewWindow) {
    if !geometry_save_allowed(app_handle) {
        return;
    }
    let Some(geometry) = current_geometry(win) else {
        return;
    };
    *PENDING_GEOMETRY.lock().unwrap_or_else(|e| e.into_inner()) = Some(geometry);
    if GEOMETRY_SAVER_RUNNING.swap(true, Ordering::SeqCst) {
        // A thread is already waiting; it will pick this value up.
        return;
    }

    let save_handle = app_handle.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(GEOMETRY_SAVE_DEBOUNCE);
            match take_pending_geometry() {
                Some(geometry) => save_geometry(&save_handle, geometry),
                None => {
                    GEOMETRY_SAVER_RUNNING.store(false, Ordering::SeqCst);
                    // A move that landed between the take above and this
                    // release would otherwise sit unwritten until the next one.
                    let missed = PENDING_GEOMETRY
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .is_some();
                    if missed && !GEOMETRY_SAVER_RUNNING.swap(true, Ordering::SeqCst) {
                        continue;
                    }
                    break;
                }
            }
        }
    });
}

/// Window events that outlive a single frame: the close sequence, and the
/// debounced geometry save behind every move.
///
/// On close: queue the geometry save, hide, end the telemetry session, then
/// wait out the save. This handler runs on the UI thread, so the hide comes
/// before the telemetry flush: anything slow ahead of it shows up as a frozen
/// window rather than a closed app.
pub(crate) fn install_window_event_handlers(app_handle: AppHandle, win: &WebviewWindow) {
    let win_for_events = win.clone();
    win.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Moved(_)) {
            queue_window_geometry_save(&app_handle, &win_for_events);
            return;
        }
        if !matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
            return;
        }
        let geometry_save_wait = spawn_window_geometry_save(&app_handle, &win_for_events);

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

        // Give the geometry save the same bound save_config's own cross-process
        // lock uses, so it either lands before we exit or is abandoned
        // deliberately rather than silently.
        if let Some(rx) = geometry_save_wait {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::{PhysicalPosition, PhysicalSize};

    // The unit bug in one assertion: a 1000x520 logical window on a 125%
    // display reports 1250x650 physical. Storing that raw is what made the
    // window grow by 25% at every launch, because the builder reads the stored
    // number as logical.
    #[test]
    fn a_physical_window_size_converts_back_to_the_logical_one() {
        let scale = 1.25;
        let physical = PhysicalSize::new(1250_u32, 650_u32);
        let logical = physical.to_logical::<f64>(scale);

        assert_eq!((logical.width, logical.height), (1000.0, 520.0));
        assert_eq!(
            (
                accshift_core::config::logical_from_physical(1250.0, scale),
                accshift_core::config::logical_from_physical(650.0, scale),
            ),
            (logical.width, logical.height),
            "the config helper and the tauri conversion must agree"
        );
    }

    #[test]
    fn a_physical_window_position_converts_back_to_the_logical_one() {
        let physical = PhysicalPosition::new(-2400_i32, 150_i32);
        let logical = physical.to_logical::<f64>(1.5);
        assert_eq!((logical.x, logical.y), (-1600.0, 100.0));
    }

    #[test]
    fn a_window_overlapping_a_monitor_is_kept() {
        let monitor = Rect::at(0.0, 0.0, 1920.0, 1040.0);
        // Fully inside.
        assert!(Rect::at(100.0, 100.0, 1000.0, 520.0).overlaps(&monitor));
        // Half off the right edge, still reachable.
        assert!(Rect::at(1900.0, 100.0, 1000.0, 520.0).overlaps(&monitor));
        // A second monitor to the left of the primary one.
        assert!(Rect::at(-1800.0, 40.0, 1000.0, 520.0)
            .overlaps(&Rect::at(-1920.0, 0.0, 1920.0, 1040.0)));
    }

    #[test]
    fn a_window_off_every_monitor_is_rejected() {
        let monitor = Rect::at(0.0, 0.0, 1920.0, 1040.0);
        // The unplugged second monitor case.
        assert!(!Rect::at(-1800.0, 40.0, 1000.0, 520.0).overlaps(&monitor));
        // Below the taskbar, off the work area.
        assert!(!Rect::at(100.0, 1040.0, 1000.0, 520.0).overlaps(&monitor));
        // Touching edges share no pixel.
        assert!(!Rect::at(1920.0, 0.0, 1000.0, 520.0).overlaps(&monitor));
    }
}
