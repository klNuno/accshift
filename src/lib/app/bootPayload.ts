import { invoke } from "@tauri-apps/api/core";
import type { CustomThemePayload } from "$lib/theme/themes";
import type { StorageManifest } from "$lib/storage/clientStorage";
import { registerUserPlatforms } from "$lib/platforms/registry";
import type { UserPlatformReport } from "$lib/platforms/descriptors";

export type { UserPlatformReport };

export interface BootPayload {
  migration: string;
  runtimeOs: string;
  storageSnapshot: {
    manifest: StorageManifest;
    stores: Record<string, unknown>;
  };
  customThemes: CustomThemePayload[];
  userPlatforms: UserPlatformReport;
}

let payload: BootPayload | null = null;

/**
 * Single IPC round trip replacing the old boot waterfall
 * (migrate_legacy_config → load_client_storage_snapshot →
 * list_custom_themes → get_runtime_os). Called once from main.ts before
 * mount; consumers read the cached result synchronously.
 */
export async function fetchBootPayload(): Promise<BootPayload> {
  payload = await invoke<BootPayload>("get_boot_payload");
  // The platforms the user added themselves only exist once this lands, so
  // the registry is filled here rather than at import time.
  registerUserPlatforms(payload.userPlatforms?.loaded ?? []);
  return payload;
}

export function getBootPayload(): BootPayload | null {
  return payload;
}
