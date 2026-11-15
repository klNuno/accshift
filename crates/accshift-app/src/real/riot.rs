//! Riot commands: Windows only,
//! there is no Linux or macOS Riot client.

use super::RealBackend;
use crate::error::BackendResult;

/// Backs `Backend::riot_capture_profile`: saves the signed-in Riot
/// session as this profile's snapshot, under the operation lock, failures
/// recorded as `profile_capture`.
#[cfg(windows)]
pub(super) fn capture_profile(backend: &RealBackend, profile_id: &str) -> BackendResult<()> {
    use accshift_core::error::PlatformError;
    use accshift_core::platforms::{ids, riot};
    let result = super::runtime::locked(&backend.ctx, |c| {
        riot::capture_profile(c, profile_id.to_string()).map_err(PlatformError::from)
    });
    backend.track_operation("profile_capture", Some(ids::RIOT), result)
}

/// Off Windows the command does not exist.
#[cfg(not(windows))]
pub(super) fn capture_profile(_backend: &RealBackend, _profile_id: &str) -> BackendResult<()> {
    Err(super::not_registered("riot_capture_profile"))
}

/// Backs `Backend::riot_check_sessions`: the profiles whose saved session is
/// past its restore window, read from the snapshots on this thread.
#[cfg(windows)]
pub(super) fn check_sessions(backend: &RealBackend) -> BackendResult<Vec<String>> {
    Ok(accshift_core::platforms::riot::expired_profile_ids(
        backend.ctx.clone(),
    )?)
}

/// Off Windows the command does not exist.
#[cfg(not(windows))]
pub(super) fn check_sessions(_backend: &RealBackend) -> BackendResult<Vec<String>> {
    Err(super::not_registered("riot_check_sessions"))
}
