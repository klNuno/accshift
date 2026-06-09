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
