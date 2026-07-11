/// Telemetry events emitted by the app.
///
/// An `Event` only carries the variable fields specific to that event. Stable
/// per-session fields (app_version, os_version, locale) live in
/// `TelemetryContext` and are merged at serialization time.
#[derive(Debug, Clone)]
pub enum Event {
    /// Daily ping for DAU / MAU measurement.
    Ping,
    /// App launch time (between `main()` and the first frame).
    AppLaunched { duration_ms: u64 },
    /// Account switch on a given platform.
    PlatformSwitch {
        platform: String,
        duration_ms: u64,
        success: bool,
    },
    /// A persona activation: how many platforms it targeted and how many
    /// switched successfully. No persona name, no account data.
    PersonaSwitch { platforms: u64, succeeded: u64 },
    /// A new account finished its add flow on a platform. Platform id only.
    AccountAdded { platform: String },
    /// The streamer-mode overlay auto-activated (streaming software detected).
    StreamerModeActivated,
    /// An accshift:// deep link triggered an action. No URL contents.
    DeepLinkUsed,
    /// End of session with total duration.
    SessionEnded { duration_ms: u64 },
    /// Snapshot of the number of accounts configured for a platform.
    /// Mode B only (requires a stable install_id).
    AccountsSnapshot { platform: String, count: u64 },
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::Ping => "ping",
            Event::AppLaunched { .. } => "app_launched",
            Event::PlatformSwitch { .. } => "platform_switch",
            Event::PersonaSwitch { .. } => "persona_switch",
            Event::AccountAdded { .. } => "account_added",
            Event::StreamerModeActivated => "streamer_mode_activated",
            Event::DeepLinkUsed => "deep_link_used",
            Event::SessionEnded { .. } => "session_ended",
            Event::AccountsSnapshot { .. } => "accounts_snapshot",
        }
    }
}

/// Stable session context (invariant for every request).
#[derive(Debug, Clone)]
pub struct TelemetryContext {
    pub app_version: String,
    pub os_version: String,
    pub locale: Option<String>,
}
