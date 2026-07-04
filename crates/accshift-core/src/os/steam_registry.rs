//! Steam `registry.vdf` helpers shared by the Linux and macOS backends.
//!
//! On these platforms Steam stores what Windows keeps in HKCU inside a
//! `registry.vdf` file. The backends differ only in where that file lives;
//! everything else is identical and lives here.

use crate::error::AppError;
use crate::platforms::steam::vdf::vdf_set_nested_value;
use std::fs;
use std::path::Path;

const REGISTRY_PATH: &[&str] = &["HKCU", "Software", "Valve", "Steam", "AutoLoginUser"];
const REMEMBER_PATH: &[&str] = &["HKCU", "Software", "Valve", "Steam", "RememberPassword"];

pub fn get_auto_login_user(path: &Path) -> Result<String, AppError> {
    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    Ok(extract_registry_value(&content, "AutoLoginUser").unwrap_or_default())
}

pub fn set_auto_login_user(path: &Path, username: &str) -> Result<(), AppError> {
    // Only fall back to the empty template when the file genuinely does not
    // exist. Any other read error (permissions, transient lock) must not
    // silently replace the user's registry.vdf with an empty template.
    let existing = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => empty_registry_vdf(),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    let updated = vdf_set_nested_value(&existing, REGISTRY_PATH, username);
    let updated = vdf_set_nested_value(&updated, REMEMBER_PATH, "1");
    crate::storage::write_bytes_atomic(path, updated.as_bytes()).map_err(AppError::RegistryWrite)
}

pub fn clear_auto_login_user(path: &Path) -> Result<(), AppError> {
    let existing = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(AppError::FileRead(e.to_string())),
    };
    let updated = vdf_set_nested_value(&existing, REGISTRY_PATH, "");
    crate::storage::write_bytes_atomic(path, updated.as_bytes()).map_err(AppError::RegistryWrite)
}

fn extract_registry_value(content: &str, key: &str) -> Option<String> {
    // Use the escape-aware tokenizer, not split('"'): a value containing an
    // escaped quote (written correctly by escape_vdf_string) would otherwise be
    // truncated at the first inner quote and re-persisted mangled on the switch
    // rollback path.
    for line in content.lines() {
        let tokens = crate::platforms::steam::vdf::vdf_tokenize_line(line.trim());
        if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case(key) {
            return Some(tokens[1].clone());
        }
    }
    None
}

fn empty_registry_vdf() -> String {
    "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "accshift-registry-test-{}-{}",
            tag,
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("registry.vdf")
    }

    #[test]
    fn get_on_missing_file_returns_empty() {
        let path = tmp_path("missing").with_file_name("does-not-exist.vdf");
        assert_eq!(get_auto_login_user(&path).unwrap(), "");
    }

    #[test]
    fn set_creates_file_from_template_and_reads_back() {
        let path = tmp_path("create");
        let _ = std::fs::remove_file(&path);
        set_auto_login_user(&path, "alice").unwrap();
        assert_eq!(get_auto_login_user(&path).unwrap(), "alice");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_overwrites_existing_value() {
        let path = tmp_path("overwrite");
        set_auto_login_user(&path, "alice").unwrap();
        set_auto_login_user(&path, "bob").unwrap();
        assert_eq!(get_auto_login_user(&path).unwrap(), "bob");
        // The key must not be duplicated by the second write.
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("\"AutoLoginUser\"").count(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clear_round_trips_through_get() {
        let path = tmp_path("clear");
        set_auto_login_user(&path, "alice").unwrap();
        clear_auto_login_user(&path).unwrap();
        assert_eq!(get_auto_login_user(&path).unwrap(), "");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn username_with_quote_round_trips() {
        // escape_vdf_string escapes the inner quote on write; the read side must
        // use the escape-aware tokenizer to get it back verbatim rather than
        // truncating at the escaped quote.
        let path = tmp_path("quote");
        set_auto_login_user(&path, "jo\"hn").unwrap();
        assert_eq!(get_auto_login_user(&path).unwrap(), "jo\"hn");
        let _ = std::fs::remove_file(&path);
    }
}
