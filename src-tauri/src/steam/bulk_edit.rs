use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::accounts::{load_app_names, list_account_games, steam_user_data_path, CopyableGame};
use super::vdf::vdf_set_nested_value;
use crate::error::AppError;

const NON_GAME_APP_IDS: &[&str] = &["7", "760"];

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
            &["Software", "Valve", "Steam", "apps", &edit.app_id, "LaunchOptions"],
            &edit.value,
        );
    }

    fs::write(&config_path, content).map_err(|e| AppError::FileRead(e.to_string()))
}

pub fn get_account_games(
    steam_path: &Path,
    steam_id: &str,
) -> Result<Vec<CopyableGame>, AppError> {
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

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}
