// Keep this to hide the extra console window in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(all(windows, not(debug_assertions)))]
mod launcher;

#[cfg(all(windows, not(debug_assertions)))]
fn main() {
    launcher::main();
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn main() {
    accshift_gui_lib::run();
}
