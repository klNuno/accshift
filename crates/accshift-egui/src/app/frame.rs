//! One frame of the app: input read, jobs polled, controllers ticked, keys
//! dispatched, the shell drawn, then the actions it returned applied. The
//! order of these phases is the app's behaviour: keep it.

use std::time::{Duration, Instant};

use accshift_app::jobs::Poll;
use egui::{Context, Event, Ui};

use super::{shell, AppModel};

impl AppModel {
    /// One frame.
    pub fn frame(&mut self, ui: &mut Ui) {
        let _span = crate::bench::span("app");
        let ctx = ui.ctx().clone();
        // Focus moves on a press: a press outside the focused widget blurs
        // it, and a click on a label then focuses its control on the
        // release (egui's default waits for the click). The zoom shortcuts
        // go through the app's binding table, not egui's own.
        ctx.options_mut(|o| {
            o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses;
            o.zoom_with_keyboard = false;
        });
        let now = ctx.input(|i| i.time);
        self.last_time = now;
        self.poll_boot(&ctx);
        self.tick_external_refresh(&ctx);
        self.sync_settings(&ctx);
        self.tick_ui_scale(&ctx, Instant::now());
        self.tick_secure(&ctx);
        self.tick_streamer(&ctx);
        self.tick_settings_panel(&ctx, Instant::now());
        self.tick_theme_editor(Instant::now());
        self.tick_theme_import(&ctx, Instant::now());
        self.pump_saves(&ctx);
        self.track_focus_modality(&ctx);
        self.pump_submenus();
        self.pump_tasks(&ctx);
        self.tick_onboarding(&ctx, now);
        self.tick_bulk_edit();
        self.sync_grid_modes();
        // No transform scope outlives the frame that opened it.
        crate::ui::text::set_transform(&ctx, None);
        crate::ui::components::track_held(&ctx);
        self.track_drag(&ctx);
        self.update_accounts(&ctx);
        self.tick_personas(&ctx);
        self.tick_deep_links(&ctx);
        self.tick_updates(&ctx);
        self.tick_toasts(&ctx);
        // The window-level key handling, before any widget reads keys.
        self.track_card_focus(&ctx);
        self.dispatch_keys(&ctx);
        self.dispatch_mouse_back(&ctx);
        self.mount_dialogs();
        self.track_bulk_paint(&ctx);
        let actions = {
            let _span = crate::bench::span("shell");
            shell::draw(self, ui, now)
        };
        self.apply(&ctx, actions);
        self.open_field_menu(&ctx);
        self.mark_frame(&ctx);
    }

    /// Lands the boot job once it is done.
    fn poll_boot(&mut self, ctx: &Context) {
        let Some(job) = &mut self.boot_job else {
            return;
        };
        match job.poll() {
            Poll::Waiting => {}
            Poll::Ready(result) => {
                self.boot_job = None;
                let _span = crate::bench::span("finish_boot");
                self.finish_boot(ctx, result);
            }
            Poll::Gone => {
                self.boot_job = None;
                self.boot_error = Some("boot job panicked".into());
            }
        }
    }

    /// The keyboard modality focus rings follow: a key without Ctrl, Alt or
    /// Cmd sets it; a press of any button that focuses a widget clears it
    /// (mouse focus), a press on nothing focusable (the background) leaves
    /// it.
    fn track_focus_modality(&mut self, ctx: &Context) {
        let on_focusable = ctx.viewport(|vp| {
            vp.hits
                .contains_pointer
                .iter()
                .any(|w| w.sense.senses_click() || w.sense.is_focusable())
        });
        let modality = ctx.input(|i| {
            i.events.iter().fold(None, |m, e| match e {
                Event::Key {
                    pressed: true,
                    modifiers,
                    ..
                } if !(modifiers.ctrl || modifiers.alt || modifiers.mac_cmd) => Some(true),
                Event::PointerButton { pressed: true, .. } if on_focusable => Some(false),
                _ => m,
            })
        });
        if let Some(keyboard) = modality {
            self.focus_visible = keyboard;
        }
    }

    /// The onboarding's timers, on the active tab.
    fn tick_onboarding(&mut self, ctx: &Context, now: f64) {
        let Some(tab) = self.booted.as_ref().map(|b| b.nav.active_tab.clone()) else {
            return;
        };
        let (_, effects) = self.onboarding.tick(&self.jobs, now, &tab);
        self.apply_effects(ctx, effects);
    }

    /// What the grid shows besides the accounts: the tour's fake accounts,
    /// which replace the grid while it runs, and the bulk edit selection.
    fn sync_grid_modes(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.accounts.tour = self.onboarding.mock_grid(b.locale);
        b.accounts.bulk_selection = b
            .bulk
            .is_open()
            .then(|| b.bulk.selected_ids().iter().cloned().collect());
    }

    /// One frame of the accounts: loads, switches, the add flow and avatar
    /// priming, then what their outcomes ask of the app.
    fn update_accounts(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        // Priming waits for a grid the user can see, in the foreground
        // window and not behind the privacy screen.
        let foreground = ctx.input(|i| {
            let vp = i.viewport();
            vp.focused.unwrap_or(true) && !vp.minimized.unwrap_or(false)
        });
        let grid_hidden = b.nav.show_settings
            || b.nav.show_personas
            || !foreground
            || b.secure.render_suspended();
        let expanded = b.stores.settings().account_display.expanded_folders;
        let _span = crate::bench::span("accounts");
        let (events, flow_events) = b.accounts.update(
            ctx,
            &self.jobs,
            &mut b.stores,
            &b.nav.active_tab,
            expanded,
            grid_hidden,
            b.locale,
            Instant::now(),
        );
        self.apply_loading_events(ctx, events);
        self.apply_add_flow_events(flow_events);
    }

    /// Expired toasts go before drawing, and the next expiry wakes the UI.
    fn tick_toasts(&mut self, ctx: &Context) {
        let instant = Instant::now();
        self.toasts.tick(instant);
        if let Some(deadline) = self.toasts.next_deadline() {
            ctx.request_repaint_after(deadline.saturating_duration_since(instant));
        }
    }

    /// The startup bench's marks: the first frames, the boot steps, then
    /// the frame the app is ready to be photographed.
    fn mark_frame(&mut self, ctx: &Context) {
        match self.frames {
            0 => {
                crate::bench::mark("ui_done");
                ctx.request_repaint();
            }
            // Frame 0 has been painted and swapped by now.
            1 => crate::bench::mark("first_frame"),
            _ => {}
        }
        self.frames += 1;
        if crate::bench::enabled() {
            self.boot_marks();
        }
        if self.ready_marked {
            return;
        }
        if self.ready() {
            crate::bench::mark("ready");
            crate::bench::request_shot(ctx);
            self.ready_marked = true;
        } else if cfg!(feature = "startup-bench") {
            // A settle deadline (the last fade's end) asks for no frame of
            // its own when nothing on screen moves: look again for the mark.
            ctx.request_repaint_after(Duration::from_millis(8));
        }
    }

    /// The boot's steps for the startup bench, each the first frame it holds.
    fn boot_marks(&mut self) {
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let steps = [
            ("boot_payload", true),
            ("accounts", b.accounts.settled()),
            (
                "avatars",
                b.accounts.settled() && !b.accounts.avatars.busy(),
            ),
        ];
        for (bit, (name, done)) in steps.into_iter().enumerate() {
            if done && self.boot_marks & (1 << bit) == 0 {
                self.boot_marks |= 1 << bit;
                crate::bench::mark(name);
            }
        }
    }
}
