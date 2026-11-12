//! The settings panel: the draft it edits, the background jobs it starts,
//! the actions its controls send, and how it saves.
//!
//! The panel edits a draft of [`AppSettings`]. Every change of a tracked
//! field queues a save 220 ms later; the save normalises the draft, commits
//! a typed PIN, merges the draft over the stored settings with
//! [`merge_settings_draft`] (a field another writer changed meanwhile is
//! kept), writes it through [`ClientStores::save_settings`], then pushes the
//! API key and the platform paths and toasts `settings.saved` at most every
//! 1.5 s. A change of the stored settings that the panel did not write is
//! adopted into every field the user has not edited. Escape and the close
//! button flush, then close.
//!
//! The Platforms tab toggles platforms, picks the startup one and runs the
//! descriptor folder calls one at a time ([`SettingsPanel::descriptor_busy`]).
//! The per-platform tabs read and edit the same state
//! ([`SettingsPanel::platform_paths`], the API key draft, the numeric inputs
//! of the data refresh).
//!
//! - [`state`]: the panel's state, its tabs and its PIN state.
//! - [`draft`]: the number fields, the toggled flags and the normalisation.
//! - [`jobs`]: the background jobs and the per-frame tick that polls them.
//! - [`cs2`]: the Steam tab's CS2 bridge card.
//! - [`actions`]: what each [`SettingsAction`] does.
//! - [`persist`]: the save, and the merge with external changes.
//!
//! [`AppSettings`]: accshift_app::settings::AppSettings
//! [`merge_settings_draft`]: accshift_app::settings::merge_settings_draft
//! [`ClientStores::save_settings`]: accshift_app::stores::ClientStores::save_settings

mod actions;
mod cs2;
mod draft;
mod jobs;
mod persist;
mod state;

use std::time::Instant;

use accshift_app::accounts::refresh::RefreshKind;
use accshift_app::actions::toasts::{AddToastOptions, ToastQueue, ToastType};
use accshift_app::i18n::Locale;
use accshift_app::settings::AnimationsMode;
use accshift_app::text::Text;

use super::theme_editor::ThemeEditorEvent;
use super::AppModel;

pub use cs2::{Cs2Form, Cs2Status};
pub use draft::{clamp_int, js_number, normalize, NumericField, NumericInput};
pub use state::{visible_tabs, SettingsPanel, TabDef};

/// Digits in a PIN code.
pub const PIN_CODE_LENGTH: usize = 4;
/// The help button's page.
pub const WIKI_URL: &str = "https://github.com/klNuno/accshift/wiki";
/// The page the integrations' help buttons open.
pub const INTEGRATIONS_WIKI_URL: &str = "https://github.com/klNuno/accshift/wiki/Settings";
/// What the telemetry collects, linked from the Privacy tab.
pub const TELEMETRY_DOC_URL: &str =
    "https://github.com/klNuno/accshift/blob/main/docs/analytics.md";
/// The CS2 bridge card's help page.
pub const CS2_WIKI_URL: &str = "https://github.com/klNuno/accshift/wiki/CS2-Bridge";

/// A boolean setting a toggle flips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingFlag {
    ShowCardNotesInline,
    ExpandedFolders,
    SuspendGraphicsWhenMinimized,
    MinimizeOnAccountSwitch,
    /// Start with the system: the save writes or removes the Windows Run
    /// value.
    StartWithSystem,
    /// The close button hides to the tray.
    CloseToTray,
    DeepLinks,
    Cli,
    /// Streamer mode: `auto` when on, `off` when off.
    StreamerMode,
    /// The Platforms tab's personas chip.
    Personas,
    /// The Steam tab's user name toggle.
    ShowUsernames,
    /// Steam starts as administrator.
    SteamRunAsAdmin,
    /// Steam's shutdown mode: `force` when on, `graceful` when off.
    SteamForceShutdown,
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
    /// A slider's or a number field's text, as it changes.
    NumericInput(NumericField, String),
    /// Commits the typed number: blur, Enter, or a slider's release.
    NumericCommit(NumericField),
    /// The new PIN field.
    PinInput(String),
    /// The current PIN field of a guarded change.
    CurrentPinInput(String),
    TogglePin,
    ReplayOnboarding,
    ExportTheme,
    /// "Import": a theme file from the clipboard.
    ImportTheme,
    /// "Customize" or "Edit": the theme editor on the selected theme.
    CustomizeTheme,
    /// The theme editor's buttons, Escape or a click on its overlay.
    ThemeEditor(ThemeEditorEvent),
    ToggleTelemetryA,
    ToggleTelemetryB,
    RetryTelemetryDeletion,
    ExportTelemetry,
    RetryTelemetryLoad,
    /// The Platforms tab's search field.
    PlatformSearch(String),
    /// A platform chip: enables or disables the platform.
    TogglePlatform(String),
    /// The startup platform select.
    DefaultPlatform(String),
    /// "Add from a file": pick, then preview.
    PickDescriptor,
    ReloadDescriptors,
    OpenDescriptorsFolder,
    /// A loaded descriptor's remove button: asks first.
    RemoveDescriptor {
        id: String,
        name: String,
    },
    /// The preview dialog's install button.
    InstallDescriptor,
    /// The preview dialog's cancel button or Escape.
    CancelDescriptorPreview,
    /// A platform tab's last login toggle.
    ToggleLastLogin(String),
    /// A platform tab's health check toggle: off when stored as `false`, on
    /// otherwise.
    ToggleHealthCheck(String),
    /// A platform tab's path field.
    PlatformPath(String, String),
    /// Its "Choose..." button: a folder picker.
    ChoosePlatformPath(String),
    /// The Roblox tab's cookie field.
    RobloxCookie(String),
    /// Its add button: adds the account the cookie signs in to.
    AddRobloxCookie,
    /// The Steam launch options field.
    SteamLaunchOptions(String),
    /// The Steam Web API key field.
    ApiKeyInput(String),
    /// "Clear key": removes the stored key.
    ClearApiKey,
    /// "Get key": opens Steam's API key page.
    OpenSteamApiKeyPage,
    /// A "Refresh now" button of the avatars or of the ban checks.
    RefreshNow(RefreshKind),
    /// The CS2 bridge toggle, once its settings are read.
    Cs2Toggle,
    /// The bridge link field; its blur saves the link.
    Cs2Url(String),
    Cs2UrlBlur,
    /// "Test": checks the link.
    Cs2Test,
    /// The token field; its blur saves a token that is not blank.
    Cs2Token(String),
    Cs2TokenBlur,
    /// "Clear token": saves an empty token.
    Cs2ClearToken,
    /// The CS2 card's "?" button.
    OpenCs2Wiki,
}

pub(super) fn toast(toasts: &mut ToastQueue, key: &str, kind: ToastType, now: Instant) {
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
        let tab = b.nav.active_tab.clone();
        b.accounts.cancel_add_flow(&self.jobs, &tab);
        b.bulk.close();
        b.nav.show_settings = true;
        self.settings_panel = Some(SettingsPanel::open(
            &b.stores,
            &self.jobs,
            &b.platforms,
            b.runtime_os,
            b.user_platforms.clone(),
        ));
    }

    /// Flushes the pending save, then closes the panel.
    pub(super) fn close_settings_panel(&mut self) {
        self.flush_settings(Instant::now());
        self.settings_panel = None;
        if let Some(b) = self.booted.as_mut() {
            b.nav.show_settings = false;
        }
    }

    /// Something else closed the panel (a platform tab, personas): the
    /// pending save still lands before the panel goes.
    pub(super) fn settings_panel_hidden(&mut self) {
        if self.booted.as_ref().is_some_and(|b| !b.nav.show_settings)
            && self.settings_panel.is_some()
        {
            self.flush_settings(Instant::now());
            self.settings_panel = None;
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

    /// mod+f in the settings scope: the Platforms tab, its search focused.
    pub(super) fn focus_platform_search(&mut self) {
        if let Some(panel) = self.settings_panel.as_mut() {
            if panel.tab_bar.active_tab() != "platforms" {
                panel.tab_bar.select("platforms");
                panel.scroll = 0.0;
            }
            panel.focus_platform_search = true;
        }
    }
}
