//! Steam session marks: "password needed" read from `loginusers.vdf`, and the
//! post-switch check that sees Steam stop at its password prompt.
//!
//! Steam keeps every remembered login itself, so accshift holds no session to
//! read. `RememberPassword` "0" means Steam asks for the password at every
//! sign-in. After a switch, Steam running with the target's `Timestamp` not
//! advanced means it never signed in. Current Steam writes no `MostRecent`
//! key at all, so the timestamp alone decides; a `MostRecent` "0" still
//! vetoes. Anything missing or unreadable marks nothing.

use super::vdf::parse_vdf;
use crate::platforms::session_marks::WatchStep;
use std::collections::BTreeMap;
use std::path::Path;

/// The target's `loginusers.vdf` entry as the post-switch check reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginEntry {
    pub steam_id: String,
    pub timestamp: Option<u64>,
    /// `None` when the key is absent, as in every entry current Steam writes.
    pub most_recent: Option<bool>,
    /// `None` when the key is absent.
    pub remember_password: Option<bool>,
}

fn entries(content: &str) -> Vec<(String, LoginEntry)> {
    parse_vdf(content)
        .into_iter()
        .map(|(steam_id, data)| {
            let account = data.get("accountname").cloned().unwrap_or_default();
            let entry = LoginEntry {
                timestamp: data.get("timestamp").and_then(|ts| ts.trim().parse().ok()),
                most_recent: data.get("mostrecent").map(|v| v.trim() == "1"),
                remember_password: data.get("rememberpassword").and_then(|v| match v.trim() {
                    "0" => Some(false),
                    "1" => Some(true),
                    _ => None,
                }),
                steam_id,
            };
            (account, entry)
        })
        .collect()
}

/// Steam ids whose entry says `RememberPassword` "0". A missing key or any
/// other value says nothing.
pub fn password_needed_ids_in(content: &str) -> Vec<String> {
    let mut ids: Vec<String> = entries(content)
        .into_iter()
        .filter(|(_, entry)| entry.remember_password == Some(false))
        .map(|(_, entry)| entry.steam_id)
        .filter(|id| !id.trim().is_empty())
        .collect();
    ids.sort();
    ids
}

/// The entry for an account name, the lowest steam id on a tie.
pub fn entry_for_account(content: &str, account_name: &str) -> Option<LoginEntry> {
    entries(content)
        .into_iter()
        .filter(|(account, _)| account.eq_ignore_ascii_case(account_name))
        .map(|(_, entry)| entry)
        .min_by(|a, b| a.steam_id.cmp(&b.steam_id))
}

fn read_loginusers(steam_path: &Path) -> Option<String> {
    std::fs::read_to_string(steam_path.join("config").join("loginusers.vdf")).ok()
}

/// [`password_needed_ids_in`] on this Steam install; an unreadable file
/// reads as no ids.
pub fn password_needed_ids(steam_path: &Path) -> Vec<String> {
    read_loginusers(steam_path)
        .map(|content| password_needed_ids_in(&content))
        .unwrap_or_default()
}

/// [`entry_for_account`] on this Steam install.
pub fn read_entry(steam_path: &Path, account_name: &str) -> Option<LoginEntry> {
    read_loginusers(steam_path).and_then(|content| entry_for_account(&content, account_name))
}

/// One post-switch read. Steam signed the target in once its timestamp moved
/// past the one before the switch; while Steam runs without that, the read
/// says "not signed in". Steam closed, the entry gone or a timestamp missing
/// reads as pending.
pub fn watch_step(before: &LoginEntry, now: Option<&LoginEntry>, steam_running: bool) -> WatchStep {
    let Some(now) = now.filter(|now| now.steam_id == before.steam_id) else {
        return WatchStep::Pending;
    };
    let advanced = match (before.timestamp, now.timestamp) {
        (Some(before), Some(now)) => now > before,
        (None, Some(_)) if now.most_recent == Some(true) => true,
        _ => return WatchStep::Pending,
    };
    if advanced && now.most_recent != Some(false) {
        WatchStep::SignedIn
    } else if steam_running {
        WatchStep::NotSignedIn
    } else {
        WatchStep::Pending
    }
}

/// Whether a switch to this entry is worth watching: an account Steam does
/// not remember already shows "password needed", a prompt is expected.
pub fn worth_watching(before: &LoginEntry) -> bool {
    before.remember_password != Some(false)
}

/// Steam id to last sign-in time (Unix s), from each entry's `Timestamp`.
pub fn sign_in_times_in(content: &str) -> BTreeMap<String, u64> {
    entries(content)
        .into_iter()
        .filter_map(|(_, entry)| Some((entry.steam_id, entry.timestamp?)))
        .collect()
}

/// [`sign_in_times_in`] on this Steam install; an unreadable file reads as
/// no times, which keeps every stored mark.
pub fn sign_in_times(steam_path: &Path) -> BTreeMap<String, u64> {
    read_loginusers(steam_path)
        .map(|content| sign_in_times_in(&content))
        .unwrap_or_default()
}

/// Whether a stored "expired" mark predates the account's last sign-in:
/// Steam signed it in since, from accshift or by hand, so the mark is stale.
pub fn mark_is_stale(stamps: &BTreeMap<String, u64>, steam_id: &str, detected_at_ms: u64) -> bool {
    stamps
        .get(steam_id)
        .is_some_and(|&signed_in_s| signed_in_s.saturating_mul(1000) > detected_at_ms)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
