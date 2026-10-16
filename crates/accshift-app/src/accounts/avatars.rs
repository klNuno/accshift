//! Per account avatar state, ported from `src/lib/shared/useAvatarLoader.svelte.ts`,
//! and the avatar images behind it.
//!
//! [`AvatarStates`] keeps the TS coalescing: updates queue in a pending map
//! that [`AvatarStates::flush`] commits once per frame (the
//! `requestAnimationFrame` flush), and readers inside this module see the
//! pending value first, like `readState`.
//!
//! [`AvatarImages`] is the part the browser did for free: turning an avatar
//! URL into something to draw. It yields an [`AvatarImage`] (a file path or
//! downloaded bytes, plus a stable key); decoding and texture upload belong
//! to the egui side. HTTP runs on the job pool through [`AvatarHttp`]; the
//! mock's avatars are local files and need no fetch.

use super::profile_cache::UrlPolicy;
use crate::platforms::{CachedPlatformProfile, PlatformAccount, PlatformProfileInfo};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// `BATCH_SIZE`: per account profile fetches run this many at a time.
pub const BATCH_SIZE: usize = 15;

/// `AvatarState`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AvatarState {
    pub url: Option<String>,
    pub loading: bool,
    pub refreshing: bool,
}

/// `Partial<AvatarState>` for [`AvatarStates::update_state`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AvatarPatch {
    pub url: Option<Option<String>>,
    pub loading: Option<bool>,
    pub refreshing: Option<bool>,
}

/// What seeding reads from the adapter.
pub trait ProfileSource {
    /// `adapter.getCachedProfile?.(id)`: `None` when the platform has no
    /// cache or no entry.
    fn cached_profile(&self, account_id: &str) -> Option<CachedPlatformProfile>;
    /// `adapter.getProfileInfo` exists.
    fn has_profile_info(&self) -> bool;
}

/// `createAvatarLoader`: the state half.
#[derive(Debug, Clone, Default)]
pub struct AvatarStates {
    states: BTreeMap<String, AvatarState>,
    pending: Option<BTreeMap<String, AvatarState>>,
    /// Bumped by each commit that changed something.
    version: u64,
}

impl AvatarStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// `states`: the committed map, as the UI reads it.
    pub fn states(&self) -> &BTreeMap<String, AvatarState> {
        &self.states
    }

    pub fn get(&self, account_id: &str) -> Option<&AvatarState> {
        self.states.get(account_id)
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Updates are waiting for [`AvatarStates::flush`].
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// `readState`: pending first, then committed.
    pub fn read(&self, account_id: &str) -> Option<&AvatarState> {
        self.pending
            .as_ref()
            .and_then(|pending| pending.get(account_id))
            .or_else(|| self.states.get(account_id))
    }

    /// `flushPending`: commits the queued updates. True when it changed the
    /// committed map.
    pub fn flush(&mut self) -> bool {
        let Some(updates) = self.pending.take() else {
            return false;
        };
        if updates.is_empty() {
            return false;
        }
        self.states.extend(updates);
        self.version += 1;
        true
    }

    fn queue(&mut self, account_id: &str, state: AvatarState) {
        self.pending
            .get_or_insert_with(BTreeMap::new)
            .insert(account_id.to_string(), state);
    }

    /// `updateState`.
    pub fn update_state(&mut self, account_id: &str, patch: AvatarPatch) {
        let previous = self.read(account_id).cloned();
        let base = previous.clone().unwrap_or_default();
        let next = AvatarState {
            url: patch.url.unwrap_or(base.url),
            loading: patch.loading.unwrap_or(base.loading),
            refreshing: patch.refreshing.unwrap_or(base.refreshing),
        };
        if previous.as_ref() == Some(&next) {
            return;
        }
        self.queue(account_id, next);
    }

    /// `mergeStates`.
    pub fn merge(&mut self, updates: BTreeMap<String, AvatarState>) {
        for (account_id, next) in updates {
            if self.read(&account_id) == Some(&next) {
                continue;
            }
            self.queue(&account_id, next);
        }
    }

    /// `seedForAccounts`: the cached avatar of each account, or a loading
    /// placeholder. Returns the accounts whose profile needs a fetch.
    pub fn seed_for_accounts<S: ProfileSource + ?Sized>(
        &mut self,
        accounts: &[PlatformAccount],
        force_refresh: bool,
        source: &S,
    ) -> Vec<PlatformAccount> {
        let mut needs_refresh = Vec::new();
        let mut updates = BTreeMap::new();
        for account in accounts {
            let existing = self.read(&account.id).is_some();
            if let Some(cached) = source.cached_profile(&account.id) {
                let should_refresh = cached.expired || force_refresh;
                updates.insert(
                    account.id.clone(),
                    AvatarState {
                        url: cached.url,
                        loading: false,
                        refreshing: should_refresh,
                    },
                );
                if should_refresh {
                    needs_refresh.push(account.clone());
                }
            } else if source.has_profile_info() {
                if !existing {
                    updates.insert(
                        account.id.clone(),
                        AvatarState {
                            url: None,
                            loading: true,
                            refreshing: false,
                        },
                    );
                }
                needs_refresh.push(account.clone());
            }
        }
        self.merge(updates);
        needs_refresh
    }

    fn resolved_state(
        &self,
        account_id: &str,
        profile: Option<&PlatformProfileInfo>,
    ) -> AvatarState {
        match profile {
            Some(profile) => AvatarState {
                url: profile.avatar_url.clone(),
                loading: profile.avatar_loading,
                refreshing: false,
            },
            // No data: keep whatever avatar is on screen, stop the spinners.
            None => AvatarState {
                url: self
                    .read(account_id)
                    .and_then(|state| state.url.clone())
                    .filter(|url| !url.is_empty()),
                loading: false,
                refreshing: false,
            },
        }
    }

    /// `applyProfileUpdate`: one resolved profile, and its display name
    /// written into `accounts`. Returns whether `accounts` changed.
    pub fn apply_profile_update(
        &mut self,
        account: &PlatformAccount,
        profile: Option<&PlatformProfileInfo>,
        accounts: &mut [PlatformAccount],
    ) -> bool {
        let state = self.resolved_state(&account.id, profile);
        self.update_state(
            &account.id,
            AvatarPatch {
                url: Some(state.url),
                loading: Some(state.loading),
                refreshing: Some(false),
            },
        );
        match profile.and_then(|profile| profile.display_name.as_deref()) {
            Some(name) if !name.is_empty() && name != account.display_name => {
                update_account_display_name(accounts, &account.id, name)
            }
            _ => false,
        }
    }

    /// `applyBatchProfileUpdates`: every resolved profile in one merge.
    /// Returns whether `accounts` changed.
    pub fn apply_batch_profile_updates(
        &mut self,
        targets: &[PlatformAccount],
        profiles: &BTreeMap<String, Option<PlatformProfileInfo>>,
        accounts: &mut [PlatformAccount],
    ) -> bool {
        let mut updates = BTreeMap::new();
        let mut names = Vec::new();
        for account in targets {
            let profile = profiles.get(&account.id).and_then(Option::as_ref);
            updates.insert(
                account.id.clone(),
                self.resolved_state(&account.id, profile),
            );
            if let Some(name) = profile.and_then(|profile| profile.display_name.as_deref()) {
                if !name.is_empty() && name != account.display_name {
                    names.push((account.id.clone(), name.to_string()));
                }
            }
        }
        self.merge(updates);
        let mut changed = false;
        for (account_id, name) in names {
            changed |= update_account_display_name(accounts, &account_id, &name);
        }
        changed
    }

    /// `removeAvatar`.
    pub fn remove(&mut self, account_id: &str) {
        if let Some(pending) = &mut self.pending {
            pending.remove(account_id);
        }
        if self.states.remove(account_id).is_some() {
            self.version += 1;
        }
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.pending = None;
        if !self.states.is_empty() {
            self.states.clear();
            self.version += 1;
        }
    }

    /// Every account id with a committed or pending state.
    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.states.keys().cloned().collect();
        if let Some(pending) = &self.pending {
            ids.extend(
                pending
                    .keys()
                    .filter(|id| !self.states.contains_key(*id))
                    .cloned(),
            );
        }
        ids
    }
}

/// `updateAccountDisplayName`.
pub fn update_account_display_name(
    accounts: &mut [PlatformAccount],
    account_id: &str,
    display_name: &str,
) -> bool {
    match accounts.iter_mut().find(|account| account.id == account_id) {
        Some(account) if account.display_name != display_name => {
            account.display_name = display_name.to_string();
            true
        }
        _ => false,
    }
}

// ------------------------------------------------------------ images

/// An avatar ready for the UI to decode. `key` is stable for the same
/// source, so the UI can key its texture cache on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvatarImage {
    /// A local file (the mock's SVGs).
    File { key: String, path: PathBuf },
    /// Downloaded bytes (PNG or JPEG from Steam and Roblox CDNs).
    Bytes { key: String, bytes: Arc<[u8]> },
}

impl AvatarImage {
    pub fn key(&self) -> &str {
        match self {
            AvatarImage::File { key, .. } | AvatarImage::Bytes { key, .. } => key,
        }
    }
}

/// Where an avatar URL points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvatarSource {
    Http(String),
    File(PathBuf),
    /// A `data:image/...;base64,` URL (a persona's cover): its bytes.
    Data(Vec<u8>),
    /// None of these: the card shows its initials.
    Unsupported,
}

/// Classifies an avatar URL under `policy`.
pub fn avatar_source(url: &str, policy: UrlPolicy) -> AvatarSource {
    let trimmed = crate::js::trim(url);
    if crate::platforms::steam::is_safe_http_url(trimmed) {
        return AvatarSource::Http(trimmed.to_string());
    }
    if policy == UrlPolicy::HttpOrFile && UrlPolicy::is_local_file(trimmed) {
        let path = trimmed.strip_prefix("file://").unwrap_or(trimmed);
        return AvatarSource::File(PathBuf::from(path));
    }
    if let Some(bytes) = data_url_bytes(trimmed) {
        return AvatarSource::Data(bytes);
    }
    AvatarSource::Unsupported
}

/// The payload of a base64 `data:image/` URL.
fn data_url_bytes(url: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let rest = url.strip_prefix("data:image/")?;
    let (meta, payload) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()
        .filter(|bytes| !bytes.is_empty())
}

/// A short cache key for a data URL: its length and a hash, so a 300 kB
/// cover is not copied into every texture key.
fn data_url_key(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("data:{}:{:016x}", url.len(), hasher.finish())
}

/// What [`AvatarImages::request`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvatarFetch {
    Ready(AvatarImage),
    /// A download is running.
    Waiting,
    /// The download failed or the URL is not drawable: show the initials.
    Failed,
    /// Start [`fetch_avatar_image`] for this URL on the job pool and hand the
    /// result to [`AvatarImages::finish`].
    Start(String),
}

#[derive(Debug, Clone)]
enum ImageEntry {
    Loading,
    Ready(AvatarImage),
    Failed,
}

/// The avatar images of the session, by URL.
#[derive(Debug, Clone)]
pub struct AvatarImages {
    policy: UrlPolicy,
    entries: HashMap<String, ImageEntry>,
}

impl AvatarImages {
    pub fn new(policy: UrlPolicy) -> Self {
        Self {
            policy,
            entries: HashMap::new(),
        }
    }

    /// The image for `url`, or what to do to get it. A local file is ready at
    /// once; an HTTP URL is fetched once, then served from memory.
    pub fn request(&mut self, url: &str) -> AvatarFetch {
        if let Some(entry) = self.entries.get(url) {
            return match entry {
                ImageEntry::Loading => AvatarFetch::Waiting,
                ImageEntry::Ready(image) => AvatarFetch::Ready(image.clone()),
                ImageEntry::Failed => AvatarFetch::Failed,
            };
        }
        match avatar_source(url, self.policy) {
            AvatarSource::File(path) => {
                let image = AvatarImage::File {
                    key: url.to_string(),
                    path,
                };
                self.entries
                    .insert(url.to_string(), ImageEntry::Ready(image.clone()));
                AvatarFetch::Ready(image)
            }
            AvatarSource::Http(_) => {
                self.entries.insert(url.to_string(), ImageEntry::Loading);
                AvatarFetch::Start(url.to_string())
            }
            AvatarSource::Data(bytes) => {
                let image = AvatarImage::Bytes {
                    key: data_url_key(url),
                    bytes: bytes.into(),
                };
                self.entries
                    .insert(url.to_string(), ImageEntry::Ready(image.clone()));
                AvatarFetch::Ready(image)
            }
            AvatarSource::Unsupported => {
                self.entries.insert(url.to_string(), ImageEntry::Failed);
                AvatarFetch::Failed
            }
        }
    }

    /// Stores a download's result.
    pub fn finish(&mut self, url: &str, result: Result<AvatarImage, String>) {
        let entry = match result {
            Ok(image) => ImageEntry::Ready(image),
            Err(_) => ImageEntry::Failed,
        };
        self.entries.insert(url.to_string(), entry);
    }

    /// Drops `url`, so the next request fetches it again.
    pub fn forget(&mut self, url: &str) {
        self.entries.remove(url);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The HTTP GET an avatar download needs; a trait so tests run offline.
pub trait AvatarHttp: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// The largest avatar accepted. Steam's full avatars are 184 px JPEGs.
pub const MAX_AVATAR_BYTES: usize = 4 * 1024 * 1024;

/// [`AvatarHttp`] over a blocking reqwest client.
#[derive(Debug, Clone)]
pub struct HttpAvatarClient {
    client: reqwest::blocking::Client,
}

impl HttpAvatarClient {
    pub fn new() -> Result<Self, String> {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map(|client| Self { client })
            .map_err(|error| error.to_string())
    }
}

impl AvatarHttp for HttpAvatarClient {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let response = self
            .client
            .get(url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| error.to_string())?;
        let bytes = response.bytes().map_err(|error| error.to_string())?;
        if bytes.len() > MAX_AVATAR_BYTES {
            return Err(format!("avatar too large: {} bytes", bytes.len()));
        }
        Ok(bytes.to_vec())
    }
}

/// The job half of [`AvatarFetch::Start`]: downloads `url`.
pub fn fetch_avatar_image(url: &str, http: &dyn AvatarHttp) -> Result<AvatarImage, String> {
    let bytes = http.get(url)?;
    if bytes.is_empty() {
        return Err("empty avatar".to_string());
    }
    Ok(AvatarImage::Bytes {
        key: url.to_string(),
        bytes: Arc::from(bytes),
    })
}
