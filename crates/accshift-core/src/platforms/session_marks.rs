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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Bumped by every mark this process stores or lifts, so the app can reread
/// the marks a background watch wrote after the switch returned.
static REVISION: AtomicU64 = AtomicU64::new(0);

/// The revision of the marks this process wrote. Cheap: one atomic read.
pub fn revision() -> u64 {
    REVISION.load(Ordering::SeqCst)
}

fn bump_revision() {
    REVISION.fetch_add(1, Ordering::SeqCst);
}

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
    config::update_config(app, |cfg| set_mark_in(cfg, platform, account_id, mark))?;
    bump_revision();
    Ok(())
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
    })?;
    bump_revision();
    Ok(())
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
    /// Another switch, setup or capture took the launcher over: what it
    /// shows now belongs to another account. Ends the watch, marks nothing.
    Superseded,
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
            WatchStep::Superseded => return WatchVerdict::Unknown,
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

/// Where each platform's watch generation lives: one small file beside the
/// operation lock, shared by every accshift process (GUI and CLI), so a CLI
/// switch supersedes a watch the GUI started and the reverse.
fn epochs_path(app: &dyn AppContext) -> Result<std::path::PathBuf, String> {
    Ok(app
        .app_local_data_dir()?
        .join("state")
        .join("session-watch.json"))
}

/// Missing or unreadable reads as no generation yet.
fn read_epochs(app: &dyn AppContext) -> BTreeMap<String, u64> {
    epochs_path(app)
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

const EPOCH_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `job` under the cross-process write lock the config writes and the
/// launcher operations take (nested when this thread already holds it).
fn with_operation_lock<T>(
    app: &dyn AppContext,
    job: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _lock =
        crate::lock::acquire_for_write(app, EPOCH_LOCK_TIMEOUT).map_err(|e| e.to_string())?;
    job()
}

/// One operation's claim on a platform's launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchTicket {
    platform: String,
    /// 0: the claim could not be stored; such a ticket never records.
    epoch: u64,
}

/// Starts a switch, setup, capture or forget on `platform`: any watch still
/// running there, in this process or another, stops counting. Call it before
/// the launcher is touched and before the config the operation saves is
/// read. A generation that cannot be stored gives a ticket that records
/// nothing.
pub fn supersede_watch(app: &dyn AppContext, platform: &str) -> WatchTicket {
    let epoch = with_operation_lock(app, || {
        let mut epochs = read_epochs(app);
        let epoch = epochs.entry(platform.to_string()).or_default();
        *epoch += 1;
        let epoch = *epoch;
        crate::storage::write_json_atomic(&epochs_path(app)?, &epochs)?;
        Ok(epoch)
    })
    .unwrap_or(0);
    WatchTicket {
        platform: platform.to_string(),
        epoch,
    }
}

impl WatchTicket {
    /// No later operation took the platform over, in any process.
    pub fn is_current(&self, app: &dyn AppContext) -> bool {
        self.epoch != 0 && read_epochs(app).get(&self.platform) == Some(&self.epoch)
    }

    /// [`watch_after_switch`] that stops once superseded. The check follows
    /// each read: a read that saw the next operation's account is dropped.
    pub fn watch(
        &self,
        app: &dyn AppContext,
        attempts: usize,
        mut read: impl FnMut() -> WatchStep,
        wait: impl FnMut(),
    ) -> WatchVerdict {
        watch_after_switch(
            attempts,
            || {
                let step = read();
                if self.is_current(app) {
                    step
                } else {
                    WatchStep::Superseded
                }
            },
            wait,
        )
    }

    /// [`apply_verdict`] for this ticket's account, unless superseded. The
    /// check and the write share one hold of the operation lock, the lock
    /// [`supersede_watch`] takes: a newer operation lands wholly before
    /// (the verdict is dropped) or wholly after (it sees the write).
    pub fn apply(
        &self,
        app: &dyn AppContext,
        account_id: &str,
        verdict: WatchVerdict,
    ) -> Result<(), String> {
        self.apply_with(app, account_id, verdict, || {})
    }

    /// [`Self::apply`] with a hook between the check and the write (tests).
    fn apply_with(
        &self,
        app: &dyn AppContext,
        account_id: &str,
        verdict: WatchVerdict,
        before_write: impl FnOnce(),
    ) -> Result<(), String> {
        if verdict == WatchVerdict::Unknown {
            return Ok(());
        }
        with_operation_lock(app, || {
            if !self.is_current(app) {
                return Ok(());
            }
            before_write();
            apply_verdict(app, &self.platform, account_id, verdict)
        })
    }
}

/// Watches still running in this process.
fn running_watches() -> &'static (Mutex<usize>, Condvar) {
    static RUNNING: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    RUNNING.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

/// Counts one running watch until dropped, panics included.
struct RunningWatch;

impl RunningWatch {
    fn start() -> Self {
        let (count, _) = running_watches();
        *count.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        Self
    }
}

impl Drop for RunningWatch {
    fn drop(&mut self) {
        let (count, ended) = running_watches();
        let mut count = count.lock().unwrap_or_else(|p| p.into_inner());
        *count = count.saturating_sub(1);
        ended.notify_all();
    }
}

/// Whether a watch of this process is still running, so the UI keeps
/// waking to land its verdict.
pub fn watches_running() -> bool {
    let (count, _) = running_watches();
    *count.lock().unwrap_or_else(|p| p.into_inner()) > 0
}

/// Waits until every watch of this process ended, at most `limit`. A
/// short-lived caller (the CLI) calls it before exiting, so a refusal seen
/// after it printed its result is still stored. True when none is left.
pub fn wait_for_watches(limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    let (count, ended) = running_watches();
    let mut count = count.lock().unwrap_or_else(|p| p.into_inner());
    while *count > 0 {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        count = match ended.wait_timeout(count, deadline - now) {
            Ok((guard, _)) => guard,
            Err(poisoned) => poisoned.into_inner().0,
        };
    }
    true
}

/// Runs a post-switch watch on its own thread, so the switch returns at once.
/// Never in unit tests: they would leave a thread polling a scratch folder.
pub fn spawn_watch(name: &str, job: impl FnOnce() + Send + 'static) {
    if cfg!(test) {
        return;
    }
    spawn_tracked(name, job);
}

/// Spawns `job` counted by [`wait_for_watches`].
fn spawn_tracked(name: &str, job: impl FnOnce() + Send + 'static) {
    let running = RunningWatch::start();
    let _ = std::thread::Builder::new()
        .name(format!("session-watch-{name}"))
        .spawn(move || {
            let _running = running;
            job();
        });
}

#[cfg(test)]
#[path = "session_marks_tests.rs"]
mod tests;
