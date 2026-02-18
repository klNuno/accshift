use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub fn parse_vdf(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current_id: Option<String> = None;
    let mut current_account: HashMap<String, String> = HashMap::new();
    let mut depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "{" {
            depth += 1;
            continue;
        }

        if trimmed == "}" {
            depth -= 1;
            if depth == 1 && current_id.is_some() {
                accounts.insert(current_id.take().unwrap(), current_account.clone());
                current_account.clear();
            }
            continue;
        }

        let parts: Vec<&str> = trimmed.split('"').collect();

        if parts.len() >= 4 {
            let key = parts[1];
            let value = parts[3];

            if depth == 2 && current_id.is_some() {
                current_account.insert(key.to_lowercase(), value.to_string());
            }
        } else if parts.len() >= 2 {
            let key = parts[1];

            if depth == 1 && !key.is_empty() && key.chars().all(|c| c.is_ascii_digit()) {
                current_id = Some(key.to_string());
            }
        }
    }

    accounts
}

pub fn set_persona_state(steam_path: &PathBuf, account_id: u32, state: &str) {
    if !["0", "1", "2", "3", "4", "5", "6", "7"].contains(&state) {
        return;
    }
    let config_path = steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf");

    if let Ok(content) = fs::read_to_string(&config_path) {
        let mut result = String::new();
        let mut found = false;

        for line in content.lines() {
            if !found && line.contains("\"PersonaState\"") {
                if let Some(pos) = line.rfind('"') {
                    if let Some(start) = line[..pos].rfind('"') {
                        let mut new_line = line[..start + 1].to_string();
                        new_line.push_str(state);
                        new_line.push_str(&line[pos..]);
                        result.push_str(&new_line);
                        result.push('\n');
                        found = true;
                        continue;
                    }
                }
            }
            result.push_str(line);
            result.push('\n');
        }

        if found {
            let _ = fs::write(&config_path, result);
        }
    }
}
