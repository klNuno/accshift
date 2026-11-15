//! Riot commands: Windows only,
//! there is no Linux or macOS Riot client.

use super::RealBackend;
use crate::error::BackendResult;
use crate::types::RiotGame;

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

/// Backs `Backend::riot_get_installed_games`: read from the Riot Client
/// metadata, empty when it cannot be read.
#[cfg(windows)]
pub(super) fn installed_games() -> BackendResult<Vec<RiotGame>> {
    use accshift_core::platforms::riot;
    Ok(riot::get_installed_games()
        .into_iter()
        .map(|game| RiotGame {
            product_id: game.product_id,
            patchline: game.patchline,
            name: game.name,
        })
        .collect())
}

/// Off Windows the command does not exist.
#[cfg(not(windows))]
pub(super) fn installed_games() -> BackendResult<Vec<RiotGame>> {
    Err(super::not_registered("riot_get_installed_games"))
}

/// Backs `Backend::riot_switch_profile_and_launch_game`: the PIN check, the
/// switch, then the game, all under the operation lock on this thread.
#[cfg(windows)]
pub(super) fn switch_and_launch_game(
    backend: &RealBackend,
    profile_id: &str,
    product_id: &str,
) -> BackendResult<()> {
    use accshift_core::error::PlatformError;
    use accshift_core::platforms::riot;
    Ok(super::runtime::locked(&backend.ctx, |c| {
        backend.pin.ensure_unlocked(&*c)?;
        riot::switch_profile_and_launch_game(c, profile_id.to_string(), product_id.to_string())
            .map_err(PlatformError::from)
    })?)
}

/// Off Windows the command does not exist.
#[cfg(not(windows))]
pub(super) fn switch_and_launch_game(
    _backend: &RealBackend,
    _profile_id: &str,
    _product_id: &str,
) -> BackendResult<()> {
    Err(super::not_registered("riot_switch_profile_and_launch_game"))
}
