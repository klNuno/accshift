//! Deduplication, preview computation, origin tagging and atomic writes into
//! accshift's config, Roblox store, client stores and encrypted snapshots.

use super::mapper::{ImportPlan, PlannedAccount, PlannedPlatform, PlannedSnapshot};
use super::{
    TcNoFormatVersion, TcNoImportReport, TcNoPlatformPreview, TcNoPreview, TcNoWarning,
    TcNoWarningKind,
};
use crate::config::{
    self, AppConfig, BattleNetAccountConfig, RiotProfileConfig, RobloxAccountConfig,
    SimpleAccountConfig, UbisoftAccountConfig,
};
use crate::platforms::now_unix_ms;
use crate::snapshot_crypto;
use crate::storage::{self, STORE_ACCOUNT_CARD_NOTES, STORE_FOLDERS};
use crate::AppContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;

/// One imported item recorded in `state/tcno-import.json` so every item stays
/// tagged with its TcNo origin and re-runs deduplicate cleanly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TcNoOriginRecord {
    pub platform_id: String,
    pub tcno_uid: String,
    pub accshift_id: String,
    pub source_version: TcNoFormatVersion,
    pub imported_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OriginLedger {
    version: u32,
    /// Keyed by `<platform_id>:<tcno_uid_lowercase>`.
    #[serde(default)]
    items: BTreeMap<String, TcNoOriginRecord>,
}

/// Snapshot of accshift's current state needed to preview or deduplicate a
/// TcNo import without modifying anything.
struct CurrentState {
    cfg: AppConfig,
    roblox_accounts: Vec<RobloxAccountConfig>,
    notes: Map<String, Value>,
    folders: Value,
    ubisoft_snapshots: Option<std::path::PathBuf>,
    epic_snapshots: Option<std::path::PathBuf>,
    origins: OriginLedger,
}

impl CurrentState {
    fn load(app: &dyn AppContext) -> Self {
        let cfg = config::load_config(app);
        let roblox_accounts = load_roblox_accounts(app, &cfg);
        let notes = read_notes_map(app);
        let folders = read_folders_value(app);
        let ubisoft_snapshots = storage::platform_snapshots_dir(app, "ubisoft").ok();
        let epic_snapshots = storage::platform_snapshots_dir(app, "epic").ok();
        let origins = load_origin_ledger(app);
        Self {
            cfg,
            roblox_accounts,
            notes,
            folders,
            ubisoft_snapshots,
            epic_snapshots,
            origins,
        }
    }

    fn account_exists(&self, acc: &PlannedAccount) -> bool {
        let id = self.resolved_accshift_id(acc);
        match acc.platform_id {
            "battle-net" => self
                .cfg
                .battle_net
                .accounts
                .iter()
                .any(|a| a.email.trim().eq_ignore_ascii_case(&id)),
            "ubisoft" => self
                .cfg
                .ubisoft
                .accounts
                .iter()
                .any(|a| a.uuid.trim().eq_ignore_ascii_case(&id)),
            "epic" => self
                .cfg
                .epic
                .accounts
                .iter()
                .any(|a| a.account_id.trim().eq_ignore_ascii_case(&id)),
            "riot" => self.cfg.riot.profiles.iter().any(|p| p.id == id),
            "roblox" => self.roblox_accounts.iter().any(|a| a.user_id.trim() == id),
            "gog" => self
                .cfg
                .gog
                .accounts
                .iter()
                .any(|a| a.account_id.trim().eq_ignore_ascii_case(&id)),
            "jagex" => self
                .cfg
                .jagex
                .accounts
                .iter()
                .any(|a| a.account_id.trim().eq_ignore_ascii_case(&id)),
            "discord" => self
                .cfg
                .discord
                .accounts
                .iter()
                .any(|a| a.account_id.trim().eq_ignore_ascii_case(&id)),
            _ => false,
        }
    }

    fn resolved_accshift_id(&self, acc: &PlannedAccount) -> String {
        let key = origin_key(acc.platform_id, &acc.tcno_uid);
        self.origins
            .items
            .get(&key)
            .map(|r| r.accshift_id.clone())
            .unwrap_or_else(|| acc.accshift_id.clone())
    }

    fn canonical_note_key(&self, platform_id: &str, id: &str) -> String {
        // Battle.net emails may already exist with different casing in config.
        if platform_id == "battle-net" {
            if let Some(existing) = self
                .cfg
                .battle_net
                .accounts
                .iter()
                .find(|a| a.email.trim().eq_ignore_ascii_case(id))
            {
                return existing.email.trim().to_string();
            }
        }
        id.to_string()
    }

    fn note_exists(&self, key: &str) -> bool {
        self.notes
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    }

    fn snapshot_exists(&self, acc: &PlannedAccount) -> bool {
        let id = self.resolved_accshift_id(acc);
        match acc.platform_id {
            "ubisoft" => self.ubisoft_snapshots.as_deref().is_some_and(|root| {
                let dir = root.join(&id);
                dir.join("user.dat").exists() || dir.join("ConnectSecureStorage.dat").exists()
            }),
            "epic" => self
                .epic_snapshots
                .as_deref()
                .is_some_and(|root| root.join(&id).join("GameUserSettings.ini").exists()),
            _ => false,
        }
    }
}

/// Compute a read-only preview of `plan` against the current state of `app`.
pub(super) fn preview_plan(app: &dyn AppContext, plan: &ImportPlan) -> TcNoPreview {
    let state = CurrentState::load(app);
    let mut preview = TcNoPreview {
        path: plan.path.clone(),
        version: plan.version,
        warnings: plan.warnings.clone(),
        ..TcNoPreview::default()
    };

    for platform in &plan.platforms {
        let stats = compute_platform_stats(&state, platform);
        preview.accounts_new += stats.accounts_new;
        preview.accounts_existing += stats.accounts_existing;
        preview.notes_new += stats.notes_new;
        preview.notes_existing += stats.notes_existing;
        preview.snapshots_new += stats.snapshots_new;
        preview.to_capture += stats.to_capture;
        preview.eglv2_skipped += stats.eglv2_skipped;
        preview.vault_detected |= stats.vault_detected;
        preview.platforms.push(stats);
    }

    preview
}

fn compute_platform_stats(state: &CurrentState, platform: &PlannedPlatform) -> TcNoPlatformPreview {
    let mut stats = TcNoPlatformPreview {
        platform_id: platform.platform_id.to_string(),
        eglv2_skipped: platform.eglv2_skipped,
        vault_detected: platform.vault_detected,
        ..TcNoPlatformPreview::default()
    };

    for acc in &platform.accounts {
        let exists = state.account_exists(acc);
        if exists {
            stats.accounts_existing += 1;
        } else {
            stats.accounts_new += 1;
        }
        let has_snap = state.snapshot_exists(acc);
        if acc.snapshot.is_some() && !has_snap {
            stats.snapshots_new += 1;
        } else if !exists && acc.needs_capture && !has_snap {
            stats.to_capture += 1;
        }
    }

    for id in platform.notes.keys() {
        let note_key = state.canonical_note_key(platform.platform_id, id);
        if state.note_exists(&note_key) {
            stats.notes_existing += 1;
        } else {
            stats.notes_new += 1;
        }
    }

    stats
}

/// Apply `plan` into `app`'s config, Roblox accounts, client stores,
/// Ubisoft/Epic encrypted snapshots and origin ledger.
pub(super) fn execute_plan(
    app: &dyn AppContext,
    plan: &ImportPlan,
) -> Result<TcNoImportReport, String> {
    let state = CurrentState::load(app);
    let now = now_unix_ms();

    let mut report = TcNoImportReport {
        path: plan.path.clone(),
        version: plan.version,
        warnings: plan.warnings.clone(),
        ..TcNoImportReport::default()
    };

    let mut notes_map = state.notes.clone();
    let mut notes_dirty = false;
    let mut folders_val = state.folders.clone();
    let mut folders_dirty = false;
    let mut origins = state.origins.clone();
    origins.version = 1;
    let mut origins_dirty = false;

    let mut roblox_accounts = state.roblox_accounts.clone();
    let mut roblox_dirty = false;

    let mut config_accounts: Vec<PlannedAccount> = Vec::new();
    let mut snapshots_to_write: Vec<(PlannedAccount, PlannedSnapshot)> = Vec::new();

    for platform in &plan.platforms {
        let stats = compute_platform_stats(&state, platform);
        report.accounts_imported += stats.accounts_new;
        report.accounts_skipped += stats.accounts_existing;
        report.notes_imported += stats.notes_new;
        report.notes_skipped += stats.notes_existing;
        report.to_capture += stats.to_capture;
        report.eglv2_skipped += stats.eglv2_skipped;
        report.vault_detected |= stats.vault_detected;
        report.platforms.push(stats);

        for acc in &platform.accounts {
            let resolved_id = state.resolved_accshift_id(acc);
            let mut acc_resolved = acc.clone();
            acc_resolved.accshift_id = resolved_id.clone();

            if acc.platform_id == "roblox" {
                if !roblox_accounts
                    .iter()
                    .any(|a| a.user_id.trim() == resolved_id)
                {
                    let display = if acc.label.is_empty() {
                        resolved_id.clone()
                    } else {
                        acc.label.clone()
                    };
                    roblox_accounts.push(RobloxAccountConfig {
                        user_id: resolved_id.clone(),
                        username: display.clone(),
                        display_name: display,
                        cookie_encrypted: String::new(),
                        last_used_at: acc.last_used_at,
                    });
                    roblox_dirty = true;
                }
            } else {
                config_accounts.push(acc_resolved.clone());
            }

            if let Some(snap) = acc.snapshot.clone() {
                if !state.snapshot_exists(&acc_resolved) {
                    snapshots_to_write.push((acc_resolved.clone(), snap));
                }
            }

            let key = origin_key(acc.platform_id, &acc.tcno_uid);
            origins.items.entry(key).or_insert_with(|| {
                origins_dirty = true;
                TcNoOriginRecord {
                    platform_id: acc.platform_id.to_string(),
                    tcno_uid: acc.tcno_uid.clone(),
                    accshift_id: resolved_id,
                    source_version: acc.source_version,
                    imported_at: now,
                }
            });
        }

        for (id, note_text) in &platform.notes {
            let key = state.canonical_note_key(platform.platform_id, id);
            if !state.note_exists(&key) {
                notes_map.insert(key, Value::String(note_text.clone()));
                notes_dirty = true;
            }
        }

        if !platform.order.is_empty() {
            let canonical_order: Vec<String> = platform
                .order
                .iter()
                .map(|id| state.canonical_note_key(platform.platform_id, id))
                .collect();
            let added =
                merge_platform_order(&mut folders_val, platform.platform_id, &canonical_order);
            if added > 0 {
                report.order_entries_added += added;
                folders_dirty = true;
            }
        }
    }

    if !config_accounts.is_empty() {
        config::update_config(app, |cfg| {
            for acc in &config_accounts {
                upsert_config_account(cfg, acc);
            }
        })?;
    }

    if roblox_dirty {
        let path = storage::roblox_accounts_path(app)?;
        storage::write_json_atomic(&path, &roblox_accounts)?;
        let clone = roblox_accounts.clone();
        config::update_config(app, |cfg| {
            cfg.roblox.accounts = clone;
        })?;
    }

    for (acc, snap) in snapshots_to_write {
        match write_snapshot(app, &acc, &snap) {
            Ok(()) => report.snapshots_imported += 1,
            // The account is already in the config: it stays, to capture on
            // its first sign-in, rather than failing the whole import.
            Err(error) => {
                report.to_capture += 1;
                report.warnings.push(TcNoWarning {
                    platform: acc.platform_id.to_string(),
                    kind: TcNoWarningKind::SnapshotFailed,
                    detail: format!(
                        "Could not import the saved session of '{}': {error}",
                        if acc.label.is_empty() {
                            &acc.accshift_id
                        } else {
                            &acc.label
                        }
                    ),
                });
            }
        }
    }

    if notes_dirty {
        storage::save_client_store(app, STORE_ACCOUNT_CARD_NOTES, &Value::Object(notes_map))?;
    }

    if folders_dirty {
        storage::save_client_store(app, STORE_FOLDERS, &folders_val)?;
    }

    if origins_dirty {
        save_origin_ledger(app, &origins)?;
    }

    Ok(report)
}

fn upsert_config_account(cfg: &mut AppConfig, acc: &PlannedAccount) {
    match acc.platform_id {
        "battle-net" => {
            if let Some(existing) = cfg
                .battle_net
                .accounts
                .iter_mut()
                .find(|a| a.email.trim().eq_ignore_ascii_case(&acc.accshift_id))
            {
                if existing.battle_tag.trim().is_empty()
                    && !acc.label.is_empty()
                    && !acc.label.eq_ignore_ascii_case(&acc.accshift_id)
                {
                    existing.battle_tag = acc.label.clone();
                }
            } else {
                let battle_tag =
                    if !acc.label.is_empty() && !acc.label.eq_ignore_ascii_case(&acc.accshift_id) {
                        acc.label.clone()
                    } else {
                        String::new()
                    };
                cfg.battle_net.accounts.push(BattleNetAccountConfig {
                    email: acc.accshift_id.clone(),
                    battle_tag,
                    last_used_at: acc.last_used_at,
                });
            }
        }
        "ubisoft" => {
            if let Some(existing) = cfg
                .ubisoft
                .accounts
                .iter_mut()
                .find(|a| a.uuid.trim().eq_ignore_ascii_case(&acc.accshift_id))
            {
                if existing.label.trim().is_empty() && !acc.label.is_empty() {
                    existing.label = acc.label.clone();
                }
            } else {
                cfg.ubisoft.accounts.push(UbisoftAccountConfig {
                    uuid: acc.accshift_id.clone(),
                    label: acc.label.clone(),
                    last_used_at: acc.last_used_at,
                });
                cfg.ubisoft
                    .forgotten_uuids
                    .retain(|u| !u.trim().eq_ignore_ascii_case(&acc.accshift_id));
            }
        }
        "epic" => upsert_simple(&mut cfg.epic.accounts, acc),
        "gog" => upsert_simple(&mut cfg.gog.accounts, acc),
        "jagex" => upsert_simple(&mut cfg.jagex.accounts, acc),
        "discord" => upsert_simple(&mut cfg.discord.accounts, acc),
        "riot" => {
            if let Some(existing) = cfg
                .riot
                .profiles
                .iter_mut()
                .find(|p| p.id == acc.accshift_id)
            {
                if existing.label.trim().is_empty() && !acc.label.is_empty() {
                    existing.label = acc.label.clone();
                }
            } else {
                let label = if acc.label.is_empty() {
                    acc.tcno_uid.clone()
                } else {
                    acc.label.clone()
                };
                cfg.riot.profiles.push(RiotProfileConfig {
                    id: acc.accshift_id.clone(),
                    label,
                    account_name: String::new(),
                    account_tag_line: String::new(),
                    account_puuid: String::new(),
                    snapshot_state: "awaiting_capture".to_string(),
                    notes: String::new(),
                    last_captured_at: None,
                    last_used_at: acc.last_used_at,
                });
            }
        }
        _ => {}
    }
}

fn upsert_simple(accounts: &mut Vec<SimpleAccountConfig>, acc: &PlannedAccount) {
    if let Some(existing) = accounts
        .iter_mut()
        .find(|a| a.account_id.trim().eq_ignore_ascii_case(&acc.accshift_id))
    {
        if existing.label.trim().is_empty() && !acc.label.is_empty() {
            existing.label = acc.label.clone();
        }
    } else {
        accounts.push(SimpleAccountConfig {
            account_id: acc.accshift_id.clone(),
            label: acc.label.clone(),
            last_used_at: acc.last_used_at,
        });
    }
}

/// Write an encrypted snapshot into the descriptor engine's snapshot directory
/// for Ubisoft (`user.dat` + `ConnectSecureStorage.dat`) or Epic legacy
/// (`GameUserSettings.ini` + `registry_account_id.txt`).
pub(super) fn write_snapshot(
    app: &dyn AppContext,
    acc: &PlannedAccount,
    snap: &PlannedSnapshot,
) -> Result<(), String> {
    let dir = storage::platform_snapshots_dir(app, acc.platform_id)?.join(&acc.accshift_id);
    let existed = dir.exists();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create snapshot dir {}: {e}", dir.display()))?;
    let written = write_snapshot_files(&dir, snap);
    if written.is_err() && !existed {
        // Half a credential set would restore as a broken session: a folder
        // this import created goes with the failure.
        let _ = fs::remove_dir_all(&dir);
    }
    written
}

fn write_snapshot_files(dir: &std::path::Path, snap: &PlannedSnapshot) -> Result<(), String> {
    match snap {
        PlannedSnapshot::Ubisoft {
            user_dat,
            connect_secure_storage,
        } => {
            snapshot_crypto::replace_encrypted_file(user_dat, &dir.join("user.dat"))?;
            snapshot_crypto::replace_encrypted_file(
                connect_secure_storage,
                &dir.join("ConnectSecureStorage.dat"),
            )?;
        }
        PlannedSnapshot::EpicLegacy {
            ini_path,
            account_id,
        } => {
            snapshot_crypto::replace_encrypted_file(ini_path, &dir.join("GameUserSettings.ini"))?;
            snapshot_crypto::replace_encrypted_bytes(
                &dir.join("registry_account_id.txt"),
                account_id.as_bytes(),
            )?;
        }
    }
    Ok(())
}

/// Merge `ordered_ids` into `folders.json`'s `itemOrder["root:<platform>"]`
/// without moving accounts already placed in folders or duplicating existing
/// entries. Returns how many entries were appended.
fn merge_platform_order(
    folders_val: &mut Value,
    platform_id: &str,
    ordered_ids: &[String],
) -> usize {
    if !folders_val.is_object() {
        *folders_val = json!({
            "version": 1,
            "folders": [],
            "itemOrder": {}
        });
    }
    let root_obj = folders_val.as_object_mut().expect("object ensured");
    if !root_obj.contains_key("version") {
        root_obj.insert("version".into(), json!(1));
    }
    if !root_obj.get("folders").is_some_and(Value::is_array) {
        root_obj.insert("folders".into(), json!([]));
    }
    if !root_obj.get("itemOrder").is_some_and(Value::is_object) {
        root_obj.insert("itemOrder".into(), json!({}));
    }

    let root_key = format!("root:{platform_id}");
    let platform_folder_ids: HashSet<String> = root_obj
        .get("folders")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let p = f.get("platform").and_then(Value::as_str)?;
                    let id = f.get("id").and_then(Value::as_str)?;
                    (p == platform_id).then(|| id.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    let item_order = root_obj
        .get_mut("itemOrder")
        .and_then(Value::as_object_mut)
        .expect("itemOrder object ensured");

    let mut already_placed: HashSet<String> = HashSet::new();
    for (bucket_key, entries) in item_order.iter() {
        if bucket_key != &root_key && !platform_folder_ids.contains(bucket_key) {
            continue;
        }
        if let Some(arr) = entries.as_array() {
            for item in arr {
                if item.get("type").and_then(Value::as_str) == Some("account") {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        already_placed.insert(id.to_ascii_lowercase());
                    }
                }
            }
        }
    }

    let bucket = item_order
        .entry(root_key)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !bucket.is_array() {
        *bucket = Value::Array(Vec::new());
    }
    let arr = bucket.as_array_mut().expect("array ensured");
    let mut added = 0usize;
    for id in ordered_ids {
        if already_placed.insert(id.to_ascii_lowercase()) {
            arr.push(json!({ "type": "account", "id": id }));
            added += 1;
        }
    }
    added
}

fn load_roblox_accounts(app: &dyn AppContext, cfg: &AppConfig) -> Vec<RobloxAccountConfig> {
    if let Ok(path) = storage::roblox_accounts_path(app) {
        if let Ok(Some(accounts)) = storage::read_json_if_exists::<Vec<RobloxAccountConfig>>(&path)
        {
            return accounts;
        }
    }
    cfg.roblox.accounts.clone()
}

fn read_notes_map(app: &dyn AppContext) -> Map<String, Value> {
    let Ok(path) = storage::client_store_path(app, STORE_ACCOUNT_CARD_NOTES) else {
        return Map::new();
    };
    match storage::read_json_if_exists::<Value>(&path) {
        Ok(Some(Value::Object(map))) => map,
        _ => Map::new(),
    }
}

fn read_folders_value(app: &dyn AppContext) -> Value {
    let Ok(path) = storage::client_store_path(app, STORE_FOLDERS) else {
        return Value::Null;
    };
    match storage::read_json_if_exists::<Value>(&path) {
        Ok(Some(val)) => val,
        _ => Value::Null,
    }
}

fn origin_key(platform_id: &str, tcno_uid: &str) -> String {
    format!("{platform_id}:{}", tcno_uid.trim().to_ascii_lowercase())
}

fn load_origin_ledger(app: &dyn AppContext) -> OriginLedger {
    let Ok(path) = storage::tcno_import_origins_path(app) else {
        return OriginLedger::default();
    };
    storage::read_json_if_exists::<OriginLedger>(&path)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn save_origin_ledger(app: &dyn AppContext, ledger: &OriginLedger) -> Result<(), String> {
    let path = storage::tcno_import_origins_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create state dir {}: {e}", parent.display()))?;
    }
    storage::write_json_atomic(&path, ledger)
}

#[cfg(all(test, windows))]
pub(super) fn read_origins_for_test(app: &dyn AppContext) -> BTreeMap<String, TcNoOriginRecord> {
    load_origin_ledger(app).items
}
