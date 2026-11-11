//! The panel's background jobs and the per-frame tick that polls them.
//!
//! The tick polls each job family in a fixed order (the toasts they raise
//! keep that order), then runs the timers, the external settings watch and
//! the save debounce.

use std::time::Instant;

use accshift_app::accounts::refresh::apply_refresh_job;
use accshift_app::actions::toasts::{AddToastOptions, ToastQueue, ToastType};
use accshift_app::actions::{load_options, AppEffect};
use accshift_app::error::{BackendError, BackendResult};
use accshift_app::i18n::{Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::platforms::descriptors::PlatformDescriptor;
use accshift_app::text::Text;
use accshift_app::types::{DescriptorPreview, UserPlatformReport};
use egui::Context;
use serde_json::Value;

use super::super::platforms::{self, PlatformDef};
use super::super::{AppModel, Booted};
use super::draft::platform_snapshot;
use super::state::{visible_tabs, SettingsPanel};
use super::toast;

/// A descriptor folder call of the Platforms tab.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum DescriptorTask {
    Reload,
    Pick,
    Install { name: String },
    Remove { id: String, name: String },
    OpenFolder,
}

/// What a descriptor folder call answered.
pub(super) enum DescriptorDone {
    Report(UserPlatformReport),
    /// The picked file and its preview (boxed, it dwarfs the rest); `None`
    /// when the picker was cancelled.
    Picked(Option<Box<(String, DescriptorPreview)>>),
    Opened,
}

/// A platform path asked for when a chip enabled the platform, or picked
/// with "Choose...".
pub(super) type PathJob = Pending<(String, BackendResult<String>)>;

/// What a finished telemetry or link call does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Followup {
    /// A failure toasts this key as an error.
    ErrorToast(&'static str),
    /// A failure toasts this key with the default type.
    InfoToast(&'static str),
    /// The stored Steam API key was cleared.
    ClearApiKey,
    /// Telemetry mode A was switched: a failure reads the state again.
    ModeA,
    /// Telemetry mode B was switched to `next`.
    ModeB { next: bool },
    /// The deletion of the telemetry data was retried.
    RetryForget,
    /// The telemetry data was exported for the clipboard.
    Export,
}

impl Followup {
    /// The telemetry state is read again once the call is over.
    fn reads_telemetry_again(self, failed: bool) -> bool {
        match self {
            Followup::ModeA => failed,
            Followup::ModeB { .. } | Followup::RetryForget => true,
            _ => false,
        }
    }
}

/// The mount job's result: whether a Steam API key is stored, then each
/// enabled platform's path.
pub(super) type HydrateResult = (bool, Vec<(String, String)>);

/// The error a job whose worker died before answering stands for.
pub(super) fn cancelled<T>() -> BackendResult<T> {
    Err(BackendError::new("cancelled"))
}

/// Polls the job held in `slot`. Once it is over the slot empties and
/// `Some` comes out: the result, or `None` when the worker died first.
pub(super) fn settle<T>(slot: &mut Option<Pending<T>>) -> Option<Option<T>> {
    let landed = match slot.as_mut()?.poll() {
        Poll::Waiting => return None,
        Poll::Ready(result) => Some(result),
        Poll::Gone => None,
    };
    *slot = None;
    Some(landed)
}

/// Polls every job of `list` in order and takes out the ones that are
/// over: those that landed come back with their results, in order; a job
/// whose worker died leaves without a word.
pub(super) fn drain_landed<J, T>(
    list: &mut Vec<J>,
    mut poll: impl FnMut(&mut J) -> Poll<T>,
) -> Vec<(J, T)> {
    let mut landed = Vec::new();
    for mut job in std::mem::take(list) {
        match poll(&mut job) {
            Poll::Waiting => list.push(job),
            Poll::Ready(result) => landed.push((job, result)),
            Poll::Gone => {}
        }
    }
    landed
}

/// The folder's descriptors become the user platforms of the tabs, the
/// account registry and the stores.
fn apply_report(b: &mut Booted, report: &UserPlatformReport) {
    let loaded: Vec<Value> = report
        .loaded
        .iter()
        .filter_map(|d| serde_json::to_value(d).ok())
        .collect();
    b.platforms = platforms::with_user(&loaded);
    let descriptors: Vec<PlatformDescriptor> = loaded
        .iter()
        .filter_map(PlatformDescriptor::from_value)
        .collect();
    b.accounts.registry.register_user_platforms(&descriptors);
    let mut ctx = b.stores.context().clone();
    ctx.platform_ids = b.platforms.iter().map(|p| p.id.clone()).collect();
    b.stores.set_context(ctx);
    b.user_platforms = Some(report.clone());
}

/// A saved bridge setting reaches the account cards: the Steam adapter's
/// bridge data is dropped, and read again when the bridge is on.
fn reload_cs2_bridge(b: &mut Booted, enabled: bool) {
    let Some(steam) = b.accounts.registry.adapter("steam") else {
        return;
    };
    let mut state = steam.state();
    state.cs2.invalidate();
    if enabled {
        let now_ms = b.accounts.clock.now_ms();
        b.accounts.cs2.queue.load(&state.cs2, true, now_ms);
    }
}

/// A descriptor notice naming the platform.
fn platform_notice(locale: Locale, key: &str, name: &str) -> String {
    accshift_app::i18n::translate_with(
        locale,
        key,
        &[("platform", TranslationValue::Str(name.to_string()))],
    )
}

impl SettingsPanel {
    /// The mount job landed: the API key flag and the paths fill in, and the
    /// settings as the panel opened on count as saved. Saves wait for this.
    fn poll_hydrate(&mut self) {
        let Some(landed) = settle(&mut self.hydrate_job) else {
            return;
        };
        let (has_key, paths) = landed.unwrap_or_default();
        self.api_key_configured = has_key;
        self.platform_paths = paths.into_iter().collect();
        let at_start = self.persisted.clone();
        self.last_persisted_snapshot = self.persist_snapshot(&at_start);
        self.last_platform_snapshot = platform_snapshot(&at_start);
        self.hydrated = true;
    }

    fn poll_telemetry(&mut self) {
        let Some(landed) = settle(&mut self.telemetry_job) else {
            return;
        };
        match landed {
            Some(Ok(state)) => {
                self.telemetry = Some(state);
                self.telemetry_error = false;
            }
            Some(Err(_)) | None => self.telemetry_error = true,
        }
    }

    /// Polls the link and telemetry calls; true when one asks to read the
    /// telemetry state again.
    fn poll_calls(&mut self, toasts: &mut ToastQueue, now: Instant) -> bool {
        let mut refresh = false;
        for ((_, followup), result) in drain_landed(&mut self.calls, |(job, _)| job.poll()) {
            refresh |= followup.reads_telemetry_again(result.is_err());
            self.follow_up(followup, result, toasts, now);
        }
        refresh
    }

    /// What a finished call changes in the panel, and the toast it raises.
    fn follow_up(
        &mut self,
        followup: Followup,
        result: BackendResult<Value>,
        toasts: &mut ToastQueue,
        now: Instant,
    ) {
        match (followup, result) {
            (Followup::ErrorToast(key), Err(_)) => toast(toasts, key, ToastType::Error, now),
            (Followup::InfoToast(key), Err(_)) => toast(toasts, key, ToastType::Info, now),
            (Followup::ClearApiKey, Ok(_)) => {
                self.api_key_configured = false;
                self.api_key.clear();
                self.api_key_touched = false;
                self.api_key_error = false;
                toast(toasts, "settings.apiKeyCleared", ToastType::Info, now);
            }
            (Followup::ClearApiKey, Err(_)) => {
                toast(toasts, "settings.apiKeyClearFailed", ToastType::Error, now);
            }
            (Followup::ModeB { next }, result) => {
                self.mode_b_busy = false;
                if result.is_err() {
                    toast(toasts, self.mode_b_failure_key(next), ToastType::Info, now);
                }
            }
            (Followup::RetryForget, result) => {
                self.mode_b_busy = false;
                let key = if result.is_ok() {
                    "settings.telemetryDeleteCompleted"
                } else {
                    "settings.telemetryDeleteRetryFailed"
                };
                toast(toasts, key, ToastType::Info, now);
            }
            (Followup::Export, result) => {
                self.export_busy = false;
                let key = match result {
                    Ok(data) => {
                        let text = serde_json::to_string_pretty(&data).unwrap_or_default();
                        self.clipboard = Some(text);
                        "settings.telemetryExported"
                    }
                    Err(_) => "settings.telemetryExportFailed",
                };
                toast(toasts, key, ToastType::Info, now);
            }
            _ => {}
        }
    }

    /// The toast of a failed mode B switch. A switch off that failed while
    /// the state already reads off means the deletion is still pending.
    fn mode_b_failure_key(&self, next: bool) -> &'static str {
        if next {
            "settings.telemetryEnableFailed"
        } else if self.telemetry.as_ref().is_some_and(|t| !t.mode_b_enabled) {
            "settings.telemetryDeletePending"
        } else {
            "settings.telemetryDisableFailed"
        }
    }

    /// The paths asked for when a chip enabled a platform; one whose
    /// platform was disabled meanwhile is dropped.
    fn poll_platform_paths(&mut self) {
        for (_, (id, path)) in drain_landed(&mut self.path_jobs, |job| job.poll()) {
            let Ok(path) = path else {
                continue;
            };
            if self.draft.enabled_platforms.contains(&id) {
                self.platform_paths.insert(id, path);
            }
        }
    }

    /// The "Choose..." pickers: only a real path replaces the field. A
    /// cancel leaves it alone; a picker that could not open says why.
    fn poll_path_picks(&mut self, toasts: &mut ToastQueue, now: Instant) {
        let picked = drain_landed(&mut self.pick_jobs, |job| job.poll());
        let cancelled = accshift_core::error::AppError::Cancelled.to_string();
        for (_, (id, path)) in picked {
            match path {
                Ok(path) => {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        self.platform_paths.insert(id, trimmed.to_string());
                    }
                }
                Err(error) if error.message != cancelled => {
                    toasts.add(
                        Text::raw(error.message),
                        AddToastOptions::kind(ToastType::Error),
                        now,
                    );
                }
                Err(_) => {}
            }
        }
    }

    /// The "Refresh now" runs that landed: their cache writes and stores
    /// apply now; their toasts and the active tab's reload come back as
    /// effects.
    fn poll_refresh_runs(&mut self, b: &mut Booted) -> Vec<AppEffect> {
        let mut effects = Vec::new();
        for slot in [&mut self.avatar_refresh, &mut self.ban_refresh] {
            let Some(Some(result)) = settle(slot) else {
                continue;
            };
            let report = apply_refresh_job(result, &mut b.stores, b.accounts.loading.caches());
            effects.extend(report.toasts.into_iter().map(AppEffect::Toast));
            if let Some(options) = report.reload_active {
                effects.push(AppEffect::LoadAccounts(options));
            }
        }
        effects
    }

    /// The Roblox cookie add; true when it added an account.
    fn poll_roblox(&mut self, jobs: &Jobs, toasts: &mut ToastQueue, now: Instant) -> bool {
        let Some(landed) = settle(&mut self.roblox_job) else {
            return false;
        };
        self.roblox_adding = false;
        let account = match landed.unwrap_or_else(cancelled) {
            Ok(account) => account,
            Err(error) => {
                self.roblox_error = error.message;
                return false;
            }
        };
        let _ = jobs.run(|b| b.telemetry_track_account_added("roblox"));
        self.roblox_cookie.clear();
        let profile = if account.display_name.is_empty() {
            account.username
        } else {
            account.display_name
        };
        toasts.add(
            Text::with(
                "roblox.setupReadyWithProfile",
                [("profile", TranslationValue::Str(profile))],
            ),
            AddToastOptions::default(),
            now,
        );
        true
    }

    /// The descriptor folder call, once it is over: a new folder report
    /// reaches the whole app, a picked file opens the preview.
    fn poll_descriptor(&mut self, b: &mut Booted) {
        let Some((job, task)) = &mut self.descriptor_job else {
            return;
        };
        let result = match job.poll() {
            Poll::Waiting => return,
            Poll::Ready(result) => result,
            Poll::Gone => cancelled(),
        };
        let task = task.clone();
        self.descriptor_job = None;
        self.descriptor_busy = false;
        match result {
            Err(error) => {
                self.descriptor_notice.clear();
                self.descriptor_error = error.message;
            }
            Ok(DescriptorDone::Report(report)) => {
                apply_report(b, &report);
                self.user_platforms = Some(report);
                self.after_descriptor_report(task);
            }
            Ok(DescriptorDone::Picked(Some(picked))) => {
                let (path, preview) = *picked;
                self.descriptor_notice.clear();
                self.descriptor_preview = Some(preview);
                self.preview_mount += 1;
                self.preview_path = path;
            }
            Ok(DescriptorDone::Picked(None) | DescriptorDone::Opened) => {}
        }
    }

    /// The notice a changed folder leaves, and what it changes in the panel.
    fn after_descriptor_report(&mut self, task: DescriptorTask) {
        let locale = self.draft.language;
        match task {
            DescriptorTask::Reload => self.descriptor_notice.clear(),
            DescriptorTask::Install { name } => {
                self.descriptor_notice = platform_notice(locale, "descriptor.installed", &name);
                self.descriptor_preview = None;
            }
            DescriptorTask::Remove { id, name } => {
                self.descriptor_notice = platform_notice(locale, "descriptor.removed", &name);
                // The platform stops existing, so it cannot stay in the
                // enabled set.
                self.draft.enabled_platforms.retain(|p| *p != id);
            }
            DescriptorTask::Pick | DescriptorTask::OpenFolder => {}
        }
    }

    /// The current-PIN field accepts input again once its wait is over.
    fn expire_current_pin_wait(&mut self, now: Instant) {
        if self
            .current_pin_busy_until
            .is_some_and(|until| now >= until)
        {
            self.current_pin_busy_until = None;
        }
    }

    /// A tab that left the list (its platform was disabled, or the list
    /// changed with the language) gives way to a visible one.
    fn ensure_active_tab_visible(&mut self, platforms: &[PlatformDef]) {
        let ids: Vec<String> = visible_tabs(platforms, &self.draft.enabled_platforms)
            .into_iter()
            .map(|t| t.id)
            .collect();
        if !ids.iter().any(|id| id == self.tab_bar.active_tab()) {
            self.tab_bar.ensure_active_visible(&ids);
        }
    }
}

impl AppModel {
    /// Once per frame while the panel is open: the jobs, the timers, the
    /// external settings watch and the save debounce, in this order.
    pub(in crate::app) fn tick_settings_panel(&mut self, ctx: &Context, now: Instant) {
        self.settings_panel_hidden();
        let jobs = &self.jobs;
        let toasts = &mut self.toasts;
        let (Some(panel), Some(b)) = (self.settings_panel.as_mut(), self.booted.as_mut()) else {
            return;
        };
        panel.poll_hydrate();
        panel.poll_telemetry();
        if panel.poll_calls(toasts, now) {
            panel.refresh_telemetry(jobs);
        }
        panel.poll_platform_paths();
        panel.poll_path_picks(toasts, now);
        panel.sync_cs2_mount(jobs);
        if let Some(enabled) = panel.tick_cs2(jobs, toasts, now) {
            reload_cs2_bridge(b, enabled);
        }
        let mut effects = panel.poll_refresh_runs(b);
        let account_added = panel.poll_roblox(jobs, toasts, now);
        panel.poll_descriptor(b);
        if let Some(text) = panel.clipboard.take() {
            ctx.copy_text(text);
        }
        panel.expire_current_pin_wait(now);
        panel.adopt_external_settings(b.stores.settings());
        panel.queue_save(now);
        panel.ensure_active_tab_visible(&b.platforms);
        if panel.take_due_save(now) {
            self.persist_settings(now);
        }
        if account_added {
            effects.push(AppEffect::LoadAccounts(load_options(
                true, false, false, false,
            )));
        }
        if !effects.is_empty() {
            self.apply_effects(ctx, effects);
        }
    }
}
