//! Window commands, and Windows 11 Snap Layouts over the custom maximize button.

#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn toggle_maximize_window(window: tauri::Window) {
    if matches!(window.is_maximized(), Ok(true)) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}

// ---------------------------------------------------------------------------
// Windows 11 Snap Layouts over the custom maximize button
// ---------------------------------------------------------------------------

/// Where the titlebar's maximize button sits, in CSS pixels relative to the
/// top-left of the webview.
///
/// The window is frameless, so Windows sees one big client area and never
/// offers the Snap Layouts flyout: that flyout only appears over a rectangle
/// the window itself reports as `HTMAXBUTTON`. The frontend owns the button's
/// geometry, so it is the frontend that measures it and hands it over.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct MaximizeButtonRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// True when `point`, in physical client pixels, falls inside `rect`, which is
/// in logical pixels at `scale`.
///
/// Pulled out of the window procedure on purpose: this is the whole decision
/// behind the Snap Layouts flyout, and it is the only part of the feature that
/// can be checked without a Windows 11 machine and a real mouse.
///
/// Compiled on Windows (the window procedure calls it) and when tests run
/// (so linux/macOS CI still covers the geometry). Left out of a normal
/// unix `cargo clippy` of the binary, where it would be dead code.
#[cfg(any(windows, test))]
pub fn hit_test_maximize(point: (f64, f64), rect: MaximizeButtonRect, scale: f64) -> bool {
    if !scale.is_finite() || scale <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return false;
    }
    let left = rect.x * scale;
    let top = rect.y * scale;
    point.0 >= left
        && point.0 < left + rect.width * scale
        && point.1 >= top
        && point.1 < top + rect.height * scale
}

#[cfg(windows)]
mod snap_layouts {
    use super::{hit_test_maximize, MaximizeButtonRect};
    use std::mem::size_of;
    use std::sync::{Mutex, MutexGuard};
    use tauri::{AppHandle, Emitter};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, ScreenToClient, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
    };
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsZoomed, PostMessageW, HTCLIENT, HTMAXBUTTON, SC_MAXIMIZE, SC_RESTORE,
        WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
        WM_NCMOUSEMOVE, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_SYSCOMMAND,
    };

    const SUBCLASS_ID: usize = 0x61636374; // "acct"

    /// Hit-test codes as `isize`, which is what a `WPARAM` and an `LRESULT`
    /// carry. The constants themselves are `i32` and the casts would otherwise
    /// be repeated at every comparison.
    const HT_CLIENT: isize = HTCLIENT as isize;
    const HT_MAX_BUTTON: isize = HTMAXBUTTON as isize;

    /// Emitted with `true` when the pointer enters the maximize button and
    /// `false` when it leaves. Once Windows owns that rectangle as non-client,
    /// the webview stops seeing pointer events over it, so CSS `:hover` never
    /// fires and the button would look dead under the cursor.
    const HOVER_EVENT: &str = "titlebar:maximize-hover";

    struct State {
        /// The button as last reported by the frontend. `None` means it is not
        /// on screen (macOS layout, or the actions hidden), and every message
        /// below then falls through untouched.
        ///
        /// One rect for the whole app: only the main window draws a titlebar.
        rect: Option<MaximizeButtonRect>,
        app: Option<AppHandle>,
        hovering: bool,
        pressed: bool,
        /// Windows already subclassed, so a second report does not stack a
        /// second copy of the procedure on the same window.
        installed: Vec<isize>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        rect: None,
        app: None,
        hovering: false,
        pressed: false,
        installed: Vec::new(),
    });

    /// A poisoned lock here means a previous panic inside the window
    /// procedure. The state is four plain values with no invariant between
    /// them, so recovering it beats killing the titlebar for the session.
    fn lock() -> MutexGuard<'static, State> {
        STATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_rect(app: &AppHandle, rect: Option<MaximizeButtonRect>) {
        let mut state = lock();
        state.app = Some(app.clone());
        state.rect = rect;
        if rect.is_none() {
            state.hovering = false;
            state.pressed = false;
        }
    }

    /// Installs the procedure. Must run on the thread that owns the window.
    pub fn install(hwnd: isize) {
        let mut state = lock();
        if state.installed.contains(&hwnd) {
            return;
        }
        let handle = HWND(hwnd as *mut core::ffi::c_void);
        if unsafe { SetWindowSubclass(handle, Some(window_proc), SUBCLASS_ID, 0) }.as_bool() {
            state.installed.push(hwnd);
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_NCHITTEST => {
                let below = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
                // Only ever upgrade the plain client area. A resize border, the
                // caption, or anything tao already claims keeps its answer, so
                // the top edge of the window stays draggable-to-resize.
                if below.0 == HT_CLIENT && over_maximize_button(hwnd, lparam) {
                    return LRESULT(HT_MAX_BUTTON);
                }
                below
            }
            WM_NCMOUSEMOVE => {
                let over = wparam.0 as isize == HT_MAX_BUTTON;
                set_hover(hwnd, over);
                if over {
                    return LRESULT(0);
                }
                unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
            }
            WM_NCMOUSELEAVE => {
                set_hover(hwnd, false);
                unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
            }
            // Swallowed: handing these to the default procedure makes Windows
            // draw its own caption button over ours and start its own maximize
            // on the way down.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam.0 as isize == HT_MAX_BUTTON => {
                lock().pressed = true;
                LRESULT(0)
            }
            WM_NCLBUTTONUP if wparam.0 as isize == HT_MAX_BUTTON => {
                let pressed = std::mem::replace(&mut lock().pressed, false);
                if pressed {
                    toggle_maximize(hwnd);
                }
                LRESULT(0)
            }
            // Right-clicking this area used to reach the webview, which shows
            // nothing there. Swallowed so the answer stays "nothing" instead of
            // the system menu the default procedure opens over a real caption
            // button. Everywhere else on the titlebar is unaffected.
            WM_NCRBUTTONDOWN | WM_NCRBUTTONUP if wparam.0 as isize == HT_MAX_BUTTON => LRESULT(0),
            _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
        }
    }

    /// The same toggle `toggle_maximize_window` performs, expressed as the
    /// message a real caption button would send. Posted rather than called so
    /// the resize happens after this message returns, and tao sees it the way
    /// it sees a click on a decorated window.
    fn toggle_maximize(hwnd: HWND) {
        let command = if unsafe { IsZoomed(hwnd) }.as_bool() {
            SC_RESTORE
        } else {
            SC_MAXIMIZE
        };
        let _ = unsafe {
            PostMessageW(
                Some(hwnd),
                WM_SYSCOMMAND,
                WPARAM(command as usize),
                LPARAM(0),
            )
        };
    }

    fn over_maximize_button(hwnd: HWND, lparam: LPARAM) -> bool {
        let Some(rect) = lock().rect else {
            return false;
        };
        // Fullscreen has no titlebar to snap from, and the frontend may not
        // have reported the button gone yet.
        if is_fullscreen(hwnd) {
            return false;
        }
        // WM_NCHITTEST carries screen coordinates, the rect is client-relative.
        let mut point = POINT {
            x: signed_low(lparam),
            y: signed_high(lparam),
        };
        if !unsafe { ScreenToClient(hwnd, &mut point) }.as_bool() {
            return false;
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
        hit_test_maximize((f64::from(point.x), f64::from(point.y)), rect, scale)
    }

    fn is_fullscreen(hwnd: HWND) -> bool {
        let mut window = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut window) }.is_err() {
            return false;
        }
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return false;
        }
        // Maximized stops at the work area; only fullscreen covers the whole
        // monitor.
        window == info.rcMonitor
    }

    fn set_hover(hwnd: HWND, over: bool) {
        let app = {
            let mut state = lock();
            if state.hovering == over {
                return;
            }
            state.hovering = over;
            if !over {
                state.pressed = false;
            }
            state.app.clone()
        };

        if over {
            // Without this there is no WM_NCMOUSELEAVE, so the button would
            // stay lit after the pointer walks off the window edge.
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE | TME_NONCLIENT,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = unsafe { TrackMouseEvent(&mut track) };
        }

        if let Some(app) = app {
            // Never inline: the emit reaches the webview, and re-entering
            // WebView2 from inside a message handler is the deadlock the repo
            // rule about eval from a callback is there to prevent.
            tauri::async_runtime::spawn(async move {
                let _ = app.emit(HOVER_EVENT, over);
            });
        }
    }

    fn signed_low(lparam: LPARAM) -> i32 {
        i32::from((lparam.0 & 0xFFFF) as u16 as i16)
    }

    fn signed_high(lparam: LPARAM) -> i32 {
        i32::from(((lparam.0 >> 16) & 0xFFFF) as u16 as i16)
    }
}

/// Reports where the titlebar's maximize button is, so Windows 11 can offer
/// Snap Layouts over it. `null` means the button is not on screen.
///
/// Synchronous on purpose: the body writes a static and posts one message to
/// the window's own thread. No-op outside Windows.
#[tauri::command]
pub fn set_maximize_button_rect(
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    rect: Option<MaximizeButtonRect>,
) {
    #[cfg(windows)]
    {
        snap_layouts::set_rect(&app_handle, rect);
        if let Ok(hwnd) = window.hwnd() {
            let hwnd = hwnd.0 as isize;
            // Subclassing must happen on the thread that owns the window.
            let _ = window.run_on_main_thread(move || snap_layouts::install(hwnd));
        }
    }
    #[cfg(not(windows))]
    let _ = (app_handle, window, rect);
}

#[cfg(test)]
mod snap_layout_tests {
    use super::{hit_test_maximize, MaximizeButtonRect};

    /// A 46x36 caption button at the right edge of a 900px titlebar, the
    /// geometry TitleBar.svelte draws today.
    const BUTTON: MaximizeButtonRect = MaximizeButtonRect {
        x: 808.0,
        y: 0.0,
        width: 46.0,
        height: 36.0,
    };

    #[test]
    fn the_middle_of_the_button_is_a_hit() {
        assert!(hit_test_maximize((831.0, 18.0), BUTTON, 1.0));
    }

    #[test]
    fn the_edges_belong_to_the_button_the_way_css_says() {
        // Left and top inclusive, right and bottom exclusive, so the close
        // button next door never loses its first column of pixels.
        assert!(hit_test_maximize((808.0, 0.0), BUTTON, 1.0));
        assert!(!hit_test_maximize((854.0, 18.0), BUTTON, 1.0));
        assert!(!hit_test_maximize((831.0, 36.0), BUTTON, 1.0));
        assert!(!hit_test_maximize((807.9, 18.0), BUTTON, 1.0));
    }

    #[test]
    fn a_point_over_the_minimize_or_close_button_is_a_miss() {
        assert!(!hit_test_maximize((790.0, 18.0), BUTTON, 1.0));
        assert!(!hit_test_maximize((870.0, 18.0), BUTTON, 1.0));
    }

    #[test]
    fn the_rect_scales_with_the_monitor_dpi() {
        // 150% (144 dpi): the same button covers 1212..1281 physical pixels.
        assert!(hit_test_maximize((1246.0, 27.0), BUTTON, 1.5));
        assert!(!hit_test_maximize((831.0, 18.0), BUTTON, 1.5));
        assert!(!hit_test_maximize((1281.0, 27.0), BUTTON, 1.5));
        assert!(hit_test_maximize((1280.9, 53.9), BUTTON, 1.5));
    }

    #[test]
    fn a_degenerate_rect_or_scale_never_claims_a_point() {
        let empty = MaximizeButtonRect {
            x: 808.0,
            y: 0.0,
            width: 0.0,
            height: 36.0,
        };
        assert!(!hit_test_maximize((808.0, 18.0), empty, 1.0));
        assert!(!hit_test_maximize((831.0, 18.0), BUTTON, 0.0));
        assert!(!hit_test_maximize((831.0, 18.0), BUTTON, f64::NAN));
        assert!(!hit_test_maximize((831.0, 18.0), BUTTON, -1.0));
    }
}
