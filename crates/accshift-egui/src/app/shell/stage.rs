//! The stage: the shell background, the title bar, the panel, then what
//! floats over it (bulk edit, toasts, dialogs, the palette and the menu).

use accshift_app::actions::dialogs::{AppDialogs, MenuInput};
use accshift_app::actions::personas::PersonaController;
use accshift_app::actions::toasts::ToastEntry;
use accshift_app::bulk_edit::{BarStep, BulkEdit};
use accshift_app::jobs::Jobs;
use accshift_app::menu::SubmenuLoader;
use accshift_app::stores::ClientStores;
use accshift_theme::ThemeRegistry;
use egui::{pos2, Rect, Ui};

use super::super::accounts::Accounts;
use super::super::palette::{PaletteCommands, PaletteState};
use super::super::personas::PersonasPanel;
use super::super::platforms::PlatformDef;
use super::super::settings_panel::{SettingsAction, SettingsPanel};
use super::super::wallpaper::Liquid;
use super::super::{Action, ThemeState};
use super::{layer, FrameView};
use crate::screens::workspace::{self, Workspace, WorkspaceInput};
use crate::screens::{personas, settings};
use crate::ui::components::bulk_edit::{self, BulkEvent, BulkProps};
use crate::ui::components::command_palette::{self, PaletteProps};
use crate::ui::components::context_menu::{self, ContextMenuProps, SubmenuLoad};
use crate::ui::components::dialog;
use crate::ui::components::secure_overlays;
use crate::ui::components::title_bar::{self, TitleBarProps};
use crate::ui::components::toast::{self, ToastProps};
use crate::ui::paint::BoxStyle;

/// What the stage draws this frame, the model's parts borrowed apart.
pub(super) struct Stage<'a> {
    pub(super) fv: &'a FrameView,
    pub(super) title_bar: &'a TitleBarProps<'a>,
    pub(super) jobs: &'a Jobs,
    pub(super) liquid: &'a Liquid,
    pub(super) theme: &'a ThemeState,
    /// An overlay covers the panel (see [`covers_panel`]).
    pub(super) modal: bool,
    pub(super) obscured: bool,
    // The panel.
    pub(super) stores: &'a ClientStores,
    pub(super) accounts: &'a mut Accounts,
    pub(super) ws: &'a mut Workspace,
    pub(super) platform: Option<&'a PlatformDef>,
    pub(super) compatible_platforms: usize,
    pub(super) settings_view: Option<settings::View>,
    pub(super) settings_panel: &'a mut Option<SettingsPanel>,
    pub(super) personas_panel: &'a mut Option<PersonasPanel>,
    pub(super) persona_ctl: &'a PersonaController,
    // Bulk edit.
    pub(super) bulk: &'a BulkEdit,
    pub(super) bulk_step: Option<BarStep>,
    pub(super) bulk_mount: u64,
    pub(super) bulk_escape: bool,
    // Toasts, dialogs, palette and menu.
    pub(super) toasts: &'a [ToastEntry],
    pub(super) theme_registry: &'a ThemeRegistry,
    pub(super) dialogs: &'a AppDialogs,
    pub(super) input_mount: Option<u64>,
    pub(super) confirm_mount: Option<u64>,
    pub(super) palette: &'a mut Option<PaletteState>,
    pub(super) palette_commands: Option<PaletteCommands>,
    pub(super) submenus: &'a [(SubmenuLoader, SubmenuLoad)],
    pub(super) open_id: u64,
    pub(super) actions: Vec<Action>,
}

impl Stage<'_> {
    /// The stage's layers, back to front. `title_bar` draws the title bar
    /// inside the stage (every OS but macOS).
    pub(super) fn draw(&mut self, ui: &mut Ui, stage: Rect, title_bar: bool) {
        let liquid = self.background(ui, stage);
        let panel = if title_bar {
            let bar_h = self.fv.title_bar_h;
            let (bar, _) = stage.split_top_bottom_at_y(stage.top() + bar_h);
            let props = self.title_bar;
            self.actions
                .extend(layer(ui, bar, |ui| title_bar::title_bar(ui, bar, props)));
            // The panel slot, under the title bar.
            Rect::from_min_max(pos2(stage.left(), stage.top() + bar_h), stage.max)
        } else {
            stage
        };
        self.panel(ui, panel);
        // The Liquid Glass sheen, over the panel and the title bar.
        if liquid {
            crate::ui::liquid::paint_sheen(ui.painter(), stage);
        }
        // The frost, above the panel and the title bar.
        secure_overlays::frost(ui, stage, self.obscured, &self.fv.vars);
        // The bulk edit bar under the toasts, its settings dialog with the
        // other dialogs.
        self.bulk_edit(ui, BarStep::Select);
        self.toasts(ui);
        self.descriptor_preview(ui);
        self.theme_editor(ui);
        self.app_dialogs(ui);
        self.bulk_edit(ui, BarStep::Settings);
        if self.bulk_escape && bulk_edit::escape_pressed(ui) {
            self.actions.push(Action::Bulk(BulkEvent::Escape));
        }
        self.palette(ui);
        self.context_menu(ui);
    }

    /// The shell's background colour and image, then Liquid Glass under
    /// the panel. Returns whether the theme is Liquid Glass itself.
    fn background(&self, ui: &Ui, stage: Rect) -> bool {
        let vars = &self.fv.vars;
        // A plain rect: `BoxStyle::paint` on a square full-window box
        // darkens a translucent fill as if it were laid twice (reported to
        // the foundation owner).
        ui.painter().rect_filled(stage, 0.0, vars.bg.to_color32());
        if !vars.bg_image.is_empty() {
            BoxStyle {
                images: vars.bg_image.clone(),
                ..BoxStyle::default()
            }
            .paint(ui.painter(), stage);
        }
        // The Liquid Glass theme by its id: a theme extending it keeps the
        // plain shell. The wallpaper backdrop comes with any Liquid Glass
        // theme once a snapshot is up.
        let liquid = self.theme.theme.id == "liquid-glass";
        let wallpaper = self.liquid.backdrop(ui.ctx());
        if liquid || wallpaper.is_some() {
            crate::ui::liquid::paint_under(
                ui.painter(),
                stage,
                vars.bg,
                liquid,
                wallpaper.as_ref(),
            );
        }
        liquid
    }

    /// The panel: the workspace, settings or personas. Under an overlay
    /// the panel gets no wheel.
    fn panel(&mut self, ui: &mut Ui, panel: Rect) {
        let ctx = ui.ctx().clone();
        let wheel = self.modal.then(|| {
            ctx.input_mut(|i| {
                let (wheel, rest) = std::mem::take(&mut i.events)
                    .into_iter()
                    .partition(|e| matches!(e, egui::Event::MouseWheel { .. }));
                i.events = rest;
                wheel
            })
        });
        let fv = self.fv;
        if !fv.nav.show_settings && !fv.nav.show_personas {
            let panel_key = fv.nav.panel_key();
            let input = WorkspaceInput {
                vars: &fv.vars,
                resolved: &self.theme.resolved,
                motion: fv.motion,
                locale: fv.locale,
                stores: self.stores,
                accounts: self.accounts,
                jobs: self.jobs,
                platform: self.platform,
                usable: fv.active_usable,
                compatible_platforms: self.compatible_platforms,
                panel_key: &panel_key,
                focus_visible: fv.focus_visible,
                dark: fv.dark,
                glass: self.theme.theme.glass,
                bulk_edit: self.bulk.is_open(),
                now: fv.now,
            };
            let ws = &mut *self.ws;
            self.actions
                .extend(layer(ui, panel, |ui| workspace::draw(ui, panel, ws, input)));
        }
        if let (Some(view), Some(sp)) = (self.settings_view.as_ref(), self.settings_panel.as_mut())
        {
            let modal = self.modal;
            self.actions.extend(layer(ui, panel, |ui| {
                settings::draw(ui, panel, sp, view, fv.focus_visible, modal)
            }));
        }
        if let Some(pp) = self
            .personas_panel
            .as_mut()
            .filter(|_| fv.nav.show_personas)
        {
            let input = personas::PersonasInput {
                vars: &fv.vars,
                resolved: &self.theme.resolved,
                locale: fv.locale,
                personas: self.persona_ctl.personas(),
                switching_id: self.persona_ctl.switching_persona_id(),
                panel: pp,
                avatars: &mut self.accounts.avatars,
                jobs: self.jobs,
            };
            self.actions
                .extend(layer(ui, panel, |ui| personas::draw(ui, panel, input)));
        }
        if let Some(wheel) = wheel {
            ctx.input_mut(|i| i.events.extend(wheel));
        }
    }

    /// The bulk edit layer of `step`: the bar, or its settings dialog.
    fn bulk_edit(&mut self, ui: &mut Ui, step: BarStep) {
        let fv = self.fv;
        let bulk = self.bulk;
        let Some(bar) = bulk.bar().filter(|_| self.bulk_step == Some(step)) else {
            return;
        };
        let current_account = self.accounts.loading.loader.current_account_id();
        let props = BulkProps {
            vars: &fv.vars,
            locale: fv.locale,
            viewport: fv.window,
            bar,
            selected: bulk.selected_ids().len(),
            active_selected: bulk.active_account_selected(current_account),
            accent_fg: fv.accent_fg,
            focus_visible: fv.focus_visible,
            dark: fv.dark,
            mount: self.bulk_mount,
        };
        let events = match step {
            BarStep::Select => bulk_edit::bar(ui, &props),
            BarStep::Settings => bulk_edit::settings(ui, &props),
        };
        self.actions.extend(events.into_iter().map(Action::Bulk));
    }

    /// The toast stack, fixed to the window, above the panel.
    fn toasts(&mut self, ui: &mut Ui) {
        let fv = self.fv;
        let props = ToastProps {
            vars: &fv.vars,
            locale: fv.locale,
            toasts: self.toasts,
            motion: fv.motion,
            now: fv.now,
        };
        self.actions.extend(
            toast::toast_stack(ui, fv.window, &props)
                .into_iter()
                .map(Action::Toast),
        );
    }

    /// The Platforms tab's descriptor preview, fixed over the window, under
    /// the app's dialogs.
    fn descriptor_preview(&mut self, ui: &mut Ui) {
        let Some(sp) = self.settings_panel.as_ref() else {
            return;
        };
        let Some(preview) = sp.descriptor_preview.as_ref() else {
            return;
        };
        let state = dialog::PreviewState {
            preview,
            busy: sp.descriptor_busy,
        };
        let events =
            dialog::descriptor_preview_dialog(ui, &self.fv.dialog_props(sp.preview_mount), &state);
        self.actions.extend(events.into_iter().map(|event| {
            Action::Settings(match event {
                dialog::DialogEvent::Confirm => SettingsAction::InstallDescriptor,
                _ => SettingsAction::CancelDescriptorPreview,
            })
        }));
    }

    /// The General tab's theme editor, fixed over the window, under the
    /// app's dialogs.
    fn theme_editor(&mut self, ui: &mut Ui) {
        let Some(editor) = self
            .settings_panel
            .as_mut()
            .and_then(|sp| sp.theme_editor.as_mut())
        else {
            return;
        };
        let fv = self.fv;
        let events = dialog::theme_editor_dialog(
            ui,
            &fv.dialog_props(editor.mount),
            self.theme_registry,
            fv.accent_fg,
            editor,
        );
        self.actions.extend(
            events
                .into_iter()
                .map(|event| Action::Settings(SettingsAction::ThemeEditor(event))),
        );
    }

    /// The input dialog, then the confirm dialog.
    fn app_dialogs(&mut self, ui: &mut Ui) {
        let fv = self.fv;
        if let (Some(state), Some(mount)) = (self.dialogs.input_dialog(), self.input_mount) {
            self.actions.extend(
                dialog::input_dialog(ui, &fv.dialog_props(mount), state)
                    .into_iter()
                    .map(|event| Action::InputDialog(mount, event)),
            );
        }
        if let (Some(state), Some(mount)) = (self.dialogs.confirm_dialog(), self.confirm_mount) {
            let bg =
                dialog::confirm_background(&state.confirm_color, &self.theme.resolved, &fv.vars);
            self.actions.extend(
                dialog::confirm_dialog(ui, &fv.dialog_props(mount), state, bg)
                    .into_iter()
                    .map(|event| Action::ConfirmDialog(mount, event)),
            );
        }
    }

    /// The command palette, after the dialogs.
    fn palette(&mut self, ui: &mut Ui) {
        let (Some(state), Some(cmds)) = (self.palette.as_mut(), self.palette_commands.as_ref())
        else {
            return;
        };
        let fv = self.fv;
        let props = PaletteProps {
            vars: &fv.vars,
            locale: fv.locale,
            viewport: fv.window,
            commands: &cmds.commands,
            accents: &cmds.accents,
            mount: state.mount,
            now: fv.now,
        };
        let mount = state.mount;
        self.actions.extend(
            command_palette::command_palette(ui, &props, &mut state.view)
                .into_iter()
                .map(|event| Action::Palette(mount, event)),
        );
    }

    /// The context menu, above the dialogs and the palette, whose field
    /// opens it too.
    fn context_menu(&mut self, ui: &mut Ui) {
        let Some(menu) = self.dialogs.context_menu() else {
            return;
        };
        let fv = self.fv;
        let active_tab = &fv.nav.active_tab;
        let adapter = self.accounts.registry.adapter(active_tab);
        let items = self.dialogs.context_menu_items(&MenuInput {
            stores: self.stores,
            adapter: adapter.as_deref(),
            active_tab,
            active_tab_usable: fv.active_usable,
            current_account_id: self.accounts.loading.loader.current_account_id(),
        });
        let props = ContextMenuProps {
            vars: &fv.vars,
            locale: fv.locale,
            open_id: self.open_id,
            items: &items,
            at: pos2(menu.x, menu.y),
            viewport: fv.screen,
            keyboard: fv.focus_visible,
            loads: self.submenus,
            now: fv.now,
        };
        let open_id = self.open_id;
        self.actions.extend(
            context_menu::context_menu(ui, &props)
                .into_iter()
                .map(|event| Action::Menu(open_id, event)),
        );
    }
}
