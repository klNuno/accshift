//! Every client store behind one value, plus the small stores of
//! `src/lib/shared/` (`accountCardColors`, `accountCardNotes`,
//! `folderCardColors`, `viewMode`, `cachedMapStore`) and
//! `src/lib/platforms/steam/defaultGameStore.ts`.
//!
//! [`ClientStores`] owns the [`ClientStorage`] and a typed copy of each store
//! the UI reads. A mutation updates the typed copy and writes its JSON into
//! the storage, which marks that store pending; the shell drains
//! [`ClientStores::take_due_saves`] into the job pool and reports each result
//! with [`ClientStores::finish_save`]. A typed copy is rebuilt (sanitized)
//! only when its store changes from outside: a snapshot or a refresh. A
//! local write keeps the value as written, like the TS caches.

use crate::folders::{self, FolderInfo, FolderSection, FolderStore, ItemRef};
use crate::i18n::Locale;
use crate::js;
use crate::personas::{self, Persona, PersonaInput, PersonaPatch};
use crate::settings::{self, AppSettings};
use crate::storage::{
    ClientStorage, PendingSave, RefreshPlan, StoreId, CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_ACCOUNT_CARD_NOTES, CLIENT_STORE_ACCOUNT_DEFAULT_GAME, CLIENT_STORE_FOLDERS,
    CLIENT_STORE_FOLDER_CARD_COLORS, CLIENT_STORE_PERSONAS, CLIENT_STORE_SETTINGS,
    CLIENT_STORE_VIEW_MODE,
};
use crate::types::ClientStorageSnapshot;
use crate::{Backend, BackendResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;

/// The platforms shipped with the app, in display order: the platform
/// registry's chrome entries (`src/lib/platforms/registry.ts`), which also
/// cover every shipped descriptor.
pub const SHIPPED_PLATFORM_IDS: [&str; 9] = [
    "steam",
    "riot",
    "battle-net",
    "ubisoft",
    "roblox",
    "epic",
    "gog",
    "jagex",
    "discord",
];

/// The built-in theme ids (`BUILT_IN_DOCUMENTS` in `src/lib/theme/themes.ts`).
pub const BUILT_IN_THEME_IDS: [&str; 6] = [
    "dark",
    "light",
    "midnight",
    "glass-dark",
    "glass-light",
    "liquid-glass",
];

/// What the sanitizers read from outside the stores: the platform list (the
/// shipped ones plus the user's descriptors), the theme list (built-ins plus
/// custom themes) and the OS's preferred languages for a fresh install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreContext {
    pub platform_ids: Vec<String>,
    pub theme_ids: Vec<String>,
    /// Most preferred first, like `navigator.languages`.
    pub preferred_languages: Vec<String>,
}

impl Default for StoreContext {
    fn default() -> Self {
        Self::shipped()
    }
}

impl StoreContext {
    /// Shipped platforms and built-in themes, no preferred language.
    pub fn shipped() -> Self {
        Self {
            platform_ids: SHIPPED_PLATFORM_IDS
                .iter()
                .map(|id| id.to_string())
                .collect(),
            theme_ids: BUILT_IN_THEME_IDS.iter().map(|id| id.to_string()).collect(),
            preferred_languages: Vec::new(),
        }
    }

    pub fn platform_ids(&self) -> impl Iterator<Item = &str> {
        self.platform_ids.iter().map(String::as_str)
    }

    pub fn knows_platform(&self, id: &str) -> bool {
        self.platform_ids.iter().any(|known| known == id)
    }

    /// `getThemeDefinition(id).id`: the id when it names a theme, the default
    /// theme otherwise.
    pub fn resolve_theme_id<'a>(&'a self, id: &'a str) -> &'a str {
        if !id.is_empty() && self.theme_ids.iter().any(|known| known == id) {
            id
        } else {
            settings::DEFAULT_THEME_ID
        }
    }
}

// ------------------------------------------------------------ cachedMapStore

/// A string map store (`createCachedMapStore`) with its entry sanitizer.
#[derive(Debug, Clone)]
pub struct StringMapStore {
    store_id: StoreId,
    sanitize_entry: fn(&str, &str) -> Option<String>,
    map: BTreeMap<String, String>,
}

impl StringMapStore {
    pub fn new(store_id: StoreId, sanitize_entry: fn(&str, &str) -> Option<String>) -> Self {
        Self {
            store_id,
            sanitize_entry,
            map: BTreeMap::new(),
        }
    }

    pub fn store_id(&self) -> StoreId {
        self.store_id
    }

    /// `sanitizeMap`: string values under non-blank keys that pass the entry
    /// sanitizer.
    pub fn load(&mut self, value: Option<&Value>) {
        self.map.clear();
        let Some(record) = js::as_record(value) else {
            return;
        };
        for (key, raw) in record {
            if js::trim(key).is_empty() {
                continue;
            }
            let Value::String(raw) = raw else {
                continue;
            };
            if let Some(safe) = (self.sanitize_entry)(key, raw) {
                self.map.insert(key.clone(), safe);
            }
        }
    }

    /// `get`: the entry, `""` when absent.
    pub fn get(&self, id: &str) -> &str {
        self.map.get(id).map_or("", String::as_str)
    }

    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.map
    }

    fn put(&mut self, id: &str, value: &str) {
        match (self.sanitize_entry)(id, value) {
            Some(safe) => {
                self.map.insert(id.to_string(), safe);
            }
            None => {
                self.map.remove(id);
            }
        }
    }

    /// `set`: a value the sanitizer refuses removes the entry.
    pub fn set(&mut self, id: &str, value: &str) {
        self.put(id, value);
    }

    /// `setMany`: the same value for every id. Returns false (and writes
    /// nothing) for an empty list.
    pub fn set_many(&mut self, ids: &[String], value: &str) -> bool {
        if ids.is_empty() {
            return false;
        }
        for id in ids {
            self.put(id, value);
        }
        true
    }

    /// `remove`.
    pub fn remove(&mut self, id: &str) {
        self.map.remove(id);
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(&self.map).unwrap_or(Value::Null)
    }
}

/// One account card colour preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorPreset {
    pub id: &'static str,
    pub color: &'static str,
}

/// `ACCOUNT_CARD_COLOR_PRESETS`.
pub const ACCOUNT_CARD_COLOR_PRESETS: [ColorPreset; 11] = [
    ColorPreset {
        id: "none",
        color: "",
    },
    ColorPreset {
        id: "blue",
        color: "#3b82f6",
    },
    ColorPreset {
        id: "cyan",
        color: "#06b6d4",
    },
    ColorPreset {
        id: "green",
        color: "#10b981",
    },
    ColorPreset {
        id: "lime",
        color: "#84cc16",
    },
    ColorPreset {
        id: "yellow",
        color: "#f59e0b",
    },
    ColorPreset {
        id: "orange",
        color: "#f97316",
    },
    ColorPreset {
        id: "red",
        color: "#f43f5e",
    },
    ColorPreset {
        id: "pink",
        color: "#ec4899",
    },
    ColorPreset {
        id: "violet",
        color: "#8b5cf6",
    },
    ColorPreset {
        id: "gray",
        color: "#71717a",
    },
];

/// `SAFE_COLOR_RE`: `#rgb`, `#rrggbb` or `#rrggbbaa`.
pub fn sanitize_card_color(_key: &str, color: &str) -> Option<String> {
    js::is_hex_color(color, &[3, 6, 8]).then(|| color.to_string())
}

/// `MAX_NOTE_LENGTH`, in UTF-16 units.
pub const MAX_NOTE_LENGTH: usize = 180;

/// `sanitizeNote`: control characters become spaces, then trim and cut.
pub fn sanitize_note(_key: &str, value: &str) -> Option<String> {
    let spaced: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = js::utf16_prefix(js::trim(&spaced), MAX_NOTE_LENGTH);
    (!clean.is_empty()).then_some(clean)
}

// ------------------------------------------------------------ viewMode

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

/// `getViewMode`: `"list"` is the list, anything else the grid.
pub fn view_mode_from_value(value: Option<&Value>) -> ViewMode {
    if js::as_str(value) == Some("list") {
        ViewMode::List
    } else {
        ViewMode::Grid
    }
}

impl ViewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewMode::Grid => "grid",
            ViewMode::List => "list",
        }
    }
}

// ------------------------------------------------------------ defaultGameStore

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultGame {
    pub app_id: String,
    pub name: String,
}

/// The default game store's `sanitize`: entries with a numeric `appId`.
pub fn sanitize_default_games(value: Option<&Value>) -> BTreeMap<String, DefaultGame> {
    let mut out = BTreeMap::new();
    let Some(record) = js::as_record(value) else {
        return out;
    };
    for (key, raw) in record {
        if js::trim(key).is_empty() {
            continue;
        }
        let Some(entry) = js::as_record(Some(raw)) else {
            continue;
        };
        let app_id = js::as_str(entry.get("appId")).map_or("", js::trim);
        let name = js::as_str(entry.get("name")).unwrap_or_default();
        if !js::is_ascii_digits(app_id) {
            continue;
        }
        out.insert(
            key.clone(),
            DefaultGame {
                app_id: app_id.to_string(),
                name: name.to_string(),
            },
        );
    }
    out
}

// ------------------------------------------------------------ ClientStores

/// Every client store the UI reads, typed, over the raw [`ClientStorage`].
#[derive(Debug, Clone)]
pub struct ClientStores {
    ctx: StoreContext,
    storage: ClientStorage,
    settings: AppSettings,
    folders: FolderStore,
    personas: Vec<Persona>,
    account_card_notes: StringMapStore,
    account_card_colors: StringMapStore,
    folder_card_colors: StringMapStore,
    default_games: BTreeMap<String, DefaultGame>,
    view_mode: ViewMode,
    /// The storage revision each typed copy was built from or written at.
    seen: BTreeMap<StoreId, u64>,
}

const TYPED_STORES: [StoreId; 8] = [
    CLIENT_STORE_SETTINGS,
    CLIENT_STORE_FOLDERS,
    CLIENT_STORE_PERSONAS,
    CLIENT_STORE_ACCOUNT_CARD_NOTES,
    CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_ACCOUNT_DEFAULT_GAME,
    CLIENT_STORE_FOLDER_CARD_COLORS,
    CLIENT_STORE_VIEW_MODE,
];

impl ClientStores {
    pub fn new(storage: ClientStorage, ctx: StoreContext) -> Self {
        let settings = settings::sanitize_settings(&Value::Null, &ctx);
        let mut stores = Self {
            ctx,
            storage,
            settings,
            folders: FolderStore::default(),
            personas: Vec::new(),
            account_card_notes: StringMapStore::new(CLIENT_STORE_ACCOUNT_CARD_NOTES, sanitize_note),
            account_card_colors: StringMapStore::new(
                CLIENT_STORE_ACCOUNT_CARD_COLORS,
                sanitize_card_color,
            ),
            folder_card_colors: StringMapStore::new(
                CLIENT_STORE_FOLDER_CARD_COLORS,
                sanitize_card_color,
            ),
            default_games: BTreeMap::new(),
            view_mode: ViewMode::Grid,
            seen: BTreeMap::new(),
        };
        for id in TYPED_STORES {
            stores.reload(id);
        }
        stores
    }

    fn reload(&mut self, id: StoreId) {
        let value = self.storage.get(id);
        match id {
            CLIENT_STORE_SETTINGS => {
                self.settings =
                    settings::sanitize_settings(value.unwrap_or(&Value::Null), &self.ctx)
            }
            CLIENT_STORE_FOLDERS => self.folders = folders::load_folder_store(value),
            CLIENT_STORE_PERSONAS => {
                self.personas = personas::sanitize_personas(
                    value.unwrap_or(&Value::Array(Vec::new())),
                    &self.ctx,
                )
            }
            CLIENT_STORE_ACCOUNT_CARD_NOTES => self.account_card_notes.load(value),
            CLIENT_STORE_ACCOUNT_CARD_COLORS => self.account_card_colors.load(value),
            CLIENT_STORE_FOLDER_CARD_COLORS => self.folder_card_colors.load(value),
            CLIENT_STORE_ACCOUNT_DEFAULT_GAME => self.default_games = sanitize_default_games(value),
            CLIENT_STORE_VIEW_MODE => self.view_mode = view_mode_from_value(value),
            _ => return,
        }
        self.seen.insert(id, self.storage.revision(id));
    }

    /// Rebuilds every typed copy whose store changed in the storage since it
    /// was built. Returns the stores it rebuilt.
    pub fn sync(&mut self) -> Vec<StoreId> {
        let stale: Vec<StoreId> = TYPED_STORES
            .into_iter()
            .filter(|id| self.seen.get(id).copied() != Some(self.storage.revision(id)))
            .collect();
        for id in &stale {
            self.reload(id);
        }
        stale
    }

    fn write(&mut self, id: StoreId, value: Value) {
        self.storage.set(id, value);
        self.seen.insert(id, self.storage.revision(id));
    }

    pub fn context(&self) -> &StoreContext {
        &self.ctx
    }

    /// Replaces the platform and theme lists. Like TS, nothing is
    /// re-sanitized until the next write or reload.
    pub fn set_context(&mut self, ctx: StoreContext) {
        self.ctx = ctx;
    }

    pub fn storage(&self) -> &ClientStorage {
        &self.storage
    }

    /// The raw storage, for the stores without a typed copy here (profile and
    /// ban caches). Call [`ClientStores::sync`] after changing a typed store
    /// through it.
    pub fn storage_mut(&mut self) -> &mut ClientStorage {
        &mut self.storage
    }

    // -------------------------------------------------------- saves

    pub fn take_due_saves(&mut self, now: Instant) -> Vec<PendingSave> {
        self.storage.take_due_saves(now)
    }

    pub fn take_all_saves(&mut self) -> Vec<PendingSave> {
        self.storage.take_all_saves()
    }

    pub fn finish_save(&mut self, store_id: StoreId, result: &BackendResult<Option<String>>) {
        self.storage.finish_save(store_id, result);
    }

    pub fn next_save_due(&self) -> Option<Instant> {
        self.storage.next_save_due()
    }

    // -------------------------------------------------------- snapshot and refresh

    pub fn apply_snapshot(&mut self, snapshot: ClientStorageSnapshot) -> Vec<StoreId> {
        self.storage.apply_snapshot(snapshot);
        self.sync()
    }

    pub fn begin_refresh(&mut self, next: &crate::types::StorageManifest) -> Option<RefreshPlan> {
        self.storage.begin_refresh(next)
    }

    /// Applies a refresh and rebuilds the typed stores it touched. Returns
    /// every changed id, as `refreshClientStorageIfChanged`.
    pub fn finish_refresh(
        &mut self,
        plan: RefreshPlan,
        snapshot: ClientStorageSnapshot,
    ) -> Vec<String> {
        let changed = self.storage.finish_refresh(plan, snapshot);
        self.sync();
        changed
    }

    /// `refreshClientStorageIfChanged`, blocking.
    pub fn refresh_blocking(&mut self, backend: &dyn Backend) -> BackendResult<Vec<String>> {
        let changed = self.storage.refresh_blocking(backend);
        self.sync();
        changed
    }

    // -------------------------------------------------------- settings

    /// `peekSettings` / `getSettings`.
    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// `hasStoredSettings`: false only on a fresh install.
    pub fn has_stored_settings(&self) -> bool {
        self.storage.get(CLIENT_STORE_SETTINGS).is_some()
    }

    /// `saveSettings`: sanitized, then written. (The TS boot-theme
    /// `localStorage` hint has no counterpart here.)
    pub fn save_settings(&mut self, next: &AppSettings) {
        let sanitized = settings::sanitize_settings(&next.to_value(), &self.ctx);
        let value = sanitized.to_value();
        self.settings = sanitized;
        self.write(CLIENT_STORE_SETTINGS, value);
    }

    /// `getCacheDuration`, in milliseconds.
    pub fn cache_duration_ms(&self) -> u64 {
        settings::cache_duration_ms(&self.settings)
    }

    pub fn language(&self) -> Locale {
        self.settings.language
    }

    // -------------------------------------------------------- folders

    pub fn folders(&self) -> &FolderStore {
        &self.folders
    }

    fn write_folders(&mut self) {
        self.folders.version = folders::CURRENT_VERSION;
        let value = self.folders.to_value();
        self.write(CLIENT_STORE_FOLDERS, value);
    }

    pub fn items_in_folder(&self, folder_id: Option<&str>, platform: &str) -> &[ItemRef] {
        self.folders.items_in_folder(folder_id, platform)
    }

    pub fn root_sections(&self, platform: &str) -> Vec<FolderSection> {
        self.folders.root_sections(platform)
    }

    pub fn create_folder(
        &mut self,
        name: &str,
        parent_id: Option<&str>,
        platform: &str,
    ) -> FolderInfo {
        let folder =
            self.folders
                .create_folder(folders::generate_folder_id(), name, parent_id, platform);
        self.write_folders();
        folder
    }

    pub fn sync_accounts(&mut self, account_ids: &[String], platform: &str) {
        if self.folders.sync_accounts(account_ids, platform) {
            self.write_folders();
        }
    }

    pub fn move_item(
        &mut self,
        item: &ItemRef,
        from_folder_id: Option<&str>,
        to_folder_id: Option<&str>,
        platform: &str,
        insert_index: Option<usize>,
    ) {
        if self
            .folders
            .move_item(item, from_folder_id, to_folder_id, platform, insert_index)
        {
            self.write_folders();
        }
    }

    pub fn reorder_items(&mut self, folder_id: Option<&str>, platform: &str, items: Vec<ItemRef>) {
        self.folders.reorder_items(folder_id, platform, items);
        self.write_folders();
    }

    pub fn delete_folder(&mut self, folder_id: &str) {
        if self.folders.delete_folder(folder_id) {
            self.write_folders();
        }
    }

    pub fn rename_folder(&mut self, folder_id: &str, new_name: &str) {
        if self.folders.rename_folder(folder_id, new_name) {
            self.write_folders();
        }
    }

    // -------------------------------------------------------- personas

    /// `getPersonas`.
    pub fn personas(&self) -> &[Persona] {
        &self.personas
    }

    fn persist_personas(&mut self, list: Vec<Persona>) {
        let value = personas::personas_to_value(&list);
        let sanitized = personas::sanitize_personas(&value, &self.ctx);
        let value = personas::personas_to_value(&sanitized);
        self.personas = sanitized;
        self.write(CLIENT_STORE_PERSONAS, value);
    }

    /// `createPersona`: returns the persona as given, with its new id.
    pub fn create_persona(&mut self, input: PersonaInput) -> Persona {
        let persona = Persona {
            id: personas::generate_persona_id(),
            name: input.name,
            color: input.color,
            image: input.image,
            assignments: input.assignments,
        };
        let mut list = self.personas.clone();
        list.push(persona.clone());
        self.persist_personas(list);
        persona
    }

    /// `updatePersona`.
    pub fn update_persona(&mut self, id: &str, patch: &PersonaPatch) {
        let list = self
            .personas
            .iter()
            .cloned()
            .map(|mut persona| {
                if persona.id == id {
                    persona.apply(patch);
                }
                persona
            })
            .collect();
        self.persist_personas(list);
    }

    /// `deletePersona`.
    pub fn delete_persona(&mut self, id: &str) {
        let list = self
            .personas
            .iter()
            .filter(|persona| persona.id != id)
            .cloned()
            .collect();
        self.persist_personas(list);
    }

    // -------------------------------------------------------- string maps

    fn write_map(&mut self, which: StoreId) {
        let value = match which {
            CLIENT_STORE_ACCOUNT_CARD_NOTES => self.account_card_notes.to_value(),
            CLIENT_STORE_ACCOUNT_CARD_COLORS => self.account_card_colors.to_value(),
            _ => self.folder_card_colors.to_value(),
        };
        self.write(which, value);
    }

    pub fn account_card_note(&self, account_id: &str) -> &str {
        self.account_card_notes.get(account_id)
    }

    pub fn account_card_notes(&self) -> &StringMapStore {
        &self.account_card_notes
    }

    pub fn set_account_card_note(&mut self, account_id: &str, note: &str) {
        self.account_card_notes.set(account_id, note);
        self.write_map(CLIENT_STORE_ACCOUNT_CARD_NOTES);
    }

    pub fn clear_account_card_note(&mut self, account_id: &str) {
        self.account_card_notes.remove(account_id);
        self.write_map(CLIENT_STORE_ACCOUNT_CARD_NOTES);
    }

    pub fn account_card_color(&self, account_id: &str) -> &str {
        self.account_card_colors.get(account_id)
    }

    pub fn set_account_card_color(&mut self, account_id: &str, color: &str) {
        self.account_card_colors.set(account_id, color);
        self.write_map(CLIENT_STORE_ACCOUNT_CARD_COLORS);
    }

    /// `setAccountCardColors`: one write for every id.
    pub fn set_account_card_colors(&mut self, account_ids: &[String], color: &str) {
        if self.account_card_colors.set_many(account_ids, color) {
            self.write_map(CLIENT_STORE_ACCOUNT_CARD_COLORS);
        }
    }

    pub fn folder_card_color(&self, folder_id: &str) -> &str {
        self.folder_card_colors.get(folder_id)
    }

    pub fn set_folder_card_color(&mut self, folder_id: &str, color: &str) {
        self.folder_card_colors.set(folder_id, color);
        self.write_map(CLIENT_STORE_FOLDER_CARD_COLORS);
    }

    // -------------------------------------------------------- default game

    /// `getDefaultGame`.
    pub fn default_game(&self, steam_id: &str) -> Option<&DefaultGame> {
        self.default_games.get(steam_id)
    }

    fn write_default_games(&mut self) {
        let value = serde_json::to_value(&self.default_games).unwrap_or(Value::Null);
        self.write(CLIENT_STORE_ACCOUNT_DEFAULT_GAME, value);
    }

    /// `setDefaultGame`: ignored unless the app id is numeric.
    pub fn set_default_game(&mut self, steam_id: &str, game: &DefaultGame) {
        let app_id = js::trim(&game.app_id);
        if !js::is_ascii_digits(app_id) {
            return;
        }
        self.default_games.insert(
            steam_id.to_string(),
            DefaultGame {
                app_id: app_id.to_string(),
                name: game.name.clone(),
            },
        );
        self.write_default_games();
    }

    /// `clearDefaultGame`: no write when there is none.
    pub fn clear_default_game(&mut self, steam_id: &str) {
        if self.default_games.remove(steam_id).is_some() {
            self.write_default_games();
        }
    }

    // -------------------------------------------------------- view mode

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.write(
            CLIENT_STORE_VIEW_MODE,
            Value::String(mode.as_str().to_string()),
        );
    }
}
