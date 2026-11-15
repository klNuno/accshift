//! Per-platform session marks: what accshift learned about an account's saved
//! session after the fact (a switch that ended on the sign-in screen) and what
//! a local file says offline (the launcher does not remember the password).
//!
//! After-the-fact marks persist in the machine-local config until a new
//! capture of the account or a switch that ends signed in clears them.
//! Offline marks are recomputed on every read and never stored. Anything
//! missing or unreadable marks nothing.

use crate::config::{self, AppConfig};
use crate::context::AppContext;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a mark says about the session.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    /// The launcher refused the saved session: sign in again.
    #[default]
    Expired,
    /// The launcher does not keep the password: it asks at every switch.
    PasswordNeeded,
}

/// Where a mark comes from.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SessionSource {
    /// Read offline from a snapshot or a launcher file.
    #[default]
    OfflineSnapshot,
    /// Seen after a switch, on the launcher's own state.
    AfterSwitch,
    /// A network probe (Roblox).
    NetworkProbe,
}

/// One account's session health.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SessionMark {
    #[serde(default)]
    pub state: SessionState,
    #[serde(default)]
    pub source: SessionSource,
    /// Unix ms.
    #[serde(default)]
    pub detected_at: u64,
}

/// A mark as the app reads it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMarkInfo {
    pub account_id: String,
    pub state: SessionState,
}

/// Every persisted mark, by platform then account id.
pub type SessionMarkStore = BTreeMap<String, BTreeMap<String, SessionMark>>;

/// Reads the stored marks; a block that does not parse reads as no marks, so
/// a damaged mark never makes the whole config file unreadable.
pub fn deserialize_lenient<'de, D>(deserializer: D) -> Result<SessionMarkStore, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Sets a mark in a loaded config.
pub fn set_mark_in(cfg: &mut AppConfig, platform: &str, account_id: &str, mark: SessionMark) {
    if platform.is_empty() || account_id.is_empty() {
        return;
    }
    cfg.session_marks
        .entry(platform.to_string())
        .or_default()
        .insert(account_id.to_string(), mark);
}

/// Lifts a mark in a loaded config. Returns whether one was there.
pub fn clear_mark_in(cfg: &mut AppConfig, platform: &str, account_id: &str) -> bool {
    let Some(marks) = cfg.session_marks.get_mut(platform) else {
        return false;
    };
    let removed = marks.remove(account_id).is_some();
    if marks.is_empty() {
        cfg.session_marks.remove(platform);
    }
    removed
}

/// Records an after-the-fact mark.
pub fn mark_after_switch(
    app: &dyn AppContext,
    platform: &str,
    account_id: &str,
    state: SessionState,
) -> Result<(), String> {
    let mark = SessionMark {
        state,
        source: SessionSource::AfterSwitch,
        detected_at: super::now_unix_ms(),
    };
    config::update_config(app, |cfg| set_mark_in(cfg, platform, account_id, mark))
}

/// Lifts a mark: a new capture, or a switch that ended signed in. A config
/// without the mark is left unwritten.
pub fn clear_mark(app: &dyn AppContext, platform: &str, account_id: &str) -> Result<(), String> {
    let has_mark = config::load_config(app)
        .session_marks
        .get(platform)
        .is_some_and(|marks| marks.contains_key(account_id));
    if !has_mark {
        return Ok(());
    }
    config::update_config(app, |cfg| {
        clear_mark_in(cfg, platform, account_id);
    })
}

/// The persisted marks of a platform with the offline ones on top. An offline
/// "password needed" wins over a stored "expired": it says why.
pub fn merge_marks(
    stored: Option<&BTreeMap<String, SessionMark>>,
    offline: impl IntoIterator<Item = (String, SessionState)>,
) -> Vec<SessionMarkInfo> {
    let mut merged: BTreeMap<String, SessionState> = stored
        .into_iter()
        .flatten()
        .map(|(id, mark)| (id.clone(), mark.state))
        .collect();
    for (id, state) in offline {
        if !id.is_empty() {
            merged.insert(id, state);
        }
    }
    merged
        .into_iter()
        .map(|(account_id, state)| SessionMarkInfo { account_id, state })
        .collect()
}

/// The marks of one platform: stored after-the-fact marks plus the offline
/// "password needed" reads (Steam `loginusers.vdf`, descriptor snapshots).
pub fn platform_marks(
    app: crate::context::AppCtx,
    platform: &str,
) -> Result<Vec<SessionMarkInfo>, String> {
    let cfg = config::load_config(app.as_ref());
    let offline: Vec<(String, SessionState)> = match platform {
        "steam" => super::steam::password_needed_steam_ids(app.clone())
            .into_iter()
            .map(|id| (id, SessionState::PasswordNeeded))
            .collect(),
        other => super::descriptor::not_remembered_ids(app.as_ref(), other)
            .into_iter()
            .map(|id| (id, SessionState::PasswordNeeded))
            .collect(),
    };
    Ok(merge_marks(cfg.session_marks.get(platform), offline))
}

/// One read of the launcher's state after a switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchStep {
    /// Nothing clear yet: booting, unreachable, unreadable.
    Pending,
    /// The target account is signed in.
    SignedIn,
    /// The launcher answered and the target is not signed in.
    NotSignedIn,
}

/// How a post-switch watch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchVerdict {
    SignedIn,
    /// The launcher kept answering "not signed in" to the end.
    Refused,
    /// No clear answer: marks nothing.
    Unknown,
}

/// Polls `read` up to `attempts` times, `wait` between reads. A sign-in ends
/// the watch at once; "refused" needs the last two reads to say "not signed
/// in", so a launcher still restoring the session or a single odd answer
/// marks nothing.
pub fn watch_after_switch(
    attempts: usize,
    mut read: impl FnMut() -> WatchStep,
    mut wait: impl FnMut(),
) -> WatchVerdict {
    let mut streak = 0usize;
    for attempt in 0..attempts {
        if attempt > 0 {
            wait();
        }
        match read() {
            WatchStep::SignedIn => return WatchVerdict::SignedIn,
            WatchStep::NotSignedIn => streak += 1,
            WatchStep::Pending => streak = 0,
        }
    }
    if streak >= 2 {
        WatchVerdict::Refused
    } else {
        WatchVerdict::Unknown
    }
}

/// Stores what a watch found: a refusal marks the account, a sign-in lifts
/// its mark, an unclear end changes nothing.
pub fn apply_verdict(
    app: &dyn AppContext,
    platform: &str,
    account_id: &str,
    verdict: WatchVerdict,
) -> Result<(), String> {
    match verdict {
        WatchVerdict::Refused => {
            mark_after_switch(app, platform, account_id, SessionState::Expired)
        }
        WatchVerdict::SignedIn => clear_mark(app, platform, account_id),
        WatchVerdict::Unknown => Ok(()),
    }
}

/// Runs a post-switch watch on its own thread, so the switch returns at once.
/// Never in unit tests: they would leave a thread polling a scratch folder.
pub fn spawn_watch(name: &str, job: impl FnOnce() + Send + 'static) {
    if cfg!(test) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name(format!("session-watch-{name}"))
        .spawn(job);
}

#[cfg(test)]
#[path = "session_marks_tests.rs"]
mod tests;
