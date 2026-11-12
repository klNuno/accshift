//! How a scene run sets up the mock backend and seeds its stores.

use std::collections::BTreeMap;
use std::path::PathBuf;

use accshift_app::mock::resolve_avatars;
use accshift_app::storage::{self, StoreId};
use serde_json::Value;

/// How a scene sets up the mock: the scenario, then the overrides the scene
/// script applies over it.
#[derive(Clone, Debug, Default)]
pub struct MockSetup {
    pub scenario: String,
    pub os: Option<String>,
    pub theme: Option<String>,
    pub locale: Option<String>,
    pub seed: Option<Value>,
    /// Streaming software "runs".
    pub streaming: bool,
    /// The CS2 bridge rows, the bridge enabled.
    pub cs2: Option<Vec<Value>>,
    /// The first-run onboarding was never completed.
    pub onboarding: bool,
    /// Commands that fail, with their message.
    pub failures: Vec<(String, String)>,
    /// The statuses of an add-account setup.
    pub setup: Option<Vec<Value>>,
    /// The wallpaper snapshot, then the window's outer position and scale
    /// factor the backdrop aligns to (physical px).
    pub wallpaper: Option<(Value, [i32; 2], f32)>,
    /// The folder of the scenario files.
    pub data_dir: PathBuf,
    /// The repo root, to resolve the mock avatar paths.
    pub repo_root: Option<PathBuf>,
}

impl MockSetup {
    /// `theme` and `locale` patch `client.settings`; a `seed` key is a store
    /// id or the suffix of its `CLIENT_STORE_` constant, an object merges over
    /// the scenario's store and anything else replaces it. Avatar paths in a
    /// seed resolve against the repo like the scenario's own.
    pub fn patch_stores(&self, stores: &mut BTreeMap<String, Value>) {
        let settings_id = storage::CLIENT_STORE_SETTINGS;
        let mut settings = stores
            .get(settings_id)
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(Default::default()));
        if let Some(theme) = &self.theme {
            settings["themeId"] = Value::String(theme.clone());
        }
        if let Some(locale) = &self.locale {
            settings["language"] = Value::String(locale.clone());
        }
        stores.insert(settings_id.to_string(), settings);
        let Some(Value::Object(seed)) = &self.seed else {
            return;
        };
        for (key, value) in seed {
            let id = store_id_for_seed(key).unwrap_or(key.as_str()).to_string();
            let value = resolve_avatars(value.clone(), self.repo_root.as_deref());
            let merged = match (value, stores.get(&id)) {
                (Value::Object(over), Some(Value::Object(base))) => {
                    let mut out = base.clone();
                    out.extend(over);
                    Value::Object(out)
                }
                (value, _) => value,
            };
            stores.insert(id, merged);
        }
    }
}

/// The store a seed key names by its `CLIENT_STORE_<SUFFIX>` suffix. The
/// suffixes follow the ids (`client.view-mode` is `VIEW_MODE`), except the
/// caches, which a seed names by their full id.
fn store_id_for_seed(key: &str) -> Option<StoreId> {
    storage::CLIENT_STORE_IDS.into_iter().find(|id| {
        id.strip_prefix("client.")
            .is_some_and(|rest| rest.replace('-', "_").eq_ignore_ascii_case(key))
    })
}
