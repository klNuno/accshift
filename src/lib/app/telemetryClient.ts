import { invoke } from "@tauri-apps/api/core";

/**
 * Fire-and-forget telemetry calls.
 *
 * Every one of these is best-effort by design: the backend drops the event
 * when the user has not consented, and the mock backend rejects the whole
 * `telemetry_` prefix outright. Neither is a problem worth surfacing, so the
 * rejection is swallowed here instead of at twenty call sites.
 */
function track(command: string, args?: Record<string, unknown>): void {
  void invoke(command, args).catch(() => {});
}

export function trackAccountAddStarted(platformId: string): void {
  track("telemetry_track_account_add_started", { platformId });
}

export function trackAccountAddCancelled(platformId: string): void {
  track("telemetry_track_account_add_cancelled", { platformId });
}

export function trackAccountAdded(platformId: string): void {
  track("telemetry_track_account_added", { platformId });
}

/**
 * Operation names and error codes are both closed vocabularies on the Rust
 * side; anything unlisted is recorded as `other` rather than sent as typed.
 */
export function trackOperationFailed(
  operation: string,
  errorCode: string,
  platformId?: string,
): void {
  track("telemetry_track_operation_failed", { operation, errorCode, platformId });
}

export type UpdateStage = "available" | "downloaded" | "applied" | "failed";

export function trackUpdate(stage: UpdateStage, targetVersion?: string, errorCode?: string): void {
  track("telemetry_track_update", { stage, targetVersion, errorCode });
}

export type SettingsSnapshot = {
  uiLanguage: string;
  enabledPlatforms: string[];
  personasEnabled: boolean;
  pinEnabled: boolean;
  cliEnabled: boolean;
  deepLinksEnabled: boolean;
  streamerMode: string;
  animations: string;
};

/** Mode B only, enforced by the queue. No theme id: a custom theme is named
 * by the user, and nothing here can tell a built-in id from that. */
export function trackSettingsSnapshot(snapshot: SettingsSnapshot): void {
  track("telemetry_track_settings_snapshot", { ...snapshot });
}
