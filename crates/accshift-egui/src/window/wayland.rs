//! Wayland: raising the window with an activation token. A Wayland client
//! cannot raise itself (winit's `focus_window` does nothing there, and a
//! minimized window cannot be brought back): the compositor raises the
//! surface an `xdg_activation_v1.activate` names, if the token comes from a
//! user action. The GlobalShortcuts portal hands one with each press.
//!
//! The request goes on winit's own Wayland connection, through a queue of
//! our own, so the surface is the one winit drew. The registry and the
//! xdg-activation proxy are made once, at the first raise, and kept for the
//! window's life: a registry per raise would stay with the compositor until
//! the connection closes.

use std::rc::Rc;

use egui_winit::winit::{event_loop::OwnedDisplayHandle, window::Window};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle};
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

/// The xdg-activation binding of one window, made at its first raise.
pub struct Raiser {
    // Drop the guest Wayland backend before the handles that keep its
    // foreign display and surface alive, including after run_app returns.
    activator: Option<Activator>,
    tried: bool,
    window: Rc<Window>,
    display: OwnedDisplayHandle,
}

impl Raiser {
    pub fn new(window: Rc<Window>, display: OwnedDisplayHandle) -> Self {
        Self {
            activator: None,
            tried: false,
            window,
            display,
        }
    }

    /// Asks the compositor to raise the window with `token`. `false` when
    /// the surface or xdg-activation is unavailable.
    pub fn raise(&mut self, token: &str) -> bool {
        if !self.tried {
            self.tried = true;
            self.activator = Activator::new(&self.window, &self.display);
        }
        self.activator
            .as_mut()
            .is_some_and(|activator| activator.activate(token))
    }
}

struct Activator {
    conn: Connection,
    queue: EventQueue<State>,
    _globals: GlobalList,
    activation: XdgActivationV1,
    surface: WlSurface,
}

impl Activator {
    fn new(window: &Window, display: &OwnedDisplayHandle) -> Option<Self> {
        let (
            RawDisplayHandle::Wayland(display),
            RawDisplayHandle::Wayland(window_display),
            RawWindowHandle::Wayland(handle),
        ) = (
            display.display_handle().ok()?.as_raw(),
            window.display_handle().ok()?.as_raw(),
            window.window_handle().ok()?.as_raw(),
        )
        else {
            return None;
        };
        if display.display != window_display.display {
            return None;
        }
        // SAFETY: Raiser keeps the owned display and its window until after
        // this guest backend and surface proxy are dropped.
        let conn = Connection::from_backend(unsafe {
            Backend::from_foreign_display(display.display.as_ptr().cast())
        });
        let surface =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) }
                .ok()?;
        let surface = WlSurface::from_id(&conn, surface).ok()?;
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let activation = globals
            .bind::<XdgActivationV1, _, _>(&queue.handle(), 1..=1, ())
            .ok()?;
        Some(Self {
            conn,
            queue,
            _globals: globals,
            activation,
            surface,
        })
    }

    fn activate(&mut self, token: &str) -> bool {
        // Registry events since the last raise: drained, not kept.
        let _ = self.queue.dispatch_pending(&mut State);
        self.activation.activate(token.to_string(), &self.surface);
        self.conn.flush().is_ok()
    }
}

impl Drop for Activator {
    fn drop(&mut self) {
        self.activation.destroy();
        let _ = self.conn.flush();
    }
}
