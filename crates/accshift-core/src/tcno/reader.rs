//! Read-only discovery and parsing of a TcNo Account Switcher data folder.
//!
//! Handles both classic C# (`ids.json` flat string map, `Settings\<Name>.json`,
//! root `SteamSettings.json`) and v4 Go (`ids.json` object with `ids`,
//! `lastused`, `tags`, `accountTags`; `Settings\<Name>Settings.json`; opt-in
//! `Vault\accounts\<Platform>\*.tcvault`; Epic `eglv2` key collision).

use super::{TcNoFormatVersion, TcNoWarning, TcNoWarningKind};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Hard cap on TcNo JSON metadata files so a damaged or hostile file cannot
/// exhaust memory.
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;

/// Hard cap on an individual session file copied into an encrypted snapshot.
const MAX_SNAPSHOT_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Mapping between an accshift platform id and its TcNo folder/settings names.
#[derive(Clone, Copy)]
pub(super) struct PlatformSpec {
    pub(super) accshift_id: &'static str,
    /// `LoginCache\<name>` directory names to inspect, in priority order.
    pub(super) cache_dirs: &'static [&'static str],
    /// Classic settings file paths relative to TcNo root.
    pub(super) classic_settings: &'static [&'static str],
    /// v4 settings file paths relative to TcNo root.
    pub(super) v4_settings: &'static [&'static str],
}

pub(super) const KNOWN_PLATFORMS: &[PlatformSpec] = &[
    PlatformSpec {
        accshift_id: "steam",
        cache_dirs: &["Steam"],
        classic_settings: &["SteamSettings.json"],
        v4_settings: &["Settings/SteamSettings.json"],
    },
    PlatformSpec {
        accshift_id: "battle-net",
        cache_dirs: &["BattleNet"],
        classic_settings: &["Settings/BattleNet.json"],
        v4_settings: &["Settings/BattleNetSettings.json"],
    },
    PlatformSpec {
        accshift_id: "ubisoft",
        cache_dirs: &["Ubisoft"],
        classic_settings: &["Settings/Ubisoft.json"],
        v4_settings: &["Settings/UbisoftSettings.json"],
    },
    PlatformSpec {
        accshift_id: "epic",
        // Process explicit legacy dir first so a legacy entry wins over an
        // eglv2 entry sharing the `Epic Games` directory name.
        cache_dirs: &["Epic Games (Legacy)", "Epic Games"],
        classic_settings: &["Settings/Epic Games.json"],
        v4_settings: &[
            "Settings/EpicGames(Legacy)Settings.json",
            "Settings/EpicGamesSettings.json",
        ],
    },
    PlatformSpec {
        accshift_id: "riot",
        cache_dirs: &["Riot Games"],
        classic_settings: &["Settings/Riot Games.json"],
        v4_settings: &["Settings/RiotGamesSettings.json"],
    },
    PlatformSpec {
        accshift_id: "roblox",
        cache_dirs: &["Roblox"],
        classic_settings: &["Settings/Roblox.json"],
        v4_settings: &["Settings/RobloxSettings.json"],
    },
    PlatformSpec {
        accshift_id: "gog",
        cache_dirs: &["GOG"],
        classic_settings: &["Settings/GOG.json"],
        v4_settings: &["Settings/GOGSettings.json"],
    },
    PlatformSpec {
        accshift_id: "jagex",
        cache_dirs: &["Jagex"],
        classic_settings: &["Settings/Jagex.json"],
        v4_settings: &["Settings/JagexSettings.json"],
    },
    PlatformSpec {
        accshift_id: "discord",
        cache_dirs: &["Discord"],
        classic_settings: &["Settings/Discord.json"],
        v4_settings: &["Settings/DiscordSettings.json"],
    },
];

/// Parsed `LoginCache\<Platform>\ids.json`.
#[derive(Debug, Default)]
pub(super) struct ParsedIds {
    pub(super) version: TcNoFormatVersion,
    /// Ordered as in the file (`serde_json` uses `preserve_order`).
    pub(super) accounts: Vec<(String, String)>,
    pub(super) last_used: BTreeMap<String, Value>,
    pub(super) account_tags: BTreeMap<String, Vec<String>>,
}

/// Content sniffed inside an Epic account's saved session folder.
#[derive(Debug, Clone)]
pub(super) enum EpicFolderKind {
    Legacy {
        ini_path: PathBuf,
        account_id: String,
    },
    Eglv2,
    Incomplete,
}

/// Content sniffed inside a Ubisoft account's saved session folder.
#[derive(Debug, Clone)]
pub(super) struct UbisoftFolderFiles {
    pub(super) user_dat: PathBuf,
    pub(super) connect_secure_storage: PathBuf,
}

/// One raw platform directory scanned under `LoginCache`.
#[derive(Debug)]
pub(super) struct ScannedCacheDir {
    pub(super) dir: PathBuf,
    pub(super) ids: ParsedIds,
    pub(super) order: Vec<String>,
    pub(super) vaulted_uids: Vec<String>,
}

/// All raw data collected for one accshift platform.
#[derive(Debug)]
pub(super) struct ScannedPlatform {
    pub(super) accshift_id: &'static str,
    pub(super) cache_dirs: Vec<ScannedCacheDir>,
    pub(super) notes: BTreeMap<String, String>,
    pub(super) vault_detected: bool,
}

/// Full read-only scan of a TcNo root folder.
#[derive(Debug)]
pub(super) struct RootScan {
    pub(super) root: PathBuf,
    pub(super) version: TcNoFormatVersion,
    pub(super) platforms: Vec<ScannedPlatform>,
    pub(super) warnings: Vec<TcNoWarning>,
}

/// Scan `root` read-only. Requires `root/LoginCache` to exist as a directory.
pub(super) fn scan_root(root: &Path) -> Result<RootScan, String> {
    if !root.is_dir() {
        return Err(format!(
            "TcNo Account Switcher folder does not exist: {}",
            root.display()
        ));
    }
    let login_cache = root.join("LoginCache");
    if !login_cache.is_dir() {
        return Err(format!(
            "Folder does not look like a TcNo Account Switcher data directory (LoginCache is missing): {}",
            root.display()
        ));
    }

    let mut warnings = Vec::new();
    let mut platforms = Vec::new();
    let mut saw_classic = false;
    let mut saw_v4 = false;

    for spec in KNOWN_PLATFORMS {
        let mut cache_dirs = Vec::new();
        let mut platform_vault = false;

        for &tcno_name in spec.cache_dirs {
            let dir = login_cache.join(tcno_name);
            let vaulted_uids = detect_vault_uids(root, tcno_name);
            let dir_vault = !vaulted_uids.is_empty();
            if dir_vault {
                platform_vault = true;
                saw_v4 = true;
            }
            if !dir.is_dir() {
                continue;
            }

            let ids_path = dir.join("ids.json");
            let ids = if ids_path.exists() {
                match read_ids_file(&ids_path, spec.accshift_id == "steam") {
                    Ok(parsed) => {
                        match parsed.version {
                            TcNoFormatVersion::Classic => saw_classic = true,
                            TcNoFormatVersion::V4 => saw_v4 = true,
                            TcNoFormatVersion::Mixed => {
                                saw_classic = true;
                                saw_v4 = true;
                            }
                            TcNoFormatVersion::Unknown => {}
                        }
                        parsed
                    }
                    Err(err) => {
                        warnings.push(TcNoWarning {
                            platform: spec.accshift_id.to_string(),
                            kind: err.kind,
                            detail: format!("{}: {}", ids_path.display(), err.detail),
                        });
                        // Never guess when ids.json has an unrecognized format:
                        // skip this platform cache dir cleanly.
                        continue;
                    }
                }
            } else {
                ParsedIds::default()
            };

            let order = read_order_file(&dir.join("order.json"), spec.accshift_id, &mut warnings);
            cache_dirs.push(ScannedCacheDir {
                dir,
                ids,
                order,
                vaulted_uids,
            });
        }

        let (notes, notes_classic, notes_v4) = read_platform_notes(root, spec, &mut warnings);
        if notes_classic {
            saw_classic = true;
        }
        if notes_v4 {
            saw_v4 = true;
        }

        if platform_vault {
            warnings.push(TcNoWarning {
                platform: spec.accshift_id.to_string(),
                kind: TcNoWarningKind::VaultEncrypted,
                detail: "TcNo vault encryption is enabled; turn off encryption in TcNo and import again to include saved sessions.".to_string(),
            });
        }

        if !cache_dirs.is_empty() || !notes.is_empty() || platform_vault {
            platforms.push(ScannedPlatform {
                accshift_id: spec.accshift_id,
                cache_dirs,
                notes,
                vault_detected: platform_vault,
            });
        }
    }

    let version = match (saw_classic, saw_v4) {
        (true, true) => TcNoFormatVersion::Mixed,
        (true, false) => TcNoFormatVersion::Classic,
        (false, true) => TcNoFormatVersion::V4,
        (false, false) => TcNoFormatVersion::Unknown,
    };

    Ok(RootScan {
        root: root.to_path_buf(),
        version,
        platforms,
        warnings,
    })
}

struct IdsParseError {
    kind: TcNoWarningKind,
    detail: String,
}

fn read_json_value(path: &Path) -> Result<Value, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("Could not stat file: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("Expected a regular file".to_string());
    }
    if meta.len() > MAX_JSON_BYTES {
        return Err(format!("File exceeds {} bytes", MAX_JSON_BYTES));
    }
    let raw = fs::read_to_string(path).map_err(|e| format!("Could not read file: {e}"))?;
    let trimmed = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    serde_json::from_str(trimmed).map_err(|e| format!("Invalid JSON: {e}"))
}

fn read_ids_file(path: &Path, is_steam: bool) -> Result<ParsedIds, IdsParseError> {
    let value = read_json_value(path).map_err(|detail| IdsParseError {
        kind: TcNoWarningKind::CorruptFile,
        detail,
    })?;
    let Value::Object(map) = value else {
        return Err(IdsParseError {
            kind: TcNoWarningKind::UnsupportedIdsFormat,
            detail: "Expected a JSON object in ids.json".to_string(),
        });
    };
    if map.is_empty() {
        return Ok(ParsedIds::default());
    }

    if let Some(ids_val) = map.get("ids") {
        return parse_v4_ids(&map, ids_val);
    }

    if map.values().all(Value::is_string) {
        let accounts = map
            .into_iter()
            .filter_map(|(uid, name)| match name {
                Value::String(name) => Some((uid, name)),
                _ => None,
            })
            .collect();
        return Ok(ParsedIds {
            version: TcNoFormatVersion::Classic,
            accounts,
            last_used: BTreeMap::new(),
            account_tags: BTreeMap::new(),
        });
    }

    // Steam v4 ids.json may hold tags/accountTags without an `ids` map.
    if is_steam && (map.contains_key("accountTags") || map.contains_key("tags")) {
        return parse_v4_ids(&map, &Value::Null);
    }

    Err(IdsParseError {
        kind: TcNoWarningKind::UnsupportedIdsFormat,
        detail: "Unrecognized ids.json layout (neither classic flat map nor v4 object)".to_string(),
    })
}

fn parse_v4_ids(map: &Map<String, Value>, ids_val: &Value) -> Result<ParsedIds, IdsParseError> {
    let accounts = match ids_val {
        Value::Null => Vec::new(),
        Value::Object(ids_obj) => {
            if !ids_obj.values().all(Value::is_string) {
                return Err(IdsParseError {
                    kind: TcNoWarningKind::UnsupportedIdsFormat,
                    detail: "v4 ids map contains non-string account names".to_string(),
                });
            }
            ids_obj
                .iter()
                .filter_map(|(uid, name)| name.as_str().map(|s| (uid.clone(), s.to_string())))
                .collect()
        }
        _ => {
            return Err(IdsParseError {
                kind: TcNoWarningKind::UnsupportedIdsFormat,
                detail: "v4 ids field is not an object".to_string(),
            });
        }
    };

    let mut last_used = BTreeMap::new();
    if let Some(Value::Object(lu)) = map.get("lastused") {
        for (uid, ts) in lu {
            last_used.insert(uid.clone(), ts.clone());
        }
    }

    // Resolve tag ids to human names when a `tags` dictionary is present.
    let mut tag_names: BTreeMap<String, String> = BTreeMap::new();
    if let Some(Value::Object(tags_obj)) = map.get("tags") {
        for (tag_id, entry) in tags_obj {
            let label = match entry {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Object(obj) => obj
                    .get("name")
                    .or_else(|| obj.get("Name"))
                    .or_else(|| obj.get("label"))
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_string()),
                _ => None,
            };
            if let Some(label) = label.filter(|s| !s.is_empty()) {
                tag_names.insert(tag_id.clone(), label);
            }
        }
    }

    let mut account_tags = BTreeMap::new();
    if let Some(Value::Object(acc_tags)) = map.get("accountTags") {
        for (uid, raw_list) in acc_tags {
            let Value::Array(items) = raw_list else {
                continue;
            };
            let resolved: Vec<String> = items
                .iter()
                .filter_map(Value::as_str)
                .map(|raw| {
                    tag_names
                        .get(raw)
                        .cloned()
                        .unwrap_or_else(|| raw.trim().to_string())
                })
                .filter(|s| !s.is_empty())
                .collect();
            if !resolved.is_empty() {
                account_tags.insert(uid.clone(), resolved);
            }
        }
    }

    Ok(ParsedIds {
        version: TcNoFormatVersion::V4,
        accounts,
        last_used,
        account_tags,
    })
}

fn read_order_file(path: &Path, platform: &str, warnings: &mut Vec<TcNoWarning>) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    match read_json_value(path) {
        Ok(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) if !s.trim().is_empty() => Some(s),
                _ => None,
            })
            .collect(),
        Ok(_) => {
            warnings.push(TcNoWarning {
                platform: platform.to_string(),
                kind: TcNoWarningKind::CorruptFile,
                detail: format!("{}: expected JSON array", path.display()),
            });
            Vec::new()
        }
        Err(detail) => {
            warnings.push(TcNoWarning {
                platform: platform.to_string(),
                kind: TcNoWarningKind::CorruptFile,
                detail: format!("{}: {detail}", path.display()),
            });
            Vec::new()
        }
    }
}

fn read_platform_notes(
    root: &Path,
    spec: &PlatformSpec,
    warnings: &mut Vec<TcNoWarning>,
) -> (BTreeMap<String, String>, bool, bool) {
    let mut notes = BTreeMap::new();
    let mut saw_classic = false;
    let mut saw_v4 = false;

    for rel in spec.classic_settings {
        let path = root.join(rel);
        if path.exists() {
            saw_classic = true;
            merge_notes_file(&path, spec.accshift_id, &mut notes, warnings);
        }
    }
    // v4 settings override classic notes when both exist.
    for rel in spec.v4_settings {
        let path = root.join(rel);
        if path.exists() {
            saw_v4 = true;
            merge_notes_file(&path, spec.accshift_id, &mut notes, warnings);
        }
    }

    (notes, saw_classic, saw_v4)
}

fn merge_notes_file(
    path: &Path,
    platform: &str,
    out: &mut BTreeMap<String, String>,
    warnings: &mut Vec<TcNoWarning>,
) {
    let value = match read_json_value(path) {
        Ok(value) => value,
        Err(detail) => {
            warnings.push(TcNoWarning {
                platform: platform.to_string(),
                kind: TcNoWarningKind::CorruptFile,
                detail: format!("{}: {detail}", path.display()),
            });
            return;
        }
    };
    let Some(obj) = value.as_object() else {
        warnings.push(TcNoWarning {
            platform: platform.to_string(),
            kind: TcNoWarningKind::CorruptFile,
            detail: format!("{}: expected JSON object", path.display()),
        });
        return;
    };
    let notes_map = obj
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("AccountNotes"))
        .and_then(|(_, v)| v.as_object());
    let Some(notes_map) = notes_map else {
        return;
    };
    for (uid, raw_note) in notes_map {
        if let Some(text) = raw_note.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                out.insert(uid.clone(), trimmed.to_string());
            }
        }
    }
}

/// List `<uid>` stems for any `.tcvault` files under
/// `Vault/accounts/<tcno_platform>/`.
fn detect_vault_uids(root: &Path, tcno_platform: &str) -> Vec<String> {
    let vault_dir = root.join("Vault").join("accounts").join(tcno_platform);
    let Ok(entries) = fs::read_dir(&vault_dir) else {
        return Vec::new();
    };
    let mut uids = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_vault = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("tcvault"));
        if is_vault {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                uids.push(stem.to_string());
            }
        }
    }
    uids
}

/// Validate that a TcNo account folder name is a single safe directory name
/// with no path traversal or separator characters.
pub(super) fn safe_account_dir(parent: &Path, account_name: &str) -> Option<PathBuf> {
    let name = account_name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0', ':']) {
        return None;
    }
    let path = parent.join(name);
    let meta = fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return None;
    }
    Some(path)
}

fn is_non_empty_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| {
        !meta.file_type().is_symlink()
            && meta.is_file()
            && meta.len() > 0
            && meta.len() <= MAX_SNAPSHOT_FILE_BYTES
    })
}

/// Sniff a Ubisoft account's plaintext cache folder for the two files
/// accshift's `ubisoft.json` descriptor snapshots.
pub(super) fn sniff_ubisoft_folder(account_dir: &Path) -> Option<UbisoftFolderFiles> {
    let user_dat = account_dir.join("user.dat");
    let connect_secure_storage = account_dir.join("ConnectSecureStorage.dat");
    if is_non_empty_regular_file(&user_dat) && is_non_empty_regular_file(&connect_secure_storage) {
        Some(UbisoftFolderFiles {
            user_dat,
            connect_secure_storage,
        })
    } else {
        None
    }
}

/// Sniff an Epic account's plaintext cache folder to distinguish legacy Epic
/// (`GameUserSettings.ini` + `reg.json` AccountId) from v4 eglv2 (`auth/` or
/// `*.dpapi`).
pub(super) fn sniff_epic_folder(account_dir: &Path) -> EpicFolderKind {
    if let Ok(entries) = fs::read_dir(account_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            if name_str.eq_ignore_ascii_case("auth")
                || name_str.to_ascii_lowercase().ends_with(".dpapi")
            {
                return EpicFolderKind::Eglv2;
            }
        }
    }

    let ini_path = account_dir.join("GameUserSettings.ini");
    let reg_path = account_dir.join("reg.json");
    if !is_non_empty_regular_file(&ini_path) || !is_non_empty_regular_file(&reg_path) {
        return EpicFolderKind::Incomplete;
    }

    match read_reg_json_account_id(&reg_path) {
        Some(account_id) => EpicFolderKind::Legacy {
            ini_path,
            account_id,
        },
        None => EpicFolderKind::Incomplete,
    }
}

/// Parse Epic's `AccountId` from TcNo's `reg.json`.
///
/// Accepts both classic flat `{"HKCU\\...:AccountId": "<32hex>"}` and v4 typed
/// `{"HKCU\\...:AccountId": {"v": "<32hex>", "t": ...}}`.
pub(super) fn read_reg_json_account_id(reg_path: &Path) -> Option<String> {
    let Value::Object(map) = read_json_value(reg_path).ok()? else {
        return None;
    };
    for (key, value) in &map {
        let lower_key = key.to_ascii_lowercase();
        let matches_key = lower_key == "accountid" || lower_key.ends_with(":accountid");
        if !matches_key {
            continue;
        }
        let raw = match value {
            Value::String(s) => Some(s.as_str()),
            Value::Object(obj) => obj
                .get("v")
                .or_else(|| obj.get("V"))
                .and_then(Value::as_str),
            _ => None,
        }?;
        let trimmed = raw.trim();
        if trimmed.len() == 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
            // Kept as written: it goes back into the registry on restore.
            return Some(trimmed.to_string());
        }
    }
    None
}
