//! The add-account flow controller, ported from
//! `src/lib/app/platformAddFlow.svelte.ts` with its App.svelte glue (lines
//! 127-138, 687-690).
//!
//! [`crate::accounts::loading::AccountLoading`] starts a setup and reports it
//! as `LoadingEvent::AddFlowStarted`; this controller then polls it every
//! 1.5 s through the platform adapter, draws the pending card and its card
//! extension, and ends it on ready, failure or cancel. Polls, cancels,
//! telemetry and the profile prime run on [`Jobs`]; what the app must do in
//! return comes back as [`AddFlowEvent`]s.

use crate::accounts::loader::LoadOptions;
use crate::accounts::profile_cache::{CacheWrite, ProfileCaches};
use crate::accounts::{Toast, ToastKind};
use crate::error::{BackendError, BackendResult};
use crate::i18n::Locale;
use crate::jobs::{Jobs, Pending, Poll};
use crate::platforms::card::{CardChip, CardLink, CardSection, ChipTone};
use crate::platforms::{AddFlowState, AddFlowStatus, PlatformAccount, PlatformRegistry};
use crate::stores::ClientStores;
use crate::text::Text;
use crate::time::Clock;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The `setTimeout` of `schedulePoll` (line 155).
pub const POLL_DELAY: Duration = Duration::from_millis(1500);

/// The kinds of `getSetupKey` (lines 37-59).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupKey {
    PendingLabel,
    WaitingForClient,
    WaitingForLogin,
    Detected,
    Connected,
    Ready,
    ReadyWithProfile,
    Failed,
    FailedMessage,
}

impl SetupKey {
    fn suffix(self) -> &'static str {
        match self {
            SetupKey::PendingLabel => "PendingLabel",
            SetupKey::WaitingForClient => "WaitingForClient",
            SetupKey::WaitingForLogin => "WaitingForLogin",
            SetupKey::Detected => "Detected",
            SetupKey::Connected => "Connected",
            SetupKey::Ready => "Ready",
            SetupKey::ReadyWithProfile => "ReadyWithProfile",
            SetupKey::Failed => "Failed",
            SetupKey::FailedMessage => "FailedMessage",
        }
    }
}

/// `getSetupKey`: `<prefix>.setup<Kind>`, battle-net under `battlenet`.
pub fn setup_key(platform_id: &str, kind: SetupKey) -> String {
    let prefix = match platform_id {
        "battle-net" => "battlenet",
        other => other,
    };
    format!("{prefix}.setup{}", kind.suffix())
}

/// `PlatformAddFlowEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddFlowEntry {
    pub platform_id: String,
    pub status: AddFlowStatus,
}

/// What the flow reads from the app (`getActiveTab`, `getCurrentFolderId`,
/// `getIsSearching`).
#[derive(Debug, Clone, Copy)]
pub struct AddFlowView<'a> {
    pub active_tab: &'a str,
    pub current_folder_id: Option<&'a str>,
    pub is_searching: bool,
}

/// What the flow needs to run its jobs.
pub struct AddFlowContext<'a> {
    pub jobs: &'a Jobs,
    pub registry: &'a PlatformRegistry,
    pub view: AddFlowView<'a>,
    /// The profile caches the account loader uses, for the prime after an
    /// add (`primeProfileAfterAdd`).
    pub caches: &'a Arc<ProfileCaches>,
    pub clock: &'a Clock,
    pub now: Instant,
}

/// What the app does in return.
#[derive(Debug, Clone, PartialEq)]
pub enum AddFlowEvent {
    /// `showToast(message)`.
    Toast(Toast),
    /// `loadAccounts(...)` on the active tab.
    LoadAccounts(LoadOptions),
    /// `onAccountAdded`: App.svelte opens the rename prompt
    /// (`dialogs.promptRenameNewAccount`, lines 135-137).
    AccountAdded {
        platform_id: String,
        account_id: String,
        display_name: Option<String>,
    },
}

/// The pending card (`pendingSetupAccount`, lines 87-101). Texts resolve at
/// draw time; [`PendingSetupAccount::to_account`] gives the card's account.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSetupAccount {
    pub id: String,
    pub display_name: Text,
    pub username: Text,
}

impl PendingSetupAccount {
    pub fn to_account(&self, locale: Locale) -> PlatformAccount {
        PlatformAccount {
            id: self.id.clone(),
            display_name: self.display_name.resolve(locale),
            username: self.username.resolve(locale),
            last_login_at_sec: None,
        }
    }
}

/// What a chip of the setup card does (the TS `onClick`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupChipAction {
    /// The red Close chip: [`PlatformAddFlow::cancel`].
    Cancel,
    /// Roblox's "Copy code": put the quick login code on the clipboard
    /// (`copyToClipboard(text, text)`, App.svelte line 133).
    CopyCode(String),
}

/// `getSetupExtensionContent`: the sections and which chips act.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetupExtension {
    pub sections: Vec<CardSection>,
    /// `((section, chip), action)` for the chips that have an `onClick`.
    pub chip_actions: Vec<((usize, usize), SetupChipAction)>,
}

impl SetupExtension {
    /// The action of chip `chip` in section `section`, if it has one.
    pub fn chip_action(&self, section: usize, chip: usize) -> Option<&SetupChipAction> {
        self.chip_actions
            .iter()
            .find(|(at, _)| *at == (section, chip))
            .map(|(_, action)| action)
    }
}

type PrimeJob = Pending<(String, Vec<CacheWrite>)>;

/// `createPlatformAddFlowController`.
#[derive(Default)]
pub struct PlatformAddFlow {
    flow: Option<AddFlowEntry>,
    poll_deadline: Option<Instant>,
    /// The setup id the poll was sent for, and its answer.
    poll: Option<(String, Pending<BackendResult<AddFlowStatus>>)>,
    /// The flow being cancelled, and the cancel's answer.
    cancel: Option<(AddFlowEntry, Pending<BackendResult<()>>)>,
    primes: Vec<PrimeJob>,
    events: Vec<AddFlowEvent>,
}

impl PlatformAddFlow {
    pub fn new() -> Self {
        Self::default()
    }

    /// `flow`.
    pub fn flow(&self) -> Option<&AddFlowEntry> {
        self.flow.as_ref()
    }

    /// The `add_flow_active` flag `AccountLoading::handle_add_account_click`
    /// takes.
    pub fn is_active(&self) -> bool {
        self.flow.is_some()
    }

    /// Jobs in flight.
    pub fn is_busy(&self) -> bool {
        self.poll.is_some() || self.cancel.is_some() || !self.primes.is_empty()
    }

    /// When [`Self::tick`] next has work.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.poll_deadline
    }

    /// `clearTimer` (lines 109-113).
    pub fn clear_timer(&mut self) {
        self.poll_deadline = None;
    }

    /// `stop` (lines 115-118).
    pub fn stop(&mut self) {
        self.clear_timer();
        self.flow = None;
    }

    fn schedule_poll(&mut self, now: Instant) {
        self.poll_deadline = Some(now + POLL_DELAY);
    }

    /// `start` (lines 228-245): a setup the account loader just began.
    pub fn start(&mut self, ctx: &AddFlowContext<'_>, platform_id: &str, status: AddFlowStatus) {
        self.flow = Some(AddFlowEntry {
            platform_id: platform_id.to_string(),
            status: status.clone(),
        });
        // The opening is the denominator of the funnel.
        track(ctx.jobs, platform_id, |backend, id| {
            backend.telemetry_track_account_add_started(id)
        });
        match status.state() {
            // Some setups finish at once (Discord adopting the session).
            AddFlowState::Ready => self.handle_ready(ctx, platform_id, &status),
            AddFlowState::Failed => report_add_failure(ctx.jobs, platform_id),
            _ => self.schedule_poll(ctx.now),
        }
    }

    /// `poll` (lines 158-194), up to the backend call: asks the adapter for
    /// the setup's status now. Ends the flow when its platform has no
    /// adapter.
    pub fn poll_now(&mut self, ctx: &AddFlowContext<'_>) {
        self.clear_timer();
        let Some(current) = self.flow.clone() else {
            self.stop();
            return;
        };
        let Some(adapter) = ctx.registry.adapter(&current.platform_id) else {
            self.stop();
            return;
        };
        let setup_id = current.status.setup_id.clone();
        let id = setup_id.clone();
        self.poll = Some((
            setup_id,
            ctx.jobs
                .run(move |backend| adapter.poll_add_flow(backend, &id)),
        ));
    }

    /// The end of `poll`.
    fn apply_poll(
        &mut self,
        ctx: &AddFlowContext<'_>,
        setup_id: &str,
        result: BackendResult<AddFlowStatus>,
    ) {
        let Some(flow) = self.flow.as_mut() else {
            return;
        };
        if flow.status.setup_id != setup_id {
            return;
        }
        let platform_id = flow.platform_id.clone();
        match result {
            Ok(next) => {
                flow.status = next.clone();
                match next.state() {
                    AddFlowState::Ready => self.handle_ready(ctx, &platform_id, &next),
                    AddFlowState::Failed => report_add_failure(ctx.jobs, &platform_id),
                    _ => self.schedule_poll(ctx.now),
                }
            }
            Err(error) => {
                report_add_failure(ctx.jobs, &platform_id);
                flow.status.state = "failed".to_string();
                flow.status.error_message = Some(error.message);
            }
        }
    }

    /// `handleReady` (lines 206-226).
    fn handle_ready(
        &mut self,
        ctx: &AddFlowContext<'_>,
        platform_id: &str,
        status: &AddFlowStatus,
    ) {
        track(ctx.jobs, platform_id, |backend, id| {
            backend.telemetry_track_account_added(id)
        });
        let adapter = ctx.registry.adapter(platform_id);
        let prime = ctx
            .registry
            .definition(platform_id)
            .is_some_and(|def| def.capabilities.prime_profile_after_add);
        if let (true, Some(adapter), Some(account_id)) =
            (prime, adapter.clone(), status.account_id.clone())
        {
            let caches = Arc::clone(ctx.caches);
            let clock = ctx.clock.clone();
            let id = platform_id.to_string();
            self.primes.push(ctx.jobs.run(move |backend| {
                let fetched =
                    caches.profile_info(&adapter, backend, &account_id, &|| clock.now_ms());
                (id, fetched.writes)
            }));
        }
        if ctx.view.active_tab == platform_id {
            self.events.push(AddFlowEvent::LoadAccounts(LoadOptions {
                silent: true,
                ..LoadOptions::default()
            }));
        }
        let text = match status.account_display_name.as_deref() {
            Some(profile) if !profile.is_empty() => Text::with(
                setup_key(platform_id, SetupKey::ReadyWithProfile),
                [("profile", profile)],
            ),
            _ => Text::key(setup_key(platform_id, SetupKey::Ready)),
        };
        self.events
            .push(AddFlowEvent::Toast(Toast::new(text, ToastKind::Default)));
        self.stop();
        if let (Some(adapter), Some(account_id)) = (adapter, status.account_id.as_deref()) {
            if adapter.supports_account_labels() && !account_id.is_empty() {
                self.events.push(AddFlowEvent::AccountAdded {
                    platform_id: platform_id.to_string(),
                    account_id: account_id.to_string(),
                    display_name: status.account_display_name.clone(),
                });
            }
        }
    }

    /// `cancel` (lines 120-141), up to the backend call. The poll timer stops
    /// at once, so no poll races the cancel.
    pub fn cancel(&mut self, ctx: &AddFlowContext<'_>) {
        let Some(current) = self.flow.clone() else {
            self.stop();
            return;
        };
        // Counted whether or not the adapter has anything to cancel: the
        // funnel measures the user walking away.
        track(ctx.jobs, &current.platform_id, |backend, id| {
            backend.telemetry_track_account_add_cancelled(id)
        });
        let Some(adapter) = ctx.registry.adapter(&current.platform_id) else {
            self.stop();
            return;
        };
        if self.cancel.is_some() {
            return;
        }
        self.clear_timer();
        let setup_id = current.status.setup_id.clone();
        self.cancel = Some((
            current,
            ctx.jobs
                .run(move |backend| adapter.cancel_add_flow(backend, &setup_id)),
        ));
    }

    /// `cancelIfConflicting` (lines 143-149): a switch, a back navigation or
    /// a palette jump on the flow's platform drops it, unless the target is
    /// the setup account itself.
    pub fn cancel_if_conflicting(
        &mut self,
        ctx: &AddFlowContext<'_>,
        target_platform_id: &str,
        target_account_id: Option<&str>,
    ) {
        let Some(current) = self.flow.as_ref() else {
            return;
        };
        if current.platform_id != target_platform_id {
            return;
        }
        if target_account_id.is_some_and(|id| !id.is_empty() && current.status.setup_id == id) {
            return;
        }
        self.cancel(ctx);
    }

    fn apply_cancel(
        &mut self,
        ctx: &AddFlowContext<'_>,
        entry: AddFlowEntry,
        result: BackendResult<()>,
    ) {
        if let Err(BackendError { message, .. }) = result {
            self.events.push(AddFlowEvent::Toast(Toast::new(
                Text::raw(message),
                ToastKind::Default,
            )));
        }
        self.stop();
        if ctx.view.active_tab == entry.platform_id {
            self.events.push(AddFlowEvent::LoadAccounts(LoadOptions {
                silent: true,
                ..LoadOptions::default()
            }));
        }
    }

    /// One frame: applies finished polls, cancels and primes, then sends the
    /// next poll when due.
    pub fn tick(
        &mut self,
        ctx: &AddFlowContext<'_>,
        stores: &mut ClientStores,
    ) -> Vec<AddFlowEvent> {
        if let Some((setup_id, pending)) = self.poll.as_mut() {
            let answer = match pending.poll() {
                Poll::Waiting => None,
                Poll::Ready(result) => Some(result),
                Poll::Gone => Some(Err(BackendError::new("Setup status check stopped"))),
            };
            if let Some(result) = answer {
                let setup_id = setup_id.clone();
                self.poll = None;
                self.apply_poll(ctx, &setup_id, result);
            }
        }
        if let Some((_, pending)) = self.cancel.as_mut() {
            let answer = match pending.poll() {
                Poll::Waiting => None,
                Poll::Ready(result) => Some(result),
                Poll::Gone => Some(Ok(())),
            };
            if let Some(result) = answer {
                if let Some((entry, _)) = self.cancel.take() {
                    self.apply_cancel(ctx, entry, result);
                }
            }
        }
        let caches = ctx.caches;
        self.primes.retain_mut(|pending| match pending.poll() {
            Poll::Waiting => true,
            Poll::Ready((platform_id, writes)) => {
                caches.apply_writes(&platform_id, stores.storage_mut(), writes);
                false
            }
            Poll::Gone => false,
        });
        if self.poll_deadline.is_some_and(|due| ctx.now >= due) && self.poll.is_none() {
            self.poll_now(ctx);
        }
        std::mem::take(&mut self.events)
    }

    // ------------------------------------------------------------ views

    fn shown_on(&self, view: &AddFlowView<'_>) -> Option<&AddFlowEntry> {
        let flow = self.flow.as_ref()?;
        if view.active_tab != flow.platform_id {
            return None;
        }
        if view.is_searching || view.current_folder_id.is_some_and(|id| !id.is_empty()) {
            return None;
        }
        Some(flow)
    }

    /// `pendingSetupAccount` (lines 87-101): the placeholder card at the
    /// root of the flow's tab, until the setup is ready.
    pub fn pending_setup_account(&self, view: &AddFlowView<'_>) -> Option<PendingSetupAccount> {
        let flow = self.shown_on(view)?;
        if flow.status.state() == AddFlowState::Ready {
            return None;
        }
        let setup_id = crate::js::trim(&flow.status.setup_id);
        if setup_id.is_empty() {
            return None;
        }
        let detected = crate::js::trim(
            flow.status
                .account_display_name
                .as_deref()
                .unwrap_or_default(),
        );
        let (display_name, username) = if detected.is_empty() {
            (
                Text::key("platform.newAccountPending"),
                pending_username(flow),
            )
        } else {
            (
                Text::raw(detected),
                Text::key(setup_key(&flow.platform_id, SetupKey::Connected)),
            )
        };
        Some(PendingSetupAccount {
            id: setup_id.to_string(),
            display_name,
            username,
        })
    }

    /// `isPendingSetupAccount` (lines 457-459).
    pub fn is_pending_setup_account(&self, view: &AddFlowView<'_>, account_id: &str) -> bool {
        self.pending_setup_account(view)
            .is_some_and(|pending| pending.id == account_id)
    }

    /// `pendingSetupAccountId` (App.svelte line 687).
    pub fn pending_setup_account_id(&self, view: &AddFlowView<'_>) -> Option<String> {
        self.pending_setup_account(view).map(|pending| pending.id)
    }

    /// `activePlatformAddSetupId` (App.svelte lines 688-690): the setup id
    /// when the flow belongs to the active tab.
    pub fn active_platform_setup_id(&self, active_tab: &str) -> Option<&str> {
        self.flow
            .as_ref()
            .filter(|flow| flow.platform_id == active_tab)
            .map(|flow| flow.status.setup_id.as_str())
    }

    /// `isForcedOpen` (lines 448-455): the setup card stays expanded.
    pub fn is_forced_open(&self, active_tab: &str, account_id: &str) -> bool {
        self.flow.as_ref().is_some_and(|flow| {
            flow.platform_id == active_tab
                && flow.status.setup_id == account_id
                && flow.status.state() != AddFlowState::Ready
        })
    }

    /// `getSetupExtensionContent` (lines 247-446). `locale` resolves the
    /// plain `lines`, which the card model keeps as strings.
    pub fn setup_extension_content(
        &self,
        account_id: &str,
        locale: Locale,
    ) -> Option<SetupExtension> {
        let flow = self.flow.as_ref()?;
        let setup_id = crate::js::trim(&flow.status.setup_id);
        if setup_id.is_empty() || setup_id != account_id {
            return None;
        }
        Some(setup_extension(flow, locale)).flatten()
    }
}

/// The one line the card shows while nothing is detected (lines 104-107).
fn pending_username(entry: &AddFlowEntry) -> Text {
    if entry.status.state() == AddFlowState::Busy {
        Text::key("platform.setupBusy")
    } else {
        Text::key(setup_key(&entry.platform_id, SetupKey::WaitingForLogin))
    }
}

fn track(jobs: &Jobs, platform_id: &str, call: fn(&dyn crate::Backend, &str) -> BackendResult<()>) {
    // Fire and forget (`telemetryClient.track`): the mock and an unconsented
    // backend refuse, nobody cares.
    let id = platform_id.to_string();
    drop(jobs.run(move |backend| {
        let _ = call(backend, &id);
    }));
}

/// `reportAddFailure` (lines 202-204): the adapters report a message, not a
/// typed kind, so the code is `other`.
fn report_add_failure(jobs: &Jobs, platform_id: &str) {
    let id = platform_id.to_string();
    drop(jobs.run(move |backend| {
        let _ = backend.telemetry_track_operation_failed("account_add", "other", Some(&id));
    }));
}

fn section_text(text: Text, loading: bool) -> CardSection {
    CardSection {
        text: Some(text),
        loading,
        ..CardSection::default()
    }
}

fn section_lines(lines: Vec<String>) -> CardSection {
    CardSection {
        lines,
        ..CardSection::default()
    }
}

fn failed_section(platform_id: &str, text: Text) -> CardSection {
    CardSection {
        title: Some(Text::key(setup_key(platform_id, SetupKey::Failed))),
        text: Some(text),
        ..CardSection::default()
    }
}

/// `createDetectedSection` (lines 61-72).
fn detected_section(platform_id: &str, display: &str) -> Option<CardSection> {
    if display.is_empty() {
        return None;
    }
    Some(CardSection {
        title: Some(Text::key(setup_key(platform_id, SetupKey::Detected))),
        text: Some(Text::raw(display)),
        chips: vec![CardChip {
            text: Text::key(setup_key(platform_id, SetupKey::Connected)),
            tone: ChipTone::Green,
        }],
        ..CardSection::default()
    })
}

fn close_chip() -> CardChip {
    CardChip {
        text: Text::key("common.close"),
        tone: ChipTone::Red,
    }
}

/// The failure section with the error lines and the Close chip
/// (`{ ...(error ? { lines: [error] } : {}), chips: [closeChip] }`).
fn close_section(error: &str) -> CardSection {
    CardSection {
        lines: if error.is_empty() {
            Vec::new()
        } else {
            vec![error.to_string()]
        },
        chips: vec![close_chip()],
        ..CardSection::default()
    }
}

fn setup_extension(flow: &AddFlowEntry, locale: Locale) -> Option<SetupExtension> {
    let platform = flow.platform_id.as_str();
    let display = crate::js::trim(
        flow.status
            .account_display_name
            .as_deref()
            .unwrap_or_default(),
    );
    let error = crate::js::trim(flow.status.error_message.as_deref().unwrap_or_default());
    let detected = detected_section(platform, display);
    let state = flow.status.state();
    let line = |key: &str| Text::key(key).resolve(locale);
    let mut actions = Vec::new();

    // Before the per-platform branches: the cross-process lock is not a
    // platform concern. Non-terminal, so the spinner stays on.
    let sections: Vec<CardSection> = if state == AddFlowState::Busy {
        std::iter::once(section_text(Text::key("platform.setupBusy"), true))
            .chain(detected)
            .collect()
    } else {
        match (platform, state) {
            (_, AddFlowState::Ready) => return None,
            ("riot", AddFlowState::WaitingForClient) => vec![
                section_text(Text::key("riot.setupWaitingForClient"), true),
                section_lines(vec![line("riot.setupStaySignedIn")]),
            ],
            ("riot", AddFlowState::Capturing) => {
                std::iter::once(section_text(Text::key("riot.setupCapturing"), true))
                    .chain(detected)
                    .collect()
            }
            ("riot", AddFlowState::Failed) => {
                actions.push(((1, 0), SetupChipAction::Cancel));
                vec![
                    failed_section(platform, Text::key("riot.setupFailedMessage")),
                    close_section(error),
                ]
            }
            ("riot", _) => {
                let first = if error.is_empty() {
                    section_text(Text::key("riot.setupWaitingForLogin"), true)
                } else {
                    section_text(Text::raw(error), false)
                };
                let second =
                    detected.unwrap_or_else(|| section_lines(vec![line("riot.setupStaySignedIn")]));
                vec![first, second]
            }
            ("roblox", AddFlowState::Failed) => {
                actions.push(((1, 0), SetupChipAction::Cancel));
                let text = if error.is_empty() {
                    Text::key("roblox.setupFailedMessage")
                } else {
                    Text::raw(error)
                };
                vec![
                    failed_section(platform, text),
                    CardSection {
                        chips: vec![close_chip()],
                        ..CardSection::default()
                    },
                ]
            }
            ("roblox", _) => {
                let mut chips = vec![CardChip {
                    text: Text::raw("Quick Login"),
                    tone: ChipTone::Green,
                }];
                if !display.is_empty() {
                    actions.push(((0, 1), SetupChipAction::CopyCode(display.to_string())));
                    chips.push(CardChip {
                        text: Text::key("roblox.copyCode"),
                        tone: ChipTone::Slate,
                    });
                }
                vec![CardSection {
                    link: Some(CardLink {
                        label: Text::raw("roblox.com/quick-login"),
                        url: "https://www.roblox.com/crossdevicelogin/ConfirmCode".to_string(),
                    }),
                    chips,
                    loading: true,
                    ..CardSection::default()
                }]
            }
            ("battle-net", AddFlowState::WaitingForClient) => vec![section_text(
                Text::key("battlenet.setupWaitingForClient"),
                true,
            )],
            ("battle-net", AddFlowState::Failed) => {
                actions.push(((1, 0), SetupChipAction::Cancel));
                vec![
                    failed_section(platform, Text::key("battlenet.setupFailedMessage")),
                    close_section(error),
                ]
            }
            ("battle-net", _) => std::iter::once(section_text(
                Text::key("battlenet.setupWaitingForLogin"),
                true,
            ))
            .chain(detected)
            .chain(std::iter::once(section_lines(vec![line(
                "battlenet.setupKeepMeLoggedIn",
            )])))
            .collect(),
            (_, AddFlowState::WaitingForClient) => vec![section_text(
                Text::key(setup_key(platform, SetupKey::WaitingForClient)),
                true,
            )],
            (_, AddFlowState::Failed) => {
                actions.push(((1, 0), SetupChipAction::Cancel));
                vec![
                    failed_section(
                        platform,
                        Text::key(setup_key(platform, SetupKey::FailedMessage)),
                    ),
                    close_section(error),
                ]
            }
            _ => std::iter::once(section_text(
                Text::key(setup_key(platform, SetupKey::WaitingForLogin)),
                true,
            ))
            .chain(detected)
            .collect(),
        }
    };
    Some(SetupExtension {
        sections,
        chip_actions: actions,
    })
}
