use std::sync::atomic::{AtomicBool, Ordering};

// Only used by the real show path, which the bench and parity builds compile
// out.
#[cfg_attr(
    any(feature = "startup-bench", feature = "parity-capture"),
    allow(unused_imports)
)]
use tauri::Manager;

#[derive(Default)]
pub struct BootState {
    completed: AtomicBool,
}

impl BootState {
    pub fn mark_completed(&self) -> bool {
        !self.completed.swap(true, Ordering::SeqCst)
    }

    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }
}

pub fn show_main_window(app_handle: &tauri::AppHandle) -> Result<(), String> {
    // Measuring startup means launching the app dozens of times, and each one
    // would steal the focus of whoever is at the keyboard. Running the bench on
    // a separate Windows desktop avoided that but changed what was measured:
    // no interactive compositor there, so frames and timers ran late. So the
    // window stays hidden instead, on the real desktop, everything else
    // identical. Never enabled in a shipped build.
    #[cfg(feature = "startup-bench")]
    {
        // The bench harness refuses to launch a binary that does not carry this
        // marker, so a build that would pop a window can never reach the loop.
        #[used]
        static MARKER: &[u8] = b"accshift-startup-bench-build";
        std::hint::black_box(MARKER);
        let _ = app_handle;
        Ok(())
    }

    #[cfg(all(feature = "parity-capture", not(feature = "startup-bench")))]
    {
        parity::show_unseen(app_handle)
    }

    #[cfg(not(any(feature = "startup-bench", feature = "parity-capture")))]
    {
        let Some(main_window) = app_handle.get_webview_window("main") else {
            return Err("Main window is unavailable".to_string());
        };

        main_window.show().map_err(|reason| reason.to_string())?;
        let _ = main_window.set_focus();
        Ok(())
    }
}

/// Reference renderer of the egui parity bench
/// (`crates/accshift-egui/parity/ref.mjs`). The harness drives the WebView2
/// over CDP and reads frames with `Page.captureScreenshot`, so the window only
/// has to exist for Chromium to keep painting: it is never seen, never
/// focused, never in the taskbar. Never enabled in a shipped build.
///
/// `ACCSHIFT_PARITY_DIR` is required: every app directory and the WebView2
/// profile live under it, so a run never writes to the real
/// `%APPDATA%\com.accshift.desktop` or `%LOCALAPPDATA%\com.accshift.desktop`.
#[cfg(feature = "parity-capture")]
pub mod parity {
    use std::path::PathBuf;

    /// The harness refuses to launch a binary that does not carry this marker,
    /// so a build that would show a window can never be driven by it.
    #[used]
    static MARKER: &[u8] = b"accshift-parity-capture-build";

    /// Root of every directory the run writes to.
    pub fn root() -> Option<PathBuf> {
        std::env::var_os("ACCSHIFT_PARITY_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    /// Exit before anything resolves a path: without the scratch root, the
    /// app would fall back on the real data directories.
    pub fn require_root() {
        std::hint::black_box(MARKER);
        if root().is_none() {
            eprintln!("parity-capture build: ACCSHIFT_PARITY_DIR is not set, refusing to start");
            std::process::exit(2);
        }
    }

    /// The WebView2 profile of the run.
    pub fn webview_dir() -> Option<PathBuf> {
        root().map(|dir| dir.join("webview"))
    }

    /// By default the window stays as built: never shown, and WebView2 still
    /// paints and captures (checked on WebView2 154 with native occlusion
    /// off). `ACCSHIFT_PARITY_WINDOW=cloak` is the fallback if a runtime stops
    /// painting a hidden window: shown without activation as a tool window at
    /// -32000,-32000 with DWM cloaking, so nothing reaches the screen either.
    pub fn show_unseen(app_handle: &tauri::AppHandle) -> Result<(), String> {
        std::hint::black_box(MARKER);
        #[cfg(windows)]
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            use tauri::Manager;
            static SHOWN: AtomicBool = AtomicBool::new(false);

            let Some(win) = app_handle.get_webview_window("main") else {
                return Err("Main window is unavailable".to_string());
            };
            let hwnd = win.hwnd().map_err(|e| e.to_string())?.0 as isize;
            if SHOWN.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            let cloak = std::env::var("ACCSHIFT_PARITY_WINDOW").as_deref() == Ok("cloak");
            if cloak {
                win.run_on_main_thread(move || win32::show_cloaked(hwnd))
                    .map_err(|e| e.to_string())?;
            }
            win32::spawn_monitor(hwnd);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = app_handle;
            Ok(())
        }
    }

    #[cfg(windows)]
    mod win32 {
        use core::ffi::c_void;
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
            SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, SWP_NOACTIVATE, SWP_NOSIZE,
            SWP_NOZORDER, SW_SHOWNOACTIVATE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };

        const DWMWA_CLOAK: u32 = 13;
        const DWMWA_CLOAKED: u32 = 14;
        const OFF_SCREEN: i32 = -32000;

        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmSetWindowAttribute(hwnd: HWND, attr: u32, value: *const c_void, size: u32)
                -> i32;
            fn DwmGetWindowAttribute(hwnd: HWND, attr: u32, value: *mut c_void, size: u32) -> i32;
        }

        fn handle(raw: isize) -> HWND {
            HWND(raw as *mut c_void)
        }

        /// Cloak, move off every monitor and leave the taskbar first, then
        /// show without activation: the order means no frame of the window
        /// is ever composed on screen.
        pub fn show_cloaked(raw: isize) {
            let hwnd = handle(raw);
            let on: i32 = 1;
            // SAFETY: `hwnd` is the live main window, owned by this (UI)
            // thread; the DWM attribute is a 4-byte BOOL.
            unsafe {
                DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &on as *const i32 as *const c_void, 4);
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let ex = (ex & !(WS_EX_APPWINDOW.0 as isize))
                    | WS_EX_TOOLWINDOW.0 as isize
                    | WS_EX_NOACTIVATE.0 as isize;
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    OFF_SCREEN,
                    OFF_SCREEN,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
                );
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            log_state(hwnd, "shown");
        }

        /// One line per sample on stderr, parsed by the harness as the proof
        /// that the window never reached the screen.
        fn log_state(hwnd: HWND, when: &str) {
            let mut cloaked: u32 = 0;
            let mut rect = RECT::default();
            // SAFETY: read-only queries on a window handle; a destroyed window
            // only makes them fail, which the zeros below report.
            let (visible, foreground) = unsafe {
                DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED,
                    &mut cloaked as *mut u32 as *mut c_void,
                    4,
                );
                let _ = GetWindowRect(hwnd, &mut rect);
                (
                    IsWindowVisible(hwnd).as_bool(),
                    GetForegroundWindow() == hwnd,
                )
            };
            eprintln!(
                "parity-window {when} visible={visible} cloaked={cloaked} foreground={foreground} rect={},{},{},{}",
                rect.left, rect.top, rect.right, rect.bottom
            );
        }

        /// Samples the window state every 250 ms until the process exits.
        pub fn spawn_monitor(raw: isize) {
            let _ = std::thread::Builder::new()
                .name("parity-window".into())
                .spawn(move || loop {
                    log_state(handle(raw), "sample");
                    std::thread::sleep(std::time::Duration::from_millis(250));
                });
        }
    }
}
