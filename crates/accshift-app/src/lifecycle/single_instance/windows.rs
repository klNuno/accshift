//! platform_impl/windows.rs of `tauri-plugin-single-instance` 2.4.4.
//!
//! Upstream creates the mutex and the message window on Tauri's main thread,
//! whose event loop pumps the window's messages. The egui shell owns no
//! Win32 loop it can lend, so here one dedicated thread creates the mutex
//! (it must own it to release it), creates the same window and pumps it.
//! Names, styles, the `WM_COPYDATA` payload and the release order on exit
//! are upstream's.

use super::{parse_windows_payload, windows_names, windows_payload, Notify, PlatformLaunch};
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW,
    GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageW,
    SetWindowLongPtrW, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, GWL_STYLE, MSG, WM_APP,
    WM_COPYDATA, WM_CREATE, WM_DESTROY, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
};

/// windows.rs line 41.
const WMCOPYDATA_SINGLE_INSTANCE_DATA: usize = 1542;

/// Ours: asks the window thread to run `destroy` (release the mutex, then
/// destroy the window, upstream's order).
const WM_SINGLE_INSTANCE_DESTROY: u32 = WM_APP + 0x1542;

struct UserData {
    notify: Notify,
    mutex: HANDLE,
}

/// The running service: the window thread and its window.
pub struct Guard {
    hwnd: isize,
    /// A handle kept open for the run when upstream leaks it (mutex found,
    /// window not found).
    leaked_mutex: isize,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.hwnd != 0 {
            // SAFETY: posting to a window this guard created; a dead handle
            // only makes the call fail.
            unsafe { PostMessageW(self.hwnd as HWND, WM_SINGLE_INSTANCE_DESTROY, 0, 0) };
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if self.leaked_mutex != 0 {
            // SAFETY: a handle this guard owns, closed once.
            unsafe { CloseHandle(self.leaked_mutex as HANDLE) };
        }
    }
}

enum ThreadAnswer {
    Primary(isize),
    Forwarded,
    Alone(isize),
}

fn encode_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(super) fn acquire(
    identifier: &str,
    args: &[String],
    cwd: &str,
    notify: Notify,
) -> PlatformLaunch {
    let (class, window, mutex) = windows_names(identifier);
    let payload = windows_payload(args, cwd);
    let (answer_tx, answer_rx) = mpsc::sync_channel::<ThreadAnswer>(1);
    let thread = std::thread::Builder::new()
        .name("accshift-single-instance".into())
        .spawn(move || run(class, window, mutex, payload, notify, answer_tx));
    let Ok(thread) = thread else {
        return PlatformLaunch::Alone(None);
    };
    match answer_rx.recv() {
        Ok(ThreadAnswer::Primary(hwnd)) => PlatformLaunch::Primary(Guard {
            hwnd,
            leaked_mutex: 0,
            thread: Some(thread),
        }),
        Ok(ThreadAnswer::Forwarded) => {
            let _ = thread.join();
            PlatformLaunch::Forwarded
        }
        Ok(ThreadAnswer::Alone(mutex)) => {
            let _ = thread.join();
            PlatformLaunch::Alone(Some(Guard {
                hwnd: 0,
                leaked_mutex: mutex,
                thread: None,
            }))
        }
        Err(_) => PlatformLaunch::Alone(None),
    }
}

/// windows.rs lines 54-108 (`setup`), then the message loop.
fn run(
    class: String,
    window: String,
    mutex: String,
    payload: String,
    notify: Notify,
    answer: mpsc::SyncSender<ThreadAnswer>,
) {
    let class_name = encode_wide(&class);
    let window_name = encode_wide(&window);
    let mutex_name = encode_wide(&mutex);
    // SAFETY: plain Win32 calls on NUL-terminated buffers that outlive them.
    unsafe {
        let hmutex = CreateMutexW(std::ptr::null(), 1, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let hwnd = FindWindowW(class_name.as_ptr(), window_name.as_ptr());
            if !hwnd.is_null() {
                let bytes = payload.as_bytes();
                let cds = COPYDATASTRUCT {
                    dwData: WMCOPYDATA_SINGLE_INSTANCE_DATA,
                    cbData: bytes.len() as _,
                    lpData: bytes.as_ptr() as _,
                };
                SendMessageW(hwnd, WM_COPYDATA, 0, &cds as *const _ as LPARAM);
                // Upstream exits right here, which closes the handle.
                CloseHandle(hmutex);
                let _ = answer.send(ThreadAnswer::Forwarded);
                return;
            }
            let _ = answer.send(ThreadAnswer::Alone(hmutex as isize));
            return;
        }

        let userdata = Box::into_raw(Box::new(UserData {
            notify,
            mutex: hmutex,
        }));
        let hwnd = create_event_target_window(&class_name, &window_name, userdata);
        if hwnd.is_null() {
            drop(Box::from_raw(userdata));
            ReleaseMutex(hmutex);
            CloseHandle(hmutex);
            let _ = answer.send(ThreadAnswer::Alone(0));
            return;
        }
        let _ = answer.send(ThreadAnswer::Primary(hwnd as isize));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// windows.rs lines 119-130 (`destroy`), on the thread that owns the mutex.
unsafe fn destroy(hwnd: HWND) {
    let userdata = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut UserData;
    if !userdata.is_null() && !(*userdata).mutex.is_null() {
        ReleaseMutex((*userdata).mutex);
        CloseHandle((*userdata).mutex);
        (*userdata).mutex = std::ptr::null_mut();
    }
    DestroyWindow(hwnd);
}

/// windows.rs lines 132-167, plus the destroy request and the end of the
/// loop this thread runs.
unsafe extern "system" fn single_instance_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create_struct.lpCreateParams as isize);
            0
        }
        WM_COPYDATA => {
            let cds = &*(lparam as *const COPYDATASTRUCT);
            if cds.dwData == WMCOPYDATA_SINGLE_INSTANCE_DATA {
                let userdata = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const UserData;
                if !userdata.is_null() && !cds.lpData.is_null() {
                    let data =
                        std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize);
                    let (cwd, args) = parse_windows_payload(data);
                    (*userdata).notify.deliver(args, cwd);
                }
            }
            1
        }
        WM_SINGLE_INSTANCE_DESTROY => {
            destroy(hwnd);
            0
        }
        WM_DESTROY => {
            let userdata = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut UserData;
            if !userdata.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let userdata = Box::from_raw(userdata);
                // A window closed some other way still frees the mutex.
                if !userdata.mutex.is_null() {
                    ReleaseMutex(userdata.mutex);
                    CloseHandle(userdata.mutex);
                }
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// windows.rs lines 169-208: a 0x0 layered, transparent, non-activating
/// tool window, restyled to `WS_VISIBLE | WS_POPUP` like upstream.
unsafe fn create_event_target_window(
    class_name: &[u16],
    window_name: &[u16],
    userdata: *mut UserData,
) -> HWND {
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(single_instance_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(std::ptr::null()),
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    // Upstream ignores the result too: a class left registered by an
    // earlier instance in the same process is reused.
    RegisterClassExW(&class);
    let hwnd = CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
        class_name.as_ptr(),
        window_name.as_ptr(),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        userdata as *const _,
    );
    if !hwnd.is_null() {
        SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_VISIBLE | WS_POPUP) as isize);
    }
    hwnd
}
