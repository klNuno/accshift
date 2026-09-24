//! The Riot client's local API: lockfile access, login state and live identity.

use super::*;

pub(super) fn riot_lockfile_path() -> Result<PathBuf, String> {
    Ok(env_path("LOCALAPPDATA")?
        .join("Riot Games")
        .join("Riot Client")
        .join("Config")
        .join("lockfile"))
}

pub(super) fn read_riot_local_api_access() -> Result<RiotLocalApiAccess, String> {
    let lockfile_path = riot_lockfile_path()?;
    let content = fs::read_to_string(&lockfile_path).map_err(|e| {
        format!(
            "Could not read Riot lockfile {}: {e}",
            lockfile_path.display()
        )
    })?;
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
    pub(super) persist: bool,
}

pub(super) fn read_riot_login_state(access: &RiotLocalApiAccess) -> RiotLoginState {
    crate::runtime::block_on(async {
        let value = match fetch_local_json(access, "/riot-login/v1/status").await {
            Ok(v) => v,
            Err(_) => {
                return RiotLoginState {
                    logged_in: false,
                    persist: false,
                }
            }
        };
        let phase = value
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let persist = value
            .get("persist")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        RiotLoginState {
            logged_in: phase.eq_ignore_ascii_case("logged_in"),
            persist,
        }
    })
}

pub(super) fn detect_live_identity() -> Result<RiotDetectedIdentity, String> {
    let access = read_riot_local_api_access()?;
    detect_live_identity_with_access(&access)
}
