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

pub(super) fn ensure_no_riot_game_running(action: &str) -> Result<(), String> {
    let running_games = running_process_names(RIOT_GAME_PROCESS_NAMES);
    if running_games.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Close Riot game processes before {action}: {}",
        running_games.join(", ")
    ))
}

pub(super) fn kill_riot_client_processes() {
    for _ in 0..KILL_RETRY_COUNT {
        // Resolve what is actually running with one refresh, then kill only
        // those. Calling kill_process per name re-scanned the whole process
        // table six times per round even when nothing was running.
        let running = os::running_process_names(RIOT_CLIENT_PROCESS_NAMES);
        for name in running {
            let _ = os::kill_process(name);
        }
        if !is_any_process_running(RIOT_CLIENT_PROCESS_NAMES) {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(KILL_RETRY_DELAY_MS));
    }
}

/// Process-wide reqwest client for the Riot local API.
///
/// Built once and reused: a fresh client per call leaks a connection pool each
/// time, and (worse) a client with no timeout lets a hung Riot Client socket
/// block its `spawn_blocking` thread forever. The 1s setup-status poll then
/// drains the blocking worker pool and the whole backend freezes. The connect
/// and request timeouts bound every call so a stuck socket fails fast.
pub(super) static RIOT_LOCAL_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(super) fn riot_local_client() -> &'static reqwest::Client {
    RIOT_LOCAL_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Riot local API client should build")
    })
}

/// Request a graceful quit via the local API, which flushes in-memory tokens
/// to disk before exiting. Falls back to force-kill if the API is unreachable
/// or the process doesn't exit within the timeout.
pub(super) fn graceful_riot_quit() {
    let access = match read_riot_local_api_access() {
        Ok(a) => a,
        Err(_) => {
            kill_riot_client_processes();
            return;
        }
    };

    // POST /process-control/v1/process/quit triggers a graceful shutdown
    let quit_ok = crate::runtime::block_on(async {
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

    if !quit_ok {
        kill_riot_client_processes();
        return;
    }

    // Wait for the process to exit (up to 8 seconds)
    for _ in 0..16 {
        if !is_any_process_running(RIOT_CLIENT_PROCESS_NAMES) {
            thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
            return;
        }
        thread::sleep(std::time::Duration::from_millis(500));
    }

    // Timed out, so force kill
    kill_riot_client_processes();
}

pub(super) fn launch_riot_client(client_path: &Path) -> Result<(), String> {
    os::hidden_command(client_path)
        .args(["--launch-product=riot-client", "--launch-patchline=live"])
        .spawn()
        .map_err(|e| format!("Could not launch Riot Client: {e}"))?;
    Ok(())
}
