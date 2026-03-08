# Changelog

## 0.6.2 - 2026-03-08

This release packages the work landed since `v0.6.0`. The `0.6.1` version bump existed on the release branch but was not tagged.

### Fixed

- Prevent stale account state and avatar loading artifacts when switching between Steam, Riot Games, and Battle.net.
- Guard shared account loading so outdated async responses cannot overwrite the currently active platform.
- Tighten Battle.net Overwatch settings-copy behavior and improve related log redaction.

### Improved

- Stabilize frontend account loading and platform actions across Steam, Riot Games, and Battle.net.
- Improve Tauri backend flows, diagnostics, and file utilities used by platform integrations.
- Refine Battle.net and Steam context menu actions and shared loader behavior.
