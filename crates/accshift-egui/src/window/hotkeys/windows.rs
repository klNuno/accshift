//! Windows: `RegisterHotKey` on a message-only window of the UI thread.
//! winit's message loop dispatches every message of the thread, this
//! window's `WM_HOTKEY` included, whether the app window shows or sits
//! hidden in the tray. No new crate: `windows-sys` already binds both.

use accshift_app::shortcuts::{Availability, Combo, RegisterError, Registrar};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, VK_LWIN, VK_RWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassW,
    SetWindowLongPtrW, GWLP_USERDATA, HWND_MESSAGE, WM_HOTKEY, WNDCLASSW,
};

use super::keys::{win_mods, win_vk};
use super::Deliver;

/// `ERROR_HOTKEY_ALREADY_REGISTERED`: another app (or this one) holds it.
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

pub struct WindowsRegistrar {
    hwnd: HWND,
    /// Behind the window's `GWLP_USERDATA`, read by the wndproc.
    deliver: *mut Deliver,
    registered: Vec<u32>,
}

impl WindowsRegistrar {
    /// The message-only window. A registrar whose window could not be made
    /// answers every registration with a failure.
    pub fn new(deliver: Deliver) -> Self {
        let deliver = Box::into_raw(Box::new(deliver));
        // SAFETY: the documented Win32 sequence with owned arguments; the
        // window and the state are released in `Drop`.
        let hwnd = unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            let class_name = wide("AccshiftHotkeys");
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
            // Registered once per process; a later call fails harmlessly.
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );
            if !hwnd.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, deliver as isize);
            }
            hwnd
        };
        Self {
            hwnd,
            deliver,
            registered: Vec::new(),
        }
    }
}

impl Registrar for WindowsRegistrar {
    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn register(&mut self, id: u32, combo: &Combo) -> Result<(), RegisterError> {
        if self.hwnd.is_null() {
            return Err(RegisterError::Failed("no message window".into()));
        }
        // SAFETY: a window this thread owns.
        let ok =
            unsafe { RegisterHotKey(self.hwnd, id as i32, win_mods(combo), win_vk(combo.key)) };
        if ok != 0 {
            self.registered.push(id);
            return Ok(());
        }
        // SAFETY: no argument.
        match unsafe { GetLastError() } {
            ERROR_HOTKEY_ALREADY_REGISTERED => Err(RegisterError::Taken),
            code => Err(RegisterError::Failed(format!(
                "RegisterHotKey error {code}"
            ))),
        }
    }

    fn unregister(&mut self, id: u32) {
        if let Some(index) = self.registered.iter().position(|r| *r == id) {
            self.registered.remove(index);
            // SAFETY: a registration of this window.
            unsafe { UnregisterHotKey(self.hwnd, id as i32) };
        }
    }
}

impl Drop for WindowsRegistrar {
    fn drop(&mut self) {
        // SAFETY: the handles came from `new` and are released once; the
        // window goes first so the wndproc never reads a freed state.
        unsafe {
            for id in self.registered.drain(..) {
                UnregisterHotKey(self.hwnd, id as i32);
            }
            if !self.hwnd.is_null() {
                DestroyWindow(self.hwnd);
            }
            drop(Box::from_raw(self.deliver));
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_HOTKEY {
        let deliver = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Deliver;
        if !deliver.is_null() {
            ((*deliver).as_ref())(wparam as u32);
        }
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Either Windows key is down now.
pub fn super_held() -> bool {
    // SAFETY: no pointer; the high bit is the key's current state.
    unsafe { GetAsyncKeyState(VK_LWIN as i32) < 0 || GetAsyncKeyState(VK_RWIN as i32) < 0 }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A shared counter as a [`Deliver`], for the tests.
#[cfg(test)]
fn recorder() -> (Deliver, std::sync::Arc<std::sync::Mutex<Vec<u32>>>) {
    use std::sync::Arc;
    let fired = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&fired);
    let deliver: Deliver = Arc::new(move |id| sink.lock().unwrap().push(id));
    (deliver, fired)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, PostMessageW, MSG, PM_REMOVE,
    };

    /// A combination nobody binds: no keyboard has F24. Not with the
    /// Windows key: Ctrl+Alt+Shift+Win is the Office key.
    fn rare() -> Combo {
        Combo::parse("Ctrl+Alt+Shift+F24").unwrap()
    }

    fn pump() {
        // SAFETY: a zeroed MSG filled by the call, dispatched on this thread.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                DispatchMessageW(&msg);
            }
        }
    }

    #[test]
    fn a_second_registration_of_one_combination_is_taken() {
        let (deliver, _) = recorder();
        let mut first = WindowsRegistrar::new(Arc::clone(&deliver));
        let mut second = WindowsRegistrar::new(deliver);
        assert_eq!(first.register(7, &rare()), Ok(()));
        assert_eq!(second.register(8, &rare()), Err(RegisterError::Taken));
        first.unregister(7);
        assert_eq!(second.register(8, &rare()), Ok(()));
    }

    #[test]
    fn wm_hotkey_delivers_the_registration_id() {
        let (deliver, fired) = recorder();
        let registrar = WindowsRegistrar::new(deliver);
        // SAFETY: a window of this thread; the message carries only the id.
        unsafe { PostMessageW(registrar.hwnd, WM_HOTKEY, 42, 0) };
        pump();
        assert_eq!(*fired.lock().unwrap(), vec![42]);
    }

    /// The real path: keys injected with `SendInput` reach the OS hotkey and
    /// come back as `WM_HOTKEY`. Ignored by default since it types on the
    /// desktop: `cargo test -p accshift-egui -- --ignored hotkey_from_input`.
    #[test]
    #[ignore]
    fn hotkey_from_input() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
            VK_MENU, VK_SHIFT,
        };
        let (deliver, fired) = recorder();
        let mut registrar = WindowsRegistrar::new(deliver);
        let combo = Combo::parse("Ctrl+Alt+Shift+F23").unwrap();
        registrar.register(5, &combo).expect("register");
        let key = |vk: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let f23 = win_vk(combo.key) as u16;
        let inputs = [
            key(VK_CONTROL, false),
            key(VK_MENU, false),
            key(VK_SHIFT, false),
            key(f23, false),
            key(f23, true),
            key(VK_SHIFT, true),
            key(VK_MENU, true),
            key(VK_CONTROL, true),
        ];
        // SAFETY: an array of initialised inputs and its length.
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        let start = std::time::Instant::now();
        while fired.lock().unwrap().is_empty() && start.elapsed().as_secs() < 2 {
            pump();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(*fired.lock().unwrap(), vec![5]);
    }
}
