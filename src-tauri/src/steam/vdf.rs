use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

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

/// Set a nested value in a VDF file by path.
///
/// `path` is a slice of section/key names relative to the root section.
/// The last element is the key to set; preceding elements are section names.
/// Example: `["friends", "DoNotDisturb"]` sets the `DoNotDisturb` key inside the `friends` section.
///
/// If the key already exists at the target path, its value is replaced.
/// If the section exists but the key does not, the key is inserted before the section's closing `}`.
/// If the section does not exist, it is created (with the key) before the file's final `}`.
pub fn vdf_set_nested_value(content: &str, path: &[&str], value: &str) -> String {
    assert!(
        path.len() >= 2,
        "path must have at least a section and a key"
    );

    let sections = &path[..path.len() - 1];
    let target_key = path[path.len() - 1];

    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::with_capacity(content.len() + 128);
    let mut depth: usize = 0;
    let mut matched_depth: usize = 0; // how many sections from `sections` we have entered
    let mut found = false;
    let mut inserted = false;

    // Two-pass: first scan to check if key exists, then build output.
    for line in lines.iter() {
        let trimmed = line.trim();

        if trimmed == "{" {
            depth += 1;
            continue;
        }

        if trimmed == "}" {
            if depth > 0 {
                if matched_depth == depth && matched_depth <= sections.len() {
                    if matched_depth > 0 {
                        matched_depth -= 1;
                    }
                }
                depth -= 1;
            }
            continue;
        }

        let parts: Vec<&str> = trimmed.split('"').collect();

        // Check if this is a section header we're looking for
        if parts.len() >= 2 && matched_depth < sections.len() && depth == matched_depth + 1 {
            let key = parts[1];
            if key.eq_ignore_ascii_case(sections[matched_depth]) {
                matched_depth += 1;
                continue;
            }
        }

        // Check if this is the target key at the right depth
        if parts.len() >= 4
            && matched_depth == sections.len()
            && depth == sections.len() + 1
            && parts[1].eq_ignore_ascii_case(target_key)
        {
            found = true;
        }
    }

    // ── second pass: build output ──
    depth = 0;
    matched_depth = 0;
    let mut key_replaced = false;

    for line in lines.iter() {
        let trimmed = line.trim();

        if trimmed == "{" {
            result.push_str(line);
            result.push('\n');
            depth += 1;
            continue;
        }

        if trimmed == "}" {
            // If we need to insert the key before the closing brace of the target section
            if !inserted && !found && matched_depth == sections.len() && depth == sections.len() + 1
            {
                let indent = "\t".repeat(depth);
                let escaped_value = escape_vdf_string(value);
                let _ = writeln!(result, "{indent}\"{target_key}\"\t\t\"{escaped_value}\"");
                inserted = true;
            }

            // If we need to insert a missing section before the parent's closing brace
            if !inserted && !found {
                // Check if this brace closes at a depth where we need to insert the remaining sections
                if matched_depth < sections.len() && depth == matched_depth + 1 {
                    // Insert all remaining sections + key
                    let base_indent = "\t".repeat(depth);
                    for (j, section) in sections[matched_depth..].iter().enumerate() {
                        let section_indent = format!("{}{}", base_indent, "\t".repeat(j));
                        let _ = writeln!(result, "{section_indent}\"{section}\"");
                        let _ = writeln!(result, "{section_indent}{{");
                    }
                    let key_indent = format!(
                        "{}{}",
                        base_indent,
                        "\t".repeat(sections.len() - matched_depth)
                    );
                    let escaped_value = escape_vdf_string(value);
                    let _ = writeln!(
                        result,
                        "{key_indent}\"{target_key}\"\t\t\"{escaped_value}\""
                    );
                    for j in (0..sections.len() - matched_depth).rev() {
                        let close_indent = format!("{}{}", base_indent, "\t".repeat(j));
                        let _ = writeln!(result, "{close_indent}}}");
                    }
                    inserted = true;
                }
            }

            if depth > 0 {
                if matched_depth == depth && matched_depth <= sections.len() {
                    if matched_depth > 0 {
                        matched_depth -= 1;
                    }
                }
                depth -= 1;
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let parts: Vec<&str> = trimmed.split('"').collect();

        // Track section entry
        if parts.len() >= 2 && matched_depth < sections.len() && depth == matched_depth + 1 {
            let key = parts[1];
            if key.eq_ignore_ascii_case(sections[matched_depth]) {
                matched_depth += 1;
                result.push_str(line);
                result.push('\n');
                continue;
            }
        }

        // Replace existing key value
        if !key_replaced
            && found
            && parts.len() >= 4
            && matched_depth == sections.len()
            && depth == sections.len() + 1
            && parts[1].eq_ignore_ascii_case(target_key)
        {
            // Rebuild line preserving original indentation
            let leading_whitespace: String =
                line.chars().take_while(|c| c.is_whitespace()).collect();
            let escaped_value = escape_vdf_string(value);
            let _ = writeln!(
                result,
                "{leading_whitespace}\"{target_key}\"\t\t\"{escaped_value}\""
            );
            key_replaced = true;
            inserted = true;
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    // If the original content didn't end with a newline, remove trailing one
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

fn escape_vdf_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn set_persona_state(steam_path: &Path, account_id: u32, state: &str) {
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

#[cfg(test)]
mod tests {
    use super::{escape_vdf_string, parse_vdf, vdf_set_nested_value};

    #[test]
    fn parse_vdf_extracts_multiple_accounts() {
        let content = r#""users"
{
    "111"
    {
        "AccountName"    "first"
        "PersonaName"    "First User"
        "Timestamp"      "123"
    }
    "222"
    {
        "AccountName"    "second"
        "PersonaName"    "Second User"
        "Timestamp"      "456"
    }
}"#;

        let parsed = parse_vdf(content);
        assert_eq!(parsed["111"]["accountname"], "first");
        assert_eq!(parsed["222"]["personaname"], "Second User");
        assert_eq!(parsed["222"]["timestamp"], "456");
    }

    #[test]
    fn escapes_vdf_string_special_characters() {
        assert_eq!(
            escape_vdf_string(r#"+exec "autoexec.cfg" -path C:\Steam"#),
            r#"+exec \"autoexec.cfg\" -path C:\\Steam"#
        );
    }

    #[test]
    fn set_nested_value_escapes_launch_options() {
        let input = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n";
        let output = vdf_set_nested_value(
            input,
            &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
            r#"+exec "autoexec.cfg" -path C:\Steam"#,
        );

        assert!(output
            .contains("\"LaunchOptions\"\t\t\"+exec \\\"autoexec.cfg\\\" -path C:\\\\Steam\""));
    }
}
