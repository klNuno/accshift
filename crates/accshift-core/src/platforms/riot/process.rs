//! Riot client and game processes: detection, graceful quit, kill, launch.

#[allow(unused_imports)]
use super::*;

pub(super) fn env_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing environment variable: {name}"))
}

pub(super) fn is_any_process_running(process_names: &[&str]) -> bool {
    os::any_process_running(process_names)
}

pub(super) fn running_process_names(process_names: &'static [&'static str]) -> Vec<&'static str> {
    // One process-table refresh for the whole batch instead of one per name.
    // The order and uniqueness of the input list are preserved.
    os::running_process_names(process_names)
}

pub(super) fn build_riot_switch_details(
    app_handle: &dyn AppContext,
    target_profile_id: Option<&str>,
) -> String {
    let cfg = config::load_config(app_handle);
    use crate::platforms::{redact_id, redact_opt};
    serde_json::json!({
        "targetProfileId": redact_opt(target_profile_id),
        "currentProfileId": redact_id(&cfg.riot.current_profile_id),
        "runningClientProcesses": running_process_names(RIOT_CLIENT_PROCESS_NAMES),
        "runningGameProcesses": running_process_names(RIOT_GAME_PROCESS_NAMES),
    })
    .to_string()
}

/// Start of the error returned when League of Legends is in champion select.
/// The app matches it to show a dedicated toast.
pub(super) const RIOT_LEAGUE_IN_CHAMP_SELECT: &str = "League of Legends is in champion select";

/// Start of the error returned when League of Legends is in an active match
/// phase (`GameStart`, `InProgress`, `Reconnect`). The app matches it.
pub(super) const RIOT_LEAGUE_GAME_ACTIVE: &str = "League of Legends is in an active match";

pub(super) fn league_phase_guard_error(phase: Option<&str>, action: &str) -> Result<(), String> {
    let Some(phase) = phase else {
        return Ok(());
    };
    if phase.eq_ignore_ascii_case("ChampSelect") {
        Err(format!(
            "{RIOT_LEAGUE_IN_CHAMP_SELECT}: finish or leave champion select before {action}."
        ))
    } else {
        Err(format!(
            "{RIOT_LEAGUE_GAME_ACTIVE} ({phase}): finish or leave the match before {action}."
        ))
    }
}

/// Refuse to close `LeagueClient` while the player is in champion select or an
/// active match. If `LeagueClient` is not running or its LCU API is unreadable,
/// keep the current behaviour and return `Ok(())`.
pub(super) fn ensure_league_not_in_protected_phase(action: &str) -> Result<(), String> {
    if !is_any_process_running(LEAGUE_CLIENT_PROCESS_NAMES) {
        return Ok(());
    }
    league_phase_guard_error(detect_protected_league_phase(), action)
}

pub(super) fn ensure_no_riot_game_running(action: &str) -> Result<(), String> {
    riot_game_guard_error(
        &running_process_names(RIOT_GAME_PROCESS_NAMES),
        || {
            is_any_process_running(LEAGUE_CLIENT_PROCESS_NAMES)
                .then(detect_protected_league_phase)
                .flatten()
        },
        action,
    )
}

/// The refusal for a running game, `Ok(())` when nothing is running. The
/// League phase is read first: a League match runs `League of Legends.exe`
/// too, and its refusal has a dedicated message the app translates.
pub(super) fn riot_game_guard_error(
    running_games: &[&str],
    league_phase: impl FnOnce() -> Option<&'static str>,
    action: &str,
) -> Result<(), String> {
    league_phase_guard_error(league_phase(), action)?;
    if !running_games.is_empty() {
        return Err(format!(
            "Close Riot game processes before {action}: {}",
            running_games.join(", ")
        ));
    }
    Ok(())
}

/// Fails when a client process outlived the quit. The session files are only
/// safe to capture or replace once the client is gone: a live client keeps its
/// tokens in memory and writes them back over a restored snapshot on exit, so
/// the switch would report success while the old account stays signed in.
pub(super) fn ensure_riot_client_closed() -> Result<(), String> {
    client_closed_or_error(&running_process_names(RIOT_CLIENT_PROCESS_NAMES))
}

pub(super) fn client_closed_or_error(survivors: &[&str]) -> Result<(), String> {
    if survivors.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Riot Client is still running and could not be closed: {}",
        survivors.join(", ")
    ))
}

pub(super) fn close_process_names() -> &'static [&'static str] {
    RIOT_CLOSE_PROCESS_NAMES
}

/// The kill rounds of a forced close, injected so tests run them without a
/// Riot client.
pub(super) struct KillIo<'a> {
    /// The League guard: `Err` while closing `LeagueClient` must be refused.
    pub guard: &'a dyn Fn() -> Result<(), String>,
    /// Kills the client processes running now.
    pub kill_running: &'a dyn Fn(),
    pub client_running: &'a dyn Fn() -> bool,
    /// The pause after a round that left the client running.
    pub wait: &'a dyn Fn(),
    pub attempts: usize,
}

/// Kills the client processes, the League guard first. A refusal before the
/// first round is [`CloseError::Refused`]; later, part of the client may be
/// gone already, so it is [`CloseError::AfterClose`].
pub(super) fn kill_riot_client_processes(action: &str) -> Result<(), CloseError> {
    let guard = || ensure_league_not_in_protected_phase(action);
    let kill_running = || {
        // Resolve what is actually running with one refresh, then kill only
        // those. Calling kill_process per name re-scanned the whole process
        // table per round even when nothing was running.
        let running = os::running_process_names(close_process_names());
        for name in running {
            let _ = os::kill_process(name);
        }
    };
    let client_running = || is_any_process_running(RIOT_CLIENT_PROCESS_NAMES);
    let wait = || thread::sleep(std::time::Duration::from_millis(KILL_RETRY_DELAY_MS));
    run_guarded_kill(&KillIo {
        guard: &guard,
        kill_running: &kill_running,
        client_running: &client_running,
        wait: &wait,
        attempts: KILL_RETRY_COUNT,
    })
}

pub(super) fn run_guarded_kill(io: &KillIo) -> Result<(), CloseError> {
    for attempt in 0..io.attempts {
        // Read before every round: a round that left the client running is
        // followed by a pause, long enough to enter champion select.
        if let Err(refusal) = (io.guard)() {
            return Err(if attempt == 0 {
                CloseError::Refused(refusal)
            } else {
                CloseError::AfterClose(refusal)
            });
        }
        (io.kill_running)();
        if !(io.client_running)() {
            break;
        }
        (io.wait)();
    }
    Ok(())
}

/// Best-effort cleanup of lingering crash handlers after a graceful quit.
/// Never retried or counted against `ensure_riot_client_closed`.
pub(super) fn kill_riot_crash_handlers() {
    let running = os::running_process_names(RIOT_CRASH_HANDLER_PROCESS_NAMES);
    for name in running {
        let _ = os::kill_process(name);
    }
}

/// Process-wide reqwest client for the Riot local API.
///
/// Built once and reused: a fresh client per call leaks a connection pool each
/// time, and (worse) a client with no timeout lets a hung Riot Client socket
/// block the calling job thread forever, one more on every 1 s setup-status
/// poll. The connect and request timeouts bound every call so a stuck socket
/// fails fast.
pub(super) static RIOT_LOCAL_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(super) fn configure_riot_local_client(
    builder: reqwest::ClientBuilder,
) -> reqwest::ClientBuilder {
    builder
        .no_proxy()
        .danger_accept_invalid_certs(true)
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
}

pub(super) fn riot_local_client() -> &'static reqwest::Client {
    RIOT_LOCAL_CLIENT.get_or_init(|| {
        configure_riot_local_client(crate::http::client_builder())
            .build()
            .expect("Riot local API client should build")
    })
}

/// Why closing the Riot client failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CloseError {
    /// The League guard refused before any quit was asked or any process
    /// killed: the client runs as before, so nothing needs relaunching.
    Refused(String),
    /// A quit was asked or a kill tried, or a step after the close failed.
    /// The caller relaunches the client so it is never left closed.
    AfterClose(String),
}

impl CloseError {
    pub(super) fn is_refusal(&self) -> bool {
        matches!(self, Self::Refused(_))
    }
}

impl From<CloseError> for String {
    fn from(error: CloseError) -> Self {
        match error {
            CloseError::Refused(message) | CloseError::AfterClose(message) => message,
        }
    }
}

/// What the local API made of the quit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuitRequest {
    /// No lockfile: the request was never sent.
    NoApi,
    /// Sent, but no answer: the client may have received it.
    Failed,
    /// The client accepted the quit.
    Sent,
}

/// The process side of a graceful quit, injected so the order of the guard,
/// the wait and the forced close runs in tests without a Riot client.
pub(super) struct QuitIo<'a> {
    /// The League guard: `Err` while closing `LeagueClient` must be refused.
    pub guard: &'a dyn Fn() -> Result<(), String>,
    pub request_quit: &'a dyn Fn() -> QuitRequest,
    pub client_running: &'a dyn Fn() -> bool,
    /// One pause between two checks of `client_running`.
    pub wait: &'a dyn Fn(),
    /// Kills the client processes, the League guard before each round, then
    /// fails when one survived.
    pub force_close: &'a dyn Fn() -> Result<(), CloseError>,
    /// Cleanup once the client exited on its own.
    pub after_exit: &'a dyn Fn(),
    /// Checks of `client_running` before the forced close.
    pub polls: usize,
}

/// Request a graceful quit via the local API, which flushes in-memory tokens
/// to disk before exiting. Falls back to force-kill if the API is unreachable
/// or the process doesn't exit within the timeout. An error means a client
/// process survived both (an elevated or hung client), or the League guard
/// refused.
pub(super) fn graceful_riot_quit() -> Result<(), String> {
    graceful_riot_quit_checked().map_err(String::from)
}

/// [`graceful_riot_quit`], telling a refusal before anything was closed apart.
pub(super) fn graceful_riot_quit_checked() -> Result<(), CloseError> {
    let guard = || ensure_league_not_in_protected_phase("closing Riot Client");
    let request_quit = request_riot_quit;
    let client_running = || is_any_process_running(RIOT_CLIENT_PROCESS_NAMES);
    let wait = || thread::sleep(std::time::Duration::from_millis(500));
    let force_close = || {
        kill_riot_client_processes("closing Riot Client")?;
        ensure_riot_client_closed().map_err(CloseError::AfterClose)
    };
    let after_exit = || {
        kill_riot_crash_handlers();
        thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
    };
    run_graceful_quit(&QuitIo {
        guard: &guard,
        request_quit: &request_quit,
        client_running: &client_running,
        wait: &wait,
        force_close: &force_close,
        after_exit: &after_exit,
        // Up to 8 seconds.
        polls: 16,
    })
}

/// POST /process-control/v1/process/quit triggers a graceful shutdown.
fn request_riot_quit() -> QuitRequest {
    let Ok(access) = read_riot_local_api_access() else {
        return QuitRequest::NoApi;
    };
    let sent = crate::runtime::block_on(async {
        let url = format!(
            "{}://127.0.0.1:{}/process-control/v1/process/quit",
            access.protocol, access.port
        );
        riot_local_client()
            .post(url)
            .basic_auth("riot", Some(access.password.as_str()))
            .send()
            .await
            .is_ok()
    });
    if sent {
        QuitRequest::Sent
    } else {
        QuitRequest::Failed
    }
}

pub(super) fn run_graceful_quit(io: &QuitIo) -> Result<(), CloseError> {
    (io.guard)().map_err(CloseError::Refused)?;
    match (io.request_quit)() {
        QuitRequest::NoApi => return forced_close(io, false),
        QuitRequest::Failed => return forced_close(io, true),
        QuitRequest::Sent => {}
    }
    for _ in 0..io.polls {
        if !(io.client_running)() {
            (io.after_exit)();
            return Ok(());
        }
        (io.wait)();
    }
    // Timed out, so force kill
    forced_close(io, true)
}

/// The forced-kill fallback. `force_close` reads the League phase again
/// before each kill round: the quit request, the wait and the rounds take
/// seconds, long enough to enter champion select, and a kill takes
/// `LeagueClient` down with the client. `quit_asked` says whether a quit
/// request may already have closed part of the client, so a refusal then
/// still has the caller relaunch it.
fn forced_close(io: &QuitIo, quit_asked: bool) -> Result<(), CloseError> {
    (io.force_close)().map_err(|error| match error {
        CloseError::Refused(refusal) if quit_asked => CloseError::AfterClose(refusal),
        error => error,
    })
}

pub(super) fn launch_riot_client(client_path: &Path) -> Result<(), String> {
    launch_riot_product(client_path, RIOT_CLIENT_PRODUCT)
}

/// Starts `product_id` on the live patchline through `RiotClientServices.exe`,
/// which hands it to a running client or opens one.
pub(super) fn launch_riot_product(client_path: &Path, product_id: &str) -> Result<(), String> {
    os::detach_stdio(os::hidden_command(client_path).args(launch_args(product_id)))
        .spawn()
        .map_err(|e| format!("Could not launch Riot Client: {e}"))?;
    Ok(())
}
