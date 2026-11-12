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
        AppendMenuW, ChangeWindowMessageFilterEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyMenu, DestroyWindow, GetCursorPos, GetWindowLongPtrW, LoadIconW, PostMessageW,
        RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW,
        TrackPopupMenu, GWLP_USERDATA, IDI_APPLICATION, MF_STRING, MSGFLT_ALLOW, TPM_RIGHTBUTTON,
        WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
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
        /// The icon as first added, added again when Explorer restarts.
        icon: NOTIFYICONDATAW,
        /// The `TaskbarCreated` message id, 0 when it could not be registered.
        taskbar_created: u32,
    }

    /// The message Explorer broadcasts to top-level windows once its taskbar
    /// exists again after a restart: every tray icon is gone by then and has
    /// to be added back. 0 when registration fails.
    pub(super) fn taskbar_created_message() -> u32 {
        let name = wide("TaskbarCreated");
        // SAFETY: a NUL-terminated wide string that outlives the call.
        unsafe { RegisterWindowMessageW(name.as_ptr()) }
    }

    /// What the tray window does with one message.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum Reaction {
        Fire(TrayAction),
        Menu,
        /// Explorer restarted: add the icon again.
        Readd,
        Default,
    }

    pub(super) fn react(
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        taskbar_created: u32,
    ) -> Reaction {
        // An unregistered id is 0, which is WM_NULL: never read it as a restart.
        if taskbar_created != 0 && msg == taskbar_created {
            return Reaction::Readd;
        }
        match msg {
            WM_TRAYICON => match (lparam & 0xFFFF) as u32 {
                WM_LBUTTONUP => Reaction::Fire(TrayAction::Open),
                WM_RBUTTONUP | WM_CONTEXTMENU => Reaction::Menu,
                _ => Reaction::Default,
            },
            WM_COMMAND => match wparam & 0xFFFF {
                ID_OPEN => Reaction::Fire(TrayAction::Open),
                ID_QUIT => Reaction::Fire(TrayAction::Quit),
                _ => Reaction::Default,
            },
            _ => Reaction::Default,
        }
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
                // A hidden top-level window, not a message-only one: only
                // top-level windows get the `TaskbarCreated` broadcast.
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
                let taskbar_created = taskbar_created_message();
                if taskbar_created != 0 {
                    // UIPI drops Explorer's broadcast to an elevated process
                    // unless the message is let through.
                    ChangeWindowMessageFilterEx(
                        hwnd,
                        taskbar_created,
                        MSGFLT_ALLOW,
                        std::ptr::null_mut(),
                    );
                }
                let state = Box::into_raw(Box::new(State {
                    callback,
                    open_label: wide(open_label),
                    quit_label: wide(quit_label),
                    icon: data,
                    taskbar_created,
                }));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
                if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                    // The wndproc reads the state until the window is gone.
                    DestroyWindow(hwnd);
                    drop(Box::from_raw(state));
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
        // Null until `new` stores it, and while the window is created.
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        let taskbar_created = state.as_ref().map_or(0, |state| state.taskbar_created);
        match react(msg, wparam, lparam, taskbar_created) {
            Reaction::Fire(action) => fire(hwnd, action),
            Reaction::Menu => show_menu(hwnd),
            Reaction::Readd => {
                if let Some(state) = state.as_ref() {
                    Shell_NotifyIconW(NIM_ADD, &state.icon);
                }
            }
            Reaction::Default => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
        0
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TrayAction;

    /// Other platforms have no tray yet; the runner treats a `None` as
    /// "cannot hide, close normally".
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

#[cfg(all(test, windows))]
mod tests {
    use super::imp::{react, taskbar_created_message, Reaction};
    use super::TrayAction;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_APP, WM_COMMAND, WM_LBUTTONUP};

    #[test]
    fn explorer_restart_adds_the_icon_again() {
        let id = taskbar_created_message();
        // Registered messages live in 0xC000..=0xFFFF and keep their id.
        assert!((0xC000..=0xFFFF).contains(&id), "{id:#x}");
        assert_eq!(taskbar_created_message(), id);
        assert_eq!(react(id, 0, 0, id), Reaction::Readd);
    }

    #[test]
    fn the_icon_and_menu_messages_keep_their_meaning() {
        let id = taskbar_created_message();
        let icon = WM_APP + 1;
        assert_eq!(
            react(icon, 0, WM_LBUTTONUP as isize, id),
            Reaction::Fire(TrayAction::Open)
        );
        assert_eq!(
            react(WM_COMMAND, 2, 0, id),
            Reaction::Fire(TrayAction::Quit)
        );
        // A failed registration (0) never reads WM_NULL as a restart.
        assert_eq!(react(0, 0, 0, 0), Reaction::Default);
    }
}
