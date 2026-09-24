//! The signed-in BattleTag, read from the launcher's logs and cache.

use super::*;

#[cfg(windows)]
pub(super) fn battle_net_cached_data_path() -> Result<PathBuf, String> {
    let local_app_data =
        env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not available".to_string())?;
    Ok(PathBuf::from(local_app_data)
        .join("Battle.net")
        .join("CachedData.db"))
}

#[cfg(windows)]
pub(super) fn latest_opened_account_id_from_logs() -> Result<Option<u64>, String> {
    let local_app_data =
        env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not available".to_string())?;
    let log_dir = PathBuf::from(local_app_data)
        .join("Battle.net")
        .join("Logs");
    if !log_dir.exists() {
        return Ok(None);
    }

    let mut newest_logs = fs::read_dir(&log_dir)
        .map_err(|e| format!("Could not read Battle.net logs {}: {e}", log_dir.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            if !file_name.starts_with("battle.net-") || !file_name.ends_with(".log") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();

    newest_logs.sort_by_key(|entry| std::cmp::Reverse(entry.0));

    for (_, path) in newest_logs.into_iter().take(8) {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for line in content.lines().rev() {
            let needle = "Opened database at: ";
            let Some(idx) = line.find(needle) else {
                continue;
            };
            let db_path = line[idx + needle.len()..].trim();
            let marker = "\\Account\\";
            let Some(account_idx) = db_path.rfind(marker) else {
                continue;
            };
            let suffix = &db_path[account_idx + marker.len()..];
            let Some((account_id, _)) = suffix.split_once("\\account.db") else {
                continue;
            };
            if let Ok(parsed) = account_id.trim().parse::<u64>() {
                return Ok(Some(parsed));
            }
        }
    }

    Ok(None)
}

// The battle tag is a cosmetic label enriched from the client's SQLite login
// cache, which only exists on Windows (LOCALAPPDATA\Battle.net\CachedData.db).
// macOS has no equivalent we read, so accounts simply show without a tag until
// the user sets one; the switch itself never depends on it.
#[cfg(not(windows))]
pub(super) fn current_battle_tag_from_cache() -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(windows)]
pub(super) fn current_battle_tag_from_cache() -> Result<Option<String>, String> {
    let Some(account_id_lo) = latest_opened_account_id_from_logs()? else {
        return Ok(None);
    };

    let db_path = battle_net_cached_data_path()?;
    if !db_path.exists() {
        return Ok(None);
    }

    let connection = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Could not open Battle.net cached data: {e}"))?;

    let mut statement = connection
        .prepare(
            "SELECT battle_tag
             FROM login_cache
             WHERE account_id_lo = ?1
             ORDER BY rowid DESC
             LIMIT 1",
        )
        .map_err(|e| format!("Could not query Battle.net login cache: {e}"))?;

    let mut rows = statement
        .query([account_id_lo as i64])
        .map_err(|e| format!("Could not read Battle.net login cache: {e}"))?;

    let Some(row) = rows
        .next()
        .map_err(|e| format!("Could not iterate Battle.net login cache: {e}"))?
    else {
        return Ok(None);
    };

    let battle_tag = row
        .get::<_, String>(0)
        .map_err(|e| format!("Could not decode Battle.net battle tag: {e}"))?;
    let trimmed = battle_tag.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(trimmed.to_string()))
}
