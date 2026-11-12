//! What the frame's components asked for, applied after drawing.

use std::time::Instant;

use egui::{Context, ViewportCommand};

use super::{platforms, Action, AppModel};
use crate::ui::components::command_palette::PaletteEvent;
use crate::ui::components::secure_overlays::{PinEvent, StreamerEvent};
use crate::ui::components::toast::ToastEvent;

impl AppModel {
    /// Applies what the frame's components asked for.
    pub fn apply(&mut self, ctx: &Context, actions: Vec<Action>) {
        for action in actions {
            self.apply_one(ctx, action);
        }
        self.mount_dialogs();
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        // An unclaimed click swallow ends with the frame.
        b.bulk.end_frame();
        b.accounts.drag.end_frame();
        crate::ui::components::set_click_eaten(ctx, false);
        let step = b.bulk.bar().map(|bar| bar.step());
        if step != b.bulk_key {
            b.bulk_key = step;
            b.bulk_mount += 1;
        }
        let key = b.nav.panel_key();
        if key != b.panel {
            b.panel = key;
            // A new panel mounts the workspace and its sections afresh.
            b.accounts.pipeline.clear_collapsed_sections();
        }
    }

    pub(super) fn apply_one(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Action::ToggleMaximize => {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            }
            Action::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
            Action::StartDrag => ctx.send_viewport_cmd(ViewportCommand::StartDrag),
            Action::SetMaximizeButtonRect(rect) => {
                // The window shell answers HTMAXBUTTON over this rect so
                // Windows 11 opens the Snap Layouts flyout.
                self.maximize_button = rect;
            }
            Action::Refresh => self.refresh_accounts(),
            Action::AccountSwitch(id) => self.account_switch(&id),
            Action::AccountActivate(id) => self.account_activate(&id),
            Action::AddAccount => self.add_account(),
            Action::SetupChip(section, chip) => self.apply_setup_chip(ctx, section, chip),
            Action::ApplyUpdate => self.apply_update(ctx),
            Action::NavigateToFolder(folder) => self.navigate_to_folder(folder),
            Action::NavigateBack => self.navigate_back(),
            Action::SearchChanged(query) => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.set_search(&query, Instant::now());
                }
            }
            Action::SetViewMode(mode) => {
                if let Some(b) = self.booted.as_mut() {
                    b.stores.set_view_mode(mode);
                }
            }
            Action::ToggleSection(key) => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.pipeline.toggle_section_collapsed(&key);
                }
            }
            Action::Pin(event) => self.apply_pin(ctx, event),
            Action::Streamer(event) => self.apply_streamer(ctx, event),
            Action::Toast(event) => self.apply_toast(event),
            Action::ContextMenu(target, at) => self.open_context_menu(ctx, target, at),
            Action::Menu(open_id, event) => self.apply_menu(ctx, open_id, event),
            Action::ContentScrolled => {
                if self.dialogs.context_menu().is_some() {
                    self.close_context_menu(ctx);
                }
            }
            Action::Palette(mount, event) => self.apply_palette(ctx, mount, event),
            Action::InputDialog(mount, event) => self.apply_input_dialog(ctx, mount, event),
            Action::ConfirmDialog(mount, event) => self.apply_confirm_dialog(ctx, mount, event),
            Action::OpenSettings => self.toggle_settings_panel(),
            Action::Settings(action) => self.apply_settings(ctx, action),
            Action::Personas(event) => self.apply_personas(ctx, event),
            Action::OpenPersonas => self.open_personas(),
            Action::BulkEdit => self.toggle_bulk_edit(),
            Action::Bulk(event) => self.apply_bulk(ctx, event),
            Action::TabChange(tab) => self.change_tab(tab),
            Action::Onboarding(event) => {
                let Some(tab) = self.booted.as_ref().map(|b| b.nav.active_tab.clone()) else {
                    return;
                };
                let now = ctx.input(|i| i.time);
                let effects = self.onboarding.apply(&self.jobs, event, now, &tab);
                self.apply_effects(ctx, effects);
            }
        }
    }

    /// The title bar's refresh: reloads the active platform's accounts.
    fn refresh_accounts(&mut self) {
        if let Some(b) = self.booted.as_mut() {
            let tab = b.nav.active_tab.clone();
            b.accounts.refresh(&self.jobs, &mut b.stores, &tab);
        }
    }

    /// A card's switch. In bulk edit the click toggles the account instead,
    /// unless it ends a paint drag. A pending setup card does not switch;
    /// another card drops a setup of this tab first.
    fn account_switch(&mut self, id: &str) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if b.bulk.is_open() {
            if !b.bulk.paint_capture_click() {
                b.bulk.toggle_account(id);
            }
            return;
        }
        if b.accounts.is_pending_setup(id) {
            return;
        }
        let tab = b.nav.active_tab.clone();
        b.accounts
            .cancel_add_flow_if_conflicting(&self.jobs, &tab, &tab, Some(id));
        let account = b
            .accounts
            .loading
            .loader
            .accounts()
            .iter()
            .find(|a| a.id == id)
            .cloned();
        if let Some(account) = account {
            self.switch_account(&account);
        }
    }

    /// A card click: drops a setup of the tab unless it is this card's.
    fn account_activate(&mut self, id: &str) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if b.bulk.is_open() {
            return;
        }
        let tab = b.nav.active_tab.clone();
        b.accounts
            .cancel_add_flow_if_conflicting(&self.jobs, &tab, &tab, Some(id));
    }

    /// The title bar's plus: starts the add-account flow.
    fn add_account(&mut self) {
        if let Some(b) = self.booted.as_mut() {
            let tab = b.nav.active_tab.clone();
            b.accounts.add_account(&self.jobs, &mut b.stores, &tab);
        }
    }

    /// Opens `folder`, `None` being the root, dropping a pending setup.
    fn navigate_to_folder(&mut self, folder: Option<String>) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let tab = b.nav.active_tab.clone();
        b.accounts
            .cancel_add_flow_if_conflicting(&self.jobs, &tab, &tab, None);
        b.accounts.navigate_to(&b.stores, &tab, folder);
    }

    /// The back card: the parent folder, dropping a pending setup when it
    /// leaves a folder.
    fn navigate_back(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let tab = b.nav.active_tab.clone();
        if b.accounts.navigation.current_folder_id().is_some() {
            b.accounts
                .cancel_add_flow_if_conflicting(&self.jobs, &tab, &tab, None);
        }
        b.accounts.navigate_back(&b.stores, &tab);
    }

    /// The personas tab, when the setting enables it: closes settings and
    /// bulk edit.
    fn open_personas(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if !b.stores.settings().personas_enabled {
            return;
        }
        b.nav.show_settings = false;
        b.bulk.close();
        b.nav.show_personas = true;
    }

    /// A platform tab: leaves personas; a usable other tab cancels the old
    /// tab's setup, closes bulk edit and settings and loads the new one.
    fn change_tab(&mut self, tab: String) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.nav.show_personas = false;
        if !platforms::usable(&b.platforms, &tab, b.runtime_os) {
            return;
        }
        if tab == b.nav.active_tab {
            b.nav.show_settings = false;
            return;
        }
        let previous = b.nav.active_tab.clone();
        b.accounts.cancel_add_flow(&self.jobs, &previous);
        b.bulk.close();
        b.nav.show_settings = false;
        b.nav.active_tab = tab.clone();
        b.accounts.change_tab(&self.jobs, &mut b.stores, &tab);
    }

    /// The PIN lock: a new attempt, its window buttons.
    fn apply_pin(&mut self, ctx: &Context, event: PinEvent) {
        match event {
            PinEvent::Attempt(value) => {
                if let Some(b) = self.booted.as_mut() {
                    b.secure.set_pin_attempt(&value);
                }
            }
            PinEvent::Minimize => self.apply_one(ctx, Action::Minimize),
            PinEvent::Close => self.apply_one(ctx, Action::Close),
            PinEvent::StartDrag => self.apply_one(ctx, Action::StartDrag),
        }
    }

    /// The streamer overlay: dismissed for now or for good, its window
    /// buttons.
    fn apply_streamer(&mut self, ctx: &Context, event: StreamerEvent) {
        match event {
            StreamerEvent::Dismiss => {
                if let Some(b) = self.booted.as_mut() {
                    b.streamer.dismiss();
                }
            }
            StreamerEvent::DisablePermanently => {
                if let Some(b) = self.booted.as_mut() {
                    b.streamer.disable_permanently(&mut b.stores);
                }
                self.sync_settings(ctx);
            }
            StreamerEvent::Minimize => self.apply_one(ctx, Action::Minimize),
            StreamerEvent::Close => self.apply_one(ctx, Action::Close),
            StreamerEvent::StartDrag => self.apply_one(ctx, Action::StartDrag),
        }
    }

    /// A toast hovered, dismissed or its action clicked.
    fn apply_toast(&mut self, event: ToastEvent) {
        match event {
            ToastEvent::Hover(id, hovered) => {
                self.toasts.set_hovered(id, hovered, Instant::now());
            }
            ToastEvent::Close(id) => self.toasts.remove(id),
            ToastEvent::Action(id) => {
                self.toasts.activate_action(id);
            }
        }
    }

    /// The command palette mounted as `mount` closes on any event; a run
    /// command then runs.
    fn apply_palette(&mut self, ctx: &Context, mount: u64, event: PaletteEvent) {
        if !self.palette.as_ref().is_some_and(|p| p.mount == mount) {
            return;
        }
        self.set_palette_open(ctx, false);
        if let PaletteEvent::Run(action) = event {
            self.run_palette(ctx, action);
        }
    }
}
