//! Bulk edit: the mode's jobs, the paint gesture over the cards, and what
//! the bar and its settings dialog report.

use std::time::Instant;

use accshift_app::actions::card_actions;
use accshift_app::bulk_edit::BulkEditBar;
use accshift_app::folders::ItemKind;
use egui::{Context, Event, Pos2, Rect};

use super::AppModel;
use crate::ui::components::bulk_edit::{BulkEvent, COVER_ID};

/// A bar event that only changes the bar's own state.
fn apply_bar_event(bar: &mut BulkEditBar, event: BulkEvent) {
    match event {
        BulkEvent::ToggleColorMenu => bar.toggle_color_menu(),
        BulkEvent::Back => bar.back(),
        BulkEvent::CycleNewsPopup => bar.news_popup = bar.news_popup.cycle(),
        BulkEvent::CycleDoNotDisturb => {
            bar.do_not_disturb = bar.do_not_disturb.cycle();
        }
        BulkEvent::Game(app_id) => bar.selected_game_id = app_id,
        BulkEvent::LaunchInput(value) => bar.launch_option_input = value,
        BulkEvent::AddLaunchOption => bar.add_launch_option(),
        BulkEvent::RemoveLaunchOption(app_id) => bar.remove_launch_option(&app_id),
        BulkEvent::PointerDown(inside) => bar.pointer_down(inside),
        _ => {}
    }
}

impl AppModel {
    /// The title bar's bulk edit button: the mode on or off for the active
    /// tab's platform.
    pub(super) fn toggle_bulk_edit(&mut self) {
        if let Some(b) = self.booted.as_mut() {
            let def = b.accounts.registry.definition(&b.nav.active_tab);
            b.bulk.toggle(def);
        }
    }

    /// One frame of the bulk edit jobs: the games load and the apply. A
    /// finished apply closes the mode and toasts.
    pub(super) fn tick_bulk_edit(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        for outcome in b.bulk.poll() {
            self.toasts.push(outcome.toast(), Instant::now());
        }
    }

    /// The paint gesture: a primary press on an account card arms it, a move
    /// paints the card under the pointer, the release ends it. The card is
    /// read from the boxes the last frame drew; the bar and its popover
    /// cover the cards under them.
    pub(super) fn track_bulk_paint(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if !b.bulk.is_open() {
            return;
        }
        let cover: Vec<Rect> = ctx
            .data(|d| d.get_temp(egui::Id::unique(COVER_ID)))
            .unwrap_or_default();
        let hit = ctx.viewport(|vp| vp.hits.click.map(|w| w.id));
        let ws = &b.workspace;
        let card_at = |pos: Pos2| -> Option<String> {
            if cover.iter().any(|r| r.contains(pos)) {
                return None;
            }
            ws.cards
                .iter()
                .rev()
                .find(|(item, r)| item.kind == ItemKind::Account && r.contains(pos))
                .map(|(item, _)| item.id.clone())
        };
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    ..
                } => {
                    // The press must land on the workspace.
                    let on_workspace = hit.is_some_and(|id| ws.owns(id));
                    let card = on_workspace.then(|| card_at(pos)).flatten();
                    b.bulk.paint_mouse_down(
                        button == egui::PointerButton::Primary,
                        card.as_deref(),
                        (pos.x, pos.y),
                    );
                }
                Event::PointerMoved(pos) => {
                    b.bulk
                        .paint_mouse_move((pos.x, pos.y), card_at(pos).as_deref());
                }
                Event::PointerButton { pressed: false, .. } => b.bulk.paint_mouse_up(),
                _ => {}
            }
        }
    }

    /// What the bar and its dialog reported.
    pub(super) fn apply_bulk(&mut self, ctx: &Context, event: BulkEvent) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        match event {
            BulkEvent::SelectAll => {
                let visible = b.accounts.lists.visible_account_ids.clone();
                b.bulk.select_all(&visible);
            }
            BulkEvent::DeselectAll => b.bulk.deselect_all(),
            BulkEvent::CopyUrls => {
                let urls = BulkEditBar::copy_urls(b.bulk.selected_ids());
                let effects = card_actions::copy_bulk_edit_urls(&urls);
                self.apply_effects(ctx, effects);
            }
            BulkEvent::Color(color) => {
                let Some(color) = b.bulk.bar_mut().map(|bar| bar.apply_color(&color)) else {
                    return;
                };
                let ids = b.bulk.selected_ids().to_vec();
                let effects = card_actions::apply_bulk_edit_card_color(&mut b.stores, &ids, &color);
                self.apply_effects(ctx, effects);
            }
            BulkEvent::Close => b.bulk.close(),
            BulkEvent::Next => b.bulk.go_to_settings(&self.jobs),
            BulkEvent::Apply => b.bulk.apply(&self.jobs),
            BulkEvent::Escape => b.bulk.escape(),
            event => {
                if let Some(bar) = b.bulk.bar_mut() {
                    apply_bar_event(bar, event);
                }
            }
        }
    }
}
