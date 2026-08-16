use crate::context::AppContext;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A theme file, one per theme, in the user's config directory.
///
/// This side stores and hands back; it does not interpret tokens. The token
/// contract, its version and the migration of a file written for an older one
/// live in `src/lib/theme` (see `docs/theming.md`), because that is where the
/// values are turned into CSS and where a bad value has to degrade gracefully.
/// What is enforced here is only what protects the filesystem and the boot
/// payload: a safe id, a sane size, and a `tokens` map that is really a map.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CustomTheme {
    /// Contract version the file was written for. Files predating the field
    /// are version 1: eleven colour tokens and nothing else.
    #[serde(default = "legacy_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The theme author's own version string, free form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub color_scheme: String,
    /// Theme this one inherits every token it does not set from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glass: Option<bool>,
    pub tokens: serde_json::Value,
}

fn legacy_schema_version() -> u32 {
    1
}

/// A theme file has no business being larger than this. Refusing early keeps a
/// pathological file out of the boot payload, which is read on every launch.
const MAX_THEME_FILE_BYTES: u64 = 64 * 1024;

fn themes_dir(app: &dyn AppContext) -> Result<PathBuf, String> {
    crate::storage::themes_dir(app)
}

fn is_safe_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_usable(theme: &CustomTheme) -> bool {
    is_safe_theme_id(&theme.id)
        && theme.extends.as_deref().is_none_or(is_safe_theme_id)
        && theme.tokens.is_object()
}

pub fn list_custom_themes(app: &dyn AppContext) -> Result<Vec<CustomTheme>, String> {
    let dir = themes_dir(app)?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let entries =
        fs::read_dir(&dir).map_err(|e| format!("Could not read themes directory: {e}"))?;
    let mut themes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_THEME_FILE_BYTES {
            continue;
        }
        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let theme: CustomTheme = match serde_json::from_str(&data) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if !is_usable(&theme) {
            continue;
        }
        themes.push(theme);
    }
    themes.sort_by_cached_key(|t| t.name.to_lowercase());
    Ok(themes)
}

pub fn save_custom_theme(app: &dyn AppContext, theme: &CustomTheme) -> Result<(), String> {
    if !is_safe_theme_id(&theme.id) {
        return Err(
            "Invalid theme ID: only alphanumeric characters, hyphens, and underscores are allowed"
                .to_string(),
        );
    }
    if let Some(parent) = theme.extends.as_deref() {
        if !is_safe_theme_id(parent) {
            return Err("Invalid base theme ID".to_string());
        }
    }
    if !theme.tokens.is_object() {
        return Err("Invalid theme: tokens must be an object".to_string());
    }
    let dir = themes_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create themes directory: {e}"))?;
    let path = dir.join(format!("{}.json", theme.id));
    crate::storage::write_json_atomic(&path, theme)
}

pub fn delete_custom_theme(app: &dyn AppContext, theme_id: &str) -> Result<(), String> {
    if !is_safe_theme_id(theme_id) {
        return Err("Invalid theme ID".to_string());
    }
    let path = themes_dir(app)?.join(format!("{}.json", theme_id));
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Could not delete theme file: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCtx {
        root: PathBuf,
    }

    impl AppContext for TestCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    fn unique_test_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-themes-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_file_without_schema_version_reads_as_version_one() {
        // Every theme a user wrote before the contract was versioned looks
        // like this. It must keep loading, and be recognisable as old so the
        // frontend can migrate it rather than treat its holes as deliberate.
        // r##"..."## because a hex colour puts `"#` inside the literal.
        let json = r##"{
            "id": "old",
            "name": "Old",
            "colorScheme": "dark",
            "tokens": { "bgCard": "#111111" }
        }"##;
        let theme: CustomTheme = serde_json::from_str(json).unwrap();
        assert_eq!(theme.schema_version, 1);
        assert_eq!(theme.extends, None);
        assert!(theme.tokens.is_object());
    }

    #[test]
    fn optional_metadata_survives_a_round_trip_and_stays_out_when_unset() {
        let json = r##"{
            "schemaVersion": 2,
            "id": "nord",
            "name": "Nord",
            "author": "someone",
            "version": "1.2.0",
            "colorScheme": "dark",
            "extends": "dark",
            "glass": true,
            "tokens": { "accent": "#88c0d0" }
        }"##;
        let theme: CustomTheme = serde_json::from_str(json).unwrap();
        assert_eq!(theme.schema_version, 2);
        assert_eq!(theme.author.as_deref(), Some("someone"));
        assert_eq!(theme.extends.as_deref(), Some("dark"));
        assert_eq!(theme.glass, Some(true));

        let written = serde_json::to_string(&theme).unwrap();
        assert!(written.contains("\"schemaVersion\":2"));
        assert!(written.contains("\"author\":\"someone\""));

        let bare = CustomTheme {
            author: None,
            version: None,
            extends: None,
            glass: None,
            ..theme
        };
        let written = serde_json::to_string(&bare).unwrap();
        assert!(!written.contains("author"));
        assert!(!written.contains("extends"));
        assert!(!written.contains("glass"));
    }

    #[test]
    fn listing_skips_files_that_could_not_be_applied() {
        let root = unique_test_root("list-skips");
        let ctx = TestCtx { root: root.clone() };
        let dir = themes_dir(&ctx).unwrap();
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("good.json"),
            br#"{"schemaVersion":2,"id":"good","name":"Good","colorScheme":"dark","tokens":{}}"#,
        )
        .unwrap();
        // A traversal attempt in the id, which is also the file name on save.
        fs::write(
            dir.join("evil.json"),
            br#"{"id":"../../evil","name":"Evil","colorScheme":"dark","tokens":{}}"#,
        )
        .unwrap();
        // Tokens as an array: nothing downstream can index it by token name.
        fs::write(
            dir.join("wrong-shape.json"),
            br#"{"id":"wrong","name":"Wrong","colorScheme":"dark","tokens":[]}"#,
        )
        .unwrap();
        fs::write(dir.join("not-json.json"), b"{ this is not json").unwrap();
        fs::write(dir.join("ignored.txt"), b"not a theme").unwrap();

        let themes = list_custom_themes(&ctx).unwrap();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "good");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn saving_refuses_an_id_or_base_that_would_escape_the_themes_directory() {
        let root = unique_test_root("save-refuses");
        let ctx = TestCtx { root: root.clone() };
        let theme = CustomTheme {
            schema_version: 2,
            id: "../escape".to_string(),
            name: "Escape".to_string(),
            author: None,
            version: None,
            color_scheme: "dark".to_string(),
            extends: None,
            glass: None,
            tokens: serde_json::json!({}),
        };
        assert!(save_custom_theme(&ctx, &theme).is_err());

        let theme = CustomTheme {
            id: "fine".to_string(),
            extends: Some("../dark".to_string()),
            ..theme
        };
        assert!(save_custom_theme(&ctx, &theme).is_err());

        let theme = CustomTheme {
            extends: Some("dark".to_string()),
            ..theme
        };
        save_custom_theme(&ctx, &theme).unwrap();
        assert_eq!(list_custom_themes(&ctx).unwrap().len(), 1);

        let _ = fs::remove_dir_all(&root);
    }
}
