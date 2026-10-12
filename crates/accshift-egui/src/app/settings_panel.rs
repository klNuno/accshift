//! The settings panel's state and its save flow, `Settings.svelte` and the
//! state its General and Privacy tabs keep (`SettingsPrivacyTab.svelte`'s
//! PIN and telemetry).
//!
//! The panel edits a draft of [`AppSettings`]. Every change of a tracked
//! field queues a save 220 ms later (`queueSave`); the save normalises the
//! draft, commits a typed PIN, merges the draft over the stored settings
//! with [`merge_settings_draft`] (a field another writer changed meanwhile
//! is kept), writes it through [`ClientStores::save_settings`], then pushes
//! the API key and the platform paths and toasts `settings.saved` at most
//! every 1.5 s. A change of the stored settings that the panel did not
//! write is adopted into every field the user has not edited
//! (`adoptExternalSettings`). Escape and the close button flush, then close.
//!
//! The Platforms tab and the per-platform tabs read and edit the same state
//! ([`SettingsPanel::platform_paths`], the API key draft, the numeric inputs
//! of the data refresh); they are drawn by a later port.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use accshift_app::actions::navigation::SettingsTabBar;
use accshift_app::actions::toasts::{AddToastOptions, ToastQueue, ToastType};
use accshift_app::error::BackendResult;
use accshift_app::i18n::{Locale, TranslationValue};
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::secure::pin::{hash_pin_code, sanitize_pin_digits, unlock_pin_session};
use accshift_app::settings::{
    api_key_draft_after_successful_save, merge_settings_draft, AnimationsMode, AppSettings,
    StreamerMode,
};
use accshift_app::stores::ClientStores;
use accshift_app::text::Text;
use accshift_app::types::TelemetryUiState;
use egui::Context;
use serde_json::Value;

use super::platforms::{self, PlatformDef, RuntimeOs};
use super::AppModel;

/// `PIN_CODE_LENGTH`.
pub const PIN_CODE_LENGTH: usize = 4;
/// `queueSave`'s debounce.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(220);
/// `SAVE_TOAST_COOLDOWN_MS`.
const SAVE_TOAST_COOLDOWN: Duration = Duration::from_millis(1500);
/// `WIKI_URL`.
pub const WIKI_URL: &str = "https://github.com/klNuno/accshift/wiki";
/// `INTEGRATIONS_WIKI_URL`.
pub const INTEGRATIONS_WIKI_URL: &str = "https://github.com/klNuno/accshift/wiki/Settings";
/// `TELEMETRY_DOC_URL`.
pub const TELEMETRY_DOC_URL: &str =
    "https://github.com/klNuno/accshift/blob/main/docs/analytics.md";

/// `clampInt`: rounded and clamped, `fallback` for a non-finite value.
pub fn clamp_int(value: Option<f64>, min: i64, max: i64, fallback: i64) -> i64 {
    match value {
        Some(v) if v.is_finite() => (v.round() as i64).clamp(min, max),
        _ => fallback,
    }
}

/// `Number(string)`: trimmed, empty is 0, anything unparsable is `NaN`.
pub fn js_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    if t.chars()
        .any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    {
        return None;
    }
    t.parse::<f64>().ok()
}

/// `createNumericInput`: the text of a number field, committed on demand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumericInput {
    pub input: String,
    pub min: i64,
    pub max: i64,
}

impl NumericInput {
    pub fn new(value: i64, min: i64, max: i64) -> Self {
        Self {
            input: value.to_string(),
            min,
            max,
        }
    }

    /// `commit`: the clamped value to store; `current` when the text is not
    /// a number. The text shows the stored value afterwards.
    pub fn commit(&mut self, current: i64) -> i64 {
        let clamped = clamp_int(js_number(&self.input), self.min, self.max, current);
        self.input = clamped.to_string();
        clamped
    }

    /// `refresh`.
    pub fn refresh(&mut self, value: i64) {
        self.input = value.to_string();
    }
}

/// The five number fields of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericField {
    UiScale,
    BgOpacity,
    AvatarCacheDays,
    BanCheckDays,
    InactivityBlur,
}

/// A boolean setting a toggle flips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingFlag {
    ShowCardNotesInline,
    ExpandedFolders,
    SuspendGraphicsWhenMinimized,
    MinimizeOnAccountSwitch,
    DeepLinks,
    Cli,
    /// `streamerMode`: `auto` when on, `off` when off.
    StreamerMode,
}

/// What the panel's components ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    /// The close button or Escape: flush, then close.
    Close,
    SelectTab(String),
    OpenHelp,
    OpenLogs,
    OpenIntegrationsWiki,
    OpenTelemetryDoc,
    Toggle(SettingFlag),
    Language(Locale),
    Animations(AnimationsMode),
    Theme(String),
    /// A slider's or a field's text (`oninput`).
    NumericInput(NumericField, String),
    /// `commit`: blur, Enter, or a slider's `change`.
    NumericCommit(NumericField),
    /// The new PIN field (`pinCodeInput`).
    PinInput(String),
    /// The current PIN field of a guarded change.
    CurrentPinInput(String),
    TogglePin,
    ReplayOnboarding,
    ExportTheme,
    ToggleTelemetryA,
    ToggleTelemetryB,
    RetryTelemetryDeletion,
    ExportTelemetry,
    RetryTelemetryLoad,
}

/// What a finished telemetry or link call does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Followup {
    /// A failure toasts this key as an error.
    ErrorToast(&'static str),
    /// `toggleModeA`: a failure reloads the state.
    ModeA,
    /// `toggleModeB`.
    ModeB {
        next: bool,
    },
    RetryForget,
    Export,
}

/// The mount job's result: whether a Steam API key is stored, then each enabled platform's path.
type HydrateResult = (bool, Vec<(String, String)>);

/// A backend call queued by an action.
type BackendCall =
    Box<dyn FnOnce(&dyn accshift_app::backend::Backend) -> BackendResult<Value> + Send>;

/// `SettingsPanel` state while it is open.
pub struct SettingsPanel {
    /// `settings`: the draft the controls edit.
    pub draft: AppSettings,
    /// `persistedSettings`: the draft as last written.
    persisted: AppSettings,
    /// The stored settings as the panel last saw them.
    seen_store: AppSettings,
    last_persisted_snapshot: String,
    last_platform_snapshot: String,
    last_tracked: String,
    save_at: Option<Instant>,
    last_saved_toast: Option<Instant>,
    hydrated: bool,
    hydrate_job: Option<Pending<HydrateResult>>,
    pub tab_bar: SettingsTabBar,
    pub ui_scale: NumericInput,
    pub bg_opacity: NumericInput,
    pub avatar_cache_days: NumericInput,
    pub ban_check_days: NumericInput,
    pub inactivity_blur: NumericInput,
    pub pin_code_input: String,
    pub pin_setup_pending: bool,
    pub pin_change_authorized: bool,
    pub current_pin_input: String,
    pub current_pin_error: String,
    current_pin_busy_until: Option<Instant>,
    pub pin_disable_pending: bool,
    pub telemetry: Option<TelemetryUiState>,
    pub telemetry_error: bool,
    telemetry_job: Option<Pending<BackendResult<TelemetryUiState>>>,
    pub mode_b_busy: bool,
    pub export_busy: bool,
    calls: Vec<(Pending<BackendResult<Value>>, Followup)>,
    pub api_key: String,
    pub api_key_touched: bool,
    pub api_key_configured: bool,
    pub api_key_error: bool,
    pub platform_paths: BTreeMap<String, String>,
    pub platform_path_errors: BTreeMap<String, bool>,
    /// `.settings-content`'s `scrollTop`.
    pub scroll: f32,
    /// Text for the clipboard once the frame has the context.
    clipboard: Option<String>,
}

/// One tab of the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct TabDef {
    pub id: String,
    pub label_key: String,
    /// `None` for the core tabs (their icon is drawn instead of a dot).
    pub accent: Option<crate::theme::Rgba>,
}

/// `coreTabConfig` then `platformTabConfig` (shipped platforms with a
/// settings tab, enabled, in registry order).
pub fn visible_tabs(platforms: &[PlatformDef], enabled: &[String]) -> Vec<TabDef> {
    let mut tabs: Vec<TabDef> = [
        ("general", "settings.general"),
        ("platforms", "settings.platforms"),
        ("privacy", "settings.privacy"),
    ]
    .iter()
    .map(|(id, key)| TabDef {
        id: (*id).to_string(),
        label_key: (*key).to_string(),
        accent: None,
    })
    .collect();
    for p in platforms {
        let Some(key) = settings_tab_key(&p.id) else {
            continue;
        };
        if p.user_provided || !enabled.contains(&p.id) {
            continue;
        }
        tabs.push(TabDef {
            id: format!("platform:{}", p.id),
            label_key: key.to_string(),
            accent: Some(p.accent),
        });
    }
    tabs
}

/// `settingsTabKey` of the shipped registry.
fn settings_tab_key(id: &str) -> Option<&'static str> {
    Some(match id {
        "steam" => "settings.steam",
        "riot" => "settings.riot",
        "battle-net" => "settings.battleNet",
        "ubisoft" => "settings.ubisoft",
        "roblox" => "settings.roblox",
        "epic" => "settings.epic",
        "gog" => "settings.gog",
        "jagex" => "settings.jagex",
        "discord" => "settings.discord",
        _ => return None,
    })
}

fn platform_snapshot(s: &AppSettings) -> String {
    let mut enabled = s.enabled_platforms.clone();
    enabled.sort();
    serde_json::json!({ "enabledPlatforms": enabled, "defaultPlatformId": s.default_platform_id })
        .to_string()
}

impl SettingsPanel {
    /// `onMount`: the draft from the store, normalised, and the API key and
    /// platform paths requested.
    pub fn open(
        stores: &ClientStores,
        jobs: &Jobs,
        platforms: &[PlatformDef],
        os: RuntimeOs,
    ) -> Self {
        let mut draft = stores.settings().clone();
        normalize(&mut draft, platforms, os);
        let enabled = draft.enabled_platforms.clone();
        let hydrate_job = Some(jobs.run(move |backend| {
            let has_key = backend.steam_has_api_key().unwrap_or(false);
            let paths = enabled
                .iter()
                .map(|id| {
                    (
                        id.clone(),
                        backend.platform_get_path(id).unwrap_or_default(),
                    )
                })
                .collect();
            (has_key, paths)
        }));
        let mut panel = SettingsPanel {
            ui_scale: NumericInput::new(draft.ui_scale_percent, 75, 150),
            bg_opacity: NumericInput::new(draft.background_opacity, 0, 100),
            avatar_cache_days: NumericInput::new(draft.data_refresh.avatar_cache_days, 0, 90),
            ban_check_days: NumericInput::new(draft.data_refresh.ban_check_days, 0, 90),
            inactivity_blur: NumericInput::new(draft.inactivity_blur_seconds, 0, 3600),
            persisted: draft.clone(),
            seen_store: stores.settings().clone(),
            last_persisted_snapshot: String::new(),
            last_platform_snapshot: platform_snapshot(&draft),
            last_tracked: String::new(),
            save_at: None,
            last_saved_toast: None,
            hydrated: false,
            hydrate_job,
            tab_bar: SettingsTabBar::new(),
            pin_code_input: String::new(),
            pin_setup_pending: false,
            pin_change_authorized: false,
            current_pin_input: String::new(),
            current_pin_error: String::new(),
            current_pin_busy_until: None,
            pin_disable_pending: false,
            telemetry: None,
            telemetry_error: false,
            telemetry_job: None,
            mode_b_busy: false,
            export_busy: false,
            calls: Vec::new(),
            api_key: String::new(),
            api_key_touched: false,
            api_key_configured: false,
            api_key_error: false,
            platform_paths: BTreeMap::new(),
            platform_path_errors: BTreeMap::new(),
            scroll: 0.0,
            clipboard: None,
            draft,
        };
        panel.last_tracked = panel.tracked();
        panel
    }

    /// `pinGuarded`.
    pub fn pin_guarded(&self) -> bool {
        self.draft.pin_enabled && !self.pin_change_authorized
    }

    /// The PIN toggle's state.
    pub fn pin_toggle_on(&self) -> bool {
        (self.draft.pin_enabled || self.pin_setup_pending) && !self.pin_disable_pending
    }

    /// The current-PIN field is waiting out a failure.
    pub fn current_pin_busy(&self) -> bool {
        self.current_pin_busy_until.is_some()
    }

    pub fn numeric(&self, field: NumericField) -> &NumericInput {
        match field {
            NumericField::UiScale => &self.ui_scale,
            NumericField::BgOpacity => &self.bg_opacity,
            NumericField::AvatarCacheDays => &self.avatar_cache_days,
            NumericField::BanCheckDays => &self.ban_check_days,
            NumericField::InactivityBlur => &self.inactivity_blur,
        }
    }

    fn numeric_mut(&mut self, field: NumericField) -> (&mut NumericInput, &mut i64) {
        let d = &mut self.draft;
        match field {
            NumericField::UiScale => (&mut self.ui_scale, &mut d.ui_scale_percent),
            NumericField::BgOpacity => (&mut self.bg_opacity, &mut d.background_opacity),
            NumericField::AvatarCacheDays => (
                &mut self.avatar_cache_days,
                &mut d.data_refresh.avatar_cache_days,
            ),
            NumericField::BanCheckDays => {
                (&mut self.ban_check_days, &mut d.data_refresh.ban_check_days)
            }
            NumericField::InactivityBlur => {
                (&mut self.inactivity_blur, &mut d.inactivity_blur_seconds)
            }
        }
    }

    /// The `$effect` dependencies of `queueSave`, as one string.
    fn tracked(&self) -> String {
        serde_json::json!({
            "settings": self.draft.to_value(),
            "pin": self.pin_code_input,
            "apiKey": self.api_key,
            "apiKeyConfigured": self.api_key_configured,
            "apiKeyTouched": self.api_key_touched,
            "paths": self.platform_paths,
        })
        .to_string()
    }

    /// `buildPersistSnapshot`.
    fn persist_snapshot(&self, settings: &AppSettings) -> String {
        let pending_api_key = if self.api_key_touched {
            self.api_key.trim().to_string()
        } else {
            String::new()
        };
        let paths: BTreeMap<&String, &str> = self
            .platform_paths
            .iter()
            .map(|(k, v)| (k, v.trim()))
            .collect();
        serde_json::json!({
            "settings": settings.to_value(),
            "pendingApiKey": pending_api_key,
            "apiKeyConfigured": self.api_key_configured,
            "platformPaths": paths,
        })
        .to_string()
    }

    fn refresh_numeric(&mut self) {
        let d = &self.draft;
        self.ui_scale.refresh(d.ui_scale_percent);
        self.bg_opacity.refresh(d.background_opacity);
        self.avatar_cache_days
            .refresh(d.data_refresh.avatar_cache_days);
        self.ban_check_days.refresh(d.data_refresh.ban_check_days);
        self.inactivity_blur.refresh(d.inactivity_blur_seconds);
    }

    /// `commitPinCode`: true when a typed PIN was written into the draft.
    fn commit_pin_code(&mut self) -> bool {
        let sanitized = sanitize_pin_digits(&self.pin_code_input);
        if !self.draft.pin_enabled && !self.pin_setup_pending {
            return false;
        }
        if self.draft.pin_enabled && !self.pin_change_authorized {
            return false;
        }
        if sanitized.len() != PIN_CODE_LENGTH {
            return false;
        }
        let Some(hash) = hash_pin_code(&sanitized) else {
            return false;
        };
        self.draft.pin_hash = hash;
        self.draft.pin_enabled = true;
        self.pin_setup_pending = false;
        self.pin_change_authorized = true;
        self.pin_code_input.clear();
        true
    }

    /// `disablePin`.
    fn disable_pin(&mut self) {
        self.draft.pin_enabled = false;
        self.pin_setup_pending = false;
        self.draft.pin_hash.clear();
        self.pin_code_input.clear();
        self.pin_disable_pending = false;
    }

    /// The Privacy tab mounts: its local state starts over and the
    /// telemetry state is fetched (`onMount(refreshTelemetry)`).
    fn mount_privacy(&mut self, jobs: &Jobs) {
        self.current_pin_input.clear();
        self.current_pin_error.clear();
        self.current_pin_busy_until = None;
        self.pin_disable_pending = false;
        self.telemetry = None;
        self.telemetry_error = false;
        self.mode_b_busy = false;
        self.export_busy = false;
        self.refresh_telemetry(jobs);
    }

    fn refresh_telemetry(&mut self, jobs: &Jobs) {
        self.telemetry_job = Some(jobs.run(|backend| backend.telemetry_get_state()));
    }
}

/// `normalizeSettings`.
pub fn normalize(s: &mut AppSettings, platforms: &[PlatformDef], os: RuntimeOs) {
    let clamp = |v: i64, min, max, fallback| clamp_int(Some(v as f64), min, max, fallback);
    s.background_opacity = clamp(s.background_opacity, 0, 100, 100);
    s.ui_scale_percent = clamp(s.ui_scale_percent, 75, 150, 100);
    s.data_refresh.avatar_cache_days = clamp(s.data_refresh.avatar_cache_days, 0, 90, 7);
    s.data_refresh.ban_check_days = clamp(s.data_refresh.ban_check_days, 0, 90, 7);
    s.inactivity_blur_seconds = clamp(s.inactivity_blur_seconds, 0, 3600, 60);
    s.platform_settings.steam.launch_options =
        s.platform_settings.steam.launch_options.trim().to_string();
    if !s.pin_enabled {
        s.pin_hash.clear();
    }
    let implemented = |id: &str| settings_tab_key(id).is_some();
    if !platforms.iter().any(|p| {
        p.id == s.default_platform_id && (implemented(&p.id) || s.enabled_platforms.contains(&p.id))
    }) {
        s.default_platform_id = "steam".to_string();
    }
    if s.enabled_platforms.is_empty() {
        s.enabled_platforms = vec!["steam".to_string()];
    }
    if !s.enabled_platforms.contains(&s.default_platform_id) {
        s.default_platform_id = s.enabled_platforms[0].clone();
    }
    let selectable: Vec<&String> = s
        .enabled_platforms
        .iter()
        .filter(|id| implemented(id) && platforms::usable(platforms, id, os))
        .collect();
    if let Some(first) = selectable.first() {
        if !selectable.contains(&&s.default_platform_id) {
            s.default_platform_id = (*first).clone();
        }
    }
}

fn toast(toasts: &mut ToastQueue, key: &str, kind: ToastType, now: Instant) {
    toasts.add(Text::key(key), AddToastOptions::kind(kind), now);
}

impl AppModel {
    /// The settings toggle of the title bar: open (or flush and close).
    pub(super) fn toggle_settings_panel(&mut self) {
        if self.settings_panel.is_some() {
            self.close_settings_panel();
            return;
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.nav.show_personas = false;
        b.nav.bulk_edit = false;
        b.nav.show_settings = true;
        self.settings_panel = Some(SettingsPanel::open(
            &b.stores,
            &self.jobs,
            &b.platforms,
            b.runtime_os,
        ));
    }

    /// `closePanel`: flush the pending save, then `onClose`.
    pub(super) fn close_settings_panel(&mut self) {
        self.flush_settings(Instant::now());
        self.settings_panel = None;
        if let Some(b) = self.booted.as_mut() {
            b.nav.show_settings = false;
        }
    }

    /// Something else closed the panel (a platform tab, personas): the
    /// pending save still lands, like `onDestroy`'s final persist.
    pub(super) fn settings_panel_hidden(&mut self) {
        if self.booted.as_ref().is_some_and(|b| !b.nav.show_settings)
            && self.settings_panel.is_some()
        {
            self.flush_settings(Instant::now());
            self.settings_panel = None;
        }
    }

    /// `flushSettingsNow`.
    fn flush_settings(&mut self, now: Instant) {
        if let Some(p) = self.settings_panel.as_mut() {
            p.save_at = None;
        }
        self.persist_settings(now);
    }

    /// `persistCurrentState`.
    fn persist_settings(&mut self, now: Instant) {
        let (Some(panel), Some(b)) = (self.settings_panel.as_mut(), self.booted.as_mut()) else {
            return;
        };
        if !panel.hydrated {
            return;
        }
        normalize(&mut panel.draft, &b.platforms, b.runtime_os);
        let pin_committed = panel.commit_pin_code();
        let snapshot = panel.persist_snapshot(&panel.draft);
        if snapshot == panel.last_persisted_snapshot {
            return;
        }
        let next_platforms = platform_snapshot(&panel.draft);
        let platforms_changed = next_platforms != panel.last_platform_snapshot;
        let previous: Value =
            serde_json::from_str(&panel.last_persisted_snapshot).unwrap_or(Value::Null);
        let draft = panel.draft.clone();
        match merge_settings_draft(b.stores.settings(), &panel.persisted, &draft) {
            Ok(merged) => b.stores.save_settings(&merged),
            Err(_) => b.stores.save_settings(&draft),
        }
        panel.seen_store = b.stores.settings().clone();
        panel.persisted = draft;
        if pin_committed {
            toast(
                &mut self.toasts,
                "settings.pinSaved",
                ToastType::Success,
                now,
            );
        }
        // `saveApiKey`.
        let mut api_key_saved = true;
        if panel.api_key_touched {
            let trimmed = panel.api_key.trim().to_string();
            if trimmed.is_empty() {
                panel.api_key_touched = false;
            } else {
                match self.backend.steam_set_api_key(&trimmed) {
                    Ok(()) => {
                        let next = api_key_draft_after_successful_save(&panel.api_key, &trimmed);
                        panel.api_key_configured = true;
                        panel.api_key = next.api_key;
                        panel.api_key_touched = next.touched;
                        panel.api_key_error = false;
                    }
                    Err(_) => {
                        panel.api_key_error = true;
                        api_key_saved = false;
                        toast(
                            &mut self.toasts,
                            "settings.apiKeySaveFailed",
                            ToastType::Error,
                            now,
                        );
                    }
                }
            }
        }
        // `savePlatformPaths`.
        let mut paths_saved = true;
        let ids: Vec<String> = panel.platform_paths.keys().cloned().collect();
        for id in ids {
            let next = panel.platform_paths[&id].trim().to_string();
            let prev = previous["platformPaths"][&id].as_str().unwrap_or("");
            if prev == next {
                continue;
            }
            match self.backend.platform_set_path(&id, &next) {
                Ok(()) => {
                    panel.platform_path_errors.insert(id, false);
                }
                Err(_) => {
                    panel.platform_path_errors.insert(id, true);
                    paths_saved = false;
                    toast(
                        &mut self.toasts,
                        "settings.pathSaveFailed",
                        ToastType::Error,
                        now,
                    );
                }
            }
        }
        panel.last_platform_snapshot = next_platforms;
        if platforms_changed {
            // `onPlatformsChanged`: a tab that is no longer enabled falls
            // back to the first one the settings allow.
            let s = b.stores.settings();
            if !s.enabled_platforms.contains(&b.nav.active_tab) {
                b.nav.active_tab = platforms::initial_tab(
                    &b.platforms,
                    &s.enabled_platforms,
                    &s.default_platform_id,
                    b.runtime_os,
                );
            }
        }
        if !api_key_saved || !paths_saved {
            return;
        }
        panel.last_persisted_snapshot = snapshot;
        if panel
            .last_saved_toast
            .is_none_or(|at| now.duration_since(at) >= SAVE_TOAST_COOLDOWN)
        {
            toast(&mut self.toasts, "settings.saved", ToastType::Info, now);
            panel.last_saved_toast = Some(now);
        }
    }

    /// Once per frame while the panel is open: jobs, timers, the external
    /// settings watch and the save debounce.
    pub(super) fn tick_settings_panel(&mut self, ctx: &Context, now: Instant) {
        self.settings_panel_hidden();
        let jobs = &self.jobs;
        let (Some(panel), Some(b)) = (self.settings_panel.as_mut(), self.booted.as_mut()) else {
            return;
        };
        if let Some(job) = &mut panel.hydrate_job {
            let landed = match job.poll() {
                Poll::Waiting => None,
                Poll::Ready(result) => Some(result),
                Poll::Gone => Some((false, Vec::new())),
            };
            if let Some((has_key, paths)) = landed {
                panel.hydrate_job = None;
                panel.api_key_configured = has_key;
                panel.platform_paths = paths.into_iter().collect();
                let at_start = panel.persisted.clone();
                panel.last_persisted_snapshot = panel.persist_snapshot(&at_start);
                panel.last_platform_snapshot = platform_snapshot(&at_start);
                panel.hydrated = true;
            }
        }
        if let Some(job) = &mut panel.telemetry_job {
            match job.poll() {
                Poll::Waiting => {}
                Poll::Ready(Ok(state)) => {
                    panel.telemetry = Some(state);
                    panel.telemetry_error = false;
                    panel.telemetry_job = None;
                }
                Poll::Ready(Err(_)) | Poll::Gone => {
                    panel.telemetry_error = true;
                    panel.telemetry_job = None;
                }
            }
        }
        let mut refresh = false;
        let mut finished = Vec::new();
        panel.calls.retain_mut(|(job, followup)| match job.poll() {
            Poll::Waiting => true,
            Poll::Ready(result) => {
                finished.push((*followup, result));
                false
            }
            Poll::Gone => false,
        });
        for (followup, result) in finished {
            match (followup, result) {
                (Followup::ErrorToast(key), Err(_)) => {
                    toast(&mut self.toasts, key, ToastType::Error, now);
                }
                (Followup::ModeA, Err(_)) => refresh = true,
                (Followup::ModeB { next }, result) => {
                    panel.mode_b_busy = false;
                    refresh = true;
                    if result.is_err() {
                        let key = if next {
                            "settings.telemetryEnableFailed"
                        } else if panel.telemetry.as_ref().is_some_and(|t| !t.mode_b_enabled) {
                            "settings.telemetryDeletePending"
                        } else {
                            "settings.telemetryDisableFailed"
                        };
                        toast(&mut self.toasts, key, ToastType::Info, now);
                    }
                }
                (Followup::RetryForget, result) => {
                    panel.mode_b_busy = false;
                    refresh = true;
                    let key = if result.is_ok() {
                        "settings.telemetryDeleteCompleted"
                    } else {
                        "settings.telemetryDeleteRetryFailed"
                    };
                    toast(&mut self.toasts, key, ToastType::Info, now);
                }
                (Followup::Export, result) => {
                    panel.export_busy = false;
                    let key = match result {
                        Ok(data) => {
                            let text = serde_json::to_string_pretty(&data).unwrap_or_default();
                            panel.clipboard = Some(text);
                            "settings.telemetryExported"
                        }
                        Err(_) => "settings.telemetryExportFailed",
                    };
                    toast(&mut self.toasts, key, ToastType::Info, now);
                }
                _ => {}
            }
        }
        if refresh {
            panel.refresh_telemetry(jobs);
        }
        if let Some(text) = panel.clipboard.take() {
            ctx.copy_text(text);
        }
        if panel
            .current_pin_busy_until
            .is_some_and(|until| now >= until)
        {
            panel.current_pin_busy_until = None;
        }
        // `adoptExternalSettings`.
        if panel.hydrated && b.stores.settings() != &panel.seen_store {
            let stored = b.stores.settings().clone();
            panel.seen_store = stored.clone();
            let draft = panel.draft.clone();
            let was_saved = panel.persist_snapshot(&panel.draft) == panel.last_persisted_snapshot;
            let next = merge_settings_draft(&stored, &panel.persisted, &draft).unwrap_or(draft);
            panel.persisted = stored;
            panel.draft = next;
            panel.refresh_numeric();
            if was_saved {
                panel.last_persisted_snapshot = panel.persist_snapshot(&panel.draft);
            }
        }
        // `$effect(... queueSave())`.
        let tracked = panel.tracked();
        if tracked != panel.last_tracked {
            panel.last_tracked = tracked;
            if panel.hydrated {
                panel.save_at = Some(now + SAVE_DEBOUNCE);
            }
        }
        // `ensureActiveVisible` on a tab list or language change.
        let enabled = panel.draft.enabled_platforms.clone();
        let ids: Vec<String> = visible_tabs(&b.platforms, &enabled)
            .into_iter()
            .map(|t| t.id)
            .collect();
        if !ids.iter().any(|id| id == panel.tab_bar.active_tab()) {
            panel.tab_bar.ensure_active_visible(&ids);
        }
        if panel.save_at.is_some_and(|at| now >= at) {
            panel.save_at = None;
            self.persist_settings(now);
        }
    }

    /// When the panel next needs a frame without input.
    pub fn settings_panel_deadline(&self) -> Option<Instant> {
        let p = self.settings_panel.as_ref()?;
        [p.save_at, p.current_pin_busy_until]
            .into_iter()
            .flatten()
            .min()
    }

    /// Applies one [`SettingsAction`].
    pub(super) fn apply_settings(&mut self, ctx: &Context, action: SettingsAction) {
        let now = Instant::now();
        if matches!(
            action,
            SettingsAction::Close | SettingsAction::ReplayOnboarding
        ) {
            // `closePanel()`; the replay then hands over to the onboarding
            // tour, which belongs to the overlays port.
            self.close_settings_panel();
            return;
        }
        let jobs = &self.jobs;
        let Some(panel) = self.settings_panel.as_mut() else {
            return;
        };
        let call = |panel: &mut SettingsPanel, followup: Followup, f: BackendCall| {
            panel.calls.push((jobs.run(f), followup));
        };
        match action {
            SettingsAction::Close | SettingsAction::ReplayOnboarding => {}
            SettingsAction::SelectTab(id) => {
                let was_privacy = panel.tab_bar.active_tab() == "privacy";
                panel.tab_bar.select(&id);
                panel.scroll = 0.0;
                if id == "privacy" && !was_privacy {
                    panel.mount_privacy(jobs);
                }
            }
            SettingsAction::OpenHelp => call(
                panel,
                Followup::ErrorToast("settings.openHelpFailed"),
                Box::new(|b| b.open_url(WIKI_URL).map(|()| Value::Null)),
            ),
            SettingsAction::OpenLogs => call(
                panel,
                Followup::ErrorToast("settings.openLogsFailed"),
                Box::new(|b| b.open_logs_folder().map(|()| Value::Null)),
            ),
            SettingsAction::OpenIntegrationsWiki => call(
                panel,
                Followup::ErrorToast("settings.openHelpFailed"),
                Box::new(|b| b.open_url(INTEGRATIONS_WIKI_URL).map(|()| Value::Null)),
            ),
            SettingsAction::OpenTelemetryDoc => call(
                panel,
                Followup::ErrorToast("settings.openHelpFailed"),
                Box::new(|b| b.open_url(TELEMETRY_DOC_URL).map(|()| Value::Null)),
            ),
            SettingsAction::Toggle(flag) => {
                let d = &mut panel.draft;
                match flag {
                    SettingFlag::ShowCardNotesInline => {
                        d.account_display.show_card_notes_inline ^= true;
                    }
                    SettingFlag::ExpandedFolders => d.account_display.expanded_folders ^= true,
                    SettingFlag::SuspendGraphicsWhenMinimized => {
                        d.suspend_graphics_when_minimized ^= true;
                    }
                    SettingFlag::MinimizeOnAccountSwitch => d.minimize_on_account_switch ^= true,
                    SettingFlag::DeepLinks => d.deep_links_enabled ^= true,
                    SettingFlag::Cli => d.cli_enabled ^= true,
                    SettingFlag::StreamerMode => {
                        d.streamer_mode = match d.streamer_mode {
                            StreamerMode::Auto => StreamerMode::Off,
                            StreamerMode::Off => StreamerMode::Auto,
                        };
                    }
                }
            }
            SettingsAction::Language(locale) => panel.draft.language = locale,
            SettingsAction::Animations(mode) => panel.draft.animations = mode,
            SettingsAction::Theme(id) => panel.draft.theme_id = id,
            SettingsAction::NumericInput(field, text) => panel.numeric_mut(field).0.input = text,
            SettingsAction::NumericCommit(field) => {
                let (input, value) = panel.numeric_mut(field);
                *value = input.commit(*value);
            }
            SettingsAction::PinInput(text) => {
                panel.pin_code_input = sanitize_pin_digits(&text)
                    .chars()
                    .take(PIN_CODE_LENGTH)
                    .collect();
            }
            SettingsAction::CurrentPinInput(text) => {
                panel.current_pin_input = sanitize_pin_digits(&text)
                    .chars()
                    .take(PIN_CODE_LENGTH)
                    .collect();
                if panel.current_pin_input.len() == PIN_CODE_LENGTH
                    && panel.current_pin_busy_until.is_none()
                {
                    // `confirmCurrentPin`. The backend checks the code on
                    // this thread: the PIN service answers locally.
                    let code = std::mem::take(&mut panel.current_pin_input);
                    panel.current_pin_error.clear();
                    let check =
                        unlock_pin_session(self.backend.as_ref(), &code, &panel.draft.pin_hash);
                    match check.failure_wait() {
                        None => {
                            panel.pin_change_authorized = true;
                            if panel.pin_disable_pending {
                                panel.disable_pin();
                            } else if let accshift_app::secure::pin::PinCheck::Match {
                                rehashed: Some(hash),
                            } = check
                            {
                                panel.draft.pin_hash = hash;
                            }
                        }
                        Some(wait) => {
                            let locale = self.booted.as_ref().map_or(Locale::En, |b| b.locale);
                            panel.current_pin_error = if wait.too_many_attempts {
                                accshift_app::i18n::translate_with(
                                    locale,
                                    "pin.tooManyAttempts",
                                    &[(
                                        "seconds",
                                        TranslationValue::Num(wait.wait_ms.div_ceil(1000) as f64),
                                    )],
                                )
                            } else {
                                accshift_app::i18n::translate(locale, "pin.invalid")
                            };
                            panel.current_pin_busy_until =
                                Some(now + Duration::from_millis(wait.wait_ms));
                        }
                    }
                }
            }
            SettingsAction::TogglePin => {
                if panel.pin_guarded() {
                    panel.pin_disable_pending = !panel.pin_disable_pending;
                } else if panel.draft.pin_enabled || panel.pin_setup_pending {
                    panel.disable_pin();
                } else {
                    panel.pin_setup_pending = true;
                }
            }
            SettingsAction::ExportTheme => {
                if let Some(b) = self.booted.as_ref() {
                    let theme = b.registry.theme(Some(&panel.draft.theme_id));
                    ctx.copy_text(accshift_theme::export_theme_json(&theme));
                    toast(
                        &mut self.toasts,
                        "settings.themeExportCopied",
                        ToastType::Info,
                        now,
                    );
                }
            }
            SettingsAction::ToggleTelemetryA => {
                if let Some(t) = panel.telemetry.as_mut() {
                    let next = !t.mode_a_enabled;
                    t.mode_a_enabled = next;
                    call(
                        panel,
                        Followup::ModeA,
                        Box::new(move |b| b.telemetry_set_mode_a(next).map(|()| Value::Null)),
                    );
                }
            }
            SettingsAction::ToggleTelemetryB => {
                if let Some(t) = panel.telemetry.as_ref() {
                    if !panel.mode_b_busy {
                        let next = !t.mode_b_enabled;
                        panel.mode_b_busy = true;
                        call(
                            panel,
                            Followup::ModeB { next },
                            Box::new(move |b| b.telemetry_set_mode_b(next).map(|()| Value::Null)),
                        );
                    }
                }
            }
            SettingsAction::RetryTelemetryDeletion => {
                if !panel.mode_b_busy {
                    panel.mode_b_busy = true;
                    call(
                        panel,
                        Followup::RetryForget,
                        Box::new(|b| b.telemetry_retry_forget().map(|()| Value::Null)),
                    );
                }
            }
            SettingsAction::ExportTelemetry => {
                if !panel.export_busy {
                    panel.export_busy = true;
                    call(panel, Followup::Export, Box::new(|b| b.telemetry_export()));
                }
            }
            SettingsAction::RetryTelemetryLoad => panel.refresh_telemetry(jobs),
        }
    }
}
