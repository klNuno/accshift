use crate::error::PlatformErrorKind;

/// Telemetry events emitted by the app.
///
/// An `Event` only carries the variable fields specific to that event. Stable
/// per-session fields (app_version, os, arch, os_version, locale, surface)
/// live in `TelemetryContext` and are merged at serialization time.
#[derive(Debug, Clone)]
pub enum Event {
    /// Daily ping for DAU / MAU measurement.
    ///
    /// Emitted by the queue itself, not by a caller: it has to fire once a day
    /// for as long as the process lives, and it carries the number of events
    /// the queue had to drop since the previous ping, which only the queue
    /// knows.
    Ping { dropped_events: u64 },
    /// First launch of this installation, emitted once ever.
    FirstRun,
    /// App launch time (between `main()` and the first frame).
    AppLaunched { duration_ms: u64 },
    /// Account switch on a given platform. `error_code` classifies a failure
    /// into a fixed vocabulary; it is never a message.
    PlatformSwitch {
        platform: String,
        duration_ms: u64,
        success: bool,
        error_code: Option<String>,
    },
    /// A persona activation: how many platforms it targeted and how many
    /// switched successfully. No persona name, no account data.
    PersonaSwitch { platforms: u64, succeeded: u64 },
    /// An add-account flow was opened for a platform.
    AccountAddStarted { platform: String },
    /// An add-account flow was abandoned before it produced an account.
    AccountAddCancelled { platform: String },
    /// A new account finished its add flow on a platform. Platform id only.
    AccountAdded { platform: String },
    /// A named operation failed. `operation` and `error_code` both come from
    /// fixed vocabularies, so no user data can reach this event.
    OperationFailed {
        operation: String,
        platform: Option<String>,
        error_code: String,
    },
    /// A stage of the in-app update flow.
    Update {
        stage: UpdateStage,
        target_version: Option<String>,
        error_code: Option<String>,
    },
    /// A CLI invocation. Subcommand name only, never its arguments.
    CliCommand {
        command: String,
        success: bool,
        error_code: Option<String>,
    },
    /// The streamer-mode overlay auto-activated (streaming software detected).
    StreamerModeActivated,
    /// An accshift:// deep link triggered an action. No URL contents.
    DeepLinkUsed,
    /// End of session with total duration.
    SessionEnded { duration_ms: u64 },
    /// Snapshot of the number of accounts configured for a platform.
    /// Mode B only (requires a stable install_id).
    AccountsSnapshot { platform: String, count: u64 },
    /// Snapshot of the non-identifying app settings, once per launch.
    ///
    /// Mode B only. Each field is low-entropy on its own, but nine of them
    /// together are a weak fingerprint, and Mode A exists precisely so that
    /// two events cannot be tied to the same installation across days.
    ///
    /// No theme id: built-in ids would be safe, but a custom theme is named by
    /// the user and there is no way for this crate to tell the two apart.
    SettingsSnapshot {
        ui_language: String,
        enabled_platforms: Vec<String>,
        personas_enabled: bool,
        pin_enabled: bool,
        cli_enabled: bool,
        deep_links_enabled: bool,
        streamer_mode: String,
        animations: String,
    },
}

/// Stage of the updater flow. Each maps to its own PostHog event name so a
/// funnel can be built without unpacking a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    Available,
    Downloaded,
    Applied,
    Failed,
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::Ping { .. } => "ping",
            Event::FirstRun => "first_run",
            Event::AppLaunched { .. } => "app_launched",
            Event::PlatformSwitch { .. } => "platform_switch",
            Event::PersonaSwitch { .. } => "persona_switch",
            Event::AccountAddStarted { .. } => "account_add_started",
            Event::AccountAddCancelled { .. } => "account_add_cancelled",
            Event::AccountAdded { .. } => "account_added",
            Event::OperationFailed { .. } => "operation_failed",
            Event::Update { stage, .. } => match stage {
                UpdateStage::Available => "update_available",
                UpdateStage::Downloaded => "update_downloaded",
                UpdateStage::Applied => "update_applied",
                UpdateStage::Failed => "update_failed",
            },
            Event::CliCommand { .. } => "cli_command",
            Event::StreamerModeActivated => "streamer_mode_activated",
            Event::DeepLinkUsed => "deep_link_used",
            Event::SessionEnded { .. } => "session_ended",
            Event::AccountsSnapshot { .. } => "accounts_snapshot",
            Event::SettingsSnapshot { .. } => "settings_snapshot",
        }
    }

    /// True for events that require a stable install_id to mean anything, and
    /// that the queue drops before a Mode A upload.
    pub fn is_mode_b_only(&self) -> bool {
        matches!(
            self,
            Event::AccountsSnapshot { .. } | Event::SettingsSnapshot { .. }
        )
    }
}

/// The value every unrecognised code collapses to.
pub const UNKNOWN_CODE: &str = "other";

/// Every error code that may reach the network.
///
/// A closed vocabulary rather than a sanitizer. Normalising an arbitrary
/// string is not enough: `C:\Users\alice\...` survives character filtering
/// with the account name intact, and an error message is exactly the kind of
/// string that carries paths and usernames. Anything not listed here becomes
/// `other`, so a caller that passes a raw message leaks a category at worst.
pub const ERROR_CODES: &[&str] = &[
    // PlatformErrorKind, one for one.
    "client_not_installed",
    "client_running",
    "account_not_found",
    "setup_expired",
    "lock_contended",
    "io",
    "crypto",
    // Updater flow.
    "check_failed",
    "download_failed",
    "install_failed",
    "relaunch_failed",
    // CLI-only outcomes.
    "cli_disabled",
    "platform_unavailable",
    "pin_denied",
    "unknown_folder",
    "folder_store_error",
    UNKNOWN_CODE,
];

/// Every operation name that may reach the network.
pub const OPERATIONS: &[&str] = &[
    "platform_switch",
    "account_add",
    "account_forget",
    "profile_capture",
    "session_check",
    "bulk_edit",
    "game_settings_copy",
    "cs2_bridge_fetch",
    "avatar_refresh",
    "ban_check",
    UNKNOWN_CODE,
];

/// Every CLI subcommand name that may reach the network.
pub const CLI_COMMANDS: &[&str] = &["list", "switch", "platforms", UNKNOWN_CODE];

/// UI languages the app ships. An unlisted value becomes `other` rather than
/// travelling as typed.
pub const UI_LANGUAGES: &[&str] = &["en", "fr", "es", "pt", "pt_br", "ru", "zh", UNKNOWN_CODE];

/// Maximum length of a normalized identifier-like value.
const MAX_CODE_LEN: usize = 40;

/// Normalizes a string to lowercase `[a-z0-9_]`.
///
/// Shape only. On its own this is NOT a privacy boundary, which is why every
/// caller pairs it with [`code_from`] and a closed vocabulary.
pub fn sanitize_code(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_CODE_LEN));
    for ch in raw.trim().chars() {
        if out.len() == MAX_CODE_LEN {
            break;
        }
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() || lowered == '_' {
            out.push(lowered);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        UNKNOWN_CODE.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Maps a value onto a closed vocabulary, or onto `other`.
///
/// The single gate that makes it impossible for a message, a path or an
/// account name to reach the network through a code field, whatever a caller
/// passes.
pub fn code_from(raw: &str, allowed: &[&'static str]) -> &'static str {
    let normalized = sanitize_code(raw);
    allowed
        .iter()
        .copied()
        .find(|candidate| *candidate == normalized)
        .unwrap_or(UNKNOWN_CODE)
}

/// Maps a platform id onto the canonical registry, or onto `other`.
///
/// Matched verbatim, not normalized: `battle-net` has to stay `battle-net` on
/// the wire. It was `battle_net` before v1.0 and dashboards already alias the
/// two across that boundary; a second spelling change would break them again.
pub fn platform_code(raw: &str) -> &'static str {
    crate::platforms::ids::ALL
        .iter()
        .copied()
        .find(|id| *id == raw)
        .unwrap_or(UNKNOWN_CODE)
}

/// Fixed error code for a typed platform failure.
pub fn error_code_for_kind(kind: PlatformErrorKind) -> &'static str {
    match kind {
        PlatformErrorKind::ClientNotInstalled => "client_not_installed",
        PlatformErrorKind::ClientRunning => "client_running",
        PlatformErrorKind::AccountNotFound => "account_not_found",
        PlatformErrorKind::SetupExpired => "setup_expired",
        PlatformErrorKind::LockContended => "lock_contended",
        PlatformErrorKind::Io => "io",
        PlatformErrorKind::Crypto => "crypto",
        PlatformErrorKind::Other => UNKNOWN_CODE,
    }
}

/// Stable session context (invariant for every request).
#[derive(Debug, Clone)]
pub struct TelemetryContext {
    pub app_version: String,
    /// Fixed identifier: `windows`, `macos`, `linux`.
    pub os: String,
    /// Target architecture: `x86_64`, `aarch64`.
    pub arch: String,
    /// Human-readable OS version, for support decisions.
    pub os_version: String,
    pub locale: Option<String>,
    /// `gui` or `cli`. Without it, a CLI switch is indistinguishable from an
    /// app switch and every per-user average is wrong.
    pub surface: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_normalizes_shape() {
        assert_eq!(sanitize_code("Account Not Found"), "account_not_found");
        assert_eq!(sanitize_code("io"), "io");
        assert_eq!(sanitize_code("  spaced  "), "spaced");
    }

    #[test]
    fn sanitize_never_returns_empty() {
        assert_eq!(sanitize_code(""), UNKNOWN_CODE);
        assert_eq!(sanitize_code("   "), UNKNOWN_CODE);
        assert_eq!(sanitize_code("!!!"), UNKNOWN_CODE);
    }

    #[test]
    fn sanitize_truncates_long_input() {
        let long = "a".repeat(200);
        assert_eq!(sanitize_code(&long).len(), MAX_CODE_LEN);
    }

    #[test]
    fn a_raw_message_cannot_leak_through_a_code_field() {
        // Character filtering alone leaves "alice" intact, which is the whole
        // reason codes go through a closed vocabulary.
        let code = code_from(r"C:\Users\alice\Steam\loginusers.vdf missing", ERROR_CODES);
        assert_eq!(code, UNKNOWN_CODE);
    }

    #[test]
    fn known_codes_survive_the_vocabulary() {
        assert_eq!(code_from("client_running", ERROR_CODES), "client_running");
        assert_eq!(code_from("Client Running", ERROR_CODES), "client_running");
        assert_eq!(code_from("switch", CLI_COMMANDS), "switch");
        assert_eq!(code_from("pt-BR", UI_LANGUAGES), "pt_br");
        assert_eq!(code_from("klingon", UI_LANGUAGES), UNKNOWN_CODE);
    }

    #[test]
    fn every_platform_error_kind_maps_into_the_vocabulary() {
        for kind in [
            PlatformErrorKind::ClientNotInstalled,
            PlatformErrorKind::ClientRunning,
            PlatformErrorKind::AccountNotFound,
            PlatformErrorKind::SetupExpired,
            PlatformErrorKind::LockContended,
            PlatformErrorKind::Io,
            PlatformErrorKind::Crypto,
            PlatformErrorKind::Other,
        ] {
            let code = error_code_for_kind(kind);
            assert!(
                ERROR_CODES.contains(&code),
                "{code} is emitted but not declared in ERROR_CODES"
            );
        }
    }

    #[test]
    fn snapshot_events_are_the_only_mode_b_only_ones() {
        assert!(Event::AccountsSnapshot {
            platform: "steam".into(),
            count: 3
        }
        .is_mode_b_only());
        assert!(!Event::Ping { dropped_events: 0 }.is_mode_b_only());
        assert!(!Event::DeepLinkUsed.is_mode_b_only());
    }

    #[test]
    fn update_stages_have_distinct_event_names() {
        let names: Vec<&str> = [
            UpdateStage::Available,
            UpdateStage::Downloaded,
            UpdateStage::Applied,
            UpdateStage::Failed,
        ]
        .into_iter()
        .map(|stage| {
            Event::Update {
                stage,
                target_version: None,
                error_code: None,
            }
            .name()
        })
        .collect();
        assert_eq!(
            names,
            [
                "update_available",
                "update_downloaded",
                "update_applied",
                "update_failed"
            ]
        );
    }
}
