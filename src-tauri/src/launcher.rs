//! Windows release entry point. The app itself lives in accshift_gui_lib.dll;
//! this exe starts WebView2, then loads the DLL and hands the start over.
//!
//! Smart App Control validates every image before it runs, at a cost that
//! grows with the file size and with the age of its verdict: a 9 MB exe paid
//! up to ~120 ms before main, and WebView2's browser process, the longest step
//! of startup, could not be asked for until then. This exe is small, asks for
//! the browser process first, and the DLL's validation then runs while that
//! process starts.

use windows::core::{s, HSTRING};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

const APP_DLL: &str = "accshift_gui_lib.dll";

static SLOT: wry::PrestartSlot = wry::PrestartSlot::new();

type RunFn = unsafe extern "C" fn(*const wry::PrestartHandoff) -> i32;

pub fn main() {
    // Same directory, options and transparency as the main window built in
    // boot.rs; the DLL builds it the usual way when they ever differ. A
    // failure here (no WebView2 runtime, say) leaves the DLL to report it.
    let handoff = dirs::data_local_dir().and_then(|dir| {
        wry::webview2_prestart(&dir.join(env!("ACCSHIFT_IDENTIFIER")), true, &SLOT).ok()
    });

    let dll = match std::env::current_exe() {
        Ok(exe) => exe.with_file_name(APP_DLL),
        Err(error) => fail(&format!("Accshift cannot locate itself: {error}")),
    };
    // On this thread: tao takes the thread that loads it for the main thread,
    // and builds its event loop only there.
    let module = match unsafe {
        LoadLibraryExW(
            &HSTRING::from(dll.as_path()),
            None,
            LOAD_WITH_ALTERED_SEARCH_PATH,
        )
    } {
        Ok(module) => module,
        Err(error) => fail(&format!(
            "Accshift cannot load {}: {error}\n\nReinstalling Accshift should fix it.",
            dll.display()
        )),
    };
    let Some(entry) = (unsafe { GetProcAddress(module, s!("accshift_run_v1")) }) else {
        fail(&format!(
            "{} does not match this accshift-gui.exe.\n\nReinstalling Accshift should fix it.",
            dll.display()
        ));
    };
    let run: RunFn = unsafe { std::mem::transmute(entry) };
    let handoff = handoff.as_ref().map_or(std::ptr::null(), |handoff| {
        handoff as *const wry::PrestartHandoff
    });
    std::process::exit(unsafe { run(handoff) });
}

fn fail(message: &str) -> ! {
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from("Accshift"),
            MB_ICONERROR | MB_OK,
        );
    }
    std::process::exit(1);
}
