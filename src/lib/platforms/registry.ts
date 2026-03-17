import type { PlatformDef } from "$lib/features/settings/types";
import { getPlatform, registerPlatform } from "$lib/shared/platform";
import type { PlatformAdapter } from "$lib/shared/platform";

export const PLATFORM_DEFS: PlatformDef[] = [
  {
    id: "steam",
    name: "Steam",
    accent: "#2563eb",
    implemented: true,
    supportedOs: ["windows", "macos"],
    settingsTabKey: "settings.steam",
    settingsComponent: () => import("./steam/SteamSettingsTab.svelte"),
    pathLabelKey: "settings.steamFolder",
    pathPlaceholder: "C:\\Program Files (x86)\\Steam",
  },
  {
    id: "riot",
    name: "Riot Games",
    accent: "#ef4444",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.riot",
    settingsComponent: () => import("./riot/RiotSettingsTab.svelte"),
    pathLabelKey: "settings.riotClientPath",
    pathPlaceholder: "C:\\Riot Games\\Riot Client\\RiotClientServices.exe",
  },
  {
    id: "battle-net",
    name: "Battle.net",
    accent: "#38bdf8",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.battleNet",
    settingsComponent: () => import("./battle-net/BattleNetSettingsTab.svelte"),
    pathLabelKey: "settings.battleNetPath",
    pathPlaceholder: "C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe",
  },
  {
    id: "ubisoft",
    name: "Ubisoft",
    accent: "#0070ff",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.ubisoft",
    settingsComponent: () => import("./ubisoft/UbisoftSettingsTab.svelte"),
    pathLabelKey: "settings.ubisoftPath",
    pathPlaceholder: "C:\\Program Files (x86)\\Ubisoft\\Ubisoft Game Launcher",
  },
];

const PLATFORM_LOADERS: Record<string, () => Promise<PlatformAdapter>> = {
  steam: () => import("./steam/adapter").then((mod) => mod.steamAdapter),
  riot: () => import("./riot/adapter").then((mod) => mod.riotAdapter),
  "battle-net": () => import("./battle-net/adapter").then((mod) => mod.battleNetAdapter),
  ubisoft: () => import("./ubisoft/adapter").then((mod) => mod.ubisoftAdapter),
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
