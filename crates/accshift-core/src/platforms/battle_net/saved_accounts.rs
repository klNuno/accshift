//! The launcher's saved account list in Battle.net.config: read, parse and write.

use super::*;

#[cfg(windows)]
pub(super) fn battle_net_config_path() -> Result<PathBuf, String> {
    let app_data = env::var("APPDATA").map_err(|_| "APPDATA is not available".to_string())?;
    Ok(PathBuf::from(app_data)
        .join("Battle.net")
        .join("Battle.net.config"))
}

// macOS keeps the same JSON config, only in a different root. The client's own
// logs confirm it: `ConfigRoot: ~/Library/Application Support/Battle.net`,
// `User Configuration File: .../Battle.net.config`.
#[cfg(target_os = "macos")]
pub(super) fn battle_net_config_path() -> Result<PathBuf, String> {
    let home = env::var("HOME").map_err(|_| "HOME is not available".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support/Battle.net")
        .join("Battle.net.config"))
}

pub(super) fn read_config_json(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("Could not read Battle.net config {}: {e}", path.display()))?;
    let value = serde_json::from_str::<Value>(&content)
        .map_err(|e| format!("Could not parse Battle.net config {}: {e}", path.display()))?;
    Ok(Some(value))
}

pub(super) fn collect_unique_accounts(
    values: impl Iterator<Item = String>,
    seen: &mut HashSet<String>,
) -> Vec<String> {
    let mut accounts = Vec::new();
    for value in values {
        let trimmed = value.trim().to_string();
        let key = normalize_account_key(&trimmed);
        if trimmed.is_empty() || !seen.insert(key) {
            continue;
        }
        accounts.push(trimmed);
    }
    accounts
}

/// Parse the comma-separated SavedAccountNames list with quote-aware CSV
/// semantics. A field wrapped in double quotes may contain literal commas, and
/// an internal quote is escaped by doubling it (`""`). Unquoted lists (the
/// common case) parse exactly as a plain `split(',')` would, so this stays
/// backward compatible with configs written by the launcher or older builds.
pub(super) fn parse_saved_account_names(raw: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                // A doubled quote inside a quoted field is a literal quote.
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut field));
            }
            other => field.push(other),
        }
    }
    fields.push(field);
    fields
}

/// Serialize an account name for the comma-separated SavedAccountNames list.
/// Fields containing a comma or a double quote are wrapped in double quotes
/// with internal quotes doubled, mirroring `parse_saved_account_names`.
pub(super) fn encode_saved_account_name(name: &str) -> String {
    if name.contains(',') || name.contains('"') {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}

pub(super) fn extract_saved_account_names(value: &Value) -> Vec<String> {
    let source = value
        .get("Client")
        .and_then(Value::as_object)
        .and_then(|client| client.get("SavedAccountNames"));

    let mut seen = HashSet::new();

    match source {
        Some(Value::String(raw)) => {
            collect_unique_accounts(parse_saved_account_names(raw).into_iter(), &mut seen)
        }
        Some(Value::Array(items)) => collect_unique_accounts(
            items
                .iter()
                .filter_map(|item| item.as_str().map(String::from)),
            &mut seen,
        ),
        _ => Vec::new(),
    }
}

pub(super) fn read_saved_accounts() -> Result<Vec<String>, String> {
    let config_path = battle_net_config_path()?;
    let Some(value) = read_config_json(&config_path)? else {
        return Ok(Vec::new());
    };
    Ok(extract_saved_account_names(&value))
}

pub(super) fn write_saved_accounts(
    app_handle: &dyn AppContext,
    accounts: &[String],
) -> Result<(), String> {
    let config_path = battle_net_config_path()?;
    let parent = config_path
        .parent()
        .ok_or_else(|| "Could not resolve Battle.net config directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Could not create Battle.net config directory: {e}"))?;

    let mut root = read_config_json(&config_path)?.unwrap_or_else(|| Value::Object(Map::new()));
    if !root.is_object() {
        root = Value::Object(Map::new());
    }

    let root_object = root
        .as_object_mut()
        .ok_or_else(|| "Battle.net config root is invalid".to_string())?;

    let client_entry = root_object
        .entry("Client".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !client_entry.is_object() {
        *client_entry = Value::Object(Map::new());
    }

    let client = client_entry
        .as_object_mut()
        .ok_or_else(|| "Battle.net client config is invalid".to_string())?;
    let serialized = accounts
        .iter()
        .map(|name| encode_saved_account_name(name))
        .collect::<Vec<_>>()
        .join(",");
    client.insert("SavedAccountNames".to_string(), Value::String(serialized));

    if config_path.exists() {
        let backup_path = config_path.with_extension("config.backup");
        if let Err(e) = fs::copy(&config_path, &backup_path) {
            log_platform_error(
                app_handle,
                "battle_net.write_saved_accounts",
                "Could not create Battle.net config backup before overwrite",
                format!("backup_path={}; error={e}", backup_path.display()),
            );
        }
    }

    let json = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Could not serialize Battle.net config: {e}"))?;
    crate::storage::write_bytes_atomic(&config_path, json.as_bytes()).map_err(|e| {
        format!(
            "Could not write Battle.net config {}: {e}",
            config_path.display()
        )
    })
}
