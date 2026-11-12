//! The platform model: everything computed before a widget draws, so the
//! egui UI only draws.
//!
//! - [`registry::PlatformRegistry`] holds the platform list (shipped plus the
//!   user's descriptors) and one [`adapter::PlatformAdapter`] per platform.
//! - [`adapter::PlatformAdapter`] holds account shapes, switch, add flow, labels,
//!   context menu entries, warnings and toast wording for one platform.
//! - [`steam`], [`riot`], [`roblox`] and [`generic`] hold the per-platform pieces.
//! - [`warnings`], [`card`], [`path_error`] and [`icons`] hold shared models.
//!
//! Every user-facing string is a [`crate::text::Text`] (an i18n key plus
//! parameters) resolved at draw time. Every backend call goes through
//! [`crate::Backend`] and blocks, so the shell runs them on the job pool.

pub mod adapter;
pub mod add_flow;
pub mod api;
pub mod card;
pub mod descriptors;
pub mod generic;
pub mod icons;
pub mod path_error;
pub mod registry;
pub mod riot;
pub mod roblox;
pub mod steam;
pub mod warnings;

use crate::text::Text;
use serde::{Deserialize, Serialize};

pub use crate::folders::FolderSection as DisplaySection;
pub use adapter::PlatformAdapter;
pub use registry::{PlatformDef, PlatformRegistry};

/// One account as every platform hands it to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAccount {
    pub id: String,
    pub display_name: String,
    pub username: String,
    /// Unix SECONDS. Steam reads seconds out of `loginusers.vdf`, every other
    /// platform stamps milliseconds, so each adapter converts once on its way
    /// here and the unit is carried by the name.
    pub last_login_at_sec: Option<f64>,
}

/// The accounts and the current one, read in one call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StartupSnapshot {
    pub accounts: Vec<PlatformAccount>,
    pub current_account: String,
}

/// The OS the backend runs on.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeOs {
    Windows,
    Linux,
    Macos,
    #[default]
    Unknown,
}

impl RuntimeOs {
    /// `get_runtime_os` answers `windows`, `linux` or `macos`.
    pub fn parse(os: &str) -> RuntimeOs {
        to_runtime_os(Some(os))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeOs::Windows => "windows",
            RuntimeOs::Linux => "linux",
            RuntimeOs::Macos => "macos",
            RuntimeOs::Unknown => "unknown",
        }
    }
}

/// The backend's OS name, `Unknown` for any other.
pub fn to_runtime_os(value: Option<&str>) -> RuntimeOs {
    match value {
        Some("windows") => RuntimeOs::Windows,
        Some("linux") => RuntimeOs::Linux,
        Some("macos") => RuntimeOs::Macos,
        _ => RuntimeOs::Unknown,
    }
}

/// A launcher path hint: one for every system, or one per system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPlaceholder {
    Text(String),
    /// In the order the entries were declared.
    PerOs(Vec<(RuntimeOs, String)>),
}

impl PathPlaceholder {
    fn for_os(&self, os: RuntimeOs) -> Option<&str> {
        match self {
            PathPlaceholder::Text(text) => Some(text),
            PathPlaceholder::PerOs(entries) => entries
                .iter()
                .find(|(entry_os, _)| *entry_os == os)
                .map(|(_, text)| text.as_str()),
        }
    }
}

/// The system's own hint, else the Windows, Linux then macOS one, else
/// empty.
pub fn resolve_path_placeholder(placeholder: Option<&PathPlaceholder>, os: RuntimeOs) -> String {
    let Some(placeholder) = placeholder else {
        return String::new();
    };
    [os, RuntimeOs::Windows, RuntimeOs::Linux, RuntimeOs::Macos]
        .into_iter()
        .find_map(|candidate| placeholder.for_os(candidate))
        .unwrap_or_default()
        .to_string()
}

/// How a platform refreshes profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProfileRefresh {
    pub avatars: bool,
    pub bans: bool,
}

/// Defaults and the sanitizer of the platform's `platformSettings[id]`
/// entry. Only Steam has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSettingsSchema {
    Steam,
}

impl PlatformSettingsSchema {
    /// `defaults()`, as the JSON the store holds.
    pub fn defaults(self) -> serde_json::Value {
        match self {
            PlatformSettingsSchema::Steam => steam::settings_schema::defaults_value(),
        }
    }

    /// `sanitize(raw, legacyRoot)`, as the JSON the store holds.
    pub fn sanitize(
        self,
        raw: Option<&serde_json::Value>,
        legacy_root: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> serde_json::Value {
        match self {
            PlatformSettingsSchema::Steam => {
                steam::settings_schema::sanitize_value(raw, legacy_root)
            }
        }
    }
}

/// What the app layer may offer for a platform, without loading its
/// adapter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformCapabilities {
    /// A multi-select bulk edit bar (titlebar button plus bar). The bar
    /// itself is a UI component.
    pub bulk_edit: bool,
    /// Takes part in the settings "refresh now" actions.
    pub profile_refresh: Option<ProfileRefresh>,
    /// Account cards can show a username line under the display name.
    pub account_usernames: bool,
    /// What an account with no known last login reads as.
    pub last_login_unknown_key: Option<&'static str>,
    /// Prefetch the profile right after a completed add flow.
    pub prime_profile_after_add: bool,
    /// Has account warning states worth rechecking on reloads.
    pub account_warnings: bool,
    /// Client-storage targets holding this platform's account data; an
    /// external change to one triggers an account reload on this tab.
    pub external_data_stores: Vec<&'static str>,
    /// The `platformSettings[id]` schema.
    pub settings: Option<PlatformSettingsSchema>,
}

/// A fetched profile. `None` from a fetch means "no data, keep the avatar
/// on screen"; `avatar_url: None` means the profile has no avatar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformProfileInfo {
    pub avatar_url: Option<String>,
    pub display_name: Option<String>,
    pub avatar_loading: bool,
}

/// A cached profile. `url: None` is a cached "no avatar" result that still
/// suppresses refetches.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CachedPlatformProfile {
    pub url: Option<String>,
    pub display_name: Option<String>,
    pub expired: bool,
}

/// The state of an add flow, plus whatever string a newer backend sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddFlowState {
    WaitingForClient,
    WaitingForLogin,
    /// Another accshift operation holds the cross-process lock. Non-terminal.
    Busy,
    Capturing,
    Ready,
    Failed,
    Other(String),
}

impl AddFlowState {
    pub fn parse(state: &str) -> Self {
        match state {
            "waiting_for_client" => AddFlowState::WaitingForClient,
            "waiting_for_login" => AddFlowState::WaitingForLogin,
            "busy" => AddFlowState::Busy,
            "capturing" => AddFlowState::Capturing,
            "ready" => AddFlowState::Ready,
            "failed" => AddFlowState::Failed,
            other => AddFlowState::Other(other.to_string()),
        }
    }
}

/// The status of one add flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddFlowStatus {
    pub setup_id: String,
    /// The raw state string, as the backend sent it.
    pub state: String,
    pub account_id: Option<String>,
    pub account_display_name: Option<String>,
    pub error_message: Option<String>,
}

impl AddFlowStatus {
    pub fn state(&self) -> AddFlowState {
        AddFlowState::parse(&self.state)
    }
}

/// What adding an account returned.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddAccountResult {
    pub setup_status: Option<AddFlowStatus>,
}

/// How a forced warning refresh ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningRefreshOutcome {
    /// Every requested account came back.
    Complete {
        checked: usize,
    },
    /// The fetch ran but some accounts are missing from the answer.
    Partial {
        checked: usize,
        requested: usize,
    },
    Failed,
    /// Nothing was fetched because the platform lacks its API key.
    NoApiKey,
}

/// How to load warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarningLoadOptions {
    pub force_refresh: bool,
    pub silent: bool,
}

impl Default for WarningLoadOptions {
    /// Not forced, silent.
    fn default() -> Self {
        Self {
            force_refresh: false,
            silent: true,
        }
    }
}

/// The counts of a bulk edit the app layer reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformBulkEditResult {
    pub succeeded: usize,
    pub failed: Vec<String>,
}

/// A toast an adapter asks for, with its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastRequest {
    pub text: Text,
    pub error: bool,
}
