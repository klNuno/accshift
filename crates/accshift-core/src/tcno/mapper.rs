//! Per-platform mapping from scanned TcNo files to canonical accshift account,
//! note, order and snapshot import plans.

use super::reader::{
    safe_account_dir, sniff_epic_folder, sniff_ubisoft_folder, EpicFolderKind, RootScan,
    ScannedCacheDir, ScannedPlatform,
};
use super::{TcNoFormatVersion, TcNoWarning, TcNoWarningKind};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

/// Matches `accshift_app::stores::MAX_NOTE_LENGTH` (UTF-16 code units).
const MAX_NOTE_UTF16_LEN: usize = 180;

/// Session snapshot ready to be encrypted into accshift's descriptor storage.
#[derive(Debug, Clone)]
pub(super) enum PlannedSnapshot {
    Ubisoft {
        user_dat: PathBuf,
        connect_secure_storage: PathBuf,
    },
    EpicLegacy {
        ini_path: PathBuf,
        account_id: String,
    },
}

/// One account to import (or match against an existing account).
#[derive(Debug, Clone)]
pub(super) struct PlannedAccount {
    pub(super) platform_id: &'static str,
    pub(super) tcno_uid: String,
    pub(super) accshift_id: String,
    pub(super) label: String,
    pub(super) last_used_at: Option<u64>,
    pub(super) source_version: TcNoFormatVersion,
    pub(super) snapshot: Option<PlannedSnapshot>,
    /// True when the account has no snapshot/session after import and will
    /// require a first sign-in or capture in accshift.
    pub(super) needs_capture: bool,
}

/// Planned import for one accshift platform.
#[derive(Debug, Clone)]
pub(super) struct PlannedPlatform {
    pub(super) platform_id: &'static str,
    pub(super) accounts: Vec<PlannedAccount>,
    /// Canonical accshift account id -> sanitized note (including v4 tags).
    pub(super) notes: BTreeMap<String, String>,
    /// Ordered canonical accshift account ids from `order.json`.
    pub(super) order: Vec<String>,
    pub(super) eglv2_skipped: usize,
    pub(super) vault_detected: bool,
}

/// Complete import plan built from a read-only [`RootScan`].
#[derive(Debug, Clone)]
pub(super) struct ImportPlan {
    pub(super) path: String,
    pub(super) version: TcNoFormatVersion,
    pub(super) platforms: Vec<PlannedPlatform>,
    pub(super) warnings: Vec<TcNoWarning>,
}

pub(super) fn build_plan(scan: RootScan) -> ImportPlan {
    let mut warnings = scan.warnings;
    let mut platforms = Vec::new();

    for scanned in scan.platforms {
        let planned = map_platform(scanned, &mut warnings);
        if !planned.accounts.is_empty()
            || !planned.notes.is_empty()
            || !planned.order.is_empty()
            || planned.eglv2_skipped > 0
            || planned.vault_detected
        {
            platforms.push(planned);
        }
    }

    ImportPlan {
        path: scan.root.display().to_string(),
        version: scan.version,
        platforms,
        warnings,
    }
}

fn map_platform(scanned: ScannedPlatform, warnings: &mut Vec<TcNoWarning>) -> PlannedPlatform {
    if scanned.accshift_id == "steam" {
        return map_steam(scanned, warnings);
    }

    let mut accounts = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut uid_to_accshift: HashMap<String, String> = HashMap::new();
    let mut name_to_accshift: HashMap<String, String> = HashMap::new();
    let mut tags_by_accshift: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut raw_orders: Vec<String> = Vec::new();
    let mut eglv2_skipped = 0usize;

    for cache_dir in &scanned.cache_dirs {
        raw_orders.extend(cache_dir.order.iter().cloned());

        for (raw_uid, raw_name) in &cache_dir.ids.accounts {
            let trimmed_uid = raw_uid.trim();
            let label = raw_name.trim().to_string();
            if trimmed_uid.is_empty() {
                continue;
            }

            if scanned.accshift_id == "epic" {
                match map_epic_account(cache_dir, trimmed_uid, &label, warnings) {
                    EpicAccountOutcome::Planned(account) => {
                        let key = account.accshift_id.to_ascii_lowercase();
                        if !seen_ids.insert(key) {
                            continue;
                        }
                        uid_to_accshift
                            .insert(trimmed_uid.to_string(), account.accshift_id.clone());
                        uid_to_accshift.insert(
                            trimmed_uid.to_ascii_lowercase(),
                            account.accshift_id.clone(),
                        );
                        if !label.is_empty() {
                            name_to_accshift
                                .insert(label.to_ascii_lowercase(), account.accshift_id.clone());
                        }
                        if let Some(tags) = cache_dir.ids.account_tags.get(raw_uid) {
                            tags_by_accshift.insert(account.accshift_id.clone(), tags.clone());
                        }
                        accounts.push(account);
                    }
                    EpicAccountOutcome::Eglv2Skipped => {
                        eglv2_skipped += 1;
                    }
                    EpicAccountOutcome::Invalid => {}
                }
                continue;
            }

            let Some(accshift_id) = normalize_account_id(scanned.accshift_id, trimmed_uid) else {
                warnings.push(skipped_id_warning(scanned.accshift_id, &label, trimmed_uid));
                continue;
            };

            let dedup_key = accshift_id.to_ascii_lowercase();
            if !seen_ids.insert(dedup_key) {
                continue;
            }

            let last_used_at = cache_dir
                .ids
                .last_used
                .get(raw_uid)
                .and_then(parse_timestamp_ms);

            let (snapshot, needs_capture) = match scanned.accshift_id {
                "battle-net" => (None, false),
                "ubisoft" => {
                    let is_vaulted = cache_dir
                        .vaulted_uids
                        .iter()
                        .any(|v| v.eq_ignore_ascii_case(trimmed_uid));
                    if is_vaulted {
                        (None, true)
                    } else if let Some(dir) = safe_account_dir(&cache_dir.dir, &label) {
                        match sniff_ubisoft_folder(&dir) {
                            Some(files) => (
                                Some(PlannedSnapshot::Ubisoft {
                                    user_dat: files.user_dat,
                                    connect_secure_storage: files.connect_secure_storage,
                                }),
                                false,
                            ),
                            None => {
                                warnings.push(TcNoWarning {
                                    platform: "ubisoft".to_string(),
                                    kind: TcNoWarningKind::SnapshotIncomplete,
                                    detail: format!(
                                        "Ubisoft account '{}' is missing user.dat or ConnectSecureStorage.dat; imported as to capture.",
                                        display_or_id(&label, &accshift_id)
                                    ),
                                });
                                (None, true)
                            }
                        }
                    } else {
                        (None, true)
                    }
                }
                // Riot, Roblox, GOG, Jagex, Discord: list + notes only ("to capture").
                _ => (None, true),
            };

            uid_to_accshift.insert(trimmed_uid.to_string(), accshift_id.clone());
            uid_to_accshift.insert(trimmed_uid.to_ascii_lowercase(), accshift_id.clone());
            if !label.is_empty() {
                name_to_accshift.insert(label.to_ascii_lowercase(), accshift_id.clone());
            }
            if let Some(tags) = cache_dir.ids.account_tags.get(raw_uid) {
                tags_by_accshift.insert(accshift_id.clone(), tags.clone());
            }

            accounts.push(PlannedAccount {
                platform_id: scanned.accshift_id,
                tcno_uid: trimmed_uid.to_string(),
                accshift_id,
                label,
                last_used_at,
                source_version: cache_dir.ids.version,
                snapshot,
                needs_capture,
            });
        }
    }

    // Build notes keyed by canonical accshift account id, folding v4 tags in.
    let mut raw_notes_by_accshift: BTreeMap<String, String> = BTreeMap::new();
    for (raw_uid, note_text) in &scanned.notes {
        let trimmed_uid = raw_uid.trim();
        let target_id = uid_to_accshift
            .get(trimmed_uid)
            .or_else(|| uid_to_accshift.get(&trimmed_uid.to_ascii_lowercase()))
            .cloned()
            .or_else(|| normalize_account_id(scanned.accshift_id, trimmed_uid));
        if let Some(id) = target_id {
            raw_notes_by_accshift.insert(id, note_text.clone());
        }
    }

    let mut note_ids: HashSet<String> = raw_notes_by_accshift.keys().cloned().collect();
    note_ids.extend(tags_by_accshift.keys().cloned());
    let mut notes = BTreeMap::new();
    for id in note_ids {
        let base = raw_notes_by_accshift.get(&id).map(String::as_str);
        let tags = tags_by_accshift.get(&id).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(composed) = compose_note(base, tags) {
            notes.insert(id, composed);
        }
    }

    let order = resolve_order(
        &raw_orders,
        &uid_to_accshift,
        &name_to_accshift,
        scanned.accshift_id,
    );

    PlannedPlatform {
        platform_id: scanned.accshift_id,
        accounts,
        notes,
        order,
        eglv2_skipped,
        vault_detected: scanned.vault_detected,
    }
}

/// Steam accounts already come from Steam's `loginusers.vdf`. Import notes
/// (keyed by SteamID64), v4 tags (folded into notes) and `order.json`.
fn map_steam(scanned: ScannedPlatform, _warnings: &mut Vec<TcNoWarning>) -> PlannedPlatform {
    let mut tags_by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut raw_orders: Vec<String> = Vec::new();
    let mut uid_to_id: HashMap<String, String> = HashMap::new();
    let mut name_to_id: HashMap<String, String> = HashMap::new();

    for cache_dir in &scanned.cache_dirs {
        raw_orders.extend(cache_dir.order.iter().cloned());
        for (raw_uid, raw_name) in &cache_dir.ids.accounts {
            if let Some(id) = normalize_account_id("steam", raw_uid) {
                uid_to_id.insert(raw_uid.trim().to_string(), id.clone());
                let name = raw_name.trim();
                if !name.is_empty() {
                    name_to_id.insert(name.to_ascii_lowercase(), id);
                }
            }
        }
        for (raw_uid, tags) in &cache_dir.ids.account_tags {
            if let Some(id) = normalize_account_id("steam", raw_uid) {
                tags_by_id.insert(id, tags.clone());
            }
        }
    }

    let mut raw_notes: BTreeMap<String, String> = BTreeMap::new();
    for (raw_uid, text) in &scanned.notes {
        if let Some(id) = normalize_account_id("steam", raw_uid) {
            raw_notes.insert(id, text.clone());
        }
    }

    let mut all_ids: HashSet<String> = raw_notes.keys().cloned().collect();
    all_ids.extend(tags_by_id.keys().cloned());
    let mut notes = BTreeMap::new();
    for id in all_ids {
        let base = raw_notes.get(&id).map(String::as_str);
        let tags = tags_by_id.get(&id).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(composed) = compose_note(base, tags) {
            notes.insert(id, composed);
        }
    }

    let order = resolve_order(&raw_orders, &uid_to_id, &name_to_id, "steam");

    PlannedPlatform {
        platform_id: "steam",
        accounts: Vec::new(),
        notes,
        order,
        eglv2_skipped: 0,
        vault_detected: scanned.vault_detected,
    }
}

enum EpicAccountOutcome {
    Planned(PlannedAccount),
    Eglv2Skipped,
    Invalid,
}

fn map_epic_account(
    cache_dir: &ScannedCacheDir,
    raw_uid: &str,
    label: &str,
    warnings: &mut Vec<TcNoWarning>,
) -> EpicAccountOutcome {
    let folder_kind = safe_account_dir(&cache_dir.dir, label).map(|dir| sniff_epic_folder(&dir));

    if matches!(folder_kind, Some(EpicFolderKind::Eglv2)) {
        warnings.push(TcNoWarning {
            platform: "epic".to_string(),
            kind: TcNoWarningKind::EpicEglv2Skipped,
            detail: format!(
                "Skipped Epic account '{}' (new Epic launcher eglv2 format is not supported).",
                display_or_id(label, raw_uid)
            ),
        });
        return EpicAccountOutcome::Eglv2Skipped;
    }

    let Some(accshift_id) = normalize_account_id("epic", raw_uid) else {
        warnings.push(TcNoWarning {
            platform: "epic".to_string(),
            kind: TcNoWarningKind::InvalidAccountId,
            detail: format!(
                "Skipped Epic account '{}' (id is not a 32-character legacy Epic AccountId).",
                display_or_id(label, raw_uid)
            ),
        });
        return EpicAccountOutcome::Invalid;
    };

    let is_vaulted = cache_dir
        .vaulted_uids
        .iter()
        .any(|v| v.eq_ignore_ascii_case(raw_uid));

    let (snapshot, needs_capture) = if is_vaulted {
        (None, true)
    } else {
        match folder_kind {
            Some(EpicFolderKind::Legacy {
                ini_path,
                account_id,
            }) if account_id.eq_ignore_ascii_case(&accshift_id) => (
                Some(PlannedSnapshot::EpicLegacy {
                    ini_path,
                    account_id,
                }),
                false,
            ),
            Some(EpicFolderKind::Legacy { .. }) | Some(EpicFolderKind::Incomplete) => {
                warnings.push(TcNoWarning {
                    platform: "epic".to_string(),
                    kind: TcNoWarningKind::SnapshotIncomplete,
                    detail: format!(
                        "Epic account '{}' is missing GameUserSettings.ini or matching reg.json AccountId; imported as to capture.",
                        display_or_id(label, &accshift_id)
                    ),
                });
                (None, true)
            }
            Some(EpicFolderKind::Eglv2) => unreachable!("handled above"),
            None => (None, true),
        }
    };

    let last_used_at = cache_dir
        .ids
        .last_used
        .get(raw_uid)
        .and_then(parse_timestamp_ms);

    EpicAccountOutcome::Planned(PlannedAccount {
        platform_id: "epic",
        tcno_uid: raw_uid.to_string(),
        accshift_id,
        label: label.to_string(),
        last_used_at,
        source_version: cache_dir.ids.version,
        snapshot,
        needs_capture,
    })
}

/// Warning for an account whose TcNo id cannot name an accshift account.
///
/// Jagex ids are random markers TcNo drops in the launcher folder, while
/// accshift mints its own id at setup, so an imported Jagex account could
/// never receive a session. Classic TcNo used the same kind of marker for
/// Discord; only v4's Discord user ids match what accshift reads.
fn skipped_id_warning(platform_id: &str, label: &str, uid: &str) -> TcNoWarning {
    let not_portable = platform_id == "jagex" || platform_id == "discord";
    let (kind, reason) = if not_portable {
        (
            TcNoWarningKind::IdentityNotPortable,
            "its TcNo id is not one accshift can match to a sign-in; add it in accshift instead",
        )
    } else {
        (
            TcNoWarningKind::InvalidAccountId,
            "its TcNo id is not a valid account id for this platform",
        )
    };
    TcNoWarning {
        platform: platform_id.to_string(),
        kind,
        detail: format!("Skipped account '{}': {reason}.", display_or_id(label, uid)),
    }
}

fn display_or_id<'a>(label: &'a str, id: &'a str) -> &'a str {
    if label.is_empty() {
        id
    } else {
        label
    }
}

/// Validate and normalize a TcNo unique id into the format accshift expects
/// for `platform_id`. Returns `None` when the id is invalid.
pub(super) fn normalize_account_id(platform_id: &str, raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        return None;
    }
    match platform_id {
        "steam" => ((15..=20).contains(&trimmed.len())
            && trimmed.bytes().all(|b| b.is_ascii_digit()))
        .then(|| trimmed.to_string()),
        "battle-net" => (trimmed.len() <= 320).then(|| trimmed.to_string()),
        "ubisoft" => Uuid::parse_str(trimmed)
            .ok()
            .map(|u| u.hyphenated().to_string()),
        "epic" => (trimmed.len() == 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| trimmed.to_ascii_lowercase()),
        "riot" => Some(deterministic_riot_id(trimmed)),
        "roblox" | "gog" => (trimmed.len() <= 32 && trimmed.bytes().all(|b| b.is_ascii_digit()))
            .then(|| trimmed.to_string()),
        // A Discord user id (snowflake), which is what accshift's Discord
        // identity reads. Classic TcNo's random marker ids are refused.
        "discord" => ((15..=20).contains(&trimmed.len())
            && trimmed.bytes().all(|b| b.is_ascii_digit()))
        .then(|| trimmed.to_string()),
        // Jagex accounts get a synthetic id at setup: nothing TcNo kept maps
        // to one.
        _ => None,
    }
}

/// Stable accshift Riot profile id tagged with its TcNo origin so re-runs
/// deduplicate cleanly. Matches `riot::paths::is_valid_profile_id` (ASCII
/// alphanumeric, `-`, `_`, max 128 chars).
fn deterministic_riot_id(tcno_uid: &str) -> String {
    let clean: String = tcno_uid
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if !clean.is_empty() && clean.len() == tcno_uid.len() {
        format!("tcno-{}", clean.to_ascii_lowercase())
    } else {
        format!("tcno-{}", short_sha256_hex(tcno_uid))
    }
}

fn short_sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut hex = String::with_capacity(24);
    for byte in &digest[..12] {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn resolve_order(
    raw_order: &[String],
    uid_to_accshift: &HashMap<String, String>,
    name_to_accshift: &HashMap<String, String>,
    platform_id: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in raw_order {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resolved = uid_to_accshift
            .get(trimmed)
            .or_else(|| uid_to_accshift.get(&trimmed.to_ascii_lowercase()))
            .or_else(|| name_to_accshift.get(&trimmed.to_ascii_lowercase()))
            .cloned()
            .or_else(|| {
                if platform_id == "steam" {
                    normalize_account_id("steam", trimmed)
                } else {
                    None
                }
            });
        if let Some(id) = resolved {
            if seen.insert(id.to_ascii_lowercase()) {
                out.push(id);
            }
        }
    }
    out
}

/// Sanitize a note identically to `accshift_app::stores::sanitize_note`,
/// folding optional v4 tags onto the end.
pub(super) fn compose_note(base: Option<&str>, tags: &[String]) -> Option<String> {
    let clean_base = base.map(clean_inline_text).unwrap_or_default();
    let mut clean_tags: Vec<String> = Vec::new();
    let mut seen_tags: HashSet<String> = HashSet::new();
    for tag in tags {
        let t = clean_inline_text(tag);
        if !t.is_empty() && seen_tags.insert(t.to_ascii_lowercase()) {
            clean_tags.push(t);
        }
    }
    let combined = match (clean_base.is_empty(), clean_tags.is_empty()) {
        (true, true) => return None,
        (false, true) => clean_base,
        (true, false) => format!("[{}]", clean_tags.join(", ")),
        (false, false) => format!("{clean_base} [{}]", clean_tags.join(", ")),
    };
    let clipped = utf16_prefix(&combined, MAX_NOTE_UTF16_LEN);
    (!clipped.is_empty()).then_some(clipped)
}

fn clean_inline_text(value: &str) -> String {
    let spaced: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.trim().to_string()
}

fn utf16_prefix(text: &str, max_units: usize) -> String {
    let mut units = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let next = ch.len_utf16();
        if units + next > max_units {
            break;
        }
        units += next;
        out.push(ch);
    }
    out.trim_end().to_string()
}

/// Parse a TcNo v4 `lastused` value (Unix seconds/ms or RFC3339 date-time)
/// into Unix milliseconds.
pub(super) fn parse_timestamp_ms(value: &serde_json::Value) -> Option<u64> {
    if let Some(n) = value.as_u64() {
        if n == 0 {
            return None;
        }
        return Some(if n < 10_000_000_000 { n * 1000 } else { n });
    }
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(n) = raw.parse::<u64>() {
        return (n > 0).then(|| if n < 10_000_000_000 { n * 1000 } else { n });
    }
    parse_rfc3339_ms(raw)
}

/// Minimal RFC3339 parser (`YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` -> Unix ms).
fn parse_rfc3339_ms(raw: &str) -> Option<u64> {
    let bytes = raw.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let year: i64 = raw.get(0..4)?.parse().ok()?;
    if bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't' | b' ') {
        return None;
    }
    let month: u32 = raw.get(5..7)?.parse().ok()?;
    let day: u32 = raw.get(8..10)?.parse().ok()?;
    let hour: i64 = raw.get(11..13)?.parse().ok()?;
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let minute: i64 = raw.get(14..16)?.parse().ok()?;
    let second: i64 = raw.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let mut rest = &raw[19..];
    if let Some(after_dot) = rest.strip_prefix('.') {
        let digits = after_dot.bytes().take_while(|b| b.is_ascii_digit()).count();
        rest = &after_dot[digits..];
    }
    let offset_secs: i64 = if rest.eq_ignore_ascii_case("Z") {
        0
    } else if rest.len() == 6
        && matches!(rest.as_bytes()[0], b'+' | b'-')
        && rest.as_bytes()[3] == b':'
    {
        let sign = if rest.as_bytes()[0] == b'+' { 1 } else { -1 };
        let oh: i64 = rest.get(1..3)?.parse().ok()?;
        let om: i64 = rest.get(4..6)?.parse().ok()?;
        sign * (oh * 3600 + om * 60)
    } else {
        return None;
    };

    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset_secs;
    u64::try_from(secs).ok()?.checked_mul(1000)
}

/// Howard Hinnant's civil-to-days algorithm (days since 1970-01-01).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
