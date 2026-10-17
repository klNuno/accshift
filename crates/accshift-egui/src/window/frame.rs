//! The main window's frame on Windows, as tao 0.35 draws Tauri's
//! (`decorations(false)`, `transparent(true)` and the default shadow, which
//! boot.rs turns off before Windows 11):
//!
//! - with the shadow, the client area sits inside a hidden frame
//!   (`calculate_insets_for_dpi`): resize borders left, right and bottom,
//!   one pixel per 96 dpi on top, so DWM draws the shadow and rounds the
//!   corners. The top resize band is the first `SM_CYFRAME` rows of the
//!   client area (tao's `WM_NCHITTEST`);
//! - without it, the whole window is client area and its edges resize
//!   (`hit_test` over `SM_CXFRAME` and `SM_CYFRAME`);
//! - maximized, the client area is the monitor's work area, a pixel short
//!   on the edge of an auto-hidden taskbar so it can still come up;
//! - the minimum track size is tao's (`adjust_size` without the caption);
//! - `set_keep_backdrop_active` (src-tauri `commands/backdrop.rs`): DWM
//!   keeps a glass theme's acrylic on an inactive window;
//! - Snap Layouts (src-tauri `commands/window.rs`): the title bar's maximize
//!   button answers `HTMAXBUTTON`, so Windows 11 opens its flyout there.
//!
//! winit gives an undecorated window no frame and no resize edges; this
//! takes those messages before its window procedure.

// An HWND is an opaque handle the system validates; nothing here
// dereferences it.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use super::placement::{self, Insets, Monitor, Rect};
use accshift_core::config::{SavedWindow, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromRect, ScreenToClient, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONULL,
};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, GetSystemMetricsForDpi,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAR, APPBARDATA,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GetClientRect, GetWindowLongW, GetWindowRect, IsZoomed,
    PostMessageW, SetWindowLongPtrW, SetWindowPos, GWLP_WNDPROC, GWL_EXSTYLE, GWL_STYLE, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTMAXBUTTON, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT, MINMAXINFO, MONITORINFOF_PRIMARY, NCCALCSIZE_PARAMS, SC_MAXIMIZE, SC_RESTORE,
    SM_CXFRAME, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CYFRAME, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOZORDER, WM_GETMINMAXINFO, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
    WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCRBUTTONDOWN,
    WM_NCRBUTTONUP, WM_SYSCOMMAND, WNDPROC, WS_CAPTION, WS_SIZEBOX,
};

/// boot.rs `FIRST_WINDOWS_11_BUILD`.
const FIRST_WINDOWS_11_BUILD: u32 = 22000;

/// The window procedure the frame replaced (winit's, or a subclass over it).
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);
static SHADOW: AtomicBool = AtomicBool::new(false);
static KEEP_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Snap Layouts: `snap_layouts::State` of src-tauri `commands/window.rs`.
struct Snap {
    /// The maximize button in physical client pixels, `[x, y, width,
    /// height]`; `None` when it is not drawn.
    rect: Option<[f64; 4]>,
    hovering: bool,
    pressed: bool,
}

static SNAP: Mutex<Snap> = Mutex::new(Snap {
    rect: None,
    hovering: false,
    pressed: false,
});
/// Repaints the window when the hover changes: once Windows owns the
/// button as non-client, egui sees no pointer over it.
static SNAP_WAKE: OnceLock<egui::Context> = OnceLock::new();

/// A poisoned lock means a panic inside the window procedure; three plain
/// values with no invariant between them, so the state is recovered.
fn snap() -> MutexGuard<'static, Snap> {
    SNAP.lock().unwrap_or_else(|e| e.into_inner())
}

/// `set_maximize_button_rect`, in physical client pixels.
pub fn set_maximize_button(rect: Option<[f64; 4]>) {
    let mut state = snap();
    state.rect = rect;
    if rect.is_none() {
        state.hovering = false;
        state.pressed = false;
    }
}

/// The `titlebar:maximize-hover` state the title bar draws as hover.
pub fn maximize_hover() -> bool {
    snap().hovering
}

/// Where hover changes ask for a frame.
pub fn wake_on_maximize_hover(ctx: &egui::Context) {
    let _ = SNAP_WAKE.set(ctx.clone());
}

/// `hit_test_maximize` at scale 1: the rect is already physical. Left and
/// top inclusive, right and bottom exclusive, as CSS.
fn hit_test_maximize(point: (f64, f64), rect: [f64; 4]) -> bool {
    let [x, y, width, height] = rect;
    width > 0.0
        && height > 0.0
        && point.0 >= x
        && point.0 < x + width
        && point.1 >= y
        && point.1 < y + height
}

/// `CurrentBuildNumber`, read once, as boot.rs reads it.
fn windows_build() -> Option<u32> {
    static BUILD: OnceLock<Option<u32>> = OnceLock::new();
    *BUILD.get_or_init(|| {
        let mut text = [0u16; 16];
        let mut bytes = std::mem::size_of_val(&text) as u32;
        // SAFETY: REG_SZ into a buffer of `bytes` bytes, which it updates.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                windows_sys::w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
                windows_sys::w!("CurrentBuildNumber"),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                text.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != 0 {
            return None;
        }
        let len = (bytes as usize / 2).min(text.len());
        String::from_utf16_lossy(&text[..len])
            .trim_end_matches('\0')
            .parse()
            .ok()
    })
}

/// boot.rs `window_shadow_is_safe`: Windows 11, or a build it cannot read.
pub fn shadow() -> bool {
    windows_build().is_none_or(|build| build >= FIRST_WINDOWS_11_BUILD)
}

/// tao `calculate_insets_for_dpi`, nothing without the shadow.
pub fn insets(dpi: u32, shadow: bool) -> Insets {
    if !shadow {
        return Insets::default();
    }
    // SAFETY: plain metric lookups.
    let frame = unsafe {
        GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    };
    let top = if windows_build().is_none_or(|build| build >= FIRST_WINDOWS_11_BUILD) {
        (dpi as f32 / 96.0).round() as i32
    } else {
        0
    };
    Insets {
        left: frame,
        top,
        right: frame,
        bottom: frame,
    }
}

const EMPTY: RECT = RECT {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

fn monitor_info(monitor: HMONITOR) -> Option<MONITORINFO> {
    if monitor.is_null() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: EMPTY,
        rcWork: EMPTY,
        dwFlags: 0,
    };
    // SAFETY: `info` is a MONITORINFO with its size set.
    (unsafe { GetMonitorInfoW(monitor, &mut info) } != 0).then_some(info)
}

/// `GetDpiForMonitor`'s effective DPI, loaded from shcore at first use like
/// tao and winit do; 96 when it fails.
fn monitor_dpi(monitor: HMONITOR) -> u32 {
    type GetDpiForMonitor = unsafe extern "system" fn(HMONITOR, i32, *mut u32, *mut u32) -> i32;
    static FUNC: OnceLock<Option<GetDpiForMonitor>> = OnceLock::new();
    let func = FUNC.get_or_init(|| {
        // SAFETY: shcore is a system DLL; the symbol has this signature.
        unsafe {
            let lib = LoadLibraryW(windows_sys::w!("shcore.dll"));
            if lib.is_null() {
                return None;
            }
            GetProcAddress(lib, c"GetDpiForMonitor".as_ptr().cast())
                .map(|f| std::mem::transmute::<_, GetDpiForMonitor>(f))
        }
    });
    let (mut x, mut y) = (0u32, 0u32);
    // SAFETY: MDT_EFFECTIVE_DPI (0) into two u32.
    match func {
        Some(f) if unsafe { f(monitor, 0, &mut x, &mut y) } >= 0 && x > 0 => x,
        _ => 96,
    }
}

/// Every display, in `EnumDisplayMonitors` order as tao lists them.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn each(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        list: LPARAM,
    ) -> windows_sys::core::BOOL {
        // SAFETY: `list` is the Vec passed below, alive for the call.
        let list = unsafe { &mut *(list as *mut Vec<Monitor>) };
        if let Some(info) = monitor_info(monitor) {
            list.push(Monitor {
                rect: rect(info.rcMonitor),
                work: rect(info.rcWork),
                dpi: monitor_dpi(monitor),
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        1
    }
    let mut list: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only touches `list`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            &mut list as *mut Vec<Monitor> as LPARAM,
        );
    }
    list
}

/// The window rect, frame included (`outer_position`, `outer_size`).
pub fn outer_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = EMPTY;
    // SAFETY: live HWND, RECT out.
    (unsafe { GetWindowRect(hwnd, &mut r) } != 0).then(|| rect(r))
}

/// The client size (`inner_size`).
pub fn inner_size(hwnd: HWND) -> Option<(i32, i32)> {
    let mut r = EMPTY;
    // SAFETY: live HWND, RECT out.
    (unsafe { GetClientRect(hwnd, &mut r) } != 0).then_some((r.right - r.left, r.bottom - r.top))
}

/// The window's DPI (`scale_factor` × 96).
pub fn dpi(hwnd: HWND) -> u32 {
    // SAFETY: live HWND.
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        dpi => dpi,
    }
}

fn set_outer(hwnd: HWND, r: Rect) {
    // SAFETY: live HWND owned by this thread. FRAMECHANGED re-runs
    // WM_NCCALCSIZE, so the frame applies at once.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            r.left,
            r.top,
            r.width(),
            r.height(),
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// Puts the frame on `hwnd` (once), then moves the hidden window where
/// Tauri's opens for `saved`.
pub fn install(hwnd: HWND, saved: &SavedWindow) {
    let shadow = shadow();
    SHADOW.store(shadow, Ordering::Relaxed);
    if PREVIOUS.load(Ordering::Relaxed) == 0 {
        // SAFETY: replaces the procedure of a window this thread owns; the
        // previous one is stored before any message can reach the new one
        // (messages are dispatched on this thread).
        let previous =
            unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, frame_proc as *const () as isize) };
        PREVIOUS.store(previous, Ordering::Relaxed);
    }
    let monitors = monitors();
    let insets = |dpi| insets(dpi, shadow);
    let Some(start) = placement::start(saved, &monitors, &insets) else {
        return;
    };
    let Some(current) = outer_rect(hwnd) else {
        return;
    };
    let (x, y) = start.origin.unwrap_or((current.left, current.top));
    let target = Rect::at(x, y, start.size.0, start.size.1);
    set_outer(hwnd, target);
    // A move onto a monitor of another DPI lets winit resize the window to
    // keep its logical size: put tao's size back.
    if outer_rect(hwnd) != Some(target) {
        set_outer(hwnd, target);
    }
    let placed = placement::after_build(saved, outer_rect(hwnd).unwrap_or(target), &monitors);
    if Some(placed) != outer_rect(hwnd) {
        set_outer(hwnd, placed);
    }
}

/// `set_keep_backdrop_active`.
pub fn keep_backdrop_active(enabled: bool) {
    KEEP_ACTIVE.store(enabled, Ordering::Relaxed);
}

fn call_previous(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: PREVIOUS holds the window procedure GWLP_WNDPROC returned.
    unsafe {
        let previous: WNDPROC = std::mem::transmute(PREVIOUS.load(Ordering::Relaxed));
        CallWindowProcW(previous, hwnd, msg, wparam, lparam)
    }
}

/// tao's `has_edge`: an auto-hidden taskbar on that edge.
fn autohide_taskbar(edge: u32) -> bool {
    let mut data = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        uEdge: edge,
        // SAFETY: a plain C struct, all zero is valid.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: APPBARDATA with its size set.
    unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAR, &mut data) != 0 }
}

fn maximized(hwnd: HWND) -> bool {
    // SAFETY: live HWND.
    unsafe { IsZoomed(hwnd) != 0 }
}

unsafe fn nc_calc_size(hwnd: HWND, lparam: LPARAM) {
    // SAFETY: with wparam set, lparam is an NCCALCSIZE_PARAMS.
    let params = unsafe { &mut *(lparam as *mut NCCALCSIZE_PARAMS) };
    if maximized(hwnd) {
        // SAFETY: rgrc[0] is a valid RECT.
        let monitor = unsafe { MonitorFromRect(&params.rgrc[0], MONITOR_DEFAULTTONULL) };
        if let Some(info) = monitor_info(monitor) {
            let mut work = info.rcWork;
            if autohide_taskbar(ABE_BOTTOM) {
                work.bottom -= 1;
            }
            if autohide_taskbar(ABE_LEFT) {
                work.left += 1;
            }
            if autohide_taskbar(ABE_TOP) {
                work.top += 1;
            }
            if autohide_taskbar(ABE_RIGHT) {
                work.right -= 1;
            }
            params.rgrc[0] = work;
        }
    } else if SHADOW.load(Ordering::Relaxed) {
        let i = insets(dpi(hwnd), true);
        params.rgrc[0].left += i.left;
        params.rgrc[0].top += i.top;
        params.rgrc[0].right -= i.right;
        params.rgrc[0].bottom -= i.bottom;
    }
}

/// tao's `WM_NCHITTEST` for an unmaximized resizable undecorated window.
fn hit_test(hwnd: HWND, lparam: LPARAM) -> Option<u32> {
    if maximized(hwnd) {
        return None;
    }
    let (cx, cy) = (
        (lparam & 0xffff) as u16 as i16 as i32,
        ((lparam >> 16) & 0xffff) as u16 as i16 as i32,
    );
    let dpi = dpi(hwnd);
    // SAFETY: plain metric lookup.
    let border_y = unsafe { GetSystemMetricsForDpi(SM_CYFRAME, dpi) };
    if SHADOW.load(Ordering::Relaxed) {
        // Only the top edge: the frame resizes the other three.
        let (width, _) = inner_size(hwnd)?;
        let mut p = POINT { x: cx, y: cy };
        // SAFETY: live HWND, POINT in and out.
        let inside = unsafe { ScreenToClient(hwnd, &mut p) } != 0;
        return (inside && p.y >= 0 && p.y <= border_y && p.x >= 0 && p.x <= width)
            .then_some(HTTOP);
    }
    // SAFETY: plain metric lookup.
    let border_x = unsafe { GetSystemMetricsForDpi(SM_CXFRAME, dpi) };
    let r = outer_rect(hwnd)?;
    let left = cx < r.left + border_x;
    let right = cx >= r.right - border_x;
    let top = cy < r.top + border_y;
    let bottom = cy >= r.bottom - border_y;
    match (left, right, top, bottom) {
        (true, false, false, false) => Some(HTLEFT),
        (false, true, false, false) => Some(HTRIGHT),
        (false, false, true, false) => Some(HTTOP),
        (false, false, false, true) => Some(HTBOTTOM),
        (true, false, true, false) => Some(HTTOPLEFT),
        (false, true, true, false) => Some(HTTOPRIGHT),
        (true, false, false, true) => Some(HTBOTTOMLEFT),
        (false, true, false, true) => Some(HTBOTTOMRIGHT),
        _ => None,
    }
}

/// tao's `WM_GETMINMAXINFO`: the builder's minimum at the window's scale,
/// through `AdjustWindowRectExForDpi` without the caption and size box.
unsafe fn min_track_size(hwnd: HWND, lparam: LPARAM) {
    let dpi = dpi(hwnd);
    let scale = dpi as f64 / 96.0;
    let mut r = RECT {
        left: 0,
        top: 0,
        right: (MIN_WINDOW_WIDTH * scale).round() as i32,
        bottom: (MIN_WINDOW_HEIGHT * scale).round() as i32,
    };
    // SAFETY: live HWND; RECT in and out.
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32 & !(WS_CAPTION | WS_SIZEBOX);
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        AdjustWindowRectExForDpi(&mut r, style, 0, ex_style, dpi);
    }
    // SAFETY: lparam of WM_GETMINMAXINFO is a MINMAXINFO.
    let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
    info.ptMinTrackSize = POINT {
        x: r.right - r.left,
        y: r.bottom - r.top,
    };
}

fn over_maximize_button(hwnd: HWND, lparam: LPARAM) -> bool {
    let Some(rect) = snap().rect else {
        return false;
    };
    // Fullscreen has no title bar to snap from.
    if fullscreen(hwnd) {
        return false;
    }
    let mut p = POINT {
        x: (lparam & 0xffff) as u16 as i16 as i32,
        y: ((lparam >> 16) & 0xffff) as u16 as i16 as i32,
    };
    // SAFETY: live HWND, POINT in and out.
    if unsafe { ScreenToClient(hwnd, &mut p) } == 0 {
        return false;
    }
    hit_test_maximize((p.x as f64, p.y as f64), rect)
}

/// The window covers its whole monitor; maximized stops at the work area.
fn fullscreen(hwnd: HWND) -> bool {
    let Some(window) = outer_rect(hwnd) else {
        return false;
    };
    // SAFETY: live HWND.
    let monitor = unsafe {
        windows_sys::Win32::Graphics::Gdi::MonitorFromWindow(
            hwnd,
            windows_sys::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
        )
    };
    monitor_info(monitor).is_some_and(|info| rect(info.rcMonitor) == window)
}

fn set_maximize_hover(hwnd: HWND, over: bool) {
    {
        let mut state = snap();
        if state.hovering == over {
            return;
        }
        state.hovering = over;
        if !over {
            state.pressed = false;
        }
    }
    if over {
        // Without it there is no WM_NCMOUSELEAVE when the pointer leaves
        // over the window edge.
        let mut track = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE | TME_NONCLIENT,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: a TRACKMOUSEEVENT with its size set, for a live HWND.
        unsafe { TrackMouseEvent(&mut track) };
    }
    if let Some(ctx) = SNAP_WAKE.get() {
        ctx.request_repaint();
    }
}

/// What a real caption button sends, posted so the resize runs after this
/// message returns.
fn toggle_maximize(hwnd: HWND) {
    let command = if maximized(hwnd) {
        SC_RESTORE
    } else {
        SC_MAXIMIZE
    };
    // SAFETY: live HWND.
    unsafe { PostMessageW(hwnd, WM_SYSCOMMAND, command as WPARAM, 0) };
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCALCSIZE if wparam != 0 => {
            // SAFETY: the message's own parameters.
            unsafe { nc_calc_size(hwnd, lparam) };
            return 0;
        }
        WM_NCHITTEST => {
            if let Some(hit) = hit_test(hwnd, lparam) {
                return hit as LRESULT;
            }
            // Only the plain client area becomes the maximize button.
            let below = call_previous(hwnd, msg, wparam, lparam);
            if below == HTCLIENT as LRESULT && over_maximize_button(hwnd, lparam) {
                return HTMAXBUTTON as LRESULT;
            }
            return below;
        }
        WM_NCMOUSEMOVE => {
            let over = wparam == HTMAXBUTTON as WPARAM;
            set_maximize_hover(hwnd, over);
            if over {
                return 0;
            }
        }
        WM_NCMOUSELEAVE => set_maximize_hover(hwnd, false),
        // Swallowed: the default procedure would draw its own caption
        // button over the title bar's and maximize on the way down.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam == HTMAXBUTTON as WPARAM => {
            snap().pressed = true;
            return 0;
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as WPARAM => {
            if std::mem::replace(&mut snap().pressed, false) {
                toggle_maximize(hwnd);
            }
            return 0;
        }
        // No system menu over the button.
        WM_NCRBUTTONDOWN | WM_NCRBUTTONUP if wparam == HTMAXBUTTON as WPARAM => return 0,
        WM_GETMINMAXINFO => {
            let result = call_previous(hwnd, msg, wparam, lparam);
            // SAFETY: the message's own parameters.
            unsafe { min_track_size(hwnd, lparam) };
            return result;
        }
        // DWM drops a system backdrop as soon as the window reports
        // itself inactive; answering "still active" (lparam -1 skips the
        // non-client repaint) keeps it. Focus events still follow
        // WM_SETFOCUS and WM_KILLFOCUS.
        WM_NCACTIVATE if wparam == 0 && KEEP_ACTIVE.load(Ordering::Relaxed) => {
            // SAFETY: live HWND.
            return unsafe { DefWindowProcW(hwnd, WM_NCACTIVATE, 1, -1) };
        }
        _ => {}
    }
    call_previous(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    //! One hidden window (never shown, never activated), the frame on it,
    //! and the messages tao answers. One test: the frame keeps one window
    //! procedure per process.
    use super::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, RegisterClassExW, SendMessageW, HTCLIENT, WNDCLASSEXW,
        WS_CLIPSIBLINGS, WS_EX_APPWINDOW, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        WS_SYSMENU,
    };

    fn lparam(x: i32, y: i32) -> LPARAM {
        ((y as u16 as u32) << 16 | x as u16 as u32) as LPARAM
    }

    #[test]
    fn the_frame_answers_like_tao() {
        // SAFETY: a class and a hidden window this test owns and destroys.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = windows_sys::w!("accshift-egui-frame-test");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: instance,
                lpszClassName: class,
                ..std::mem::zeroed()
            };
            RegisterClassExW(&wc);
            // tao's styles for `decorations(false)`, resizable, hidden.
            CreateWindowExW(
                WS_EX_WINDOWEDGE | WS_EX_APPWINDOW,
                class,
                windows_sys::w!(""),
                WS_CAPTION
                    | WS_CLIPSIBLINGS
                    | WS_SYSMENU
                    | WS_SIZEBOX
                    | WS_MAXIMIZEBOX
                    | WS_MINIMIZEBOX,
                0,
                0,
                800,
                600,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let saved = SavedWindow {
            size: None,
            position: None,
            physical_position: None,
        };
        install(hwnd, &saved);

        let monitors = monitors();
        let shadow = shadow();
        let insets = |d| insets(d, shadow);
        let start = placement::start(&saved, &monitors, &insets).expect("a monitor");
        let outer = outer_rect(hwnd).unwrap();
        assert_eq!((outer.width(), outer.height()), start.size);
        assert_eq!(Some((outer.left, outer.top)), start.origin);
        let dpi = dpi(hwnd);
        let i = super::insets(dpi, shadow);
        // The client area is the start size without the frame: the
        // builder's inner size, 1000x680 at the monitor's scale.
        let scale = dpi as f64 / 96.0;
        assert_eq!(
            inner_size(hwnd),
            Some((
                outer.width() - i.left - i.right,
                outer.height() - i.top - i.bottom
            ))
        );
        let primary = monitors.iter().find(|m| m.primary).unwrap();
        if primary.work.width() as f64 > 1032.0 * scale {
            assert_eq!(inner_size(hwnd).unwrap().0, (1000.0 * scale).round() as i32);
        }

        // SAFETY: plain messages to the window this test owns.
        unsafe {
            let mut client = POINT { x: 0, y: 0 };
            windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut client);
            let hit = |x, y| SendMessageW(hwnd, WM_NCHITTEST, 0, lparam(x, y)) as u32;
            let middle = (client.x + 200, client.y + 200);
            assert_eq!(hit(middle.0, middle.1), HTCLIENT);
            if shadow {
                // The top band of the client area resizes; the other edges
                // are the hidden frame's.
                assert_eq!(hit(client.x + 200, client.y + 1), HTTOP);
                assert_eq!(hit(outer.left + 1, outer.top + 200), HTLEFT);
            } else {
                assert_eq!(hit(outer.left + 1, outer.top + 200), HTLEFT);
                assert_eq!(hit(outer.left + 1, outer.top + 1), HTTOPLEFT);
                assert_eq!(hit(outer.right - 1, outer.bottom - 1), HTBOTTOMRIGHT);
            }

            let mut info: MINMAXINFO = std::mem::zeroed();
            SendMessageW(
                hwnd,
                WM_GETMINMAXINFO,
                0,
                &mut info as *mut MINMAXINFO as LPARAM,
            );
            let mut r = RECT {
                left: 0,
                top: 0,
                right: (400.0 * scale).round() as i32,
                bottom: (300.0 * scale).round() as i32,
            };
            AdjustWindowRectExForDpi(
                &mut r,
                WS_CLIPSIBLINGS | WS_SYSMENU | WS_MAXIMIZEBOX | WS_MINIMIZEBOX,
                0,
                WS_EX_WINDOWEDGE | WS_EX_APPWINDOW,
                dpi,
            );
            assert_eq!(
                (info.ptMinTrackSize.x, info.ptMinTrackSize.y),
                (r.right - r.left, r.bottom - r.top)
            );

            // Snap Layouts: the reported button, and only there.
            set_maximize_button(Some([300.0, 0.0, 46.0, 36.0]));
            assert_eq!(hit(client.x + 323, client.y + 18), HTMAXBUTTON);
            assert_eq!(hit(client.x + 299, client.y + 18), HTCLIENT);
            assert_eq!(hit(client.x + 346, client.y + 18), HTCLIENT);
            assert_eq!(hit(middle.0, middle.1), HTCLIENT);
            set_maximize_button(None);
            assert_eq!(hit(client.x + 323, client.y + 18), HTCLIENT);

            // Kept active: DefWindowProc gets "active" back.
            keep_backdrop_active(true);
            assert_eq!(SendMessageW(hwnd, WM_NCACTIVATE, 0, 0), 1);
            keep_backdrop_active(false);

            DestroyWindow(hwnd);
        }
    }
}
