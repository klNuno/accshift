use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::accounts::{
    list_account_games, load_app_names, steam_user_data_path, CopyableGame, NON_GAME_APP_IDS,
};
use super::vdf::vdf_set_nested_value;
use crate::error::AppError;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptionEdit {
    pub app_id: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkEditRequest {
    pub steam_ids: Vec<String>,
    pub news_popup: Option<bool>,
    pub do_not_disturb: Option<bool>,
    pub launch_options: Vec<LaunchOptionEdit>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkEditFailure {
    pub steam_id: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkEditResult {
    pub succeeded: u32,
    pub failed: Vec<BulkEditFailure>,
}

pub fn apply_bulk_edit(steam_path: &Path, request: &BulkEditRequest) -> BulkEditResult {
    let mut succeeded: u32 = 0;
    let mut failed: Vec<BulkEditFailure> = Vec::new();

    for steam_id in &request.steam_ids {
        match apply_for_account(steam_path, steam_id, request) {
            Ok(()) => succeeded += 1,
            Err(e) => failed.push(BulkEditFailure {
                steam_id: steam_id.clone(),
                error: e.to_string(),
            }),
        }
    }

    BulkEditResult { succeeded, failed }
}

fn apply_for_account(
    steam_path: &Path,
    steam_id: &str,
    request: &BulkEditRequest,
) -> Result<(), AppError> {
    let userdata = steam_user_data_path(steam_path, steam_id)?;
    let config_path = userdata.join("config").join("localconfig.vdf");

    if !config_path.exists() {
        return Err(AppError::FileRead(format!(
            "localconfig.vdf not found for {}",
            steam_id
        )));
    }

    let mut content =
        fs::read_to_string(&config_path).map_err(|e| AppError::FileRead(e.to_string()))?;

    if let Some(news) = request.news_popup {
        let val = if news { "1" } else { "0" };
        content = vdf_set_nested_value(&content, &["news", "NotifyAvailableGames"], val);
    }

    if let Some(dnd) = request.do_not_disturb {
        let val = if dnd { "1" } else { "0" };
        content = vdf_set_nested_value(&content, &["friends", "DoNotDisturb"], val);
    }

    for edit in &request.launch_options {
        if edit.app_id.is_empty() || !edit.app_id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        content = vdf_set_nested_value(
            &content,
            &[
                "Software",
                "Valve",
                "Steam",
                "apps",
                &edit.app_id,
                "LaunchOptions",
            ],
            &edit.value,
        );
    }

    crate::storage::write_bytes_atomic(&config_path, content.as_bytes()).map_err(AppError::FileRead)
}

pub fn get_account_games(steam_path: &Path, steam_id: &str) -> Result<Vec<CopyableGame>, AppError> {
    let userdata = steam_user_data_path(steam_path, steam_id)?;
    let game_ids = list_account_games(&userdata)?;
    let names = load_app_names(steam_path);

    let mut games: Vec<CopyableGame> = game_ids
        .iter()
        .filter(|id| !NON_GAME_APP_IDS.contains(&id.as_str()))
        .filter_map(|id| {
            names.get(id).map(|name| CopyableGame {
                app_id: id.clone(),
                name: name.clone(),
            })
        })
        .collect();

    games.sort_by_key(|g| g.name.to_lowercase());
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // SteamID64 whose low 32 bits (the userdata folder name) is 1.
    const TEST_STEAM_ID: &str = "76561197960265729";
    // SteamID64 with no matching userdata folder written by these tests.
    const MISSING_STEAM_ID: &str = "76561197960265730";

    const BASE_CONFIG: &str = "\"UserLocalConfigStore\"\n{\n\t\"news\"\n\t{\n\t\t\"NotifyAvailableGames\"\t\t\"1\"\n\t}\n\t\"friends\"\n\t{\n\t\t\"DoNotDisturb\"\t\t\"0\"\n\t}\n\t\"Software\"\n\t{\n\t}\n}\n";

    fn bulk_edit_test_root(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("accshift-bulkedit-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_localconfig(steam_path: &Path, steam_id: &str, content: &str) -> PathBuf {
        let account_id = crate::platforms::steam::accounts::steam_id_to_account_id(steam_id)
            .expect("test steam id must be numeric");
        let config_dir = steam_path
            .join("userdata")
            .join(account_id.to_string())
            .join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("localconfig.vdf");
        fs::write(&config_path, content).unwrap();
        config_path
    }

    #[test]
    fn apply_for_account_toggles_news_popup_and_do_not_disturb() {
        let root = bulk_edit_test_root("toggle");
        let config_path = write_localconfig(&root, TEST_STEAM_ID, BASE_CONFIG);

        let request = BulkEditRequest {
            steam_ids: vec![TEST_STEAM_ID.to_string()],
            news_popup: Some(false),
            do_not_disturb: Some(true),
            launch_options: Vec::new(),
        };

        apply_for_account(&root, TEST_STEAM_ID, &request)
            .expect("apply_for_account should succeed");

        let updated = fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("\"NotifyAvailableGames\"\t\t\"0\""));
        assert!(updated.contains("\"DoNotDisturb\"\t\t\"1\""));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_for_account_applies_multiple_launch_options_and_skips_non_numeric_app_id() {
        let root = bulk_edit_test_root("launchopts");
        let config_path = write_localconfig(&root, TEST_STEAM_ID, BASE_CONFIG);

        let request = BulkEditRequest {
            steam_ids: vec![TEST_STEAM_ID.to_string()],
            news_popup: None,
            do_not_disturb: None,
            launch_options: vec![
                LaunchOptionEdit {
                    app_id: "730".to_string(),
                    value: "-novid -fullscreen".to_string(),
                },
                LaunchOptionEdit {
                    app_id: "440".to_string(),
                    value: "-console".to_string(),
                },
                LaunchOptionEdit {
                    app_id: "not_numeric".to_string(),
                    value: "-should-be-skipped".to_string(),
                },
            ],
        };

        apply_for_account(&root, TEST_STEAM_ID, &request)
            .expect("apply_for_account should succeed");

        let updated = fs::read_to_string(&config_path).unwrap();
        assert!(updated.contains("\"LaunchOptions\"\t\t\"-novid -fullscreen\""));
        assert!(updated.contains("\"LaunchOptions\"\t\t\"-console\""));
        assert!(!updated.contains("not_numeric"));
        assert!(!updated.contains("-should-be-skipped"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_for_account_errors_when_localconfig_is_missing() {
        let root = bulk_edit_test_root("missing");

        let request = BulkEditRequest {
            steam_ids: vec![TEST_STEAM_ID.to_string()],
            news_popup: Some(true),
            do_not_disturb: None,
            launch_options: Vec::new(),
        };

        let err = apply_for_account(&root, TEST_STEAM_ID, &request)
            .expect_err("missing localconfig.vdf must be an error");
        assert!(matches!(err, AppError::FileRead(_)));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_bulk_edit_reports_success_and_failure_per_account() {
        let root = bulk_edit_test_root("mixed");
        write_localconfig(&root, TEST_STEAM_ID, BASE_CONFIG);

        let request = BulkEditRequest {
            steam_ids: vec![TEST_STEAM_ID.to_string(), MISSING_STEAM_ID.to_string()],
            news_popup: Some(false),
            do_not_disturb: None,
            launch_options: Vec::new(),
        };

        let result = apply_bulk_edit(&root, &request);
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].steam_id, MISSING_STEAM_ID);

        let _ = fs::remove_dir_all(&root);
    }
}
