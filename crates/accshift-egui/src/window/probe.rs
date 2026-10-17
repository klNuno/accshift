//! Debug builds only: the window shell of a normal run, proved without
//! touching the user's app folders or desktop. `ACCSHIFT_EGUI_PROBE_DIR`
//! points the real backend at a scratch folder; the window is cloaked,
//! never activated and kept out of the taskbar, so showing it changes
//! nothing on screen. A thread prints where the frame put it and when boot
//! showed it, moves it (the debounced geometry save), then closes it (the
//! close sequence). Run it twice: the second launch opens where the first
//! one closed. `<dir>/webview` stands for the user's local app data, where a
//! v0.x install left its WebView2 `localStorage`: an empty scratch folder
//! has none, and `write_legacy_fixture_for_the_probe` in accshift-app's
//! tests writes one there.

use super::frame;
use accshift_core::{AppContext, AppCtx};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, PostMessageW, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WM_CLOSE,
};

/// The scratch folder, when the variable is set.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("ACCSHIFT_EGUI_PROBE_DIR").map(PathBuf::from)
}

/// Every app directory under the scratch folder.
struct Scratch(PathBuf);

impl AppContext for Scratch {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("config"))
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("data"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.0.join("cache"))
    }
}

pub fn context(dir: PathBuf) -> AppCtx {
    Arc::new(Scratch(dir))
}

/// The probe's moves, from a thread: the window belongs to the UI thread,
/// which answers them between frames.
pub fn spawn(hwnd: HWND) {
    let hwnd = hwnd as isize;
    let started = Instant::now();
    std::thread::spawn(move || {
        let hwnd = hwnd as HWND;
        let rect = || frame::outer_rect(hwnd).unwrap_or_default();
        let inner = || frame::inner_size(hwnd).unwrap_or_default();
        let r = rect();
        eprintln!(
            "probe: placed outer {},{} {}x{} inner {:?} dpi {} shadow {}",
            r.left,
            r.top,
            r.width(),
            r.height(),
            inner(),
            frame::dpi(hwnd),
            frame::shadow(),
        );
        // SAFETY: a query on the live main window.
        while unsafe { IsWindowVisible(hwnd) } == 0 {
            if started.elapsed() > Duration::from_secs(8) {
                eprintln!("probe: never shown");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("probe: shown after {} ms", started.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(300));
        let r = rect();
        // SAFETY: the main window, moved without activation or resize.
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                r.left + 40,
                r.top + 30,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        let r = rect();
        eprintln!("probe: moved to {},{}", r.left, r.top);
        std::thread::sleep(Duration::from_millis(1200));
        eprintln!("probe: closing");
        // SAFETY: what the system menu's Close posts.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    });
}
