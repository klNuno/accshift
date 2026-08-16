//! Per-module log levels, and a verbose mode that expires on its own.
//!
//! Two constraints shaped this. The user must never have to edit a file to
//! raise the level, so the state lives next to the log and is driven by a
//! command. And the verbose mode has to switch itself back off: a debug level
//! left on by accident is how a 2 MiB log budget turns into ten minutes of
//! retention.
//!
//! The GUI and the CLI are separate processes reading the same file, so the
//! in-process cache is deliberately short-lived: a level raised from the CLI
//! reaches a running GUI within [`CACHE_TTL`].

use super::event::{now_unix_ms, Level};
use crate::context::AppContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const LEVELS_FILE_NAME: &str = "log-levels.json";
const CACHE_TTL: Duration = Duration::from_secs(2);
const DEFAULT_LEVEL: Level = Level::Info;
/// Ceiling for a temporary debug window. Long enough to reproduce a switch,
/// short enough that a forgotten one cannot eat the retention budget.
pub const MAX_TEMPORARY_DEBUG_MS: u64 = 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryDebug {
    pub level: Level,
    /// Empty means every module.
    pub modules: Vec<String>,
    pub until_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelConfig {
    pub default: Level,
    pub modules: BTreeMap<String, Level>,
    pub temporary: Option<TemporaryDebug>,
}

impl Default for LevelConfig {
    fn default() -> Self {
        Self {
            default: DEFAULT_LEVEL,
            modules: BTreeMap::new(),
            temporary: None,
        }
    }
}

impl LevelConfig {
    /// Level a record from `source` must reach to be written.
    ///
    /// The most specific module prefix wins (`platform.steam` beats
    /// `platform`), and an active temporary window can only ever make it more
    /// verbose, never less.
    pub fn effective_for(&self, source: &str, now_ms: u128) -> Level {
        let mut level = self
            .modules
            .iter()
            .filter(|(module, _)| matches_module(module, source))
            .max_by_key(|(module, _)| module.len())
            .map(|(_, level)| *level)
            .unwrap_or(self.default);

        if let Some(temporary) = &self.temporary {
            let active = temporary.until_ms > now_ms
                && (temporary.modules.is_empty()
                    || temporary
                        .modules
                        .iter()
                        .any(|module| matches_module(module, source)));
            if active && temporary.level < level {
                level = temporary.level;
            }
        }

        level
    }

    /// Whether a temporary window is still open at `now_ms`.
    pub fn temporary_active(&self, now_ms: u128) -> bool {
        self.temporary
            .as_ref()
            .is_some_and(|temporary| temporary.until_ms > now_ms)
    }

    pub fn to_json(&self, now_ms: u128) -> Value {
        json!({
            "default": self.default.as_str(),
            "modules": self
                .modules
                .iter()
                .map(|(module, level)| (module.clone(), json!(level.as_str())))
                .collect::<serde_json::Map<String, Value>>(),
            "temporaryDebug": self.temporary.as_ref().filter(|t| t.until_ms > now_ms).map(|t| json!({
                "level": t.level.as_str(),
                "modules": t.modules,
                "untilMs": t.until_ms as u64,
                "remainingMs": (t.until_ms - now_ms) as u64,
            })),
        })
    }
}

/// `platform` covers `platform.steam`; `*` covers everything.
fn matches_module(module: &str, source: &str) -> bool {
    if module == "*" {
        return true;
    }
    source == module
        || (source.starts_with(module) && source.as_bytes().get(module.len()) == Some(&b'.'))
}

// ---------------------------------------------------------------------------
// On-disk shape
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawLevelConfig {
    default: Option<String>,
    modules: BTreeMap<String, String>,
    temporary_debug: Option<RawTemporaryDebug>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RawTemporaryDebug {
    level: Option<String>,
    modules: Vec<String>,
    until_ms: u64,
}

impl From<RawLevelConfig> for LevelConfig {
    fn from(raw: RawLevelConfig) -> Self {
        LevelConfig {
            default: raw
                .default
                .as_deref()
                .and_then(Level::parse)
                .unwrap_or(DEFAULT_LEVEL),
            modules: raw
                .modules
                .into_iter()
                .filter_map(|(module, level)| Level::parse(&level).map(|level| (module, level)))
                .collect(),
            temporary: raw.temporary_debug.and_then(|temporary| {
                let level = temporary.level.as_deref().and_then(Level::parse)?;
                Some(TemporaryDebug {
                    level,
                    modules: temporary.modules,
                    until_ms: u128::from(temporary.until_ms),
                })
            }),
        }
    }
}

impl LevelConfig {
    fn to_raw(&self, now_ms: u128) -> RawLevelConfig {
        RawLevelConfig {
            default: Some(self.default.as_str().to_string()),
            modules: self
                .modules
                .iter()
                .map(|(module, level)| (module.clone(), level.as_str().to_string()))
                .collect(),
            // An expired window is dropped on the next write rather than kept
            // as dead state nobody can explain later.
            temporary_debug: self
                .temporary
                .as_ref()
                .filter(|temporary| temporary.until_ms > now_ms)
                .map(|temporary| RawTemporaryDebug {
                    level: Some(temporary.level.as_str().to_string()),
                    modules: temporary.modules.clone(),
                    until_ms: temporary.until_ms.min(u128::from(u64::MAX)) as u64,
                }),
        }
    }
}

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

static CACHE: Mutex<Option<(PathBuf, Instant, LevelConfig)>> = Mutex::new(None);

pub fn levels_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(crate::storage::app_log_root(app_handle)?.join(LEVELS_FILE_NAME))
}

/// Read the level configuration, from a short-lived cache when possible.
pub fn load(app_handle: &dyn AppContext) -> LevelConfig {
    let Ok(path) = levels_file_path(app_handle) else {
        return LevelConfig::default();
    };

    {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_path, loaded_at, config)) = cache.as_ref() {
            if cached_path == &path && loaded_at.elapsed() < CACHE_TTL {
                return config.clone();
            }
        }
    }

    let config = read_from_disk(&path);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache = Some((path, Instant::now(), config.clone()));
    config
}

fn read_from_disk(path: &std::path::Path) -> LevelConfig {
    // A missing or unreadable file is the normal case (nobody ever changed a
    // level), and a corrupt one must not silence logging: both fall back to
    // the defaults.
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<RawLevelConfig>(&text).ok())
        .map(LevelConfig::from)
        .unwrap_or_default()
}

/// Effective level for `source`, the single question the emit path asks.
pub fn effective_level(app_handle: &dyn AppContext, source: &str) -> Level {
    load(app_handle).effective_for(source, now_unix_ms())
}

fn store(app_handle: &dyn AppContext, config: &LevelConfig) -> Result<(), String> {
    let path = levels_file_path(app_handle)?;
    let raw = config.to_raw(now_unix_ms());
    let payload = serde_json::to_vec_pretty(&raw)
        .map_err(|reason| format!("Could not serialize log levels: {reason}"))?;
    super::write_atomic(&path, &payload)?;
    invalidate_cache();
    Ok(())
}

/// Forget the cached configuration. Called by every writer so the change is
/// visible to this process immediately instead of within [`CACHE_TTL`].
pub fn invalidate_cache() {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache = None;
}

/// Set (or clear, with `None`) the level of one module. `module` = `None`
/// targets the default level.
pub fn set_level(
    app_handle: &dyn AppContext,
    module: Option<&str>,
    level: Option<Level>,
) -> Result<LevelConfig, String> {
    let mut config = load(app_handle);
    match (module, level) {
        (None, Some(level)) => config.default = level,
        (None, None) => config.default = DEFAULT_LEVEL,
        (Some(module), Some(level)) => {
            config.modules.insert(module.to_string(), level);
        }
        (Some(module), None) => {
            config.modules.remove(module);
        }
    }
    store(app_handle, &config)?;

    super::event::event(&super::catalog::DIAGNOSTICS_LEVEL_CHANGED)
        .source("diagnostics.levels")
        .field("module", module.unwrap_or("*"))
        .field("newLevel", level.map(Level::as_str).unwrap_or("default"))
        .emit(app_handle);

    Ok(config)
}

/// Open a verbose window that closes itself after `duration_ms`.
pub fn start_temporary_debug(
    app_handle: &dyn AppContext,
    level: Level,
    modules: Vec<String>,
    duration_ms: u64,
) -> Result<LevelConfig, String> {
    let duration_ms = duration_ms.clamp(1_000, MAX_TEMPORARY_DEBUG_MS);
    let mut config = load(app_handle);
    config.temporary = Some(TemporaryDebug {
        level,
        modules: modules.clone(),
        until_ms: now_unix_ms() + u128::from(duration_ms),
    });
    store(app_handle, &config)?;

    super::event::event(&super::catalog::DIAGNOSTICS_DEBUG_ENABLED)
        .source("diagnostics.levels")
        .field("durationMs", duration_ms)
        .field("newLevel", level.as_str())
        .field(
            "modules",
            Value::Array(modules.iter().map(|m| json!(m)).collect()),
        )
        .emit(app_handle);

    Ok(config)
}

/// Close the verbose window early.
pub fn stop_temporary_debug(app_handle: &dyn AppContext) -> Result<LevelConfig, String> {
    let mut config = load(app_handle);
    let was_active = config.temporary_active(now_unix_ms());
    config.temporary = None;
    store(app_handle, &config)?;
    if was_active {
        super::event::event(&super::catalog::DIAGNOSTICS_DEBUG_EXPIRED)
            .source("diagnostics.levels")
            .emit(app_handle);
    }
    Ok(config)
}

/// Drop an expired window from the file. Called at session start so the state
/// on disk matches what the app actually does.
pub fn expire_temporary_debug(app_handle: &dyn AppContext) {
    let config = load(app_handle);
    let now = now_unix_ms();
    if config.temporary.is_some() && !config.temporary_active(now) {
        let mut cleaned = config;
        cleaned.temporary = None;
        let _ = store(app_handle, &cleaned);
        super::event::event(&super::catalog::DIAGNOSTICS_DEBUG_EXPIRED)
            .source("diagnostics.levels")
            .emit(app_handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::test_support::TestCtx;

    #[test]
    fn default_level_hides_debug_and_keeps_info() {
        let config = LevelConfig::default();
        assert_eq!(config.effective_for("platform.steam", 0), Level::Info);
    }

    #[test]
    fn most_specific_module_prefix_wins() {
        let mut config = LevelConfig::default();
        config.modules.insert("platform".into(), Level::Warn);
        config.modules.insert("platform.steam".into(), Level::Debug);

        assert_eq!(config.effective_for("platform.steam", 0), Level::Debug);
        assert_eq!(config.effective_for("platform.riot", 0), Level::Warn);
        assert_eq!(config.effective_for("storage", 0), Level::Info);
    }

    // A prefix must match on a dot boundary, otherwise `platform` would also
    // configure a hypothetical `platformer` module.
    #[test]
    fn module_prefixes_match_on_dot_boundaries() {
        assert!(matches_module("platform", "platform.steam"));
        assert!(matches_module("platform", "platform"));
        assert!(!matches_module("platform", "platforms.steam"));
        assert!(matches_module("*", "anything"));
    }

    #[test]
    fn temporary_window_only_makes_logging_more_verbose() {
        let mut config = LevelConfig::default();
        config.modules.insert("platform".into(), Level::Error);
        config.temporary = Some(TemporaryDebug {
            level: Level::Debug,
            modules: vec![],
            until_ms: 1_000,
        });

        assert_eq!(config.effective_for("platform.steam", 500), Level::Debug);
        // A window asking for less than the module already emits changes
        // nothing.
        config.temporary = Some(TemporaryDebug {
            level: Level::Warn,
            modules: vec![],
            until_ms: 1_000,
        });
        assert_eq!(config.effective_for("storage", 500), Level::Info);
    }

    #[test]
    fn temporary_window_reverts_on_its_own() {
        let mut config = LevelConfig::default();
        config.temporary = Some(TemporaryDebug {
            level: Level::Trace,
            modules: vec![],
            until_ms: 1_000,
        });

        assert_eq!(config.effective_for("platform", 999), Level::Trace);
        // One millisecond past the deadline the normal level is back, with no
        // command, no restart and no file edit.
        assert_eq!(config.effective_for("platform", 1_000), Level::Info);
        assert!(!config.temporary_active(1_000));
    }

    #[test]
    fn temporary_window_can_target_one_module() {
        let mut config = LevelConfig::default();
        config.temporary = Some(TemporaryDebug {
            level: Level::Debug,
            modules: vec!["platform.riot".into()],
            until_ms: 1_000,
        });

        assert_eq!(config.effective_for("platform.riot", 0), Level::Debug);
        assert_eq!(config.effective_for("platform.steam", 0), Level::Info);
    }

    #[test]
    fn levels_round_trip_through_the_file() {
        let ctx = TestCtx::new("levels-roundtrip");

        set_level(&ctx, Some("platform.steam"), Some(Level::Debug)).expect("set module level");
        set_level(&ctx, None, Some(Level::Warn)).expect("set default level");

        let reloaded = read_from_disk(&levels_file_path(&ctx).expect("path"));
        assert_eq!(reloaded.default, Level::Warn);
        assert_eq!(
            reloaded.modules.get("platform.steam").copied(),
            Some(Level::Debug)
        );

        set_level(&ctx, Some("platform.steam"), None).expect("clear module level");
        let cleared = read_from_disk(&levels_file_path(&ctx).expect("path"));
        assert!(cleared.modules.is_empty());
    }

    #[test]
    fn temporary_debug_is_clamped_and_persisted() {
        let ctx = TestCtx::new("levels-temporary");

        start_temporary_debug(&ctx, Level::Trace, vec![], u64::MAX).expect("start");
        let stored = read_from_disk(&levels_file_path(&ctx).expect("path"));
        let temporary = stored.temporary.expect("temporary window");
        assert_eq!(temporary.level, Level::Trace);
        assert!(temporary.until_ms <= now_unix_ms() + u128::from(MAX_TEMPORARY_DEBUG_MS));

        stop_temporary_debug(&ctx).expect("stop");
        assert!(read_from_disk(&levels_file_path(&ctx).expect("path"))
            .temporary
            .is_none());
    }

    // A file someone hand-edited into nonsense must not take logging down
    // with it.
    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let ctx = TestCtx::new("levels-corrupt");
        let path = levels_file_path(&ctx).expect("path");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "{ not json").expect("write");

        assert_eq!(read_from_disk(&path), LevelConfig::default());
    }
}
