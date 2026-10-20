//! A notification-area (tray) icon for the normal run, so a `--tray` launch
//! and close-to-tray have somewhere to live. Hand-rolled on `windows-sys` to
//! keep the exe lean (every megabyte adds to the launch-time hash under Smart
//! App Control): a hidden message window receives the icon callbacks and the
//! popup-menu commands and forwards them to the winit loop through a callback
//! the runner wires to its [`EventLoopProxy`]. Non-Windows builds get a stub
//! that creates nothing.
//!
//! [`EventLoopProxy`]: egui_winit::winit::event_loop::EventLoopProxy

/// What the tray asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Left click or the "Open" item: show and focus the window.
    Open,
    /// The "Quit" item: run the normal close sequence and exit.
    Quit,
}

pub use imp::Tray;

#[cfg(windows)]
mod imp {
    use super::TrayAction;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        ExtractIconW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
        NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        GetCursorPos, GetWindowLongPtrW, LoadIconW, PostMessageW, RegisterClassW,
        SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, GWLP_USERDATA, IDI_APPLICATION,
        MF_STRING, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL,
        WM_RBUTTONUP, WNDCLASSW,
    };

    /// The icon's callback message; the low word of its lParam is the mouse
    /// message (legacy version, since we never call `NIM_SETVERSION`).
    const WM_TRAYICON: u32 = WM_APP + 1;
    const ID_OPEN: usize = 1;
    const ID_QUIT: usize = 2;
    /// This process keeps one icon.
    const ICON_UID: u32 = 1;

    /// Held behind the window's `GWLP_USERDATA`, read by the wndproc.
    struct State {
        callback: Box<dyn Fn(TrayAction)>,
        open_label: Vec<u16>,
        quit_label: Vec<u16>,
    }

    pub struct Tray {
        hwnd: HWND,
        state: *mut State,
    }

    impl Tray {
        /// Register the icon. `None` when the window or the icon could not be
        /// created, so the caller can fall back to a normal close.
        pub fn new(
            open_label: &str,
            quit_label: &str,
            tooltip: &str,
            callback: Box<dyn Fn(TrayAction)>,
        ) -> Option<Tray> {
            // SAFETY: each call is the documented Win32 sequence with owned
            // arguments; the window and icon are released in `Drop`.
            unsafe {
                let hinstance = GetModuleHandleW(std::ptr::null());
                let class_name = wide("AccshiftTrayWindow");
                let class = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(wndproc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance,
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: std::ptr::null_mut(),
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class_name.as_ptr(),
                };
                // A second run reuses the class already registered; the error
                // is harmless since `CreateWindowExW` still finds it.
                RegisterClassW(&class);
                let window_name = wide("Accshift");
                let hwnd = CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    window_name.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    hinstance,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return None;
                }
                let state = Box::into_raw(Box::new(State {
                    callback,
                    open_label: wide(open_label),
                    quit_label: wide(quit_label),
                }));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);

                let mut data: NOTIFYICONDATAW = std::mem::zeroed();
                data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                data.hWnd = hwnd;
                data.uID = ICON_UID;
                data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
                data.uCallbackMessage = WM_TRAYICON;
                data.hIcon = load_icon(hinstance);
                let tip = wide(tooltip);
                let n = tip.len().min(data.szTip.len());
                data.szTip[..n].copy_from_slice(&tip[..n]);
                if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                    drop(Box::from_raw(state));
                    DestroyWindow(hwnd);
                    return None;
                }
                Some(Tray { hwnd, state })
            }
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            // SAFETY: the handles came from `new` and are released once.
            unsafe {
                let mut data: NOTIFYICONDATAW = std::mem::zeroed();
                data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                data.hWnd = self.hwnd;
                data.uID = ICON_UID;
                Shell_NotifyIconW(NIM_DELETE, &data);
                DestroyWindow(self.hwnd);
                drop(Box::from_raw(self.state));
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The exe's own icon, the taskbar icon the installed app ships; the
    /// generic application icon when the file carries none.
    unsafe fn load_icon(hinstance: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
        if let Ok(exe) = std::env::current_exe() {
            let path = wide(&exe.to_string_lossy());
            let icon = ExtractIconW(hinstance, path.as_ptr(), 0);
            // Null is an error, 1 means the file has no icon.
            if !icon.is_null() && icon as usize != 1 {
                return icon;
            }
        }
        LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
    }

    unsafe fn fire(hwnd: HWND, action: TrayAction) {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        if !state.is_null() {
            ((*state).callback)(action);
        }
    }

    unsafe fn show_menu(hwnd: HWND) {
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        if state.is_null() {
            return;
        }
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, ID_OPEN, (*state).open_label.as_ptr());
        AppendMenuW(menu, MF_STRING, ID_QUIT, (*state).quit_label.as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Without this the menu does not dismiss on a click elsewhere; the
        // trailing WM_NULL is the documented follow-up for a tray menu.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_TRAYICON => {
                let mouse = (lparam & 0xFFFF) as u32;
                match mouse {
                    WM_LBUTTONUP => fire(hwnd, TrayAction::Open),
                    WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_COMMAND => {
                match wparam & 0xFFFF {
                    ID_OPEN => fire(hwnd, TrayAction::Open),
                    ID_QUIT => fire(hwnd, TrayAction::Quit),
                    _ => {}
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TrayAction;

    /// Other platforms have no tray in this spike; the runner treats a `None`
    /// as "cannot hide, close normally".
    pub struct Tray;

    impl Tray {
        pub fn new(
            _open_label: &str,
            _quit_label: &str,
            _tooltip: &str,
            _callback: Box<dyn Fn(TrayAction)>,
        ) -> Option<Tray> {
            None
        }
    }
}
