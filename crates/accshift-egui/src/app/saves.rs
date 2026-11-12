//! Store writes: the debounced saves through the job pool each frame, and
//! the blocking flush on close.

use std::time::Instant;

use accshift_app::jobs::Poll;
use egui::Context;

use super::AppModel;

impl AppModel {
    /// The app state flushed on close, after the UI scale's own teardown:
    /// the pending UI scale, the settings panel's debounced save, the saves
    /// in flight, then every store write still queued, blocking. The first
    /// error wins.
    pub fn flush_app_state(&mut self) -> Result<(), String> {
        self.flush_ui_scale();
        self.flush_settings(Instant::now());
        let Some(b) = self.booted.as_mut() else {
            return Ok(());
        };
        let mut first_error = None;
        for (id, pending) in self.saves.drain(..) {
            if let Some(result) = pending.wait() {
                if let Err(e) = &result {
                    first_error.get_or_insert_with(|| e.message.clone());
                }
                b.stores.finish_save(id, &result);
            }
        }
        for save in b.stores.take_all_saves() {
            let result = self
                .backend
                .save_client_storage_store(save.store_id, &save.value);
            if let Err(e) = &result {
                first_error.get_or_insert_with(|| e.message.clone());
            }
            b.stores.finish_save(save.store_id, &result);
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Writes the UI scale a shortcut changed and the debounce has not saved
    /// yet into the settings.
    fn flush_ui_scale(&mut self) {
        let live = self.ui_scale_live;
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let stored = b.stores.settings().ui_scale_percent;
        let Some(value) = self.ui_scale.destroy(live.unwrap_or(stored)) else {
            return;
        };
        if stored == value {
            return;
        }
        let mut next = b.stores.settings().clone();
        next.ui_scale_percent = value;
        b.stores.save_settings(&next);
    }

    /// Saves the stores asked for, through the job pool, and wakes the UI
    /// for the next debounced write: without input nothing else draws a frame
    /// at its deadline, and the change would wait for the next mouse move.
    pub(super) fn pump_saves(&mut self, ctx: &Context) {
        let panel_deadline = self.settings_panel_deadline();
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let now = Instant::now();
        let next = [b.stores.next_save_due(), panel_deadline]
            .into_iter()
            .flatten()
            .filter(|due| *due > now)
            .min();
        if let Some(due) = next {
            ctx.request_repaint_after(due.saturating_duration_since(now));
        }
        for save in b.stores.take_due_saves(now) {
            let store_id = save.store_id;
            let pending = self
                .jobs
                .run(move |backend| backend.save_client_storage_store(store_id, &save.value));
            self.saves.push((store_id, pending));
        }
        self.saves.retain_mut(|(id, pending)| match pending.poll() {
            Poll::Waiting => true,
            Poll::Ready(result) => {
                b.stores.finish_save(id, &result);
                false
            }
            Poll::Gone => false,
        });
    }
}
