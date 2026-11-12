//! The context menu and the input and confirm dialogs: opening a menu on a
//! right click, its submenu loads, the dialogs' mount numbers, and what the
//! user's answer asks for.

use accshift_app::actions::dialogs::{AppDialogs, DialogContext};
use accshift_app::actions::AppEffect;
use accshift_app::jobs::Poll;
use accshift_app::menu::{build_context_menu_items, SubmenuLoader, ACCOUNT_CONTEXT_GROUP_ORDER};
use egui::{Context, Pos2};

use super::{AppModel, MenuTarget};
use crate::ui::components::context_menu::{self, MenuEvent, SubmenuLoad};
use crate::ui::components::controls::field_menu;
use crate::ui::components::dialog::DialogEvent;

impl AppModel {
    /// A right click on a card, a folder or the workspace background.
    pub(super) fn open_context_menu(&mut self, ctx: &Context, target: MenuTarget, at: Pos2) {
        // A right click on the open menu targets the menu, not the content.
        if self.dialogs.context_menu().is_some() && context_menu::contains(ctx, at) {
            return;
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if let (true, MenuTarget::Account(account)) = (b.bulk.is_open(), &target) {
            // In bulk edit the right click toggles the account instead.
            b.bulk.toggle_account(&account.id);
            return;
        }
        // The pending setup card has no menu; any other target drops a
        // setup of this tab, unless it is the setup account itself.
        let tab = b.nav.active_tab.clone();
        let target_account = match &target {
            MenuTarget::Account(account) if b.accounts.is_pending_setup(&account.id) => return,
            MenuTarget::Account(account) => Some(account.id.clone()),
            MenuTarget::Folder(_) | MenuTarget::Background => None,
        };
        b.accounts.cancel_add_flow_if_conflicting(
            &self.jobs,
            &tab,
            &tab,
            target_account.as_deref(),
        );
        match target {
            MenuTarget::Account(account) => {
                self.dialogs.open_account_context_menu(at.x, at.y, account)
            }
            MenuTarget::Folder(folder) => self.dialogs.open_folder_context_menu(at.x, at.y, folder),
            MenuTarget::Background => self.dialogs.open_background_context_menu(at.x, at.y),
        }
        self.menu_open_id += 1;
        self.forget_menu(ctx);
    }

    /// A right click on a text field this frame: its edit menu.
    pub(super) fn open_field_menu(&mut self, ctx: &Context) {
        let Some((at, items)) = field_menu::take_request(ctx) else {
            return;
        };
        self.dialogs.open_custom_context_menu(at.x, at.y, items);
        self.menu_open_id += 1;
        self.forget_menu(ctx);
        ctx.request_repaint();
    }

    pub(super) fn close_context_menu(&mut self, ctx: &Context) {
        self.dialogs.close_context_menu();
        self.forget_menu(ctx);
    }

    /// Drops what the last menu left: its loaders and its drawn state.
    pub(super) fn forget_menu(&mut self, ctx: &Context) {
        self.submenus.clear();
        self.submenu_jobs.clear();
        context_menu::reset(ctx);
    }

    /// What the menu opened as `open_id` reported. Events of a menu replaced
    /// in the same frame are stale.
    pub(super) fn apply_menu(&mut self, ctx: &Context, open_id: u64, event: MenuEvent) {
        if open_id != self.menu_open_id || self.dialogs.context_menu().is_none() {
            return;
        }
        match event {
            MenuEvent::Close => self.close_context_menu(ctx),
            MenuEvent::Load(loader) => self.load_submenu(loader),
            MenuEvent::Activate(command) => {
                let Some(effects) = self.in_dialog_context(|d, c| d.activate(&command, c)) else {
                    return;
                };
                self.forget_menu(ctx);
                self.apply_effects(ctx, effects);
            }
        }
    }

    /// Runs `f` on the dialogs with what they act on: the stores, the active
    /// tab and the open folder. `None` before boot.
    fn in_dialog_context(
        &mut self,
        f: impl FnOnce(&mut AppDialogs, &mut DialogContext<'_>) -> Vec<AppEffect>,
    ) -> Option<Vec<AppEffect>> {
        let b = self.booted.as_mut()?;
        let folder = b
            .accounts
            .navigation
            .current_folder_id()
            .map(str::to_string);
        Some(f(
            &mut self.dialogs,
            &mut DialogContext {
                stores: &mut b.stores,
                active_tab: &b.nav.active_tab,
                current_folder_id: folder.as_deref(),
            },
        ))
    }

    /// Numbers the dialogs that just opened, forgets the closed ones.
    pub(super) fn mount_dialogs(&mut self) {
        let mounts = &mut self.mounts;
        let mut mount = |open: bool, slot: &mut Option<u64>| match (open, *slot) {
            (true, None) => {
                *mounts += 1;
                *slot = Some(*mounts);
            }
            (false, Some(_)) => *slot = None,
            _ => {}
        };
        mount(self.dialogs.input_dialog().is_some(), &mut self.input_mount);
        mount(
            self.dialogs.confirm_dialog().is_some(),
            &mut self.confirm_mount,
        );
    }

    /// What the input dialog mounted as `mount` reported.
    pub(super) fn apply_input_dialog(&mut self, ctx: &Context, mount: u64, event: DialogEvent) {
        if self.input_mount != Some(mount) {
            return;
        }
        match event {
            DialogEvent::Input(value) => {
                if let Some(dialog) = self.dialogs.input_dialog_mut() {
                    dialog.set_value(value);
                }
            }
            DialogEvent::Cancel => self.dialogs.close_input_dialog(),
            DialogEvent::Submit | DialogEvent::Confirm => {
                if let Some(effects) = self.in_dialog_context(|d, c| d.submit_input(c)) {
                    self.apply_effects(ctx, effects);
                }
            }
        }
    }

    /// What the confirm dialog mounted as `mount` reported.
    pub(super) fn apply_confirm_dialog(&mut self, ctx: &Context, mount: u64, event: DialogEvent) {
        if self.confirm_mount != Some(mount) {
            return;
        }
        let effects = match event {
            DialogEvent::Cancel => self.dialogs.close_confirm_dialog(),
            DialogEvent::Confirm | DialogEvent::Submit => {
                let Some(effects) = self.in_dialog_context(|d, c| d.confirm_current(c)) else {
                    return;
                };
                effects
            }
            DialogEvent::Input(_) => return,
        };
        self.apply_effects(ctx, effects);
    }

    /// A submenu's loader on the job pool, its items grouped like the menu.
    fn load_submenu(&mut self, loader: SubmenuLoader) {
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let Some(adapter) = b.accounts.registry.adapter(&b.nav.active_tab) else {
            return;
        };
        self.submenus.retain(|(l, _)| *l != loader);
        self.submenus.push((loader.clone(), SubmenuLoad::Loading));
        self.submenu_jobs.retain(|(l, _)| *l != loader);
        let job_loader = loader.clone();
        let job = self.jobs.run(move |backend| {
            adapter.load_submenu(&job_loader, backend).map(|actions| {
                build_context_menu_items(actions, &ACCOUNT_CONTEXT_GROUP_ORDER)
                    .into_iter()
                    .map(Into::into)
                    .collect()
            })
        });
        self.submenu_jobs.push((loader, job));
    }

    /// Lands finished submenu loads.
    pub(super) fn pump_submenus(&mut self) {
        let submenus = &mut self.submenus;
        self.submenu_jobs.retain_mut(|(loader, job)| {
            let state = match job.poll() {
                Poll::Waiting => return true,
                Poll::Ready(Ok(items)) => SubmenuLoad::Ready(items),
                Poll::Ready(Err(_)) | Poll::Gone => SubmenuLoad::Failed,
            };
            if let Some(slot) = submenus.iter_mut().find(|(l, _)| l == loader) {
                slot.1 = state;
            }
            false
        });
    }
}
