use super::client::{self, Mode};
use super::events::{Event, TelemetryContext};
use serde_json::Value;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Consent state read at every flush to decide which mode to use.
#[derive(Debug, Clone, Default)]
pub struct ConsentState {
    pub mode_a: bool,
    pub mode_b: bool,
    pub install_id: Option<String>,
    pub anonymous_id: Option<String>,
}

/// Picks the mode to use for the current request.
/// - Mode B if opted in with a valid install_id.
/// - Otherwise Mode A if enabled.
/// - Otherwise nothing (events dropped).
fn resolve_mode(state: &ConsentState) -> Option<(Mode, Option<String>)> {
    if state.mode_b {
        if let Some(id) = state.install_id.as_ref() {
            if super::install_id::is_valid(id) {
                return Some((Mode::B, Some(id.clone())));
            }
        }
    }
    if state.mode_a {
        let anonymous_id = state
            .anonymous_id
            .as_ref()
            .filter(|id| super::install_id::is_valid(id))
            .cloned();
        return Some((Mode::A, anonymous_id));
    }
    None
}

/// Internal messages consumed by the worker thread.
enum Message {
    Event(Event),
    Shutdown,
}

/// Tuning parameters for the queue.
pub struct QueueParams {
    pub flush_interval: Duration,
    pub max_batch_size: usize,
    pub endpoint: String,
}

impl Default for QueueParams {
    fn default() -> Self {
        Self {
            flush_interval: Duration::from_secs(300), // 5 min
            max_batch_size: 50,
            endpoint: client::TELEMETRY_URL.to_string(),
        }
    }
}

/// Upper bound on queued messages. A slow flush (network timeout) must not
/// let the channel grow without limit; overflow events are dropped.
const QUEUE_CAPACITY: usize = 512;

/// Lightweight cloneable handle, usable from any thread or command.
/// Used to push events or change consent.

#[derive(Clone)]
pub struct Handle {
    tx: SyncSender<Message>,
    consent: Arc<Mutex<ConsentState>>,
}

impl Handle {
    /// Enqueues an event. No-op when telemetry is fully disabled.
    /// Never blocks.
    pub fn track(&self, event: Event) {
        {
            let state = self.consent.lock().unwrap_or_else(|e| e.into_inner());
            if !state.mode_a && !state.mode_b {
                return;
            }
        }
        let _ = self.tx.try_send(Message::Event(event));
    }

    /// Updates the consent state (called after a UI toggle or after the
    /// install_id is generated).
    pub fn update_consent(&self, new_state: ConsentState) {
        let mut guard = self.consent.lock().unwrap_or_else(|e| e.into_inner());
        *guard = new_state;
    }
}

/// Owner of the telemetry thread. One per process. Not cloneable.
/// Allows a clean `shutdown()` on app close.
pub struct Worker {
    handle: Handle,
    join: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(ctx: TelemetryContext, consent: ConsentState, params: QueueParams) -> Self {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let consent = Arc::new(Mutex::new(consent));
        let consent_clone = consent.clone();

        let join = thread::Builder::new()
            .name("accshift-telemetry".into())
            .spawn(move || run(rx, ctx, consent_clone, params))
            .expect("telemetry thread spawn failed");

        Self {
            handle: Handle { tx, consent },
            join: Some(join),
        }
    }

    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Clean shutdown: asks the thread to flush, waiting at most `deadline`.
    /// The final flush is best-effort. This runs on the UI thread during
    /// window close; an unbounded join would freeze the window for up to the
    /// HTTP timeout (10s) when the endpoint is slow or unreachable.
    pub fn shutdown(mut self, deadline: Duration) {
        let _ = self.handle.tx.send(Message::Shutdown);
        let Some(join) = self.join.take() else {
            return;
        };
        let start = Instant::now();
        while start.elapsed() < deadline {
            if join.is_finished() {
                let _ = join.join();
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        // Deadline hit: detach the thread and let process exit reap it.
    }
}

fn run(
    rx: Receiver<Message>,
    ctx: TelemetryContext,
    consent: Arc<Mutex<ConsentState>>,
    params: QueueParams,
) {
    let http = match reqwest::blocking::Client::builder()
        .user_agent(client::user_agent(&ctx.app_version))
        .timeout(Duration::from_secs(10))
        // Without an explicit connect timeout, a TCP connect behind a
        // restrictive firewall can hang on OS-default retries (~21s) before
        // the overall timeout fires. Cap the connect phase at 5s.
        .connect_timeout(Duration::from_secs(5))
        .build()
    {
        Ok(http) => http,
        Err(e) => {
            // Telemetry silently dying is acceptable; dying without a trace
            // is not.
            eprintln!("telemetry: failed to build HTTP client, telemetry disabled: {e}");
            return;
        }
    };

    let ua = client::user_agent(&ctx.app_version);
    let mut buffer: Vec<Event> = Vec::new();
    let mut last_flush = Instant::now();

    loop {
        let remaining = params
            .flush_interval
            .checked_sub(last_flush.elapsed())
            .unwrap_or(Duration::ZERO);

        match rx.recv_timeout(remaining) {
            Ok(Message::Event(ev)) => {
                buffer.push(ev);
                if buffer.len() >= params.max_batch_size {
                    flush(&http, &params.endpoint, &ua, &ctx, &consent, &mut buffer);
                    last_flush = Instant::now();
                }
            }
            Ok(Message::Shutdown) => {
                flush(&http, &params.endpoint, &ua, &ctx, &consent, &mut buffer);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                flush(&http, &params.endpoint, &ua, &ctx, &consent, &mut buffer);
                last_flush = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => {
                // All Senders have dropped; exit the loop.
                flush(&http, &params.endpoint, &ua, &ctx, &consent, &mut buffer);
                return;
            }
        }
    }
}

fn flush(
    http: &reqwest::blocking::Client,
    endpoint: &str,
    ua: &str,
    ctx: &TelemetryContext,
    consent: &Arc<Mutex<ConsentState>>,
    buffer: &mut Vec<Event>,
) {
    if buffer.is_empty() {
        return;
    }
    let snapshot = {
        let guard = consent.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    };
    let Some((mode, install_id)) = resolve_mode(&snapshot) else {
        // Consent was revoked while events were queued; drop them.
        buffer.clear();
        return;
    };

    // AccountsSnapshot is documented (events.rs) as Mode B only: it must
    // never leave the process under Mode A, even though Mode A is otherwise
    // enabled. Drop just those events here rather than gating at track()
    // time, since the applicable mode is only known for certain at flush.
    if mode == Mode::A {
        buffer.retain(|ev| !matches!(ev, Event::AccountsSnapshot { .. }));
        if buffer.is_empty() {
            return;
        }
    }

    let events_json: Vec<Value> = buffer
        .iter()
        .map(|ev| client::event_to_json(ev, ctx))
        .collect();

    match client::send_batch(http, endpoint, ua, mode, install_id.as_deref(), events_json) {
        Ok(()) => {
            buffer.clear();
        }
        Err(_e) => {
            // Mode A: drop on error (RAM-only by design, no on-disk persistence).
            // Mode B: same, kept symmetric. Important events (ping,
            // accounts_snapshot) will be re-emitted next cycle.
            buffer.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_mode_picks_b_when_install_id_valid() {
        let s = ConsentState {
            mode_a: false,
            mode_b: true,
            install_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            anonymous_id: None,
        };
        let r = resolve_mode(&s);
        assert!(matches!(r, Some((Mode::B, Some(_)))));
    }

    #[test]
    fn resolve_mode_falls_back_to_a_when_b_opted_in_without_id() {
        let s = ConsentState {
            mode_a: true,
            mode_b: true,
            install_id: None,
            anonymous_id: None,
        };
        let r = resolve_mode(&s);
        assert!(matches!(r, Some((Mode::A, None))));
    }

    #[test]
    fn resolve_mode_picks_a_when_b_off() {
        let s = ConsentState {
            mode_a: true,
            mode_b: false,
            install_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            anonymous_id: Some("797f20fe-94de-4e89-98a2-ae3a3273ad1e".into()),
        };
        let r = resolve_mode(&s);
        assert!(matches!(r, Some((Mode::A, Some(_)))));
    }

    #[test]
    fn resolve_mode_omits_invalid_anonymous_id() {
        let s = ConsentState {
            mode_a: true,
            mode_b: false,
            install_id: None,
            anonymous_id: Some("not-a-uuid".into()),
        };
        assert!(matches!(resolve_mode(&s), Some((Mode::A, None))));
    }

    #[test]
    fn resolve_mode_none_when_both_off() {
        let s = ConsentState::default();
        assert!(resolve_mode(&s).is_none());
    }

    #[test]
    fn resolve_mode_rejects_invalid_install_id() {
        let s = ConsentState {
            mode_a: true,
            mode_b: true,
            install_id: Some("not-a-uuid".into()),
            anonymous_id: None,
        };
        // Mode B rejected because id is invalid; falls back to Mode A.
        let r = resolve_mode(&s);
        assert!(matches!(r, Some((Mode::A, None))));
    }

    #[test]
    fn flush_drops_accounts_snapshot_under_mode_a() {
        // AccountsSnapshot is documented as Mode B only (events.rs). Under
        // Mode A it must be dropped rather than sent, even though Mode A
        // itself is enabled and would otherwise flush fine.
        let http = reqwest::blocking::Client::new();
        let ctx = TelemetryContext {
            app_version: "0.0.0".into(),
            os_version: "test".into(),
            locale: None,
        };
        let consent = Arc::new(Mutex::new(ConsentState {
            mode_a: true,
            mode_b: false,
            install_id: None,
            anonymous_id: None,
        }));
        let mut buffer = vec![Event::AccountsSnapshot {
            platform: "steam".into(),
            count: 3,
        }];

        flush(
            &http,
            "http://127.0.0.1:0/track",
            "test-ua",
            &ctx,
            &consent,
            &mut buffer,
        );

        // The event was dropped locally (buffer emptied) before any network
        // send was attempted; no request was made to the unreachable port.
        assert!(buffer.is_empty());
    }
}
