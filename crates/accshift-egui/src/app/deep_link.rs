//! Deep links: `createDeepLinkController` (`useDeepLink.svelte.ts`) wired
//! the way `App.svelte` wires it. The controller is the one of
//! `accshift-app` ([`DeepLinkController`]); this module answers its actions
//! with the app's own: the confirm dialog, the tab change, the loads and the
//! switch.
//!
//! URLs arrive from a second launch and from the scenes' `deeplink` step,
//! through [`AppModel::open_urls`]. The URL of a cold start is not replayed:
//! `onOpenUrl` only listens for `deep-link://new-url`, which the plugin
//! emits before the page listens.
//!
//! The controller is polled once boot finished, since its view reads the
//! stores; a link that comes earlier waits for it, as `waitUntil(isBootReady)`
//! does.

use std::time::{Duration, Instant};

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::loading::AccountLoading;
use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::actions::dialogs::{ConfirmConfig, ConfirmTicket};
use accshift_app::error::BackendResult;
use accshift_app::jobs::{Pending, Poll};
use accshift_app::lifecycle::deep_link::{
    DeepLinkAction, DeepLinkAnswer, DeepLinkController, DeepLinkView,
};
use accshift_app::platforms::PlatformAccount;
use accshift_app::text::Text;
use egui::Context;

use super::{Action, AppModel};

/// What the controller's last action waits for.
enum Waiting {
    /// `loadPlatformAccounts(platformId)`.
    PlatformAccounts(Pending<BackendResult<Vec<PlatformAccount>>>),
    /// `loadAccounts(true)` on the visible tab: answered once no load is in
    /// flight, whether it landed or failed (the TS promise settles either way).
    Reload,
    /// `confirmSwitch`: the confirm dialog's ticket.
    Confirm(ConfirmTicket),
    /// `switchToAccount`: the switch of this account id.
    Switch(String),
}

/// The controller and what it waits for.
#[derive(Default)]
pub(super) struct DeepLinks {
    controller: DeepLinkController,
    waiting: Option<Waiting>,
}

impl DeepLinks {
    /// `deepLink.start()` from `onMount`.
    pub(super) fn started() -> Self {
        let mut links = Self::default();
        links.controller.start();
        links
    }
}

/// How often a busy controller is polled for its timeouts and waits (boot,
/// the loader).
const POLL_EVERY: Duration = Duration::from_millis(100);

/// `account.displayName || account.username || account.id`.
fn account_label(account: &PlatformAccount) -> String {
    [&account.display_name, &account.username, &account.id]
        .into_iter()
        .find(|value| !value.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// What the controller is answered with, or why it stops.
enum Step {
    Answer(DeepLinkAnswer),
    /// `handleUrl`'s `catch`: the work in flight threw.
    Fail,
}

impl AppModel {
    /// `onOpenUrl`'s handler: each URL goes to `handleUrl`.
    pub fn open_urls(&mut self, ctx: &Context, urls: &[String]) {
        self.deep_links.controller.on_urls(urls, Instant::now());
        ctx.request_repaint();
    }

    /// Settles what the last action waits for, then lets the controller go
    /// as far as the app allows. Once per frame, after the loaders ran.
    pub(super) fn tick_deep_links(&mut self, ctx: &Context) {
        let step = match self.deep_links.waiting.take() {
            None => None,
            Some(Waiting::PlatformAccounts(mut job)) => match job.poll() {
                Poll::Waiting => {
                    self.deep_links.waiting = Some(Waiting::PlatformAccounts(job));
                    return;
                }
                Poll::Ready(Ok(accounts)) => {
                    Some(Step::Answer(DeepLinkAnswer::PlatformAccounts(accounts)))
                }
                Poll::Ready(Err(_)) | Poll::Gone => Some(Step::Fail),
            },
            Some(Waiting::Reload) => {
                let loading = self
                    .booted
                    .as_ref()
                    .is_some_and(|b| b.accounts.loading.loads_in_flight());
                if loading {
                    self.deep_links.waiting = Some(Waiting::Reload);
                    return;
                }
                Some(Step::Answer(DeepLinkAnswer::AccountsLoaded))
            }
            // The dialog and the loader answer these.
            Some(waiting) => {
                self.deep_links.waiting = Some(waiting);
                return;
            }
        };
        match step {
            Some(step) => self.step_deep_link(ctx, step),
            None => {
                if !self.deep_links.controller.is_busy() {
                    return;
                }
                let now = Instant::now();
                let actions =
                    self.with_deep_link_view(|controller, view| controller.poll(view, now));
                self.apply_deep_link_actions(ctx, actions);
            }
        }
        if self.deep_links.controller.is_busy() && self.deep_links.waiting.is_none() {
            ctx.request_repaint_after(POLL_EVERY);
        }
    }

    /// A settled confirm: `confirmSwitch`'s answer, when it asked.
    pub(super) fn settle_deep_link_confirm(
        &mut self,
        ctx: &Context,
        ticket: ConfirmTicket,
        allowed: bool,
    ) {
        if !matches!(&self.deep_links.waiting, Some(Waiting::Confirm(pending)) if *pending == ticket)
        {
            return;
        }
        self.deep_links.waiting = None;
        self.step_deep_link(ctx, Step::Answer(DeepLinkAnswer::Confirmed(allowed)));
    }

    /// A finished switch: `switchToAccount`'s answer, when it ran it.
    pub(super) fn deep_link_switch_finished(
        &mut self,
        ctx: &Context,
        account_id: &str,
        succeeded: bool,
    ) {
        if !matches!(&self.deep_links.waiting, Some(Waiting::Switch(id)) if id == account_id) {
            return;
        }
        self.deep_links.waiting = None;
        self.step_deep_link(ctx, Step::Answer(DeepLinkAnswer::Switched(succeeded)));
    }

    fn step_deep_link(&mut self, ctx: &Context, step: Step) {
        let now = Instant::now();
        let actions = self.with_deep_link_view(|controller, view| match step {
            Step::Answer(answer) => controller.answer(answer, view, now),
            Step::Fail => controller.fail(view, now),
        });
        self.apply_deep_link_actions(ctx, actions);
    }

    /// The controller with what it reads of the app. Nothing before boot.
    fn with_deep_link_view(
        &mut self,
        run: impl FnOnce(&mut DeepLinkController, &DeepLinkView<'_>) -> Vec<DeepLinkAction>,
    ) -> Vec<DeepLinkAction> {
        let Some(b) = self.booted.as_ref() else {
            return Vec::new();
        };
        let loader = &b.accounts.loading.loader;
        let view = DeepLinkView {
            boot_ready: true,
            pin_locked: b.secure.is_pin_locked(),
            settings: b.stores.settings(),
            registry: &b.accounts.registry,
            runtime_os: b.accounts.runtime_os,
            active_tab: &b.nav.active_tab,
            loader_loading: loader.loading(),
            accounts: loader.accounts(),
        };
        run(&mut self.deep_links.controller, &view)
    }

    /// The deps of `createDeepLinkController` in `App.svelte`.
    fn apply_deep_link_actions(&mut self, ctx: &Context, actions: Vec<DeepLinkAction>) {
        for action in actions {
            match action {
                DeepLinkAction::Toast(text) => {
                    self.toasts
                        .push(Toast::new(text, ToastKind::Default), Instant::now());
                }
                DeepLinkAction::LoadPlatformAccounts(platform_id) => {
                    let job = self.booted.as_ref().and_then(|b| {
                        AccountLoading::load_platform_accounts(
                            &self.jobs,
                            &b.accounts.registry,
                            &platform_id,
                        )
                    });
                    match job {
                        Some(job) => self.deep_links.waiting = Some(Waiting::PlatformAccounts(job)),
                        // `ensurePlatformLoaded` found no adapter: `[]`.
                        None => self.step_deep_link(
                            ctx,
                            Step::Answer(DeepLinkAnswer::PlatformAccounts(Vec::new())),
                        ),
                    }
                }
                DeepLinkAction::LoadAccounts => {
                    if let Some(b) = self.booted.as_mut() {
                        let tab = b.nav.active_tab.clone();
                        b.accounts.load(
                            &self.jobs,
                            &mut b.stores,
                            &tab,
                            LoadOptions {
                                silent: true,
                                show_refreshed_toast: false,
                                force_refresh: false,
                                check_bans: false,
                                defer_background: true,
                            },
                        );
                    }
                    self.deep_links.waiting = Some(Waiting::Reload);
                }
                DeepLinkAction::Confirm {
                    account,
                    platform_name,
                } => {
                    let mut effects = Vec::new();
                    let ticket = self.dialogs.request_confirm(
                        ConfirmConfig {
                            title: Text::key("dialog.deepLinkSwitchTitle"),
                            message: Text::with(
                                "dialog.deepLinkSwitchMessage",
                                [
                                    ("platform", platform_name),
                                    ("account", account_label(&account)),
                                ],
                            ),
                            confirm_label: Some(Text::key("dialog.deepLinkSwitchConfirm")),
                            confirm_color: None,
                        },
                        &mut effects,
                    );
                    self.deep_links.waiting = Some(Waiting::Confirm(ticket));
                    self.apply_effects(ctx, effects);
                }
                DeepLinkAction::ChangeTab(tab) => {
                    self.apply_one(ctx, Action::TabChange(tab));
                    self.step_deep_link(ctx, Step::Answer(DeepLinkAnswer::TabChanged));
                }
                DeepLinkAction::Switch(account) => {
                    if self.switch_account(&account) {
                        self.deep_links.waiting = Some(Waiting::Switch(account.id));
                    } else {
                        self.step_deep_link(ctx, Step::Answer(DeepLinkAnswer::Switched(false)));
                    }
                }
            }
        }
    }
}
