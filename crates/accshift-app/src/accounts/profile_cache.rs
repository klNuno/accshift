//! The persisted profile caches, ported from `src/lib/shared/profileCache.ts`
//! and the two caches built on it (`platforms/steam/profileCache.ts`,
//! `platforms/roblox/profileCache.ts`).
//!
//! The cache lives in a client store, which the UI thread owns, while fetches
//! run on the job pool. So a fetch does not write the store: it returns the
//! [`CacheWrite`]s TS would have made, in order, and the UI applies them with
//! [`ProfileCache::apply_writes`]. Reads ([`ProfileCache::cached_profile`])
//! keep the TS revision memo. The in-flight map is shared between job threads
//! ([`InFlight`]): a second request for an id already being fetched waits for
//! that fetch instead of starting one, in both the unit and the batch path.

use crate::backend::Backend;
use crate::error::BackendResult;
use crate::platforms::{CachedPlatformProfile, PlatformProfileInfo};
use crate::storage::{ClientStorage, StoreId};
use crate::types::{ProfileInfo, RobloxProfileInfo};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// Which avatar URLs a cache and the avatar images accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UrlPolicy {
    /// `isSafeHttpUrl`: `http:` and `https:` only. The real backend.
    #[default]
    HttpOnly,
    /// Also local files. The mock: its avatars are files under
    /// `src/lib/mock/avatars/`, where the Tauri mock served them from the
    /// Vite dev server over `http:`.
    HttpOrFile,
}

impl UrlPolicy {
    /// An absolute path, a `file://` URL, or a repo-relative mock avatar.
    pub fn is_local_file(url: &str) -> bool {
        let path = url.strip_prefix("file://").unwrap_or(url);
        !path.is_empty()
            && (std::path::Path::new(path).is_absolute()
                || path.starts_with('/')
                || path.starts_with(crate::mock::AVATAR_DIR))
    }

    pub fn accepts(self, url: &str) -> bool {
        crate::platforms::steam::is_safe_http_url(url)
            || (self == UrlPolicy::HttpOrFile && Self::is_local_file(url))
    }
}

/// `CacheEntry`. `url: None` is a negative entry: fetched, no avatar.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheEntry {
    pub url: Option<String>,
    pub display_name: Option<String>,
    /// Unix milliseconds.
    pub timestamp: f64,
}

impl CacheEntry {
    fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert(
            "url".into(),
            self.url.clone().map_or(Value::Null, Value::String),
        );
        if let Some(name) = &self.display_name {
            out.insert("displayName".into(), Value::String(name.clone()));
        }
        out.insert(
            "timestamp".into(),
            serde_json::Number::from_f64(self.timestamp).map_or(Value::Null, Value::Number),
        );
        Value::Object(out)
    }
}

/// One store write a fetch made, to replay on the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheWrite {
    pub account_id: String,
    pub entry: CacheEntry,
}

/// `ProfileCacheOptions` minus the fetchers, which the call sites pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    pub store_id: StoreId,
    /// Total fetch attempts.
    pub attempts: u32,
    pub retry_delay: Duration,
    /// Store `url: None` when every attempt returned a profile without a
    /// usable avatar.
    pub cache_negative_results: bool,
}

/// `platforms/steam/profileCache.ts`: four attempts 750 ms apart, then a
/// negative entry (profile data can lag right after Steam writes
/// `loginusers.vdf`; private profiles expose no avatar).
pub const STEAM_PROFILE_CACHE: CacheConfig = CacheConfig {
    store_id: crate::storage::CLIENT_STORE_STEAM_PROFILE_CACHE,
    attempts: 4,
    retry_delay: Duration::from_millis(750),
    cache_negative_results: true,
};

/// `platforms/roblox/profileCache.ts`: one attempt, no negative entries.
pub const ROBLOX_PROFILE_CACHE: CacheConfig = CacheConfig {
    store_id: crate::storage::CLIENT_STORE_ROBLOX_PROFILE_CACHE,
    attempts: 1,
    retry_delay: Duration::ZERO,
    cache_negative_results: false,
};

/// `getAvatarUrl` and `getDisplayName` of a cache.
pub trait CachedProfile: Clone + Send + 'static {
    fn avatar_url(&self) -> Option<&str>;
    fn display_name(&self) -> Option<&str> {
        None
    }
}

impl CachedProfile for ProfileInfo {
    fn avatar_url(&self) -> Option<&str> {
        self.avatar_url.as_deref()
    }
    fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

impl CachedProfile for RobloxProfileInfo {
    fn avatar_url(&self) -> Option<&str> {
        self.avatar_url.as_deref()
    }
}

/// What expiry compares against: `cacheDurationProvider()` and the session
/// start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CacheExpiry {
    /// `getCacheDuration()`; 0 expires every entry older than the session.
    pub duration_ms: u64,
    /// `SESSION_START_MS`.
    pub session_start_ms: f64,
    pub now_ms: f64,
}

impl CacheExpiry {
    fn expired(&self, timestamp: f64) -> bool {
        if self.duration_ms == 0 {
            timestamp < self.session_start_ms
        } else {
            self.now_ms - timestamp > self.duration_ms as f64
        }
    }
}

// ------------------------------------------------------------ in flight

#[derive(Debug)]
struct Slot<P> {
    value: Mutex<Option<Option<P>>>,
    ready: Condvar,
}

impl<P: Clone> Slot<P> {
    fn new() -> Self {
        Self {
            value: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn set(&self, value: Option<P>) {
        let mut guard = lock(&self.value);
        *guard = Some(value);
        self.ready.notify_all();
    }

    fn wait(&self) -> Option<P> {
        let mut guard = lock(&self.value);
        loop {
            if let Some(value) = guard.as_ref() {
                return value.clone();
            }
            guard = self
                .ready
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Fetch slots keyed by account id.
type Claimed<P> = Vec<(String, Arc<Slot<P>>)>;

/// A batch fetcher: profiles keyed by id for a list of ids.
pub type BatchFetcher<'a, P> = &'a dyn Fn(&[String]) -> BackendResult<BTreeMap<String, P>>;

/// `inFlight`: the fetches running now, shared between job threads.
#[derive(Debug)]
pub struct InFlight<P> {
    slots: Mutex<HashMap<String, Arc<Slot<P>>>>,
}

impl<P> Default for InFlight<P> {
    fn default() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }
}

impl<P: Clone> InFlight<P> {
    pub fn len(&self) -> usize {
        lock(&self.slots).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Claims every id nobody fetches yet; the others come back as slots to
    /// wait on.
    fn claim(&self, ids: &[String]) -> (Claimed<P>, Claimed<P>) {
        let mut slots = lock(&self.slots);
        let mut mine = Vec::new();
        let mut theirs = Vec::new();
        for id in ids {
            match slots.get(id) {
                Some(slot) => theirs.push((id.clone(), Arc::clone(slot))),
                None => {
                    let slot = Arc::new(Slot::new());
                    slots.insert(id.clone(), Arc::clone(&slot));
                    mine.push((id.clone(), slot));
                }
            }
        }
        (mine, theirs)
    }

    fn release(&self, id: &str, slot: &Arc<Slot<P>>, value: Option<P>) {
        slot.set(value);
        let mut slots = lock(&self.slots);
        if slots
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            slots.remove(id);
        }
    }
}

// ------------------------------------------------------------ cache

/// One fetch's answer: the profiles and the cache writes to replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched<T> {
    pub value: T,
    pub writes: Vec<CacheWrite>,
}

/// `createProfileCache`.
#[derive(Debug)]
pub struct ProfileCache<P> {
    config: CacheConfig,
    policy: UrlPolicy,
    /// `cachedEntries` and `cachedRevision`.
    memo: Mutex<Option<(u64, BTreeMap<String, CacheEntry>)>>,
    in_flight: InFlight<P>,
}

impl<P: CachedProfile> ProfileCache<P> {
    pub fn new(config: CacheConfig, policy: UrlPolicy) -> Self {
        Self {
            config,
            policy,
            memo: Mutex::new(None),
            in_flight: InFlight::default(),
        }
    }

    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    pub fn policy(&self) -> UrlPolicy {
        self.policy
    }

    pub fn in_flight(&self) -> &InFlight<P> {
        &self.in_flight
    }

    /// `sanitizeEntry`.
    fn sanitize_entry(&self, value: &Value) -> Option<CacheEntry> {
        let raw = value.as_object()?;
        let url = raw.get("url");
        let valid = match url {
            Some(Value::Null) => self.config.cache_negative_results,
            Some(Value::String(text)) => {
                !crate::js::trim(text).is_empty() && self.policy.accepts(text)
            }
            _ => false,
        };
        if !valid {
            return None;
        }
        let timestamp = crate::js::to_number(raw.get("timestamp"));
        if !timestamp.is_finite() || timestamp < 0.0 {
            return None;
        }
        Some(CacheEntry {
            url: url.and_then(Value::as_str).map(str::to_string),
            display_name: raw
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::to_string),
            timestamp,
        })
    }

    fn sanitize(&self, value: Option<&Value>) -> BTreeMap<String, CacheEntry> {
        let mut out = BTreeMap::new();
        let Some(Value::Object(map)) = value else {
            return out;
        };
        for (account_id, entry) in map {
            if crate::js::trim(account_id).is_empty() {
                continue;
            }
            if let Some(entry) = self.sanitize_entry(entry) {
                out.insert(account_id.clone(), entry);
            }
        }
        out
    }

    /// `getCache`: the sanitized entries, rebuilt only when the store's
    /// revision moved.
    pub fn entries(&self, storage: &ClientStorage) -> BTreeMap<String, CacheEntry> {
        let revision = storage.revision(self.config.store_id);
        let mut memo = lock(&self.memo);
        if let Some((seen, entries)) = memo.as_ref() {
            if *seen == revision {
                return entries.clone();
            }
        }
        let entries = self.sanitize(storage.get(self.config.store_id));
        *memo = Some((revision, entries.clone()));
        entries
    }

    fn entry(&self, storage: &ClientStorage, account_id: &str) -> Option<CacheEntry> {
        let revision = storage.revision(self.config.store_id);
        {
            let memo = lock(&self.memo);
            if let Some((seen, entries)) = memo.as_ref() {
                if *seen == revision {
                    return entries.get(account_id).cloned();
                }
            }
        }
        self.entries(storage).get(account_id).cloned()
    }

    /// `getCachedProfile`.
    pub fn cached_profile(
        &self,
        storage: &ClientStorage,
        account_id: &str,
        expiry: &CacheExpiry,
    ) -> Option<CachedPlatformProfile> {
        let entry = self.entry(storage, account_id)?;
        Some(CachedPlatformProfile {
            url: entry.url,
            display_name: entry.display_name,
            expired: expiry.expired(entry.timestamp),
        })
    }

    /// `saveCache`.
    fn save(&self, storage: &mut ClientStorage, entries: BTreeMap<String, CacheEntry>) {
        let value = Value::Object(
            entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.to_value()))
                .collect(),
        );
        storage.set(self.config.store_id, value);
        *lock(&self.memo) = Some((storage.revision(self.config.store_id), entries));
    }

    /// `setCachedProfile`: ignored for a URL the policy refuses.
    pub fn set_cached_profile(
        &self,
        storage: &mut ClientStorage,
        account_id: &str,
        url: &str,
        display_name: Option<&str>,
        now_ms: f64,
    ) {
        if !self.policy.accepts(url) {
            return;
        }
        self.apply_writes(
            storage,
            vec![CacheWrite {
                account_id: account_id.to_string(),
                entry: CacheEntry {
                    url: Some(url.to_string()),
                    display_name: display_name.map(str::to_string),
                    timestamp: now_ms,
                },
            }],
        );
    }

    /// Replays a fetch's writes, in order, in one store write.
    pub fn apply_writes(&self, storage: &mut ClientStorage, writes: Vec<CacheWrite>) {
        if writes.is_empty() {
            return;
        }
        let mut entries = self.entries(storage);
        for write in writes {
            entries.insert(write.account_id, write.entry);
        }
        self.save(storage, entries);
    }

    fn has_safe_avatar_url(&self, profile: &P) -> bool {
        let avatar = crate::js::trim(profile.avatar_url().unwrap_or_default());
        !avatar.is_empty() && self.policy.accepts(avatar)
    }

    fn positive_write(&self, account_id: &str, profile: &P, now_ms: f64) -> Option<CacheWrite> {
        if !self.has_safe_avatar_url(profile) {
            return None;
        }
        Some(CacheWrite {
            account_id: account_id.to_string(),
            entry: CacheEntry {
                url: profile.avatar_url().map(str::to_string),
                display_name: profile.display_name().map(str::to_string),
                timestamp: now_ms,
            },
        })
    }

    fn negative_write(&self, account_id: &str, profile: &P, now_ms: f64) -> CacheWrite {
        CacheWrite {
            account_id: account_id.to_string(),
            entry: CacheEntry {
                url: None,
                display_name: profile.display_name().map(str::to_string),
                timestamp: now_ms,
            },
        }
    }

    fn pause(&self, attempt: u32) {
        if attempt + 1 < self.config.attempts && !self.config.retry_delay.is_zero() {
            std::thread::sleep(self.config.retry_delay);
        }
    }

    /// `fetchWithRetries`.
    fn fetch_with_retries(
        &self,
        account_id: &str,
        fetcher: &dyn Fn(&str) -> BackendResult<Option<P>>,
        now_ms: &dyn Fn() -> f64,
        writes: &mut Vec<CacheWrite>,
    ) -> Option<P> {
        let mut last = None;
        for attempt in 0..self.config.attempts {
            if let Ok(Some(profile)) = fetcher(account_id) {
                writes.extend(self.positive_write(account_id, &profile, now_ms()));
                let safe = self.has_safe_avatar_url(&profile);
                last = Some(profile);
                if safe {
                    return last;
                }
            }
            self.pause(attempt);
        }
        if self.config.cache_negative_results {
            if let Some(profile) = &last {
                writes.push(self.negative_write(account_id, profile, now_ms()));
            }
        }
        last
    }

    /// `fetchProfile`: one account, deduplicated against every fetch in
    /// flight. Blocks; run it on the job pool.
    pub fn fetch_profile(
        &self,
        account_id: &str,
        fetcher: &dyn Fn(&str) -> BackendResult<Option<P>>,
        now_ms: &dyn Fn() -> f64,
    ) -> Fetched<Option<P>> {
        let ids = [account_id.to_string()];
        let (mine, theirs) = self.in_flight.claim(&ids);
        if let Some((_, slot)) = theirs.into_iter().next() {
            return Fetched {
                value: slot.wait(),
                writes: Vec::new(),
            };
        }
        let mut writes = Vec::new();
        let value = self.fetch_with_retries(account_id, fetcher, now_ms, &mut writes);
        if let Some((id, slot)) = mine.into_iter().next() {
            self.in_flight.release(&id, &slot, value.clone());
        }
        Fetched { value, writes }
    }

    /// `batchFetchWithRetries`: retries only the ids without a usable avatar.
    fn batch_fetch_with_retries(
        &self,
        account_ids: &[String],
        batch: BatchFetcher<'_, P>,
        now_ms: &dyn Fn() -> f64,
        writes: &mut Vec<CacheWrite>,
    ) -> BTreeMap<String, Option<P>> {
        let mut results: BTreeMap<String, Option<P>> =
            account_ids.iter().map(|id| (id.clone(), None)).collect();
        let mut remaining: Vec<String> = account_ids.to_vec();
        for attempt in 0..self.config.attempts {
            if let Ok(fetched) = batch(&remaining) {
                for (account_id, profile) in fetched {
                    if !results.contains_key(&account_id) {
                        continue;
                    }
                    writes.extend(self.positive_write(&account_id, &profile, now_ms()));
                    results.insert(account_id, Some(profile));
                }
            }
            remaining.retain(|id| {
                results
                    .get(id)
                    .and_then(Option::as_ref)
                    .is_none_or(|profile| !self.has_safe_avatar_url(profile))
            });
            if remaining.is_empty() {
                break;
            }
            self.pause(attempt);
        }
        if self.config.cache_negative_results {
            for account_id in account_ids {
                if let Some(Some(profile)) = results.get(account_id) {
                    if !self.has_safe_avatar_url(profile) {
                        writes.push(self.negative_write(account_id, profile, now_ms()));
                    }
                }
            }
        }
        results
    }

    /// `fetchProfiles`: many accounts through `batch` when given, else one
    /// [`ProfileCache::fetch_profile`] per id. Ids already in flight are
    /// waited on, not fetched again. Blocks; run it on the job pool.
    pub fn fetch_profiles(
        &self,
        account_ids: &[String],
        batch: Option<BatchFetcher<'_, P>>,
        fetcher: &dyn Fn(&str) -> BackendResult<Option<P>>,
        now_ms: &dyn Fn() -> f64,
    ) -> Fetched<BTreeMap<String, Option<P>>> {
        let mut unique: Vec<String> = Vec::new();
        for id in account_ids {
            if !unique.contains(id) {
                unique.push(id.clone());
            }
        }
        let mut writes = Vec::new();
        let mut value = BTreeMap::new();
        let Some(batch) = batch else {
            for id in &unique {
                let fetched = self.fetch_profile(id, fetcher, now_ms);
                writes.extend(fetched.writes);
                value.insert(id.clone(), fetched.value);
            }
            return Fetched { value, writes };
        };
        let (mine, theirs) = self.in_flight.claim(&unique);
        if !mine.is_empty() {
            let ids: Vec<String> = mine.iter().map(|(id, _)| id.clone()).collect();
            let results = self.batch_fetch_with_retries(&ids, batch, now_ms, &mut writes);
            for (id, slot) in mine {
                let profile = results.get(&id).cloned().flatten();
                self.in_flight.release(&id, &slot, profile.clone());
                value.insert(id, profile);
            }
        }
        for (id, slot) in theirs {
            value.insert(id, slot.wait());
        }
        Fetched { value, writes }
    }
}

// ------------------------------------------------------------ the two caches

/// The Steam and Roblox caches of a session, plus the platform profile
/// mapping (`toPlatformProfileInfo`) under the same URL policy.
#[derive(Debug)]
pub struct ProfileCaches {
    pub steam: ProfileCache<ProfileInfo>,
    pub roblox: ProfileCache<RobloxProfileInfo>,
    /// `SESSION_START_MS`.
    pub session_start_ms: f64,
}

impl ProfileCaches {
    pub fn new(policy: UrlPolicy, session_start_ms: f64) -> Self {
        Self {
            steam: ProfileCache::new(STEAM_PROFILE_CACHE, policy),
            roblox: ProfileCache::new(ROBLOX_PROFILE_CACHE, policy),
            session_start_ms,
        }
    }

    pub fn policy(&self) -> UrlPolicy {
        self.steam.policy()
    }

    /// The expiry rule for `now_ms` with the settings' cache duration.
    pub fn expiry(&self, duration_ms: u64, now_ms: f64) -> CacheExpiry {
        CacheExpiry {
            duration_ms,
            session_start_ms: self.session_start_ms,
            now_ms,
        }
    }

    /// `adapter.getCachedProfile(id)`: Steam and Roblox only.
    pub fn cached_profile(
        &self,
        platform_id: &str,
        storage: &ClientStorage,
        account_id: &str,
        expiry: &CacheExpiry,
    ) -> Option<CachedPlatformProfile> {
        match platform_id {
            "steam" => self.steam.cached_profile(storage, account_id, expiry),
            "roblox" => self.roblox.cached_profile(storage, account_id, expiry),
            _ => None,
        }
    }

    /// Whether `platform_id` has a cache here.
    pub fn has_cache(platform_id: &str) -> bool {
        matches!(platform_id, "steam" | "roblox")
    }

    /// Replays a fetch's writes into the platform's store.
    pub fn apply_writes(
        &self,
        platform_id: &str,
        storage: &mut ClientStorage,
        writes: Vec<CacheWrite>,
    ) {
        match platform_id {
            "steam" => self.steam.apply_writes(storage, writes),
            "roblox" => self.roblox.apply_writes(storage, writes),
            _ => {}
        }
    }

    /// Steam's `toPlatformProfileInfo` under this policy.
    pub fn steam_profile_info(&self, profile: Option<&ProfileInfo>) -> Option<PlatformProfileInfo> {
        let profile = profile?;
        let avatar = crate::js::trim(profile.avatar_url.as_deref().unwrap_or_default());
        Some(PlatformProfileInfo {
            avatar_url: (!avatar.is_empty() && self.policy().accepts(avatar))
                .then(|| avatar.to_string()),
            display_name: profile.display_name.clone(),
            avatar_loading: false,
        })
    }

    /// Roblox's `getProfileInfo` mapping under this policy.
    pub fn roblox_profile_info(
        &self,
        info: Option<&RobloxProfileInfo>,
    ) -> Option<PlatformProfileInfo> {
        let info = info?;
        Some(PlatformProfileInfo {
            avatar_url: info
                .avatar_url
                .as_deref()
                .filter(|url| !url.is_empty() && self.policy().accepts(url))
                .map(str::to_string),
            display_name: None,
            avatar_loading: false,
        })
    }

    /// `adapter.getProfileInfo(id)` through the caches: Steam and Roblox
    /// fetch with their cache rules, Riot reads its profile cache, others
    /// have none. Blocks.
    pub fn profile_info(
        &self,
        adapter: &crate::platforms::PlatformAdapter,
        backend: &dyn Backend,
        account_id: &str,
        now_ms: &dyn Fn() -> f64,
    ) -> Fetched<Option<PlatformProfileInfo>> {
        match adapter.id() {
            "steam" => {
                let fetched = self.steam.fetch_profile(
                    account_id,
                    &|id| backend.steam_get_profile_info(id),
                    now_ms,
                );
                Fetched {
                    value: self.steam_profile_info(fetched.value.as_ref()),
                    writes: fetched.writes,
                }
            }
            "roblox" => {
                let fetched = self.roblox.fetch_profile(
                    account_id,
                    &|id| backend.roblox_get_profile_info(id).map(Some),
                    now_ms,
                );
                Fetched {
                    value: self.roblox_profile_info(fetched.value.as_ref()),
                    writes: fetched.writes,
                }
            }
            _ => Fetched {
                value: adapter.profile_info(backend, account_id).ok().flatten(),
                writes: Vec::new(),
            },
        }
    }

    /// `adapter.getProfileInfos(ids)`: Steam's batch path. `None` for other
    /// platforms (they have no batch fetcher). Blocks.
    pub fn steam_profile_infos(
        &self,
        backend: &dyn Backend,
        account_ids: &[String],
        now_ms: &dyn Fn() -> f64,
    ) -> Fetched<BTreeMap<String, Option<PlatformProfileInfo>>> {
        let fetched = self.steam.fetch_profiles(
            account_ids,
            Some(&|ids: &[String]| backend.steam_get_profile_infos(ids)),
            &|id| backend.steam_get_profile_info(id),
            now_ms,
        );
        let value = account_ids
            .iter()
            .map(|id| {
                let profile = fetched.value.get(id).cloned().flatten();
                (id.clone(), self.steam_profile_info(profile.as_ref()))
            })
            .collect();
        Fetched {
            value,
            writes: fetched.writes,
        }
    }
}
