//! Generic in-memory tracking for platform setup flows.
//!
//! Every platform with a "sign in to add an account" flow keeps a map of
//! pending setup jobs keyed by setup id, expired after a TTL of inactivity.
//! This type owns the map, the TTL purge and the lock-poisoning error
//! messages so platforms only describe their job payload.
//!
//! Riot has its own mechanism persisted in config and does not use this.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Default inactivity TTL shared by the platforms (5 minutes).
pub(crate) const DEFAULT_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

struct Entry<T> {
    job: T,
    last_touched_at: u64,
}

pub(crate) struct SetupJobs<T> {
    /// Human-readable platform name used in error messages ("GOG", "Epic"...).
    label: &'static str,
    ttl_ms: u64,
    // OnceLock instead of Mutex<HashMap> directly: HashMap::new() is not
    // const (RandomState), and these live in statics.
    jobs: OnceLock<Mutex<HashMap<String, Entry<T>>>>,
}

impl<T> SetupJobs<T> {
    pub(crate) const fn new(label: &'static str, ttl_ms: u64) -> Self {
        Self {
            label,
            ttl_ms,
            jobs: OnceLock::new(),
        }
    }

    fn map(&self) -> &Mutex<HashMap<String, Entry<T>>> {
        self.jobs.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Lock the map and drop entries whose TTL elapsed.
    fn lock(&self) -> Result<MutexGuard<'_, HashMap<String, Entry<T>>>, String> {
        let mut guard = self
            .map()
            .lock()
            .map_err(|_| format!("{} setup storage is unavailable", self.label))?;
        let ttl_ms = self.ttl_ms;
        guard.retain(|_, entry| !super::setup_expired(entry.last_touched_at, ttl_ms));
        Ok(guard)
    }

    /// Register a new job under `setup_id`, stamped with the current time.
    pub(crate) fn insert(&self, setup_id: String, job: T) -> Result<(), String> {
        let mut jobs = self.lock()?;
        jobs.insert(
            setup_id,
            Entry {
                job,
                last_touched_at: super::now_unix_ms(),
            },
        );
        Ok(())
    }

    /// Remove and return the job, if still tracked. Used by cancel paths that
    /// need the payload (e.g. to revoke a remote login code).
    pub(crate) fn take(&self, setup_id: &str) -> Result<Option<T>, String> {
        Ok(self.lock()?.remove(setup_id).map(|entry| entry.job))
    }

    /// Drop the job, ignoring a poisoned lock. Used after a job completes,
    /// where the status result matters more than map hygiene.
    pub(crate) fn remove(&self, setup_id: &str) {
        if let Ok(mut jobs) = self.map().lock() {
            jobs.remove(setup_id);
        }
    }

    /// Cancel the job: purge expired entries and drop this one if present.
    pub(crate) fn cancel(&self, setup_id: &str) -> Result<(), String> {
        self.take(setup_id).map(|_| ())
    }
}

impl<T: Clone> SetupJobs<T> {
    /// Refresh the job's TTL and return a clone of its payload. Errors when
    /// the id is unknown or the job already expired.
    pub(crate) fn touch(&self, setup_id: &str) -> Result<T, String> {
        let mut jobs = self.lock()?;
        let entry = jobs
            .get_mut(setup_id)
            .ok_or_else(|| format!("{} setup session not found", self.label))?;
        entry.last_touched_at = super::now_unix_ms();
        Ok(entry.job.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_then_touch_returns_payload() {
        static JOBS: SetupJobs<u32> = SetupJobs::new("Test", DEFAULT_SETUP_TTL_MS);
        JOBS.insert("id-1".into(), 42).unwrap();
        assert_eq!(JOBS.touch("id-1").unwrap(), 42);
    }

    #[test]
    fn touch_unknown_id_reports_platform_label() {
        static JOBS: SetupJobs<u32> = SetupJobs::new("Test", DEFAULT_SETUP_TTL_MS);
        let err = JOBS.touch("missing").unwrap_err();
        assert_eq!(err, "Test setup session not found");
    }

    #[test]
    fn expired_job_is_purged_on_next_access() {
        static JOBS: SetupJobs<u32> = SetupJobs::new("Test", 0);
        JOBS.insert("id-1".into(), 1).unwrap();
        // TTL 0: expired as soon as any time elapses. now_unix_ms has ms
        // resolution, so wait one tick to guarantee elapsed > 0.
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(JOBS.touch("id-1").is_err());
    }

    #[test]
    fn take_returns_payload_and_removes() {
        static JOBS: SetupJobs<&'static str> = SetupJobs::new("Test", DEFAULT_SETUP_TTL_MS);
        JOBS.insert("id-1".into(), "payload").unwrap();
        assert_eq!(JOBS.take("id-1").unwrap(), Some("payload"));
        assert_eq!(JOBS.take("id-1").unwrap(), None);
    }
}
