//! Wayland: raising the window with an activation token. A Wayland client
//! cannot raise itself (winit's `focus_window` does nothing there, and a
//! minimized window cannot be brought back): the compositor raises the
//! surface an `xdg_activation_v1.activate` names, if the token comes from a
//! user action. The GlobalShortcuts portal hands one with each press.
//!
//! The request goes on winit's own Wayland connection, through a queue of
//! our own, so the surface is the one winit drew.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

/// The window draws on a Wayland surface.
pub fn is_wayland(window: &impl HasDisplayHandle) -> bool {
    matches!(
        window.display_handle().map(|display| display.as_raw()),
        Ok(RawDisplayHandle::Wayland(_))
    )
}

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: XdgActivationV1);

/// Asks the compositor to raise `window` with `token`. `false` when the
/// window is not a Wayland surface or the compositor lacks xdg-activation.
pub fn activate(window: &(impl HasDisplayHandle + HasWindowHandle), token: &str) -> bool {
    let (Ok(display), Ok(handle)) = (window.display_handle(), window.window_handle()) else {
        return false;
    };
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) =
        (display.as_raw(), handle.as_raw())
    else {
        return false;
    };
    // SAFETY: winit's display and surface outlive this call, which keeps no
    // reference past its end.
    let conn = Connection::from_backend(unsafe {
        Backend::from_foreign_display(display.display.as_ptr().cast())
    });
    let Ok(surface) =
        (unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) })
    else {
        return false;
    };
    let Ok(surface) = WlSurface::from_id(&conn, surface) else {
        return false;
    };
    let Ok((globals, queue)) = registry_queue_init::<State>(&conn) else {
        return false;
    };
    let Ok(activation) = globals.bind::<XdgActivationV1, _, _>(&queue.handle(), 1..=1, ()) else {
        return false;
    };
    activation.activate(token.to_string(), &surface);
    activation.destroy();
    conn.flush().is_ok()
}
