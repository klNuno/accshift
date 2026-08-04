//! Telemetry for the CLI.
//!
//! The CLI reports under the same consent as the app, reads the same config,
//! and sends to the same Worker. Without this, every `accshift switch` was
//! invisible: the switch counts, the platform mix and the failure rates all
//! described app users only, and a CLI-first user looked like someone who had
//! stopped using the tool.
//!
//! Two differences from the app's queue:
//! - No `ping`. A short-lived process would emit one per invocation and turn
//!   a single user typing five commands into five daily actives.
//! - A bounded flush at the end of the command instead of a periodic one. The
//!   process is about to exit, so there is no later.

use accshift_core::telemetry::{self, ConsentState, Event, QueueParams, Worker};
use std::time::Duration;

/// How long the process lingers to flush its single event.
///
/// The command's output has already been printed when this runs, so the cost
/// is a process that exits slightly later, never a delayed result. Still kept
/// tight: a POST to the edge lands in well under this on a working link, and
/// a machine with no network pays the full deadline on every single command.
/// A CLI that normally answers in 50 ms cannot afford half a second of that.
const FLUSH_DEADLINE: Duration = Duration::from_millis(400);

/// A CLI run's telemetry, or nothing at all when consent is absent.
pub struct CliTelemetry {
    worker: Worker,
}

impl CliTelemetry {
    /// Builds a queue when telemetry is enabled for this installation.
    ///
    /// Returns None when both modes are off, in which case nothing is spawned
    /// at all: a user who opted out does not pay for a thread.
    pub fn start(ctx: &accshift_core::AppCtx) -> Option<Self> {
        let cfg = accshift_core::config::load_config(&**ctx);
        let consent: ConsentState = telemetry::consent_from_config(&cfg.telemetry);
        if !consent.mode_a && !consent.mode_b {
            return None;
        }
        let tctx = telemetry::context_for(env!("CARGO_PKG_VERSION"), "cli");
        let params = QueueParams {
            emit_ping: false,
            ..Default::default()
        };
        Some(Self {
            worker: Worker::spawn(tctx, consent, params),
        })
    }

    /// Records the command outcome and flushes, bounded.
    pub fn finish(self, command: &str, error_code: Option<&str>) {
        self.worker.handle().track(Event::CliCommand {
            command: command.to_string(),
            success: error_code.is_none(),
            error_code: error_code.map(str::to_string),
        });
        self.worker.shutdown(FLUSH_DEADLINE);
    }
}

/// Maps a CLI exit code onto the error vocabulary shared with the app.
///
/// Returns None for a success. The CLI's own `unknown_account` wording maps
/// onto `account_not_found` so one query answers "how often does a switch
/// fail because the account is gone", whichever surface it came from.
pub fn error_code_for_exit(exit: u8) -> Option<&'static str> {
    use crate::exit;
    match exit {
        exit::OK => None,
        exit::PLATFORM_UNAVAILABLE => Some("platform_unavailable"),
        exit::UNKNOWN_ACCOUNT => Some("account_not_found"),
        exit::LOCK_CONTENDED => Some("lock_contended"),
        exit::IO => Some("io"),
        exit::PIN_DENIED => Some("pin_denied"),
        exit::CLI_DISABLED => Some("cli_disabled"),
        _ => Some("other"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit;

    #[test]
    fn success_has_no_error_code() {
        assert_eq!(error_code_for_exit(exit::OK), None);
    }

    #[test]
    fn every_exit_code_maps_to_the_shared_vocabulary() {
        for code in [
            exit::GENERIC,
            exit::PLATFORM_UNAVAILABLE,
            exit::UNKNOWN_ACCOUNT,
            exit::LOCK_CONTENDED,
            exit::IO,
            exit::PIN_DENIED,
            exit::CLI_DISABLED,
        ] {
            let mapped = error_code_for_exit(code).expect("a failure must carry a code");
            assert!(
                accshift_core::telemetry::ERROR_CODES.contains(&mapped),
                "{mapped} is emitted but not declared in ERROR_CODES"
            );
        }
    }

    #[test]
    fn an_unmapped_exit_code_degrades_instead_of_leaking() {
        assert_eq!(error_code_for_exit(u8::MAX), Some("other"));
    }
}
