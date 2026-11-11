//! The save: the debounce, the write of the merged settings, the API key
//! and platform path pushes, and the merge of settings another writer
//! stored meanwhile.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use accshift_app::actions::toasts::{AddToastOptions, ToastType};
use accshift_app::backend::Backend;
use accshift_app::i18n::TranslationValue;
use accshift_app::platforms::path_error::platform_path_error_message;
use accshift_app::settings::{
    api_key_draft_after_successful_save, merge_settings_draft, AppSettings,
};
use accshift_app::text::Text;
use serde_json::Value;

use super::super::platforms::{self, PlatformDef};
use super::super::{AppModel, Booted};
use super::draft::{normalize, platform_snapshot};
use super::state::SettingsPanel;
use super::toast;

/// How long after the last tracked change the save runs.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(220);
/// The shortest gap between two "saved" toasts.
const SAVE_TOAST_COOLDOWN: Duration = Duration::from_millis(1500);

impl SettingsPanel {
    /// Every field whose change queues a save, as one string.
    pub(super) fn tracked(&self) -> String {
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

    /// What a save of `settings` writes, as one string: a save whose
    /// snapshot equals the last one has nothing to do.
    pub(super) fn persist_snapshot(&self, settings: &AppSettings) -> String {
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

    /// A change of a tracked field restarts the save's debounce, once the
    /// panel is hydrated.
    pub(super) fn queue_save(&mut self, now: Instant) {
        let tracked = self.tracked();
        if tracked == self.last_tracked {
            return;
        }
        self.last_tracked = tracked;
        if self.hydrated {
            self.save_at = Some(now + SAVE_DEBOUNCE);
        }
    }

    /// True, and the debounce cleared, once the queued save is due.
    pub(super) fn take_due_save(&mut self, now: Instant) -> bool {
        let due = self.save_at.is_some_and(|at| now >= at);
        if due {
            self.save_at = None;
        }
        due
    }

    /// Adopts a change of the stored settings the panel did not write: it
    /// lands in every field the user has not edited, and a draft that was
    /// saved stays saved.
    pub(super) fn adopt_external_settings(&mut self, stored: &AppSettings) {
        if !self.hydrated || stored == &self.seen_store {
            return;
        }
        let stored = stored.clone();
        self.seen_store = stored.clone();
        let draft = self.draft.clone();
        let was_saved = self.persist_snapshot(&self.draft) == self.last_persisted_snapshot;
        let next = merge_settings_draft(&stored, &self.persisted, &draft).unwrap_or(draft);
        self.persisted = stored;
        self.draft = next;
        self.refresh_numeric();
        if was_saved {
            self.last_persisted_snapshot = self.persist_snapshot(&self.draft);
        }
    }

    /// Pushes a typed Steam API key; false when the backend refused it. A
    /// blank key is dropped, the stored one stays.
    fn save_api_key(&mut self, backend: &dyn Backend) -> bool {
        if !self.api_key_touched {
            return true;
        }
        let trimmed = self.api_key.trim().to_string();
        if trimmed.is_empty() {
            self.api_key_touched = false;
            return true;
        }
        if backend.steam_set_api_key(&trimmed).is_err() {
            self.api_key_error = true;
            return false;
        }
        let next = api_key_draft_after_successful_save(&self.api_key, &trimmed);
        self.api_key_configured = true;
        self.api_key = next.api_key;
        self.api_key_touched = next.touched;
        self.api_key_error = false;
        true
    }

    /// Pushes every platform path that changed since the `previous` save;
    /// returns the toasts of the ones the backend refused.
    fn save_platform_paths(
        &mut self,
        backend: &dyn Backend,
        platforms: &[PlatformDef],
        previous: &Value,
    ) -> Vec<Text> {
        let mut failures = Vec::new();
        for (id, path) in &self.platform_paths {
            let next = path.trim();
            if previous["platformPaths"][id].as_str().unwrap_or("") == next {
                continue;
            }
            let saved = backend.platform_set_path(id, next);
            if let Err(error) = &saved {
                failures.push(path_save_failure(platforms, id, &error.message));
            }
            self.platform_path_errors.insert(id.clone(), saved.is_err());
        }
        failures
    }

    /// True, and the cooldown restarted, when the "saved" toast may show.
    fn saved_toast_due(&mut self, now: Instant) -> bool {
        let due = self
            .last_saved_toast
            .is_none_or(|at| now.duration_since(at) >= SAVE_TOAST_COOLDOWN);
        if due {
            self.last_saved_toast = Some(now);
        }
        due
    }
}

/// The toast of a refused platform path: a known error code says what is
/// wrong with the folder, the rest names the platform.
fn path_save_failure(platforms: &[PlatformDef], id: &str, message: &str) -> Text {
    let name = platforms
        .iter()
        .find(|p| p.id == id)
        .map_or_else(|| id.to_string(), |p| p.name.clone());
    platform_path_error_message(
        message,
        Text::with(
            "settings.pathSaveFailed",
            [("platform", TranslationValue::Str(name))],
        ),
    )
}

/// Start with the system is the one setting with an OS side effect: the
/// Run value is written or removed when it flips. `Some` holds its new
/// state.
fn autostart_change(previous: &Value, draft: &AppSettings) -> Option<bool> {
    let was = previous["settings"]["startWithSystem"]
        .as_bool()
        .unwrap_or(false);
    (was != draft.start_with_system).then_some(draft.start_with_system)
}

/// The enabled platforms changed: a tab that is no longer enabled falls
/// back to the first one the settings allow.
fn keep_active_tab_enabled(b: &mut Booted) {
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

impl AppModel {
    /// Saves now, without waiting for the debounce.
    pub(in crate::app) fn flush_settings(&mut self, now: Instant) {
        if let Some(p) = self.settings_panel.as_mut() {
            p.save_at = None;
        }
        self.persist_settings(now);
    }

    /// Writes the draft when it differs from the last save: the settings,
    /// then the autostart, the API key and the platform paths. A refused
    /// key or path keeps the save pending; a full save toasts.
    pub(super) fn persist_settings(&mut self, now: Instant) {
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
        let autostart = autostart_change(&previous, &draft);
        match merge_settings_draft(b.stores.settings(), &panel.persisted, &draft) {
            Ok(merged) => b.stores.save_settings(&merged),
            Err(_) => b.stores.save_settings(&draft),
        }
        panel.seen_store = b.stores.settings().clone();
        panel.persisted = draft;
        if let Some(enabled) = autostart {
            if self.backend.set_autostart(enabled).is_err() {
                toast(
                    &mut self.toasts,
                    "settings.autostartFailed",
                    ToastType::Error,
                    now,
                );
            }
        }
        if pin_committed {
            toast(
                &mut self.toasts,
                "settings.pinSaved",
                ToastType::Success,
                now,
            );
        }
        let api_key_saved = panel.save_api_key(self.backend.as_ref());
        if !api_key_saved {
            toast(
                &mut self.toasts,
                "settings.apiKeySaveFailed",
                ToastType::Error,
                now,
            );
        }
        let path_failures =
            panel.save_platform_paths(self.backend.as_ref(), &b.platforms, &previous);
        let paths_saved = path_failures.is_empty();
        for text in path_failures {
            self.toasts
                .add(text, AddToastOptions::kind(ToastType::Error), now);
        }
        panel.last_platform_snapshot = next_platforms;
        if platforms_changed {
            keep_active_tab_enabled(b);
        }
        if !api_key_saved || !paths_saved {
            return;
        }
        panel.last_persisted_snapshot = snapshot;
        if panel.saved_toast_due(now) {
            toast(&mut self.toasts, "settings.saved", ToastType::Info, now);
        }
    }
}
