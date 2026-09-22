use std::sync::atomic::{AtomicBool, Ordering};

// Only used by the real show path, which the bench build compiles out.
#[cfg_attr(feature = "startup-bench", allow(unused_imports))]
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

    #[cfg(not(feature = "startup-bench"))]
    {
        let Some(main_window) = app_handle.get_webview_window("main") else {
            return Err("Main window is unavailable".to_string());
        };

        main_window.show().map_err(|reason| reason.to_string())?;
        let _ = main_window.set_focus();
        Ok(())
    }
}
