import { invoke } from "@tauri-apps/api/core";
import type { CustomThemePayload } from "$lib/theme/themes";
import type { StorageManifest } from "$lib/storage/clientStorage";
// Imported lazily inside `fetchBootPayload`, not statically: `registry.ts`
// reads store-id constants from `clientStorage.ts`, which imports this module,
// so a static edge here closes the cycle
// clientStorage -> bootPayload -> registry -> clientStorage. Under the mock
// alias that cycle resolves registry first and the app dies at load with
// `Cannot access 'CLIENT_STORE_STEAM_PROFILE_CACHE' before initialization`,
// leaving `#app` empty and the window black. The registry is only needed once
// the payload has landed, so the dynamic import costs nothing.
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

declare global {
  interface Window {
    /** Asked by index.html before the bundle loaded (vite.config.js). */
    __accshiftBootPayload?: Promise<unknown>;
  }
}

let payload: BootPayload | null = null;

/**
 * Single IPC round trip replacing the old boot waterfall
 * (migrate_legacy_config → load_client_storage_snapshot →
 * list_custom_themes → get_runtime_os). Called once from main.ts before
 * mount; consumers read the cached result synchronously.
 */
export async function fetchBootPayload(): Promise<BootPayload> {
  // The document asked already, while the bundle was loading. That answer is
  // used once; any later call asks again.
  const early = window.__accshiftBootPayload as Promise<BootPayload> | undefined;
  window.__accshiftBootPayload = undefined;
  payload = await (early ?? invoke<BootPayload>("get_boot_payload"));
  // The platforms the user added themselves only exist once this lands, so
  // the registry is filled here rather than at import time.
  const { registerUserPlatforms } = await import("$lib/platforms/registry");
  registerUserPlatforms(payload.userPlatforms?.loaded ?? []);
  return payload;
}

export function getBootPayload(): BootPayload | null {
  return payload;
}
