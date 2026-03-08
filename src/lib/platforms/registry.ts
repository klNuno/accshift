import type { PlatformDef } from "$lib/features/settings/types";
import { getPlatform, registerPlatform } from "$lib/shared/platform";
import type { PlatformAdapter } from "$lib/shared/platform";

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
    implemented: true,
    supportedOs: ["windows"],
  },
];

const PLATFORM_LOADERS: Record<string, () => Promise<PlatformAdapter>> = {
  steam: () => import("./steam/adapter").then((mod) => mod.steamAdapter),
  riot: () => import("./riot/adapter").then((mod) => mod.riotAdapter),
  "battle-net": () => import("./battle-net/adapter").then((mod) => mod.battleNetAdapter),
};

const platformLoadTasks = new Map<string, Promise<PlatformAdapter>>();

export async function ensurePlatformLoaded(platformId: string): Promise<PlatformAdapter | undefined> {
  const existing = getPlatform(platformId);
  if (existing) return existing;

  const loadPlatform = PLATFORM_LOADERS[platformId];
  if (!loadPlatform) return undefined;

  const pending = platformLoadTasks.get(platformId);
  if (pending) {
    return pending;
  }

  const task = loadPlatform()
    .then((adapter) => {
      if (!getPlatform(adapter.id)) {
        registerPlatform(adapter);
      }
      return adapter;
    })
    .finally(() => {
      platformLoadTasks.delete(platformId);
    });

  platformLoadTasks.set(platformId, task);
  return task;
}

export function getPlatformDefinition(platformId: string): PlatformDef | undefined {
  return PLATFORM_DEFS.find((platform) => platform.id === platformId);
}
