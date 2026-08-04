use super::client::{self, Mode};
use super::events::{Event, TelemetryContext};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

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
    Event(Event, SystemTime),
    Shutdown,
}

/// Tuning parameters for the queue.
pub struct QueueParams {
    /// Delay before the first flush of the process.
    ///
    /// Short on purpose. Everything a launch produces (first_run,
    /// app_launched, the snapshots) is emitted in the first second, and a
    /// session shorter than the steady-state interval used to depend entirely
    /// on the bounded flush at window close. Those are exactly the sessions of
    /// someone who hit a bug and quit.
    pub first_flush_interval: Duration,
    /// Delay between flushes once the first one has happened.
    pub flush_interval: Duration,
    /// Upper bound on the retry backoff after repeated failures.
    pub max_backoff: Duration,
    pub max_batch_size: usize,
    pub endpoint: String,
    /// Whether this queue emits the daily `ping`.
    ///
    /// False for the CLI: a short-lived process would emit one ping per
    /// invocation and inflate the daily active count with what is really a
    /// single user typing a command five times.
    pub emit_ping: bool,
}

impl Default for QueueParams {
    fn default() -> Self {
        Self {
            first_flush_interval: Duration::from_secs(20),
            flush_interval: Duration::from_secs(300), // 5 min
            max_backoff: Duration::from_secs(3600),   // 1 h
            max_batch_size: 50,
            endpoint: client::TELEMETRY_URL.to_string(),
            emit_ping: true,
        }
    }
}

/// Interval between two `ping` events while the process keeps running.
const PING_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Upper bound on queued messages. A slow flush (network timeout) must not
/// let the channel grow without limit; overflow events are dropped.
const QUEUE_CAPACITY: usize = 512;

/// Upper bound on events held for retry.
///
/// The buffer now survives a failed flush, so it needs a ceiling of its own:
/// a machine that is offline for a day would otherwise accumulate every event
/// it ever produced. Oldest first, because a stale event is the least useful.
const MAX_BUFFERED_EVENTS: usize = 200;

/// Lightweight cloneable handle, usable from any thread or command.
/// Used to push events or change consent.
#[derive(Clone)]
pub struct Handle {
    tx: SyncSender<Message>,
    consent: Arc<Mutex<ConsentState>>,
    dropped: Arc<AtomicU64>,
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
        if self
            .tx
            .try_send(Message::Event(event, SystemTime::now()))
            .is_err()
        {
            // A full channel means the worker is stuck on a slow flush. The
            // event is lost, which is by design, but the loss is counted so
            // the next ping can report it instead of silently skewing totals.
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
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
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_clone = dropped.clone();

        let join = thread::Builder::new()
            .name("accshift-telemetry".into())
            .spawn(move || run(rx, ctx, consent_clone, params, dropped_clone))
            .expect("telemetry thread spawn failed");

        Self {
            handle: Handle {
                tx,
                consent,
                dropped,
            },
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

/// Everything the worker loop carries between iterations.
struct WorkerState {
    buffer: Vec<(Event, SystemTime)>,
    /// Consecutive failed flushes, used to space out retries.
    consecutive_failures: u32,
    /// None until the first ping of the process has been queued.
    last_ping: Option<Instant>,
}

fn run(
    rx: Receiver<Message>,
    ctx: TelemetryContext,
    consent: Arc<Mutex<ConsentState>>,
    params: QueueParams,
    dropped: Arc<AtomicU64>,
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
    let mut state = WorkerState {
        buffer: Vec::new(),
        consecutive_failures: 0,
        last_ping: None,
    };
    let mut last_flush = Instant::now();
    let mut next_interval = params.first_flush_interval;

    loop {
        let remaining = next_interval
            .checked_sub(last_flush.elapsed())
            .unwrap_or(Duration::ZERO);

        match rx.recv_timeout(remaining) {
            Ok(Message::Event(ev, at)) => {
                push_bounded(&mut state.buffer, (ev, at), &dropped);
                if state.buffer.len() >= params.max_batch_size {
                    flush(&http, &params, &ua, &ctx, &consent, &mut state, &dropped);
                    last_flush = Instant::now();
                    next_interval = retry_interval(&params, state.consecutive_failures);
                }
            }
            Ok(Message::Shutdown) => {
                flush(&http, &params, &ua, &ctx, &consent, &mut state, &dropped);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                flush(&http, &params, &ua, &ctx, &consent, &mut state, &dropped);
                last_flush = Instant::now();
                next_interval = retry_interval(&params, state.consecutive_failures);
            }
            Err(RecvTimeoutError::Disconnected) => {
                // All Senders have dropped; exit the loop.
                flush(&http, &params, &ua, &ctx, &consent, &mut state, &dropped);
                return;
            }
        }
    }
}

/// Appends an event, dropping the oldest one when the buffer is full.
fn push_bounded(
    buffer: &mut Vec<(Event, SystemTime)>,
    entry: (Event, SystemTime),
    dropped: &Arc<AtomicU64>,
) {
    if buffer.len() >= MAX_BUFFERED_EVENTS {
        buffer.remove(0);
        dropped.fetch_add(1, Ordering::Relaxed);
    }
    buffer.push(entry);
}

/// Delay before the next flush attempt.
///
/// Doubles per consecutive failure so a machine that has been offline for an
/// hour stops retrying every five minutes, and is capped so it always comes
/// back on its own once the network returns.
fn retry_interval(params: &QueueParams, consecutive_failures: u32) -> Duration {
    if consecutive_failures == 0 {
        return params.flush_interval;
    }
    let factor = 1u32 << consecutive_failures.min(8);
    params
        .flush_interval
        .saturating_mul(factor)
        .min(params.max_backoff)
}

/// True when a `ping` is due: once at the first flush that has consent, then
/// once a day for as long as the process lives.
fn ping_is_due(last_ping: Option<Instant>) -> bool {
    match last_ping {
        None => true,
        Some(at) => at.elapsed() >= PING_INTERVAL,
    }
}

fn flush(
    http: &reqwest::blocking::Client,
    params: &QueueParams,
    ua: &str,
    ctx: &TelemetryContext,
    consent: &Arc<Mutex<ConsentState>>,
    state: &mut WorkerState,
    dropped: &Arc<AtomicU64>,
) {
    let snapshot = {
        let guard = consent.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    };
    let Some((mode, install_id)) = resolve_mode(&snapshot) else {
        // Consent was revoked (or never given) while events were queued; drop
        // them. Checked before the ping below so an installation that has not
        // completed onboarding never emits one.
        state.buffer.clear();
        return;
    };

    // The ping is minted here rather than at startup: consent can arrive after
    // the process does (onboarding), and a ping pushed before that would be
    // dropped by `track` and leave the day uncounted.
    if params.emit_ping && ping_is_due(state.last_ping) {
        let dropped_events = dropped.swap(0, Ordering::Relaxed);
        push_bounded(
            &mut state.buffer,
            (Event::Ping { dropped_events }, SystemTime::now()),
            dropped,
        );
        state.last_ping = Some(Instant::now());
    }

    if state.buffer.is_empty() {
        return;
    }

    // Snapshot events are documented (events.rs) as Mode B only: they must
    // never leave the process under Mode A, even though Mode A is otherwise
    // enabled. Drop just those events here rather than gating at track()
    // time, since the applicable mode is only known for certain at flush.
    if mode == Mode::A {
        state.buffer.retain(|(ev, _)| !ev.is_mode_b_only());
        if state.buffer.is_empty() {
            return;
        }
    }

    let events_json: Vec<Value> = state
        .buffer
        .iter()
        .map(|(ev, at)| client::event_to_json(ev, ctx, *at))
        .collect();

    match client::send_batch(
        http,
        &params.endpoint,
        ua,
        mode,
        install_id.as_deref(),
        events_json,
    ) {
        Ok(()) => {
            state.buffer.clear();
            state.consecutive_failures = 0;
        }
        Err(_e) => {
            // Kept for the next attempt. Telemetry is still RAM-only: nothing
            // is written to disk, and an app that closes before the network
            // comes back forgets these events entirely. What changed is that a
            // transient failure no longer costs a whole batch, which used to
            // silently under-count every user with an unreliable connection.
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            trim_to_capacity(&mut state.buffer, dropped);
        }
    }
}

/// Enforces the retry ceiling after a failure, oldest events first.
fn trim_to_capacity(buffer: &mut Vec<(Event, SystemTime)>, dropped: &Arc<AtomicU64>) {
    if buffer.len() <= MAX_BUFFERED_EVENTS {
        return;
    }
    let excess = buffer.len() - MAX_BUFFERED_EVENTS;
    buffer.drain(..excess);
    dropped.fetch_add(excess as u64, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> TelemetryContext {
        TelemetryContext {
            app_version: "0.0.0".into(),
            os: "test".into(),
            arch: "x86_64".into(),
            os_version: "test".into(),
            locale: None,
            surface: "gui".into(),
        }
    }

    /// Endpoint that refuses instantly, so a flush fails without a timeout.
    fn unreachable_params() -> QueueParams {
        QueueParams {
            endpoint: "http://127.0.0.1:1/track".into(),
            ..Default::default()
        }
    }

    fn new_state(buffer: Vec<(Event, SystemTime)>) -> WorkerState {
        WorkerState {
            buffer,
            consecutive_failures: 0,
            last_ping: Some(Instant::now()),
        }
    }

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
    fn flush_drops_mode_b_only_events_under_mode_a() {
        // Snapshot events are documented as Mode B only (events.rs). Under
        // Mode A they must be dropped rather than sent, even though Mode A
        // itself is enabled and would otherwise flush fine.
        let http = reqwest::blocking::Client::new();
        let ctx = test_ctx();
        let consent = Arc::new(Mutex::new(ConsentState {
            mode_a: true,
            mode_b: false,
            install_id: None,
            anonymous_id: None,
        }));
        let dropped = Arc::new(AtomicU64::new(0));
        let mut state = new_state(vec![
            (
                Event::AccountsSnapshot {
                    platform: "steam".into(),
                    count: 3,
                },
                SystemTime::now(),
            ),
            (
                Event::SettingsSnapshot {
                    ui_language: "fr".into(),
                    enabled_platforms: vec!["steam".into()],
                    personas_enabled: true,
                    pin_enabled: false,
                    cli_enabled: true,
                    deep_links_enabled: true,
                    streamer_mode: "auto".into(),
                    animations: "system".into(),
                },
                SystemTime::now(),
            ),
        ]);

        flush(
            &http,
            &unreachable_params(),
            "test-ua",
            &ctx,
            &consent,
            &mut state,
            &dropped,
        );

        // Dropped locally before any network send was attempted.
        assert!(state.buffer.is_empty());
        assert_eq!(state.consecutive_failures, 0);
    }

    #[test]
    fn flush_without_consent_drops_and_never_pings() {
        let http = reqwest::blocking::Client::new();
        let consent = Arc::new(Mutex::new(ConsentState::default()));
        let dropped = Arc::new(AtomicU64::new(0));
        let mut state = WorkerState {
            buffer: vec![(Event::DeepLinkUsed, SystemTime::now())],
            consecutive_failures: 0,
            last_ping: None,
        };

        flush(
            &http,
            &unreachable_params(),
            "test-ua",
            &test_ctx(),
            &consent,
            &mut state,
            &dropped,
        );

        assert!(state.buffer.is_empty());
        // An installation that never completed onboarding must not be counted
        // as a daily active user.
        assert!(state.last_ping.is_none());
    }

    #[test]
    fn a_failed_flush_keeps_the_batch_for_the_next_attempt() {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(200))
            .connect_timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        let consent = Arc::new(Mutex::new(ConsentState {
            mode_a: true,
            mode_b: false,
            install_id: None,
            anonymous_id: Some("797f20fe-94de-4e89-98a2-ae3a3273ad1e".into()),
        }));
        let dropped = Arc::new(AtomicU64::new(0));
        let mut state = new_state(vec![(
            Event::AppLaunched { duration_ms: 10 },
            SystemTime::now(),
        )]);

        flush(
            &http,
            &unreachable_params(),
            "test-ua",
            &test_ctx(),
            &consent,
            &mut state,
            &dropped,
        );

        assert_eq!(state.buffer.len(), 1, "batch must survive a failed send");
        assert_eq!(state.consecutive_failures, 1);
    }

    #[test]
    fn the_ping_is_minted_once_then_once_a_day() {
        assert!(ping_is_due(None));
        assert!(!ping_is_due(Some(Instant::now())));
    }

    #[test]
    fn backoff_grows_then_stops_at_the_ceiling() {
        let params = QueueParams::default();
        assert_eq!(retry_interval(&params, 0), params.flush_interval);
        assert_eq!(retry_interval(&params, 1), params.flush_interval * 2);
        assert_eq!(retry_interval(&params, 2), params.flush_interval * 4);
        // Capped, and never zero: an offline machine still comes back on its
        // own once the network returns.
        assert_eq!(retry_interval(&params, 30), params.max_backoff);
        assert!(retry_interval(&params, 30) > Duration::ZERO);
    }

    #[test]
    fn the_buffer_has_a_ceiling_and_counts_what_it_drops() {
        let dropped = Arc::new(AtomicU64::new(0));
        let mut buffer = Vec::new();
        for i in 0..(MAX_BUFFERED_EVENTS + 5) {
            push_bounded(
                &mut buffer,
                (
                    Event::AppLaunched {
                        duration_ms: i as u64,
                    },
                    SystemTime::now(),
                ),
                &dropped,
            );
        }

        assert_eq!(buffer.len(), MAX_BUFFERED_EVENTS);
        assert_eq!(dropped.load(Ordering::Relaxed), 5);
        // Oldest first: the five discarded events are the five oldest.
        let first = &buffer[0].0;
        assert!(matches!(first, Event::AppLaunched { duration_ms: 5 }));
    }
}
