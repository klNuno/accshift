//! Platform-specific window, monitor, resize-edge, and keyboard helpers for the software runner.

use std::time::Duration;

use egui_winit::winit;
use winit::event::WindowEvent;
use winit::monitor::MonitorHandle;
use winit::window::Window;

use crate::bench;

use super::Live;

/// `show_main_window`: show, then `set_focus` for a normal run.
pub(super) fn show_window(window: &Window, focus: bool) {
    window.set_visible(true);
    bench::mark("visible");
    if focus {
        window.focus_window();
    }
    bench::mark("shown");
}

/// The winit window's geometry: inner size and outer origin. Wayland has no
/// origin to give: the size goes alone.
pub(super) fn geometry(window: &Window) -> Option<crate::window::Geometry> {
    let outer = window.outer_position().ok();
    let inner = window.inner_size();
    crate::window::Geometry::read(
        window.is_maximized(),
        window.is_minimized().unwrap_or(false),
        window.scale_factor(),
        (inner.width, inner.height),
        outer.map(|outer| (outer.x, outer.y)),
    )
}

/// One frame at the refresh rate of `monitor`.
pub(super) fn frame_interval(monitor: Option<&MonitorHandle>) -> Option<Duration> {
    let mhz = monitor?.refresh_rate_millihertz().filter(|&mhz| mhz > 0)?;
    Some(Duration::from_secs_f64(1000.0 / f64::from(mhz)))
}

/// The resize edge under the pointer, when the window can be resized.
#[cfg(target_os = "linux")]
pub(super) fn resize_edge(live: &Live) -> Option<crate::window::edges::Edge> {
    use crate::window::edges;
    let window = &live.window;
    if window.is_maximized() || window.fullscreen().is_some() || !window.is_resizable() {
        return None;
    }
    let size = window.inner_size();
    edges::hit(
        live.pointer?,
        (f64::from(size.width), f64::from(size.height)),
        edges::BAND * window.scale_factor(),
    )
}

/// Tracks the pointer for the resize edges and hands a press on one to the
/// window manager's interactive resize. True when egui must not see the
/// event.
#[cfg(target_os = "linux")]
pub(super) fn edge_press(live: &mut Live, event: &WindowEvent) -> bool {
    use crate::window::edges::Edge;
    use winit::event::{ElementState, MouseButton};
    use winit::window::ResizeDirection;
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            live.pointer = Some((position.x, position.y));
        }
        WindowEvent::CursorLeft { .. } => live.pointer = None,
        WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: MouseButton::Left,
            ..
        } => {
            let Some(edge) = resize_edge(live) else {
                return false;
            };
            let direction = match edge {
                Edge::North => ResizeDirection::North,
                Edge::South => ResizeDirection::South,
                Edge::East => ResizeDirection::East,
                Edge::West => ResizeDirection::West,
                Edge::NorthEast => ResizeDirection::NorthEast,
                Edge::NorthWest => ResizeDirection::NorthWest,
                Edge::SouthEast => ResizeDirection::SouthEast,
                Edge::SouthWest => ResizeDirection::SouthWest,
            };
            return live.window.drag_resize_window(direction).is_ok();
        }
        _ => {}
    }
    false
}

/// Whether Windows has part of the client area marked for repaint: it does
/// when it shows, restores or otherwise invalidates the window, and then it
/// expects all of it back. softbuffer validates the whole window on every
/// present, so a damaged-tiles present would swallow that request and leave
/// stale or blank areas until the next full present.
#[cfg(windows)]
pub(super) fn os_wants_full_paint(window: &Window) -> bool {
    let Some(hwnd) = hwnd(window) else {
        return false;
    };
    // SAFETY: live HWND owned by this runner; a null rect only asks whether
    // the update region is empty.
    unsafe {
        windows_sys::Win32::Graphics::Gdi::GetUpdateRect(hwnd as _, std::ptr::null_mut(), 0) != 0
    }
}

pub(super) fn hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(not(windows))]
pub(super) fn os_wants_full_paint(_window: &Window) -> bool {
    false
}

/// [`crate::window::frame::take_content_lost`].
#[cfg(windows)]
pub(super) fn take_content_lost() -> bool {
    crate::window::frame::take_content_lost()
}

#[cfg(not(windows))]
pub(super) fn take_content_lost() -> bool {
    false
}

/// egui-winit drops the context menu key, egui having none: it goes in as
/// [`crate::CONTEXT_MENU_KEY`].
pub(super) fn context_menu_key(live: &mut Live, event: &WindowEvent) -> bool {
    use winit::keyboard::{Key, NamedKey};
    let key = match event {
        WindowEvent::ModifiersChanged(held) => {
            // As egui-winit reads them.
            let held = held.state();
            live.held = held;
            let mac = cfg!(target_os = "macos");
            live.modifiers = egui::Modifiers {
                alt: held.alt_key(),
                ctrl: held.control_key(),
                shift: held.shift_key(),
                mac_cmd: mac && held.super_key(),
                command: if mac {
                    held.super_key()
                } else {
                    held.control_key()
                },
            };
            return false;
        }
        WindowEvent::KeyboardInput { event: key, .. } => key,
        _ => return false,
    };
    if key.logical_key != Key::Named(NamedKey::ContextMenu) {
        return false;
    }
    let modifiers = live.modifiers;
    live.state.egui_input_mut().events.push(egui::Event::Key {
        key: crate::CONTEXT_MENU_KEY,
        physical_key: Some(crate::CONTEXT_MENU_KEY),
        pressed: key.state.is_pressed(),
        repeat: key.repeat,
        modifiers,
    });
    true
}

pub(super) fn is_shortcut_press(pressed: bool, repeat: bool, synthetic: bool) -> bool {
    pressed && !repeat && !synthetic
}
