use std::sync::atomic::{AtomicBool, Ordering};

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
    let Some(main_window) = app_handle.get_webview_window("main") else {
        return Err("Main window is unavailable".to_string());
    };

    main_window.show().map_err(|reason| reason.to_string())?;
    let _ = main_window.set_focus();
    Ok(())
}
