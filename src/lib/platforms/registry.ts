import type { PlatformDef } from "$lib/features/settings/types";
import { getPlatform, registerPlatform } from "$lib/shared/platform";
import { riotAdapter } from "./riot/adapter";
import { steamAdapter } from "./steam/adapter";

export const PLATFORM_DEFS: PlatformDef[] = [
  {
    id: "steam",
    name: "Steam",
    accent: "#3b82f6",
    implemented: true,
    supportedOs: ["windows"],
  },
  {
    id: "riot",
    name: "Riot Games",
    accent: "#ef4444",
    implemented: true,
    supportedOs: ["windows"],
  },
  {
    id: "battle-net",
    name: "Battle.net",
    accent: "#60a5fa",
    implemented: false,
    supportedOs: ["windows"],
  },
];

const BUILTIN_ADAPTERS = [steamAdapter, riotAdapter];
let registered = false;

export function registerBuiltinPlatforms() {
  if (registered) return;
  for (const adapter of BUILTIN_ADAPTERS) {
    if (!getPlatform(adapter.id)) {
      registerPlatform(adapter);
    }
  }
  registered = true;
}

export function getPlatformDefinition(platformId: string): PlatformDef | undefined {
  return PLATFORM_DEFS.find((platform) => platform.id === platformId);
}
