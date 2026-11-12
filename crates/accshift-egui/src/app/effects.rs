//! What the app's controllers ask of it: the effects of a menu command, a
//! dialog or the onboarding, the outcomes of the account loader and the
//! add flow, and the menu tasks and renames run on the job pool.

use std::time::Instant;

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::LoadingEvent;
use accshift_app::actions::dialogs::{finish_rename_account, rename_account_blocking};
use accshift_app::actions::toasts::AddToastOptions;
use accshift_app::actions::{card_actions, load_options, toasts, AppEffect};
use accshift_app::jobs::{Pending, Poll};
use accshift_app::menu::{PlatformTask, TaskEffect};
use accshift_app::platform_add_flow::{AddFlowEvent, SetupChipAction};
use accshift_app::platforms::PlatformAccount;
use accshift_app::text::Text;
use egui::Context;

use super::{Action, AppModel};
use crate::ui::components::controls::field_menu;

/// Takes the finished jobs out of `jobs`, their results in order.
fn take_finished<T>(jobs: &mut Vec<Pending<T>>) -> Vec<T> {
    let mut done = Vec::new();
    jobs.retain_mut(|job| match job.poll() {
        Poll::Waiting => true,
        Poll::Ready(result) => {
            done.push(result);
            false
        }
        Poll::Gone => false,
    });
    done
}

impl AppModel {
    /// The effects a controller asked for, in order. Effects no egui flow
    /// produces are ignored.
    pub(super) fn apply_effects(&mut self, ctx: &Context, effects: Vec<AppEffect>) {
        for effect in effects {
            match effect {
                AppEffect::Toast(toast) => {
                    self.toasts.push(toast, Instant::now());
                }
                AppEffect::RefreshAccounts => self.apply_one(ctx, Action::Refresh),
                AppEffect::SwitchAccount(account) => {
                    self.switch_account(&account);
                }
                AppEffect::LoadAccounts(options) => self.load_accounts(options),
                AppEffect::RefreshCurrentItems => self.refresh_current_items(),
                AppEffect::CopyToClipboard { text, label } => {
                    self.copy_to_clipboard(ctx, text, label);
                }
                AppEffect::CopyBulkUrls(urls) => self.copy_bulk_urls(ctx, urls),
                AppEffect::RemoveAccount(account_id) => self.remove_account(&account_id),
                AppEffect::RunPlatformTask(task) => self.run_platform_task(task),
                AppEffect::RenameAccount {
                    platform_id,
                    account_id,
                    label,
                } => self.rename_account(&platform_id, account_id, label),
                // The stores hold the new colours and notes; the view reads
                // them every frame.
                AppEffect::CardColorsChanged | AppEffect::CardNotesChanged => {}
                AppEffect::ConfirmSettled { ticket, allowed } => {
                    self.settle_descriptor_removal(ticket, allowed);
                    self.settle_persona_confirm(ticket, allowed);
                    self.settle_deep_link_confirm(ctx, ticket, allowed);
                }
                AppEffect::CustomMenu(id) => {
                    if !field_menu::run(ctx, &id) {
                        self.apply_persona_menu(ctx, &id);
                    }
                }
                AppEffect::AddFlowCancel => self.cancel_add_flow(),
                AppEffect::AddFlowCancelIfConflicting {
                    platform_id,
                    account_id,
                } => self.cancel_add_flow_if_conflicting(&platform_id, account_id.as_deref()),
                // The tab alone, no load (the onboarding tour).
                AppEffect::SetActiveTab(tab) => {
                    if let Some(b) = self.booted.as_mut() {
                        b.nav.active_tab = tab;
                    }
                }
                _ => {}
            }
        }
    }

    /// Loads the active tab's accounts.
    fn load_accounts(&mut self, options: LoadOptions) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let tab = b.nav.active_tab.clone();
        b.accounts.load(&self.jobs, &mut b.stores, &tab, options);
    }

    /// Re-reads the open folder's items from the stores.
    fn refresh_current_items(&mut self) {
        if let Some(b) = self.booted.as_mut() {
            b.accounts
                .navigation
                .refresh_current_items(&b.stores, &b.nav.active_tab);
        }
    }

    /// Writes `text` to the clipboard and toasts `label`. Nothing before
    /// boot.
    fn copy_to_clipboard(&mut self, ctx: &Context, text: String, label: Text) {
        let Some(locale) = self.booted.as_ref().map(|b| b.locale) else {
            return;
        };
        ctx.copy_text(text);
        let toast = toasts::copy_result_toast(&label.resolve(locale), true);
        self.toasts.push(toast, Instant::now());
    }

    fn copy_bulk_urls(&mut self, ctx: &Context, urls: Vec<String>) {
        ctx.copy_text(urls.join("\n"));
        let toast = card_actions::bulk_urls_copied_toast(urls.len(), true);
        self.toasts.push(toast, Instant::now());
    }

    /// Drops an account from the active tab's grid and folders.
    fn remove_account(&mut self, account_id: &str) {
        if let Some(b) = self.booted.as_mut() {
            b.accounts
                .remove_account(&mut b.stores, &b.nav.active_tab, account_id);
        }
    }

    /// Cancels the active tab's add-account setup.
    fn cancel_add_flow(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let tab = b.nav.active_tab.clone();
        b.accounts.cancel_add_flow(&self.jobs, &tab);
    }

    /// Cancels the active tab's setup unless it is for `account_id` on
    /// `platform_id`.
    fn cancel_add_flow_if_conflicting(&mut self, platform_id: &str, account_id: Option<&str>) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let tab = b.nav.active_tab.clone();
        b.accounts
            .cancel_add_flow_if_conflicting(&self.jobs, &tab, platform_id, account_id);
    }

    /// A menu task on the job pool, with the active tab's adapter.
    fn run_platform_task(&mut self, task: PlatformTask) {
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let Some(adapter) = b.accounts.registry.adapter(&b.nav.active_tab) else {
            return;
        };
        let settings = b.stores.settings().clone();
        let job = self
            .jobs
            .run(move |backend| adapter.run_task(&task, backend, &settings));
        self.task_jobs.push(job);
    }

    /// The adapter's account label set on the job pool.
    fn rename_account(&mut self, platform_id: &str, account_id: String, label: String) {
        let Some(adapter) = self
            .booted
            .as_ref()
            .and_then(|b| b.accounts.registry.adapter(platform_id))
        else {
            return;
        };
        let job = self
            .jobs
            .run(move |backend| rename_account_blocking(backend, &adapter, &account_id, &label));
        self.rename_jobs.push(job);
    }

    /// Lands finished menu tasks and renames, each task's effects in the
    /// order it reported them.
    pub(super) fn pump_tasks(&mut self, ctx: &Context) {
        let outcomes = take_finished(&mut self.task_jobs);
        let renames = take_finished(&mut self.rename_jobs);
        for effect in outcomes.into_iter().flat_map(|o| o.effects) {
            self.apply_task_effect(effect);
        }
        for result in renames {
            self.apply_effects(ctx, finish_rename_account(result));
        }
    }

    /// What a finished menu task asks of the app.
    fn apply_task_effect(&mut self, effect: TaskEffect) {
        match effect {
            TaskEffect::Toast(text) => {
                self.toasts
                    .add(text, AddToastOptions::default(), Instant::now());
            }
            TaskEffect::RefreshAccounts => {
                self.load_accounts(load_options(true, false, false, false));
            }
            TaskEffect::RemoveAccount(id) => self.remove_account(&id),
            TaskEffect::SetDefaultGame { steam_id, game } => {
                if let Some(b) = self.booted.as_mut() {
                    b.stores.set_default_game(&steam_id, &game);
                }
            }
            TaskEffect::ClearDefaultGame { steam_id } => {
                if let Some(b) = self.booted.as_mut() {
                    b.stores.clear_default_game(&steam_id);
                }
            }
            // The backend refused the call behind the PIN: raise the lock.
            TaskEffect::PinLocked => {
                if let Some(b) = self.booted.as_mut() {
                    b.secure.handle_backend_locked();
                }
            }
        }
    }

    /// Switches to `account` on the active tab, refused while a persona
    /// switch runs; the outcome comes back from the loader. False when it
    /// did not start.
    pub(super) fn switch_account(&mut self, account: &PlatformAccount) -> bool {
        let Some(b) = self.booted.as_mut() else {
            return false;
        };
        let persona_switching = b.personas.switching();
        let tab = b.nav.active_tab.clone();
        b.accounts
            .switch(&self.jobs, &mut b.stores, &tab, account, persona_switching)
    }

    /// What the account loader's outcomes ask of the app: its toasts, and
    /// minimizing the window after a successful switch when the setting asks.
    /// A finished switch also answers the deep link that ran it;
    /// `Accounts::update` starts the add flow from `AddFlowStarted` itself.
    pub(super) fn apply_loading_events(&mut self, ctx: &Context, events: Vec<LoadingEvent>) {
        for event in events {
            match event {
                LoadingEvent::Toast(toast) => {
                    self.toasts.push(toast, Instant::now());
                }
                LoadingEvent::StickyToast(toast) => {
                    self.toasts.push_sticky(toast, Instant::now());
                }
                LoadingEvent::DismissToast(text) => self.toasts.remove_text(&text),
                LoadingEvent::MinimizeWindow => self.apply_one(ctx, Action::Minimize),
                LoadingEvent::SwitchFinished {
                    account_id,
                    succeeded,
                } => self.deep_link_switch_finished(ctx, &account_id, succeeded),
                LoadingEvent::AddFlowStarted { .. }
                | LoadingEvent::AccountsLoaded { .. }
                | LoadingEvent::Cs2Check { .. } => {}
            }
        }
    }

    /// What the add flow asks of the app: its toasts, the rename prompt once
    /// a setup added an account, and account reloads.
    pub(super) fn apply_add_flow_events(&mut self, events: Vec<AddFlowEvent>) {
        for event in events {
            match event {
                AddFlowEvent::Toast(toast) => {
                    self.toasts.push(toast, Instant::now());
                }
                AddFlowEvent::AccountAdded {
                    platform_id,
                    account_id,
                    display_name,
                } => {
                    if let Some(b) = self.booted.as_ref() {
                        self.dialogs.prompt_rename_new_account(
                            &b.accounts.registry,
                            &platform_id,
                            &account_id,
                            display_name.as_deref(),
                        );
                    }
                }
                AddFlowEvent::LoadAccounts(options) => self.load_accounts(options),
            }
        }
    }

    /// A clickable chip of the setup card: Close cancels the setup, Roblox's
    /// "Copy code" copies the quick login code.
    pub(super) fn apply_setup_chip(&mut self, ctx: &Context, section: usize, chip: usize) {
        let Some(action) = self.booted.as_ref().and_then(|b| {
            b.accounts
                .setup
                .as_ref()
                .and_then(|(_, extension)| extension.chip_action(section, chip))
                .cloned()
        }) else {
            return;
        };
        match action {
            SetupChipAction::Cancel => self.cancel_add_flow(),
            SetupChipAction::CopyCode(code) => {
                self.copy_to_clipboard(ctx, code.clone(), Text::raw(code));
            }
        }
    }
}
