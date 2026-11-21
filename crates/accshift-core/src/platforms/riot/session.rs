//! Offline session expiry: the PSL restore window of a saved snapshot.
//!
//! Riot Client 137+ writes, next to its refresh token, when the token was
//! created (`last_token_creation_time`, Unix ms) and how long a restore may
//! wait (`max_duration_between_restores`, seconds, about 453 days). A saved
//! snapshot past that bound is refused at the next launch. The read is local
//! and plain YAML: no network call, no token decoded. Anything missing or
//! unreadable is unknown, never expired.

#[allow(unused_imports)]
use super::*;

/// Grace past the deadline before a session reads as expired, so a clock
/// skew or a rounding of the window never flags a live session.
const EXPIRY_GRACE_MS: u64 = 24 * 60 * 60 * 1000;

/// Below this, a creation time is in seconds rather than milliseconds.
const SECONDS_CUTOFF: u64 = 10_000_000_000;

/// What a saved snapshot says about its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionHealth {
    /// Past the restore window: the client will ask for a sign-in.
    Expired,
    /// Within the restore window.
    Valid,
    /// No readable bound (legacy client, no token, unreadable file).
    Unknown,
}

fn yaml_number(value: &str) -> Option<u64> {
    let value = value.split(" #").next().unwrap_or(value).trim();
    value
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .trim()
        .parse()
        .ok()
}

/// The PSL restore deadline (Unix ms): `last_token_creation_time` plus
/// `max_duration_between_restores`, both under `psl: authorization:
/// riot-client:`, and only with a non-empty refresh token there.
pub(super) fn psl_restore_deadline(contents: &str) -> Option<u64> {
    let mut in_psl = false;
    let mut client_indent: Option<usize> = None;
    let mut created: Option<u64> = None;
    let mut window_s: Option<u64> = None;
    let mut has_token = false;
    for line in contents.lines() {
        if let Some(key) = top_level_key(line) {
            in_psl = key == "psl";
            client_indent = None;
            continue;
        }
        let trimmed = line.trim();
        if !in_psl || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(open) = client_indent {
            if indent <= open {
                client_indent = None;
            }
        }
        if trimmed.trim_end() == "riot-client:" {
            client_indent = Some(indent);
            continue;
        }
        if client_indent.is_none() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        match key.trim() {
            "last_token_creation_time" => created = yaml_number(value),
            "max_duration_between_restores" => window_s = yaml_number(value),
            "refresh_token" => {
                let value = value.split(" #").next().unwrap_or(value).trim();
                has_token = !matches!(value, "" | "''" | "\"\"" | "~" | "null");
            }
            _ => {}
        }
    }
    if !has_token {
        return None;
    }
    let created = created.filter(|&ms| ms > 0)?;
    let created_ms = if created < SECONDS_CUTOFF {
        created.checked_mul(1000)?
    } else {
        created
    };
    created_ms.checked_add(window_s.filter(|&s| s > 0)?.checked_mul(1000)?)
}

/// The session health of a settings file body at `now_ms`.
pub fn session_health(contents: &str, now_ms: u64) -> SessionHealth {
    match psl_restore_deadline(contents) {
        Some(deadline) if now_ms > deadline.saturating_add(EXPIRY_GRACE_MS) => {
            SessionHealth::Expired
        }
        Some(_) => SessionHealth::Valid,
        None => SessionHealth::Unknown,
    }
}

/// The health of one profile's saved snapshot under `profiles_root`.
pub(super) fn snapshot_session_health(
    profiles_root: &Path,
    profile_id: &str,
    now_ms: u64,
) -> SessionHealth {
    if !is_valid_profile_id(profile_id) {
        return SessionHealth::Unknown;
    }
    let path = profiles_root
        .join(profile_id)
        .join(RIOT_SNAPSHOT_ITEMS[0].snapshot_name);
    let Ok(bytes) = snapshot_crypto::read_decrypted_bytes(&path) else {
        return SessionHealth::Unknown;
    };
    match String::from_utf8(bytes) {
        Ok(contents) => session_health(&contents, now_ms),
        Err(_) => SessionHealth::Unknown,
    }
}

/// The ids of `profiles` whose saved session is past its restore window.
/// Only profiles with a ready snapshot are read.
pub(super) fn expired_profile_ids_in(
    profiles: &[RiotProfileConfig],
    profiles_root: &Path,
    now_ms: u64,
) -> Vec<String> {
    profiles
        .iter()
        .filter(|profile| profile.snapshot_state == "ready")
        .filter(|profile| {
            snapshot_session_health(profiles_root, &profile.id, now_ms) == SessionHealth::Expired
        })
        .map(|profile| profile.id.clone())
        .collect()
}

/// The visible profiles whose saved session has expired, read offline from
/// their snapshots. A profile whose snapshot cannot be read is left out.
pub fn expired_profile_ids(app_handle: AppCtx) -> Result<Vec<String>, String> {
    let cfg = config::load_config(&app_handle);
    let root = app_profiles_root(&*app_handle)?;
    Ok(expired_profile_ids_in(
        &visible_profiles(&cfg),
        &root,
        crate::platforms::now_unix_ms(),
    ))
}
