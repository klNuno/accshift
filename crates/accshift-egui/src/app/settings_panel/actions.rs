//! What each [`SettingsAction`] does. The match stays one arm per action;
//! the longer arms call a method named after what they do, grouped below
//! by concern: tabs, PIN, theme, telemetry and links, platforms and paths,
//! descriptors, the Roblox cookie and the data refresh.

use std::time::{Duration, Instant};

use accshift_app::accounts::refresh::{run_refresh_job, RefreshKind};
use accshift_app::actions::dialogs::{ConfirmConfig, ConfirmTicket};
use accshift_app::actions::toasts::ToastType;
use accshift_app::backend::Backend;
use accshift_app::error::BackendResult;
use accshift_app::i18n::{Locale, TranslationValue};
use accshift_app::jobs::Jobs;
use accshift_app::secure::pin::{unlock_pin_session, PinCheck};
use accshift_app::text::Text;
use egui::Context;
use serde_json::Value;

use super::super::platforms::{self, PlatformDef, RuntimeOs};
use super::super::theme_editor::{ThemeEditor, ThemeEditorEvent, ThemeImport};
use super::super::{AppModel, Booted};
use super::draft::toggle_flag;
use super::jobs::{DescriptorDone, DescriptorTask, Followup};
use super::state::{pin_digits, SettingsPanel};
use super::{
    toast, SettingsAction, CS2_WIKI_URL, INTEGRATIONS_WIKI_URL, PIN_CODE_LENGTH, TELEMETRY_DOC_URL,
    WIKI_URL,
};

/// The confirm a descriptor's remove button asks.
fn removal_confirm(name: &str) -> ConfirmConfig {
    ConfirmConfig {
        title: Text::with(
            "descriptor.removeConfirmTitle",
            [("platform", TranslationValue::Str(name.to_string()))],
        ),
        message: Text::key("descriptor.removeConfirmMessage"),
        confirm_label: Some(Text::key("descriptor.remove")),
        confirm_color: None,
    }
}

impl AppModel {
    /// Applies one [`SettingsAction`].
    pub(in crate::app) fn apply_settings(&mut self, ctx: &Context, action: SettingsAction) {
        let now = Instant::now();
        if matches!(
            action,
            SettingsAction::Close | SettingsAction::ReplayOnboarding
        ) {
            // The pending save lands and the panel closes; the replay then
            // opens the onboarding.
            self.close_settings_panel();
            if action == SettingsAction::ReplayOnboarding {
                self.onboarding.open();
            }
            return;
        }
        let jobs = &self.jobs;
        let Some(panel) = self.settings_panel.as_mut() else {
            return;
        };
        match action {
            SettingsAction::Close | SettingsAction::ReplayOnboarding => {}
            SettingsAction::SelectTab(id) => panel.select_tab(jobs, id),
            SettingsAction::OpenHelp => panel.open_help_page(jobs, WIKI_URL),
            SettingsAction::OpenLogs => {
                panel.call(jobs, Followup::ErrorToast("settings.openLogsFailed"), |b| {
                    b.open_logs_folder()
                })
            }
            SettingsAction::OpenIntegrationsWiki => {
                panel.open_help_page(jobs, INTEGRATIONS_WIKI_URL);
            }
            SettingsAction::OpenTelemetryDoc => panel.open_help_page(jobs, TELEMETRY_DOC_URL),
            SettingsAction::Toggle(flag) => toggle_flag(&mut panel.draft, flag),
            SettingsAction::Language(locale) => panel.draft.language = locale,
            SettingsAction::Animations(mode) => panel.draft.animations = mode,
            SettingsAction::Theme(id) => panel.draft.theme_id = id,
            SettingsAction::NumericInput(field, text) => panel.numeric_mut(field).0.input = text,
            SettingsAction::NumericCommit(field) => {
                let (input, value) = panel.numeric_mut(field);
                *value = input.commit(*value);
            }
            SettingsAction::PinInput(text) => panel.pin_code_input = pin_digits(&text),
            SettingsAction::CurrentPinInput(text) => {
                let locale = self.booted.as_ref().map_or(Locale::En, |b| b.locale);
                panel.enter_current_pin(self.backend.as_ref(), locale, &text, now);
            }
            SettingsAction::TogglePin => panel.toggle_pin(),
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
            SettingsAction::ImportTheme => panel.import_theme(ctx),
            SettingsAction::CustomizeTheme => {
                if let Some(b) = self.booted.as_ref() {
                    panel.customize_theme(b);
                }
            }
            SettingsAction::ThemeEditor(event) => {
                panel.theme_editor_event(event, self.booted.as_ref(), jobs);
            }
            SettingsAction::ToggleTelemetryA => panel.toggle_telemetry_a(jobs),
            SettingsAction::ToggleTelemetryB => panel.toggle_telemetry_b(jobs),
            SettingsAction::RetryTelemetryDeletion => panel.retry_telemetry_deletion(jobs),
            SettingsAction::ExportTelemetry => panel.export_telemetry(jobs),
            SettingsAction::RetryTelemetryLoad => panel.refresh_telemetry(jobs),
            SettingsAction::PlatformSearch(text) => panel.platform_search = text,
            SettingsAction::TogglePlatform(id) => {
                if let Some(b) = self.booted.as_ref() {
                    panel.toggle_platform(jobs, &b.platforms, b.runtime_os, &id);
                }
            }
            SettingsAction::DefaultPlatform(id) => panel.draft.default_platform_id = id,
            SettingsAction::ReloadDescriptors => {
                panel.run_descriptor_task(jobs, DescriptorTask::Reload, |b| {
                    b.reload_user_platforms().map(DescriptorDone::Report)
                });
            }
            SettingsAction::PickDescriptor => panel.pick_descriptor(jobs),
            SettingsAction::InstallDescriptor => panel.install_descriptor(jobs),
            SettingsAction::CancelDescriptorPreview => panel.descriptor_preview = None,
            SettingsAction::OpenDescriptorsFolder => {
                panel.run_descriptor_task(jobs, DescriptorTask::OpenFolder, |b| {
                    b.open_descriptors_folder().map(|()| DescriptorDone::Opened)
                });
            }
            SettingsAction::RemoveDescriptor { id, name } => {
                let mut effects = Vec::new();
                let ticket = self
                    .dialogs
                    .request_confirm(removal_confirm(&name), &mut effects);
                panel.pending_removal = Some((id, name, ticket));
                self.apply_effects(ctx, effects);
            }
            SettingsAction::ToggleLastLogin(id) => {
                let shown = &mut panel.draft.account_display.show_last_login_per_platform;
                let next = !shown.get(&id).copied().unwrap_or(false);
                shown.insert(id, next);
            }
            SettingsAction::ToggleHealthCheck(id) => {
                let checks = &mut panel.draft.health_check_per_platform;
                let next = checks.get(&id) == Some(&false);
                checks.insert(id, next);
            }
            SettingsAction::PlatformPath(id, path) => {
                panel.platform_paths.insert(id, path);
            }
            SettingsAction::ChoosePlatformPath(id) => panel.choose_platform_path(jobs, id),
            SettingsAction::RobloxCookie(text) => panel.roblox_cookie = text,
            SettingsAction::AddRobloxCookie => panel.add_roblox_cookie(jobs),
            SettingsAction::SteamLaunchOptions(text) => {
                panel.draft.platform_settings.steam.launch_options = text;
            }
            SettingsAction::ApiKeyInput(text) => {
                panel.api_key = text;
                panel.api_key_touched = true;
            }
            SettingsAction::ClearApiKey => {
                panel.call(jobs, Followup::ClearApiKey, |b| b.steam_set_api_key(""));
            }
            SettingsAction::OpenSteamApiKeyPage => panel.call(
                jobs,
                Followup::InfoToast("settings.openApiKeyFailed"),
                |b| b.steam_open_api_key_page(),
            ),
            SettingsAction::OpenCs2Wiki => panel.open_help_page(jobs, CS2_WIKI_URL),
            SettingsAction::RefreshNow(kind) => {
                if let Some(b) = self.booted.as_ref() {
                    panel.start_refresh(jobs, b, kind);
                }
            }
            action @ (SettingsAction::Cs2Toggle
            | SettingsAction::Cs2Url(_)
            | SettingsAction::Cs2UrlBlur
            | SettingsAction::Cs2Test
            | SettingsAction::Cs2Token(_)
            | SettingsAction::Cs2TokenBlur
            | SettingsAction::Cs2ClearToken) => {
                if let Some(form) = panel.cs2.as_mut() {
                    form.apply(jobs, action);
                }
            }
        }
    }

    /// The removal confirm settled: an allowed removal runs.
    pub(in crate::app) fn settle_descriptor_removal(
        &mut self,
        ticket: ConfirmTicket,
        allowed: bool,
    ) {
        let jobs = &self.jobs;
        let Some(panel) = self.settings_panel.as_mut() else {
            return;
        };
        let Some((id, name, _)) = panel
            .pending_removal
            .take_if(|(_, _, pending)| *pending == ticket)
        else {
            return;
        };
        if allowed {
            let remove = id.clone();
            panel.run_descriptor_task(jobs, DescriptorTask::Remove { id, name }, move |b| {
                b.descriptor_remove(&remove).map(DescriptorDone::Report)
            });
        }
    }
}

/// Tabs.
impl SettingsPanel {
    /// Shows another tab. Leaving the Platforms tab clears its search and
    /// notices, any change of tab clears the Roblox tab's cookie and error,
    /// and entering the Privacy tab starts its state over.
    fn select_tab(&mut self, jobs: &Jobs, id: String) {
        let was_privacy = self.tab_bar.active_tab() == "privacy";
        if self.tab_bar.active_tab() == "platforms" && id != "platforms" {
            self.platform_search.clear();
            self.descriptor_notice.clear();
            self.descriptor_error.clear();
        }
        if self.tab_bar.active_tab() != id {
            self.roblox_cookie.clear();
            self.roblox_error.clear();
        }
        self.tab_bar.select(&id);
        self.scroll = 0.0;
        if id == "privacy" && !was_privacy {
            self.mount_privacy(jobs);
        }
    }
}

/// PIN.
impl SettingsPanel {
    /// The current PIN field of a guarded change. A full code is checked at
    /// once, on this thread (the PIN service answers locally): a match
    /// authorises the change, or turns the PIN off when that was asked; a
    /// miss says why and holds the field for the wait it imposes.
    fn enter_current_pin(
        &mut self,
        backend: &dyn Backend,
        locale: Locale,
        text: &str,
        now: Instant,
    ) {
        self.current_pin_input = pin_digits(text);
        if self.current_pin_input.len() != PIN_CODE_LENGTH || self.current_pin_busy_until.is_some()
        {
            return;
        }
        let code = std::mem::take(&mut self.current_pin_input);
        self.current_pin_error.clear();
        let check = unlock_pin_session(backend, &code, &self.draft.pin_hash);
        if let Some(wait) = check.failure_wait() {
            self.current_pin_error = if wait.too_many_attempts {
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
            self.current_pin_busy_until = Some(now + Duration::from_millis(wait.wait_ms));
            return;
        }
        self.pin_change_authorized = true;
        if self.pin_disable_pending {
            self.disable_pin();
        } else if let PinCheck::Match {
            rehashed: Some(hash),
        } = check
        {
            self.draft.pin_hash = hash;
        }
    }

    /// The PIN toggle: a guarded PIN asks for the current code before it
    /// turns off; otherwise it turns off, or starts the setup.
    fn toggle_pin(&mut self) {
        if self.pin_guarded() {
            self.pin_disable_pending = !self.pin_disable_pending;
        } else if self.draft.pin_enabled || self.pin_setup_pending {
            self.disable_pin();
        } else {
            self.pin_setup_pending = true;
        }
    }
}

/// Theme.
impl SettingsPanel {
    /// Reads a theme file from the clipboard, unless a read is under way.
    fn import_theme(&mut self, ctx: &Context) {
        if self.theme_import.is_none() {
            crate::clipboard::request_text(ctx);
            self.theme_import = Some(ThemeImport::Reading(0));
        }
    }

    /// Opens the theme editor on the selected theme.
    fn customize_theme(&mut self, b: &Booted) {
        self.theme_editor_mount += 1;
        self.theme_editor = Some(ThemeEditor::start_from(
            &b.registry,
            &self.draft.theme_id,
            b.locale,
            self.draft.background_opacity as f64,
            self.theme_editor_mount,
        ));
    }

    fn theme_editor_event(
        &mut self,
        event: ThemeEditorEvent,
        booted: Option<&Booted>,
        jobs: &Jobs,
    ) {
        match event {
            // The tick puts the settings' theme back.
            ThemeEditorEvent::Cancel => self.theme_editor = None,
            ThemeEditorEvent::Save => {
                if let (Some(editor), Some(b)) = (self.theme_editor.as_mut(), booted) {
                    editor.save(&b.registry, jobs);
                }
            }
            // A copy to the clipboard, without a toast.
            ThemeEditorEvent::Export => {
                if let Some(editor) = self.theme_editor.as_ref() {
                    self.clipboard = Some(editor.export());
                }
            }
        }
    }
}

/// Telemetry and links.
impl SettingsPanel {
    /// Queues a backend call; `followup` says what its answer does.
    fn call_for_value(
        &mut self,
        jobs: &Jobs,
        followup: Followup,
        f: impl FnOnce(&dyn Backend) -> BackendResult<Value> + Send + 'static,
    ) {
        self.calls.push((jobs.run(f), followup));
    }

    /// Queues a backend call that answers nothing but success or failure.
    fn call(
        &mut self,
        jobs: &Jobs,
        followup: Followup,
        f: impl FnOnce(&dyn Backend) -> BackendResult<()> + Send + 'static,
    ) {
        self.call_for_value(jobs, followup, move |b| f(b).map(|()| Value::Null));
    }

    /// Opens a help page in the browser; a failure toasts.
    fn open_help_page(&mut self, jobs: &Jobs, url: &'static str) {
        self.call(
            jobs,
            Followup::ErrorToast("settings.openHelpFailed"),
            move |b| b.open_url(url),
        );
    }

    /// Flips mode A at once; a failure reads the state again.
    fn toggle_telemetry_a(&mut self, jobs: &Jobs) {
        let Some(t) = self.telemetry.as_mut() else {
            return;
        };
        let next = !t.mode_a_enabled;
        t.mode_a_enabled = next;
        self.call(jobs, Followup::ModeA, move |b| b.telemetry_set_mode_a(next));
    }

    /// Asks to flip mode B; the toggle waits for the answer.
    fn toggle_telemetry_b(&mut self, jobs: &Jobs) {
        let Some(t) = self.telemetry.as_ref() else {
            return;
        };
        if self.mode_b_busy {
            return;
        }
        let next = !t.mode_b_enabled;
        self.mode_b_busy = true;
        self.call(jobs, Followup::ModeB { next }, move |b| {
            b.telemetry_set_mode_b(next)
        });
    }

    fn retry_telemetry_deletion(&mut self, jobs: &Jobs) {
        if self.mode_b_busy {
            return;
        }
        self.mode_b_busy = true;
        self.call(jobs, Followup::RetryForget, |b| b.telemetry_retry_forget());
    }

    fn export_telemetry(&mut self, jobs: &Jobs) {
        if self.export_busy {
            return;
        }
        self.export_busy = true;
        self.call_for_value(jobs, Followup::Export, |b| b.telemetry_export());
    }
}

/// Platforms and their paths.
impl SettingsPanel {
    /// A platform chip: the last selectable platform stays on; enabling one
    /// asks for its path. Selectable is known and supported on this system.
    fn toggle_platform(&mut self, jobs: &Jobs, defs: &[PlatformDef], os: RuntimeOs, id: &str) {
        let enabled = &mut self.draft.enabled_platforms;
        if enabled.iter().any(|p| p == id) {
            let selectable: Vec<&String> = enabled
                .iter()
                .filter(|p| platforms::usable(defs, p, os))
                .collect();
            if selectable.len() <= 1 && selectable.iter().any(|p| *p == id) {
                return;
            }
            enabled.retain(|p| p != id);
            return;
        }
        if !platforms::usable(defs, id, os) {
            return;
        }
        enabled.push(id.to_string());
        if !self.platform_paths.contains_key(id) {
            self.platform_paths.insert(id.to_string(), String::new());
            let id = id.to_string();
            self.path_jobs.push(jobs.run(move |backend| {
                let path = backend.platform_get_path(&id);
                (id, path)
            }));
        }
    }

    /// A path field's "Choose..." button: a folder picker.
    fn choose_platform_path(&mut self, jobs: &Jobs, id: String) {
        self.pick_jobs.push(jobs.run(move |b| {
            let path = b.platform_select_path(&id);
            (id, path)
        }));
    }
}

/// Descriptors.
impl SettingsPanel {
    /// Runs one descriptor folder call at a time, the buttons disabled
    /// meanwhile.
    pub(super) fn run_descriptor_task(
        &mut self,
        jobs: &Jobs,
        task: DescriptorTask,
        call: impl FnOnce(&dyn Backend) -> BackendResult<DescriptorDone> + Send + 'static,
    ) {
        if self.descriptor_job.is_some() {
            return;
        }
        self.descriptor_busy = true;
        self.descriptor_error.clear();
        self.descriptor_job = Some((jobs.run(call), task));
    }

    /// "Add from a file": picks a descriptor and reads its preview.
    fn pick_descriptor(&mut self, jobs: &Jobs) {
        self.run_descriptor_task(jobs, DescriptorTask::Pick, |b| {
            // A cancelled or failed picker leaves everything alone.
            let Ok(path) = b.descriptor_select_file() else {
                return Ok(DescriptorDone::Picked(None));
            };
            if path.is_empty() {
                return Ok(DescriptorDone::Picked(None));
            }
            let preview = b.descriptor_preview_file(&path)?;
            Ok(DescriptorDone::Picked(Some(Box::new((path, preview)))))
        });
    }

    /// Installs the previewed descriptor.
    fn install_descriptor(&mut self, jobs: &Jobs) {
        let Some(preview) = self.descriptor_preview.as_ref() else {
            return;
        };
        let name = preview.descriptor.name.clone();
        let path = self.preview_path.clone();
        self.run_descriptor_task(jobs, DescriptorTask::Install { name }, move |b| {
            b.descriptor_install_file(&path).map(DescriptorDone::Report)
        });
    }
}

/// The Roblox cookie and the data refresh.
impl SettingsPanel {
    /// Adds the Roblox account a pasted cookie signs in to, unless the field
    /// is blank or an add is under way.
    fn add_roblox_cookie(&mut self, jobs: &Jobs) {
        let trimmed = self.roblox_cookie.trim().to_string();
        if trimmed.is_empty() || self.roblox_adding {
            return;
        }
        self.roblox_adding = true;
        self.roblox_error.clear();
        self.roblox_job = Some(jobs.run(move |b| {
            // The paste is the second door into adding a Roblox account: it
            // reports its own funnel start. The failure half is reported by
            // the backend command.
            let _ = b.telemetry_track_account_add_started("roblox");
            b.roblox_add_account_by_cookie(&trimmed)
        }));
    }

    /// A "Refresh now" button, unless a run of that kind is in flight. The
    /// run reads the stored settings and the stores as they are now, works
    /// on copies and hands its writes back to the tick.
    fn start_refresh(&mut self, jobs: &Jobs, b: &Booted, kind: RefreshKind) {
        if self.refresh_loading(kind) {
            return;
        }
        let registry = b.accounts.registry.clone();
        let caches = std::sync::Arc::clone(b.accounts.loading.caches());
        let stores = b.stores.clone();
        let settings = b.stores.settings().clone();
        let tab = b.nav.active_tab.clone();
        let clock = b.accounts.clock.clone();
        let job = jobs.run(move |backend| {
            let now_ms = move || clock.now_ms();
            run_refresh_job(
                kind, &registry, &tab, backend, &caches, stores, &settings, &now_ms,
            )
        });
        match kind {
            RefreshKind::Avatars => self.avatar_refresh = Some(job),
            RefreshKind::Bans => self.ban_refresh = Some(job),
        }
    }
}
