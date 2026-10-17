//! The secure screen as `App.svelte` feeds it (`useSecureScreen.svelte.ts`):
//! activity, window state and the settings in, its events out. Streamer
//! mode (`useStreamerMode.svelte.ts`) too.

use std::time::Instant;

use accshift_app::lifecycle::updater::APP_VERSION;
use accshift_app::secure::screen::{persist_pin_hash, SecureEvent, SecureInputs, WindowView};
use egui::{Context, Event};

use super::AppModel;

impl AppModel {
    /// One frame of streamer mode, before drawing. App.svelte starts the
    /// poll on `boot-ready`: here, the first frame with the boot payload.
    pub(super) fn tick_streamer(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let now = Instant::now();
        // `enabled` derives from the settings.
        b.streamer.sync_settings(b.stores.settings());
        if !b.streamer.is_running() {
            b.streamer.start(&self.jobs, now);
        }
        // `isHidden`: minimized, which also hides the page.
        let hidden = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        b.streamer.tick(&self.jobs, now, hidden);
        if let Some(due) = b.streamer.next_deadline() {
            ctx.request_repaint_after(due.saturating_duration_since(now));
        }
    }

    /// One frame of the secure screen, before drawing.
    pub(super) fn tick_secure(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let now = Instant::now();
        // The blur's `mousemove`, `mousedown` and `keydown` listeners.
        ctx.input(|i| {
            for event in &i.events {
                match event {
                    Event::PointerMoved(_) => b.secure.on_pointer_move(now),
                    Event::PointerButton { pressed: true, .. } => b.secure.on_mouse_down(now),
                    Event::Key { pressed: true, .. } => b.secure.on_key_down(now),
                    _ => {}
                }
            }
        });
        // `handleSettingsClosed`, whatever closed the panel.
        let in_settings = b.nav.show_settings;
        if b.secure_in_settings && !in_settings {
            b.secure.handle_settings_closed(now);
        }
        b.secure_in_settings = in_settings;
        let (minimized, focused) = ctx.input(|i| {
            let vp = i.viewport();
            (vp.minimized.unwrap_or(false), vp.focused.unwrap_or(true))
        });
        let selection = !in_settings && b.accounts.registry.adapter(&b.nav.active_tab).is_some();
        let inputs = SecureInputs {
            settings: b.stores.settings(),
            is_account_selection_view: selection,
            window: WindowView {
                foreground: focused,
                minimized,
                page_visible: !minimized,
            },
            app_version: APP_VERSION,
        };
        let events = b.secure.update(&inputs, &self.jobs, now);
        if !b.secure_mounted {
            b.secure.handle_app_mounted(&self.jobs, now);
            b.secure_mounted = true;
        }
        if let Some(due) = b.secure.next_deadline() {
            ctx.request_repaint_after(due.saturating_duration_since(now));
        }
        for event in events {
            match event {
                SecureEvent::CloseContextMenu => self.close_context_menu(ctx),
                SecureEvent::FocusPinInput => self.pin_focus = true,
                SecureEvent::PersistPinHash(hash) => {
                    if let Some(b) = self.booted.as_mut() {
                        persist_pin_hash(&mut b.stores, &hash);
                    }
                }
            }
        }
    }
}
