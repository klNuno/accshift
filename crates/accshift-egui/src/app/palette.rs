//! The command palette as `App.svelte` mounts it: `paletteOpen`, the
//! getters of `createCommandRegistry` read from the app, and its `run`
//! callbacks mapped onto the app's actions. The view is
//! `ui::components::command_palette`.

use std::collections::BTreeSet;

use accshift_app::actions::palette::{
    palette_commands, PaletteAction, PaletteCommand, RegistryInput,
};
use accshift_app::folders::FolderInfo;
use accshift_app::platforms::PlatformDef as AppPlatformDef;
use accshift_app::stores::ViewMode;
use egui::Context;

use super::platforms::RuntimeOs;
use super::{Action, AppModel};
use crate::theme::Rgba;
use crate::ui::components::command_palette::{self, PaletteView};

/// The open palette: its own state and the mount it opened as.
pub struct PaletteState {
    pub view: PaletteView,
    pub mount: u64,
}

/// The registry's commands and the colour of each one's dot.
pub struct PaletteCommands {
    pub commands: Vec<PaletteCommand>,
    pub accents: Vec<Option<Rgba>>,
}

impl AppModel {
    /// `setPaletteOpen`: opening mounts a fresh palette (empty query, first
    /// row, the field focused); closing unmounts it and the field's focus
    /// goes with it.
    pub(super) fn set_palette_open(&mut self, ctx: &Context, open: bool) {
        if open == self.palette.is_some() {
            return;
        }
        if open {
            self.mounts += 1;
            self.palette = Some(PaletteState {
                view: PaletteView::new(),
                mount: self.mounts,
            });
        } else if let Some(state) = self.palette.take() {
            let input = command_palette::input_id(state.mount);
            ctx.memory_mut(|m| m.surrender_focus(input));
        }
    }

    /// `commandRegistry.getCommands()`.
    pub(super) fn palette_commands(&self) -> PaletteCommands {
        let Some(b) = self.booted.as_ref() else {
            return PaletteCommands {
                commands: Vec::new(),
                accents: Vec::new(),
            };
        };
        let os = b.runtime_os;
        let settings = b.stores.settings();
        let enabled: Vec<&super::platforms::PlatformDef> = b
            .platforms
            .iter()
            .filter(|p| settings.enabled_platforms.contains(&p.id))
            .collect();
        let defs = b.accounts.registry.defs();
        let enabled_defs: Vec<&AppPlatformDef> = enabled
            .iter()
            .filter_map(|p| defs.iter().find(|d| d.id == p.id))
            .collect();
        let unavailable: BTreeSet<String> = enabled
            .iter()
            .filter(|p| !p.supports(os))
            .map(|p| p.id.clone())
            .collect();
        let active = b.platforms.iter().find(|p| p.id == b.nav.active_tab);
        let usable = active.is_some_and(|p| p.supports(os));
        let folders = b.stores.folders();
        let current_folders: Vec<&FolderInfo> = b
            .accounts
            .navigation
            .folder_items()
            .iter()
            .filter_map(|item| folders.folder(&item.id))
            .collect();
        let loader = &b.accounts.loading.loader;
        let commands = palette_commands(&RegistryInput {
            accounts: loader.accounts(),
            current_account_id: loader.current_account_id(),
            enabled_platforms: &enabled_defs,
            unavailable_platform_ids: &unavailable,
            active_tab: &b.nav.active_tab,
            active_tab_usable: usable,
            current_folders: &current_folders,
            current_folder_id: b.accounts.navigation.current_folder_id(),
            bulk_edit_available: active.is_some_and(|p| p.bulk_edit)
                && usable
                && !b.nav.show_settings,
            personas_enabled: settings.personas_enabled,
            update_ready: self.updates.cta().is_some(),
            view_mode: b.stores.view_mode(),
            is_mac: os == RuntimeOs::Macos,
        });
        // A tab's dot is its platform accent, drawn like the tab bar's.
        let accents = commands
            .iter()
            .map(|command| {
                command.accent.as_ref()?;
                let id = command.id.strip_prefix("nav:tab:")?;
                b.platforms.iter().find(|p| p.id == id).map(|p| p.accent)
            })
            .collect();
        PaletteCommands { commands, accents }
    }

    /// The `run` callbacks of `createCommandRegistry`, after the palette
    /// closed.
    pub(super) fn run_palette(&mut self, ctx: &Context, action: PaletteAction) {
        match action {
            // `addFlow.cancelIfConflicting` then the switch, bulk edit or
            // not.
            PaletteAction::SwitchToAccount(account) => {
                if let Some(b) = self.booted.as_mut() {
                    let tab = b.nav.active_tab.clone();
                    b.accounts.cancel_add_flow_if_conflicting(
                        &self.jobs,
                        &tab,
                        &tab,
                        Some(&account.id),
                    );
                }
                self.switch_account(&account);
            }
            PaletteAction::AddAccount => self.apply_one(ctx, Action::AddAccount),
            PaletteAction::RefreshAccounts => self.apply_one(ctx, Action::Refresh),
            PaletteAction::NewFolder => self.dialogs.open_new_folder_dialog(),
            PaletteAction::ToggleViewMode => {
                let next = match self.booted.as_ref().map(|b| b.stores.view_mode()) {
                    Some(ViewMode::Grid) => ViewMode::List,
                    _ => ViewMode::Grid,
                };
                self.apply_one(ctx, Action::SetViewMode(next));
            }
            // The UI zoom is not applied by the egui shell yet.
            PaletteAction::ZoomReset => {}
            PaletteAction::ToggleBulkEdit => self.apply_one(ctx, Action::BulkEdit),
            PaletteAction::OpenPersonas => self.apply_one(ctx, Action::OpenPersonas),
            PaletteAction::ApplyUpdate => self.apply_one(ctx, Action::ApplyUpdate),
            PaletteAction::ChangeTab(tab) => self.apply_one(ctx, Action::TabChange(tab)),
            PaletteAction::OpenFolder(id) => {
                self.apply_one(ctx, Action::NavigateToFolder(Some(id)))
            }
            PaletteAction::NavigateToParent => self.apply_one(ctx, Action::NavigateBack),
            PaletteAction::ToggleSettings => {
                if let Some(b) = self.booted.as_mut() {
                    b.nav.show_personas = false;
                }
                self.toggle_settings_panel();
            }
        }
        ctx.request_repaint();
    }
}
