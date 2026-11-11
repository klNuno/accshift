//! The panel's state while it is open, its tabs, and the PIN state of the
//! Privacy tab.

use std::collections::BTreeMap;
use std::time::Instant;

use accshift_app::accounts::refresh::{RefreshJobResult, RefreshKind};
use accshift_app::actions::dialogs::ConfirmTicket;
use accshift_app::actions::navigation::SettingsTabBar;
use accshift_app::error::BackendResult;
use accshift_app::jobs::{Jobs, Pending};
use accshift_app::secure::pin::{hash_pin_code, sanitize_pin_digits};
use accshift_app::settings::AppSettings;
use accshift_app::stores::ClientStores;
use accshift_app::types::{DescriptorPreview, RobloxAccount, TelemetryUiState, UserPlatformReport};

use super::super::platforms::{PlatformDef, RuntimeOs};
use super::super::theme_editor::{ThemeEditor, ThemeImport};
use super::cs2::Cs2Form;
use super::draft::{normalize, platform_snapshot, NumericField, NumericInput};
use super::jobs::{DescriptorDone, DescriptorTask, Followup, HydrateResult, PathJob};
use super::PIN_CODE_LENGTH;
use serde_json::Value;

/// The settings panel's state while it is open.
pub struct SettingsPanel {
    /// The draft the controls edit.
    pub draft: AppSettings,
    /// The draft as last written.
    pub(super) persisted: AppSettings,
    /// The stored settings as the panel last saw them.
    pub(super) seen_store: AppSettings,
    pub(super) last_persisted_snapshot: String,
    pub(super) last_platform_snapshot: String,
    pub(super) last_tracked: String,
    pub(super) save_at: Option<Instant>,
    pub(super) last_saved_toast: Option<Instant>,
    pub(super) hydrated: bool,
    pub(super) hydrate_job: Option<Pending<HydrateResult>>,
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
    pub(super) current_pin_busy_until: Option<Instant>,
    pub pin_disable_pending: bool,
    pub telemetry: Option<TelemetryUiState>,
    pub telemetry_error: bool,
    pub(super) telemetry_job: Option<Pending<BackendResult<TelemetryUiState>>>,
    pub mode_b_busy: bool,
    pub export_busy: bool,
    pub(super) calls: Vec<(Pending<BackendResult<Value>>, Followup)>,
    pub api_key: String,
    pub api_key_touched: bool,
    pub api_key_configured: bool,
    pub api_key_error: bool,
    pub platform_paths: BTreeMap<String, String>,
    pub platform_path_errors: BTreeMap<String, bool>,
    pub(super) path_jobs: Vec<PathJob>,
    /// The folder pickers of the "Choose..." buttons.
    pub(super) pick_jobs: Vec<PathJob>,
    /// The Roblox tab's own state, cleared when the tab goes: the cookie
    /// field, an add under way and its error.
    pub roblox_cookie: String,
    pub roblox_adding: bool,
    pub roblox_error: String,
    pub(super) roblox_job: Option<Pending<BackendResult<RobloxAccount>>>,
    /// The Steam tab's CS2 bridge card, while the tab is shown.
    pub cs2: Option<Cs2Form>,
    /// The "Refresh now" runs in flight.
    pub(super) avatar_refresh: Option<Pending<RefreshJobResult>>,
    pub(super) ban_refresh: Option<Pending<RefreshJobResult>>,
    /// The Platforms tab's search text.
    pub platform_search: String,
    /// mod+f asked for the search field: focus it and select its text.
    pub focus_platform_search: bool,
    /// What the backend last read in the descriptor folder.
    pub user_platforms: Option<UserPlatformReport>,
    pub descriptor_busy: bool,
    pub descriptor_notice: String,
    pub descriptor_error: String,
    pub(super) descriptor_job: Option<(Pending<BackendResult<DescriptorDone>>, DescriptorTask)>,
    /// The picked file the preview dialog shows.
    pub descriptor_preview: Option<DescriptorPreview>,
    /// Bumped each time the preview opens: its animations and focus restart.
    pub preview_mount: u64,
    /// The path the picker returned, held until the user decides.
    pub(super) preview_path: String,
    /// The descriptor the removal confirm is about, and its ticket.
    pub(super) pending_removal: Option<(String, String, ConfirmTicket)>,
    /// The theme editor, open over the General tab.
    pub theme_editor: Option<ThemeEditor>,
    /// Bumped per open of the theme editor.
    pub(super) theme_editor_mount: u64,
    /// A theme import from the clipboard under way.
    pub theme_import: Option<ThemeImport>,
    /// The scroll offset of the tab's content.
    pub scroll: f32,
    /// Text for the clipboard once the frame has the context.
    pub(super) clipboard: Option<String>,
}

/// One tab of the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct TabDef {
    pub id: String,
    pub label_key: String,
    /// `None` for the core tabs (their icon is drawn instead of a dot).
    pub accent: Option<crate::theme::Rgba>,
}

/// The sidebar's tabs: the three core ones, then each shipped platform
/// with a settings tab that is enabled, in registry order.
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

/// The label key of a shipped platform's settings tab; `None` for a
/// platform without one.
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

/// The digits of a PIN field's text, at most a code's length.
pub(super) fn pin_digits(text: &str) -> String {
    sanitize_pin_digits(text)
        .chars()
        .take(PIN_CODE_LENGTH)
        .collect()
}

impl SettingsPanel {
    /// Opens the panel: the draft from the store, normalised, and a job that
    /// reads whether a Steam API key is stored and each enabled platform's
    /// path.
    pub fn open(
        stores: &ClientStores,
        jobs: &Jobs,
        platforms: &[PlatformDef],
        os: RuntimeOs,
        user_platforms: Option<UserPlatformReport>,
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
            path_jobs: Vec::new(),
            pick_jobs: Vec::new(),
            roblox_cookie: String::new(),
            roblox_adding: false,
            roblox_error: String::new(),
            roblox_job: None,
            cs2: None,
            avatar_refresh: None,
            ban_refresh: None,
            platform_search: String::new(),
            focus_platform_search: false,
            user_platforms,
            descriptor_busy: false,
            descriptor_notice: String::new(),
            descriptor_error: String::new(),
            descriptor_job: None,
            descriptor_preview: None,
            preview_mount: 0,
            theme_editor: None,
            theme_editor_mount: 0,
            theme_import: None,
            preview_path: String::new(),
            pending_removal: None,
            scroll: 0.0,
            clipboard: None,
            draft,
        };
        panel.last_tracked = panel.tracked();
        panel
    }

    /// A PIN is set and the current one was not typed yet: changing it asks
    /// for the current one first.
    pub fn pin_guarded(&self) -> bool {
        self.draft.pin_enabled && !self.pin_change_authorized
    }

    /// The PIN toggle's state.
    pub fn pin_toggle_on(&self) -> bool {
        (self.draft.pin_enabled || self.pin_setup_pending) && !self.pin_disable_pending
    }

    /// A "Refresh now" run of this kind is in flight.
    pub fn refresh_loading(&self, kind: RefreshKind) -> bool {
        match kind {
            RefreshKind::Avatars => self.avatar_refresh.is_some(),
            RefreshKind::Bans => self.ban_refresh.is_some(),
        }
    }

    /// The Steam tab is drawn: active, and its path read.
    pub(super) fn steam_mounted(&self) -> bool {
        self.tab_bar.active_tab() == "platform:steam" && self.platform_paths.contains_key("steam")
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

    /// A number field and the setting it edits.
    pub(super) fn numeric_mut(&mut self, field: NumericField) -> (&mut NumericInput, &mut i64) {
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

    /// Every number field shows the draft's value again.
    pub(super) fn refresh_numeric(&mut self) {
        let d = &self.draft;
        self.ui_scale.refresh(d.ui_scale_percent);
        self.bg_opacity.refresh(d.background_opacity);
        self.avatar_cache_days
            .refresh(d.data_refresh.avatar_cache_days);
        self.ban_check_days.refresh(d.data_refresh.ban_check_days);
        self.inactivity_blur.refresh(d.inactivity_blur_seconds);
    }

    /// Writes a typed PIN into the draft when the PIN may change (being set
    /// up, or authorised by the current one) and the code is complete; true
    /// when it did.
    pub(super) fn commit_pin_code(&mut self) -> bool {
        let sanitized = sanitize_pin_digits(&self.pin_code_input);
        if !self.draft.pin_enabled && !self.pin_setup_pending {
            return false;
        }
        if self.pin_guarded() {
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

    /// Turns the PIN off and forgets the code.
    pub(super) fn disable_pin(&mut self) {
        self.draft.pin_enabled = false;
        self.pin_setup_pending = false;
        self.draft.pin_hash.clear();
        self.pin_code_input.clear();
        self.pin_disable_pending = false;
    }

    /// The Privacy tab is entered: its local state starts over and the
    /// telemetry state is read again.
    pub(super) fn mount_privacy(&mut self, jobs: &Jobs) {
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

    pub(super) fn refresh_telemetry(&mut self, jobs: &Jobs) {
        self.telemetry_job = Some(jobs.run(|backend| backend.telemetry_get_state()));
    }
}
