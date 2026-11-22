//! The Riot client's local API: lockfile access, login state and live identity.

#[allow(unused_imports)]
use super::*;

pub(super) fn riot_lockfile_path() -> Result<PathBuf, String> {
    Ok(env_path("LOCALAPPDATA")?
        .join("Riot Games")
        .join("Riot Client")
        .join("Config")
        .join("lockfile"))
}

pub(super) fn parse_lockfile_content(content: &str) -> Result<RiotLocalApiAccess, String> {
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() != 5 {
        return Err("Riot lockfile format is invalid".into());
    }

    let port = parts[2]
        .parse::<u16>()
        .map_err(|e| format!("Invalid Riot lockfile port: {e}"))?;
    if port < 1024 {
        return Err("Riot lockfile port is outside the expected range".into());
    }
    let protocol = parts[4].trim();
    if protocol != "http" && protocol != "https" {
        return Err("Riot lockfile protocol is invalid".into());
    }
    let password = parts[3].trim();
    if password.is_empty() {
        return Err("Riot lockfile password is empty".into());
    }

    Ok(RiotLocalApiAccess {
        protocol: protocol.to_string(),
        port,
        password: password.to_string(),
    })
}

pub(super) fn read_lockfile_at(lockfile_path: &Path) -> Result<RiotLocalApiAccess, String> {
    let content = fs::read_to_string(lockfile_path).map_err(|e| {
        format!(
            "Could not read Riot lockfile {}: {e}",
            lockfile_path.display()
        )
    })?;
    parse_lockfile_content(&content)
}

pub(super) fn read_riot_local_api_access() -> Result<RiotLocalApiAccess, String> {
    read_lockfile_at(&riot_lockfile_path()?)
}

pub(super) async fn fetch_local_json(
    access: &RiotLocalApiAccess,
    path: &str,
) -> Result<Value, String> {
    let url = format!("{}://127.0.0.1:{}{}", access.protocol, access.port, path);
    let response = riot_local_client()
        .get(url)
        .basic_auth("riot", Some(access.password.as_str()))
        .send()
        .await
        .map_err(|e| format!("Could not query Riot local endpoint {path}: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Riot local endpoint {path} returned {}",
            response.status()
        ));
    }

    response
        .json::<Value>()
        .await
        .map_err(|e| format!("Could not parse Riot local response {path}: {e}"))
}

pub(super) fn detect_live_identity_with_access(
    access: &RiotLocalApiAccess,
) -> Result<RiotDetectedIdentity, String> {
    crate::runtime::block_on(async {
        let alias = fetch_local_json(access, "/player-account/aliases/v1/active")
            .await
            .ok()
            .and_then(|json| serde_json::from_value::<RiotAliasResponse>(json).ok());

        let account_name = alias
            .as_ref()
            .map(|a| trim_or_empty(&a.game_name))
            .unwrap_or_default();
        let account_tag_line = alias
            .as_ref()
            .map(|a| trim_or_empty(&a.tag_line))
            .unwrap_or_default();

        let userinfo = fetch_local_json(access, "/riot-client-auth/v1/userinfo")
            .await
            .unwrap_or(Value::Null);
        let account_puuid = userinfo
            .get("sub")
            .and_then(Value::as_str)
            .map(trim_or_empty)
            .unwrap_or_default();

        if account_name.is_empty() && account_tag_line.is_empty() && account_puuid.is_empty() {
            return Err("Riot local API did not return account identity".into());
        }

        Ok(RiotDetectedIdentity {
            account_name,
            account_tag_line,
            account_puuid,
        })
    })
}

pub(super) struct RiotLoginState {
    pub(super) logged_in: bool,
    /// The "Stay signed in" box as the API states it; `None` when no route
    /// gives a bool (a PSL client leaves the legacy `persist` null).
    pub(super) persist: Option<bool>,
}

impl RiotLoginState {
    pub(super) const SIGNED_OUT: Self = Self {
        logged_in: false,
        persist: None,
    };
}

/// The RSO session, which every Riot Client since the PSL settings format
/// (137+) answers: `type` is `authenticated` once the login (2FA included)
/// is done, `persistLogin` is the "Stay signed in" box. `None` when the body
/// has no `type`, so the caller can try the legacy route.
pub(super) fn login_state_from_rso_session(value: &Value) -> Option<RiotLoginState> {
    let kind = value.get("type").and_then(Value::as_str)?;
    Some(RiotLoginState {
        logged_in: kind.eq_ignore_ascii_case("authenticated"),
        persist: value.get("persistLogin").and_then(Value::as_bool),
    })
}

/// The legacy `riot-login` status of older clients. A PSL client leaves its
/// `persist` null, which reads as `None`.
pub(super) fn login_state_from_legacy_status(value: &Value) -> RiotLoginState {
    let phase = value
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or_default();
    RiotLoginState {
        logged_in: phase.eq_ignore_ascii_case("logged_in"),
        persist: value.get("persist").and_then(Value::as_bool),
    }
}

/// Read the login from the RSO session, or from the legacy status when the
/// RSO session is not there yet (404 "RSO is not yet initialized" while the
/// client boots) or not at all (older clients). Any failure reads as signed
/// out, and the setup poll asks again a second later. A signed-in RSO session
/// without `persistLogin` takes the legacy flag when that one is a bool.
pub(super) fn read_riot_login_state(access: &RiotLocalApiAccess) -> RiotLoginState {
    crate::runtime::block_on(async {
        let legacy = || async {
            fetch_local_json(access, "/riot-login/v1/status")
                .await
                .map(|value| login_state_from_legacy_status(&value))
                .ok()
        };
        let rso = fetch_local_json(access, "/rso-auth/v1/session")
            .await
            .ok()
            .as_ref()
            .and_then(login_state_from_rso_session);
        match rso {
            Some(mut state) => {
                if state.logged_in && state.persist.is_none() {
                    state.persist = legacy().await.and_then(|legacy| legacy.persist);
                }
                state
            }
            None => legacy().await.unwrap_or(RiotLoginState::SIGNED_OUT),
        }
    })
}

/// The RSO session as a post-switch watch reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RiotSessionType {
    /// No answer yet: no lockfile, no RSO (404 while the client boots), an
    /// error, or a body without `type`.
    Booting,
    Authenticated,
    /// RSO answered with another `type`: the restored token was refused.
    NotAuthenticated,
}

/// `/rso-auth/v1/session` as a session type. `None` is no answer.
pub(super) fn session_type_from_rso(value: Option<&Value>) -> RiotSessionType {
    let kind = value
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if kind.is_empty() {
        RiotSessionType::Booting
    } else if kind.eq_ignore_ascii_case("authenticated") {
        RiotSessionType::Authenticated
    } else {
        RiotSessionType::NotAuthenticated
    }
}

impl RiotSessionType {
    pub(super) fn watch_step(self) -> crate::platforms::session_marks::WatchStep {
        use crate::platforms::session_marks::WatchStep;
        match self {
            Self::Booting => WatchStep::Pending,
            Self::Authenticated => WatchStep::SignedIn,
            Self::NotAuthenticated => WatchStep::NotSignedIn,
        }
    }
}

/// One read of the RSO session. Any failure reads as booting.
pub(super) fn read_rso_session_type() -> RiotSessionType {
    let Ok(access) = read_riot_local_api_access() else {
        return RiotSessionType::Booting;
    };
    crate::runtime::block_on(async {
        let value = fetch_local_json(&access, "/rso-auth/v1/session").await.ok();
        session_type_from_rso(value.as_ref())
    })
}

pub(super) fn detect_live_identity() -> Result<RiotDetectedIdentity, String> {
    let access = read_riot_local_api_access()?;
    detect_live_identity_with_access(&access)
}

pub(super) const LEAGUE_GAMEFLOW_PHASE_ENDPOINT: &str = "/lol-gameflow/v1/gameflow-phase";

/// League of Legends gameflow phases where killing `LeagueClient` would cause
/// a champion-select dodge or interrupt an active match.
const LEAGUE_PROTECTED_PHASES: &[&str] = &["ChampSelect", "GameStart", "InProgress", "Reconnect"];

/// Returns the canonical protected phase name when `value` (the JSON body of
/// `/lol-gameflow/v1/gameflow-phase`) names a phase in which closing
/// `LeagueClient` must be refused. Any other value returns `None`.
pub(super) fn protected_league_phase(value: &Value) -> Option<&'static str> {
    let phase = value.as_str()?.trim();
    LEAGUE_PROTECTED_PHASES
        .iter()
        .copied()
        .find(|protected| protected.eq_ignore_ascii_case(phase))
}

/// Probe the League Client (LCU) gameflow phase on `access`.
/// Returns `Some(phase)` only when the LCU answers with a protected phase;
/// any unreachable socket, non-200 status, or unreadable body returns `None`
/// so callers keep their current behaviour.
pub(super) fn read_protected_league_phase_with_access(
    access: &RiotLocalApiAccess,
) -> Option<&'static str> {
    crate::runtime::block_on(async {
        let value = fetch_local_json(access, LEAGUE_GAMEFLOW_PHASE_ENDPOINT)
            .await
            .ok()?;
        protected_league_phase(&value)
    })
}

/// Candidate paths for the League Client (LCU) `lockfile`, derived from Riot
/// product metadata (`league_of_legends.live` / `league_of_legends.pbe`), a
/// sibling `League of Legends` folder next to `riot_install_dir`, and an
/// optional OS default. Duplicates are removed while preserving order.
pub(super) fn league_lockfile_candidates_in(
    metadata_root: Option<&Path>,
    riot_install_dir: Option<&Path>,
    default_lockfile: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut push_unique = |path: PathBuf| {
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    };

    if let Some(root) = metadata_root {
        for patchline in ["live", "pbe"] {
            let folder = format!("league_of_legends.{patchline}");
            let settings = root
                .join(&folder)
                .join(format!("{folder}.product_settings.yaml"));
            if let Some(install_path) = fs::read_to_string(settings)
                .ok()
                .and_then(|contents| product_install_path(&contents))
            {
                push_unique(PathBuf::from(install_path).join("lockfile"));
            }
        }
    }

    if let Some(parent) = riot_install_dir.and_then(Path::parent) {
        push_unique(parent.join("League of Legends").join("lockfile"));
    }

    if let Some(default) = default_lockfile {
        push_unique(default.to_path_buf());
    }

    candidates
}

/// Probe each readable LCU lockfile candidate until one reports a protected
/// gameflow phase. Missing, unreadable or stale lockfiles are skipped.
pub(super) fn detect_protected_league_phase_from_candidates(
    candidates: &[PathBuf],
) -> Option<&'static str> {
    for path in candidates {
        let Ok(access) = read_lockfile_at(path) else {
            continue;
        };
        if let Some(phase) = read_protected_league_phase_with_access(&access) {
            return Some(phase);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn default_league_lockfile_path() -> PathBuf {
    PathBuf::from("/Applications/League of Legends.app/Contents/LoL/lockfile")
}

#[cfg(not(target_os = "macos"))]
fn default_league_lockfile_path() -> PathBuf {
    PathBuf::from(r"C:\Riot Games\League of Legends\lockfile")
}

/// Probe the live League Client for a protected gameflow phase. Returns `None`
/// if no lockfile is readable or the LCU API does not report a protected phase.
pub(super) fn detect_protected_league_phase() -> Option<&'static str> {
    let metadata = metadata_root();
    let riot_install_dir = detect_installation_path_from_installs()
        .and_then(|path| PathBuf::from(path).parent().map(Path::to_path_buf));
    let default_lockfile = default_league_lockfile_path();
    let candidates = league_lockfile_candidates_in(
        metadata.as_deref(),
        riot_install_dir.as_deref(),
        Some(default_lockfile.as_path()),
    );
    detect_protected_league_phase_from_candidates(&candidates)
}
