//! The deep-link controller: one link at a time walked from the URL to an
//! account switch, through actions the shell answers.
//!
//! A global shortcut bound to an account takes the same walk from its parsed
//! request ([`DeepLinkController::on_shortcut`]): the user pressed it, so it
//! skips the deep-link setting and the confirm, and keeps the PIN refusal,
//! the platform checks, the tab change, the lookup and the toasts.

use super::link::{find_account, parse_deep_link, DeepLinkSwitchRequest};
use crate::accounts::loading::is_platform_usable;
use crate::platforms::{PlatformAccount, PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;
use crate::text::Text;
use std::time::{Duration, Instant};

/// How long a link waits for boot to finish.
pub const BOOT_TIMEOUT: Duration = Duration::from_millis(20_000);
/// How long a link waits for the visible tab's accounts to load.
pub const LOAD_TIMEOUT: Duration = Duration::from_millis(15_000);

/// What the controller reads of the app on each [`DeepLinkController::poll`].
pub struct DeepLinkView<'a> {
    pub boot_ready: bool,
    pub pin_locked: bool,
    pub settings: &'a AppSettings,
    pub registry: &'a PlatformRegistry,
    pub runtime_os: RuntimeOs,
    pub active_tab: &'a str,
    pub loader_loading: bool,
    /// The visible tab's loaded accounts.
    pub accounts: &'a [PlatformAccount],
}

impl DeepLinkView<'_> {
    /// True while the visible tab still loads and the wait started at
    /// `since` has not timed out.
    fn loader_busy(&self, since: Instant, now: Instant) -> bool {
        self.loader_loading && now.duration_since(since) <= LOAD_TIMEOUT
    }
}

/// The work the controller hands the shell. Each one is answered with the
/// matching [`DeepLinkAnswer`].
#[derive(Debug, Clone, PartialEq)]
pub enum DeepLinkAction {
    Toast(Text),
    /// Load the accounts of this platform: answer `PlatformAccounts`.
    LoadPlatformAccounts(String),
    /// Reload the visible tab's accounts: answer `AccountsLoaded`.
    LoadAccounts,
    /// Ask the user to confirm the switch: answer `Confirmed`. A shell
    /// without the gate answers `true` at once.
    Confirm {
        account: PlatformAccount,
        platform_name: String,
    },
    /// Show this platform's tab: answer `TabChanged`.
    ChangeTab(String),
    /// Switch to the account: answer `Switched`.
    Switch(PlatformAccount),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeepLinkAnswer {
    PlatformAccounts(Vec<PlatformAccount>),
    AccountsLoaded,
    Confirmed(bool),
    TabChanged,
    Switched(bool),
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    /// The URL waits for boot; it goes on after the timeout.
    WaitBootForUrl {
        url: String,
        since: Instant,
    },
    /// The parsed link waits for boot; it gives up after the timeout.
    WaitBootForSwitch {
        request: DeepLinkSwitchRequest,
        shortcut: bool,
        since: Instant,
    },
    AwaitPlatformAccounts(Switch),
    WaitLoader {
        switch: Switch,
        since: Instant,
    },
    AwaitReload(Switch),
    AwaitConfirm(Switch),
    AwaitTabChange(Switch),
    WaitLoaderAfterTab {
        switch: Switch,
        since: Instant,
    },
    AwaitReloadAfterTab(Switch),
    AwaitSwitch(Switch),
}

#[derive(Debug, Clone, PartialEq)]
struct Switch {
    request: DeepLinkSwitchRequest,
    /// From a global shortcut: no confirm.
    shortcut: bool,
    platform_name: String,
    on_other_tab: bool,
    account: Option<PlatformAccount>,
}

impl Switch {
    /// The confirmed account, found again by id in a fresh list.
    fn confirmed_in<'a>(&self, accounts: &'a [PlatformAccount]) -> Option<&'a PlatformAccount> {
        let id = self
            .account
            .as_ref()
            .map_or("", |account| account.id.as_str());
        accounts.iter().find(|candidate| candidate.id == id)
    }

    /// The name the success toast shows: display name, username or id.
    fn account_label(&self) -> String {
        self.account
            .as_ref()
            .and_then(|account| {
                [&account.display_name, &account.username, &account.id]
                    .into_iter()
                    .find(|value| !value.is_empty())
                    .cloned()
            })
            .unwrap_or_default()
    }
}

/// Why a link is refused before any account is looked up, as the toast to
/// show; else the platform's name.
fn admit_platform(
    request: &DeepLinkSwitchRequest,
    shortcut: bool,
    view: &DeepLinkView<'_>,
) -> Result<String, Text> {
    if !shortcut && !view.settings.deep_links_enabled {
        return Err(Text::key("toast.deepLinkDisabled"));
    }
    let Some(def) = view.registry.definition(&request.platform_id) else {
        return Err(Text::with(
            "toast.deepLinkUnknownPlatform",
            [("platform", request.platform_id.clone())],
        ));
    };
    let refuse = |key: &str| Text::with(key, [("platform", def.name.clone())]);
    if !is_platform_usable(view.registry, &request.platform_id, view.runtime_os) {
        return Err(refuse("app.platformUnsupportedOs"));
    }
    if !view
        .settings
        .enabled_platforms
        .contains(&request.platform_id)
    {
        return Err(refuse("toast.deepLinkPlatformDisabled"));
    }
    Ok(def.name.clone())
}

/// What waits for the link in flight to end.
#[derive(Debug, Clone, PartialEq)]
enum Incoming {
    Url(String),
    Shortcut(DeepLinkSwitchRequest),
}

/// One link at a time, the latest one queued.
#[derive(Debug)]
pub struct DeepLinkController {
    started: bool,
    phase: Phase,
    pending: Option<Incoming>,
}

impl Default for DeepLinkController {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepLinkController {
    pub fn new() -> Self {
        Self {
            started: false,
            phase: Phase::Idle,
            pending: None,
        }
    }

    /// Listen from now on. URLs delivered before are not replayed.
    pub fn start(&mut self) {
        self.started = true;
    }

    /// Stop listening. A link in flight finishes.
    pub fn stop(&mut self) {
        self.started = false;
    }

    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// The listener: each URL is handled in turn.
    pub fn on_urls<S: AsRef<str>>(&mut self, urls: &[S], now: Instant) {
        if !self.started {
            return;
        }
        for url in urls {
            self.handle_url(url.as_ref().to_string(), now);
        }
    }

    /// A global shortcut bound to an account fired: its switch, queued like
    /// a link.
    pub fn on_shortcut(&mut self, request: DeepLinkSwitchRequest, now: Instant) {
        self.handle(Incoming::Shortcut(request), now);
    }

    /// True while the switch in flight came from a global shortcut.
    pub fn is_shortcut(&self) -> bool {
        match &self.phase {
            Phase::Idle | Phase::WaitBootForUrl { .. } => false,
            Phase::WaitBootForSwitch { shortcut, .. } => *shortcut,
            Phase::AwaitPlatformAccounts(switch)
            | Phase::WaitLoader { switch, .. }
            | Phase::AwaitReload(switch)
            | Phase::AwaitConfirm(switch)
            | Phase::AwaitTabChange(switch)
            | Phase::WaitLoaderAfterTab { switch, .. }
            | Phase::AwaitReloadAfterTab(switch)
            | Phase::AwaitSwitch(switch) => switch.shortcut,
        }
    }

    fn handle_url(&mut self, url: String, now: Instant) {
        self.handle(Incoming::Url(url), now);
    }

    /// Starts a link; while busy, only the latest one is kept for later.
    fn handle(&mut self, incoming: Incoming, now: Instant) {
        if self.is_busy() {
            self.pending = Some(incoming);
            return;
        }
        self.phase = match incoming {
            Incoming::Url(url) => Phase::WaitBootForUrl { url, since: now },
            Incoming::Shortcut(request) => Phase::WaitBootForSwitch {
                request,
                shortcut: true,
                since: now,
            },
        };
    }

    /// Ends the link in flight and starts the queued one, if any.
    fn finish(&mut self, now: Instant) {
        self.phase = Phase::Idle;
        if let Some(next) = self.pending.take() {
            self.handle(next, now);
        }
    }

    /// Advances as far as the view allows; call once per frame while busy.
    pub fn poll(&mut self, view: &DeepLinkView<'_>, now: Instant) -> Vec<DeepLinkAction> {
        let mut actions = Vec::new();
        loop {
            let before = self.phase.clone();
            self.step(view, now, &mut actions);
            if self.phase == before {
                return actions;
            }
        }
    }

    fn step(&mut self, view: &DeepLinkView<'_>, now: Instant, actions: &mut Vec<DeepLinkAction>) {
        match std::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => {}
            Phase::WaitBootForUrl { url, since } => {
                if !view.boot_ready && now.duration_since(since) <= BOOT_TIMEOUT {
                    self.phase = Phase::WaitBootForUrl { url, since };
                    return;
                }
                let Some(request) = parse_deep_link(&url) else {
                    if view.settings.deep_links_enabled {
                        actions.push(DeepLinkAction::Toast(Text::key("toast.deepLinkInvalid")));
                    }
                    return self.finish(now);
                };
                self.phase = Phase::WaitBootForSwitch {
                    request,
                    shortcut: false,
                    since: now,
                };
            }
            Phase::WaitBootForSwitch {
                request,
                shortcut,
                since,
            } => {
                if view.boot_ready {
                    return self.gate(request, shortcut, view, now, actions);
                }
                if now.duration_since(since) > BOOT_TIMEOUT {
                    return self.finish(now);
                }
                self.phase = Phase::WaitBootForSwitch {
                    request,
                    shortcut,
                    since,
                };
            }
            Phase::WaitLoader { switch, since } => {
                if view.active_tab != switch.request.platform_id {
                    actions.push(DeepLinkAction::LoadPlatformAccounts(
                        switch.request.platform_id.clone(),
                    ));
                    self.phase = Phase::AwaitPlatformAccounts(switch);
                    return;
                }
                if view.loader_busy(since, now) {
                    self.phase = Phase::WaitLoader { switch, since };
                    return;
                }
                match find_account(view.accounts, &switch.request.account_ref) {
                    Some(account) => self.found(switch, account.clone(), view, now, actions),
                    None => {
                        actions.push(DeepLinkAction::LoadAccounts);
                        self.phase = Phase::AwaitReload(switch);
                    }
                }
            }
            Phase::WaitLoaderAfterTab { switch, since } => {
                if view.loader_busy(since, now) {
                    self.phase = Phase::WaitLoaderAfterTab { switch, since };
                    return;
                }
                match switch.confirmed_in(view.accounts) {
                    Some(account) => self.switch(switch, account.clone(), actions),
                    None => {
                        actions.push(DeepLinkAction::LoadAccounts);
                        self.phase = Phase::AwaitReloadAfterTab(switch);
                    }
                }
            }
            waiting => self.phase = waiting,
        }
    }

    /// The checks a parsed link passes once boot is done, then the account
    /// lookup on its platform's tab or another one.
    fn gate(
        &mut self,
        request: DeepLinkSwitchRequest,
        shortcut: bool,
        view: &DeepLinkView<'_>,
        now: Instant,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        if view.pin_locked {
            return self.finish(now);
        }
        let platform_name = match admit_platform(&request, shortcut, view) {
            Ok(name) => name,
            Err(toast) => {
                actions.push(DeepLinkAction::Toast(toast));
                return self.finish(now);
            }
        };
        let on_other_tab = view.active_tab != request.platform_id;
        let switch = Switch {
            shortcut,
            platform_name,
            on_other_tab,
            account: None,
            request,
        };
        if on_other_tab {
            actions.push(DeepLinkAction::LoadPlatformAccounts(
                switch.request.platform_id.clone(),
            ));
            self.phase = Phase::AwaitPlatformAccounts(switch);
        } else {
            self.phase = Phase::WaitLoader { switch, since: now };
        }
    }

    /// Asks to confirm the account the link names in `accounts`, or ends
    /// the link with the not-found toast.
    fn confirm_named(
        &mut self,
        switch: Switch,
        accounts: &[PlatformAccount],
        view: &DeepLinkView<'_>,
        actions: &mut Vec<DeepLinkAction>,
        now: Instant,
    ) {
        match find_account(accounts, &switch.request.account_ref) {
            Some(account) => self.found(switch, account.clone(), view, now, actions),
            None => self.not_found(&switch, actions, now),
        }
    }

    fn not_found(&mut self, switch: &Switch, actions: &mut Vec<DeepLinkAction>, now: Instant) {
        actions.push(DeepLinkAction::Toast(Text::with(
            "toast.deepLinkAccountNotFound",
            [
                ("platform", switch.platform_name.clone()),
                ("account", switch.request.account_ref.clone()),
            ],
        )));
        self.finish(now);
    }

    /// The account is found: a link asks to confirm it, a shortcut goes on.
    fn found(
        &mut self,
        mut switch: Switch,
        account: PlatformAccount,
        view: &DeepLinkView<'_>,
        now: Instant,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        if switch.shortcut {
            switch.account = Some(account);
            return self.confirmed(switch, view, now, actions);
        }
        actions.push(DeepLinkAction::Confirm {
            account: account.clone(),
            platform_name: switch.platform_name.clone(),
        });
        switch.account = Some(account);
        self.phase = Phase::AwaitConfirm(switch);
    }

    fn switch(
        &mut self,
        mut switch: Switch,
        account: PlatformAccount,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        actions.push(DeepLinkAction::Switch(account.clone()));
        switch.account = Some(account);
        self.phase = Phase::AwaitSwitch(switch);
    }

    /// After a confirmed switch: change tab first when needed, else switch.
    fn confirmed(
        &mut self,
        switch: Switch,
        view: &DeepLinkView<'_>,
        now: Instant,
        actions: &mut Vec<DeepLinkAction>,
    ) {
        if switch.on_other_tab || view.active_tab != switch.request.platform_id {
            actions.push(DeepLinkAction::ChangeTab(
                switch.request.platform_id.clone(),
            ));
            self.phase = Phase::AwaitTabChange(switch);
        } else if let Some(account) = switch.account.clone() {
            self.switch(switch, account, actions);
        } else {
            self.finish(now);
        }
    }

    /// The work the last action started failed (a platform's account list
    /// failed to load). The link ends without a toast and the queued one,
    /// if any, starts.
    pub fn fail(&mut self, view: &DeepLinkView<'_>, now: Instant) -> Vec<DeepLinkAction> {
        if self.is_busy() {
            self.finish(now);
        }
        self.poll(view, now)
    }

    /// The shell's answer to the last action. `view` is read after it.
    pub fn answer(
        &mut self,
        answer: DeepLinkAnswer,
        view: &DeepLinkView<'_>,
        now: Instant,
    ) -> Vec<DeepLinkAction> {
        let mut actions = Vec::new();
        match (std::mem::replace(&mut self.phase, Phase::Idle), answer) {
            (Phase::AwaitPlatformAccounts(switch), DeepLinkAnswer::PlatformAccounts(accounts)) => {
                self.confirm_named(switch, &accounts, view, &mut actions, now);
            }
            (Phase::AwaitReload(switch), DeepLinkAnswer::AccountsLoaded) => {
                if view.active_tab != switch.request.platform_id {
                    actions.push(DeepLinkAction::LoadPlatformAccounts(
                        switch.request.platform_id.clone(),
                    ));
                    self.phase = Phase::AwaitPlatformAccounts(switch);
                } else {
                    self.confirm_named(switch, view.accounts, view, &mut actions, now);
                }
            }
            (Phase::AwaitConfirm(switch), DeepLinkAnswer::Confirmed(allowed)) => {
                if allowed {
                    self.confirmed(switch, view, now, &mut actions);
                } else {
                    self.finish(now);
                }
            }
            (Phase::AwaitTabChange(switch), DeepLinkAnswer::TabChanged) => {
                if view.active_tab != switch.request.platform_id {
                    self.finish(now);
                } else {
                    self.phase = Phase::WaitLoaderAfterTab { switch, since: now };
                }
            }
            (Phase::AwaitReloadAfterTab(switch), DeepLinkAnswer::AccountsLoaded) => {
                match switch.confirmed_in(view.accounts) {
                    Some(account) => self.switch(switch, account.clone(), &mut actions),
                    None => self.not_found(&switch, &mut actions, now),
                }
            }
            (Phase::AwaitSwitch(switch), DeepLinkAnswer::Switched(switched)) => {
                if switched {
                    actions.push(DeepLinkAction::Toast(Text::with(
                        "toast.deepLinkSwitched",
                        [("account", switch.account_label())],
                    )));
                }
                self.finish(now);
            }
            (phase, _) => self.phase = phase,
        }
        actions.extend(self.poll(view, now));
        actions
    }
}
