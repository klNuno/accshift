//! The profile caches (`profileCache.ts`: retries, negative entries, fetches
//! in flight, expiry) and the avatar image handoff. No TS test covers these
//! files; the cases follow the TS behaviour.

use accshift_app::accounts::avatars::{
    avatar_source, fetch_avatar_image, AvatarFetch, AvatarHttp, AvatarImage, AvatarImages,
    AvatarSource,
};
use accshift_app::accounts::profile_cache::{
    CacheConfig, CacheExpiry, ProfileCache, UrlPolicy, ROBLOX_PROFILE_CACHE,
};
use accshift_app::error::BackendResult;
use accshift_app::storage::ClientStorage;
use accshift_app::types::RobloxProfileInfo;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

const URL: &str = "https://cdn.example/a.png";

fn config(attempts: u32, negative: bool) -> CacheConfig {
    CacheConfig {
        attempts,
        retry_delay: Duration::ZERO,
        cache_negative_results: negative,
        ..ROBLOX_PROFILE_CACHE
    }
}

fn profile(url: Option<&str>) -> RobloxProfileInfo {
    RobloxProfileInfo {
        avatar_url: url.map(str::to_string),
    }
}

fn now() -> f64 {
    1_000.0
}

fn fresh() -> CacheExpiry {
    CacheExpiry {
        duration_ms: 60_000,
        session_start_ms: 0.0,
        now_ms: 1_000.0,
    }
}

#[test]
fn retries_until_a_usable_avatar_comes_back() {
    let cache = ProfileCache::new(config(4, true), UrlPolicy::HttpOnly);
    let calls = AtomicUsize::new(0);
    let fetched = cache.fetch_profile(
        "a",
        &|_| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(profile((call == 2).then_some(URL))))
        },
        &now,
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(fetched.value, Some(profile(Some(URL))));
    assert_eq!(fetched.writes.len(), 1);

    let mut storage = ClientStorage::new();
    cache.apply_writes(&mut storage, fetched.writes);
    let cached = cache.cached_profile(&storage, "a", &fresh()).unwrap();
    assert_eq!(cached.url.as_deref(), Some(URL));
    assert!(!cached.expired);
}

#[test]
fn stores_a_negative_entry_once_every_attempt_lacks_an_avatar() {
    let cache = ProfileCache::new(config(2, true), UrlPolicy::HttpOnly);
    let fetched = cache.fetch_profile("a", &|_| Ok(Some(profile(None))), &now);
    assert_eq!(fetched.writes.len(), 1);
    assert_eq!(fetched.writes[0].entry.url, None);

    let mut storage = ClientStorage::new();
    cache.apply_writes(&mut storage, fetched.writes);
    assert_eq!(
        cache.cached_profile(&storage, "a", &fresh()).unwrap().url,
        None
    );

    let without = ProfileCache::new(config(2, false), UrlPolicy::HttpOnly);
    assert!(without
        .fetch_profile("a", &|_| Ok(Some(profile(None))), &now)
        .writes
        .is_empty());
}

#[test]
fn shares_one_fetch_between_threads_asking_for_the_same_account() {
    let cache = Arc::new(ProfileCache::new(config(1, false), UrlPolicy::HttpOnly));
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Barrier::new(2));
    let first = {
        let (cache, calls, gate) = (cache.clone(), calls.clone(), gate.clone());
        std::thread::spawn(move || {
            cache.fetch_profile(
                "a",
                &|_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    gate.wait();
                    // Leave the second caller time to find this fetch.
                    std::thread::sleep(Duration::from_millis(50));
                    Ok(Some(profile(Some(URL))))
                },
                &now,
            )
        })
    };
    gate.wait();
    let second = cache.fetch_profile(
        "a",
        &|_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        },
        &now,
    );
    let first = first.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(first.value, Some(profile(Some(URL))));
    assert_eq!(second.value, Some(profile(Some(URL))));
    assert!(second.writes.is_empty(), "the waiter writes nothing");
    assert!(cache.in_flight().is_empty());
}

#[test]
fn batch_fetches_retry_only_the_ids_still_missing() {
    let cache = ProfileCache::new(config(3, true), UrlPolicy::HttpOnly);
    let asked = std::sync::Mutex::new(Vec::new());
    let batch = |ids: &[String]| -> BackendResult<BTreeMap<String, RobloxProfileInfo>> {
        asked.lock().unwrap().push(ids.to_vec());
        Ok(ids
            .iter()
            .map(|id| (id.clone(), profile((id == "a").then_some(URL))))
            .collect())
    };
    let ids = vec!["a".to_string(), "b".to_string(), "a".to_string()];
    let fetched = cache.fetch_profiles(&ids, Some(&batch), &|_| Ok(None), &now);
    assert_eq!(
        *asked.lock().unwrap(),
        [
            vec!["a".to_string(), "b".to_string()],
            vec!["b".to_string()],
            vec!["b".to_string()]
        ]
    );
    assert_eq!(fetched.value["a"], Some(profile(Some(URL))));
    assert_eq!(fetched.value["b"], Some(profile(None)));
    // One positive entry for a, one negative for b.
    assert_eq!(fetched.writes.len(), 2);
}

#[test]
fn entries_expire_by_duration_or_by_session_when_the_duration_is_zero() {
    let cache = ProfileCache::<RobloxProfileInfo>::new(config(1, false), UrlPolicy::HttpOnly);
    let mut storage = ClientStorage::new();
    cache.set_cached_profile(&mut storage, "a", URL, None, 1_000.0);

    let by_duration = |now_ms| CacheExpiry {
        duration_ms: 500,
        session_start_ms: 0.0,
        now_ms,
    };
    assert!(
        !cache
            .cached_profile(&storage, "a", &by_duration(1_500.0))
            .unwrap()
            .expired
    );
    assert!(
        cache
            .cached_profile(&storage, "a", &by_duration(1_501.0))
            .unwrap()
            .expired
    );

    let by_session = |session_start_ms| CacheExpiry {
        duration_ms: 0,
        session_start_ms,
        now_ms: 99_999.0,
    };
    assert!(
        !cache
            .cached_profile(&storage, "a", &by_session(1_000.0))
            .unwrap()
            .expired
    );
    assert!(
        cache
            .cached_profile(&storage, "a", &by_session(1_001.0))
            .unwrap()
            .expired
    );
}

#[test]
fn the_url_policy_keeps_files_to_the_mock() {
    let file = "src/lib/mock/avatars/1.svg";
    let strict = ProfileCache::<RobloxProfileInfo>::new(config(1, false), UrlPolicy::HttpOnly);
    let mut storage = ClientStorage::new();
    strict.set_cached_profile(&mut storage, "a", file, None, 0.0);
    assert!(strict.cached_profile(&storage, "a", &fresh()).is_none());

    let mock = ProfileCache::<RobloxProfileInfo>::new(config(1, false), UrlPolicy::HttpOrFile);
    mock.set_cached_profile(&mut storage, "a", file, None, 0.0);
    assert_eq!(
        mock.cached_profile(&storage, "a", &fresh())
            .unwrap()
            .url
            .as_deref(),
        Some(file)
    );
}

// ------------------------------------------------------------ images

struct FakeHttp(Result<Vec<u8>, String>);

impl AvatarHttp for FakeHttp {
    fn get(&self, _url: &str) -> Result<Vec<u8>, String> {
        self.0.clone()
    }
}

#[test]
fn avatar_sources_follow_the_policy() {
    assert_eq!(
        avatar_source(URL, UrlPolicy::HttpOnly),
        AvatarSource::Http(URL.into())
    );
    assert_eq!(
        avatar_source("javascript:alert(1)", UrlPolicy::HttpOrFile),
        AvatarSource::Unsupported
    );
    assert_eq!(
        avatar_source("src/lib/mock/avatars/2.svg", UrlPolicy::HttpOnly),
        AvatarSource::Unsupported
    );
    assert_eq!(
        avatar_source("file:///C:/avatars/2.svg", UrlPolicy::HttpOrFile),
        AvatarSource::File(PathBuf::from("/C:/avatars/2.svg"))
    );
    // A persona cover: the bytes of a base64 data URL, under either policy.
    assert_eq!(
        avatar_source("data:image/webp;base64,UklGRg==", UrlPolicy::HttpOnly),
        AvatarSource::Data(b"RIFF".to_vec())
    );
    assert_eq!(
        avatar_source("data:image/svg+xml,<svg/>", UrlPolicy::HttpOrFile),
        AvatarSource::Unsupported
    );
    assert_eq!(
        avatar_source("data:text/html;base64,PGI+", UrlPolicy::HttpOrFile),
        AvatarSource::Unsupported
    );
}

#[test]
fn images_hand_over_files_at_once_and_download_urls_once() {
    let mut images = AvatarImages::new(UrlPolicy::HttpOrFile);
    let file = "src/lib/mock/avatars/3.svg";
    assert_eq!(
        images.request(file),
        AvatarFetch::Ready(AvatarImage::File {
            key: file.into(),
            path: PathBuf::from(file)
        })
    );

    assert_eq!(images.request(URL), AvatarFetch::Start(URL.into()));
    assert_eq!(images.request(URL), AvatarFetch::Waiting);
    let image = fetch_avatar_image(URL, &FakeHttp(Ok(vec![1, 2, 3]))).unwrap();
    assert_eq!(image.key(), URL);
    images.finish(URL, Ok(image.clone()));
    assert_eq!(images.request(URL), AvatarFetch::Ready(image));

    let broken = "https://cdn.example/broken.png";
    assert_eq!(images.request(broken), AvatarFetch::Start(broken.into()));
    images.finish(
        broken,
        fetch_avatar_image(broken, &FakeHttp(Ok(Vec::new()))),
    );
    assert_eq!(images.request(broken), AvatarFetch::Failed);
    images.forget(broken);
    assert_eq!(images.request(broken), AvatarFetch::Start(broken.into()));
}
