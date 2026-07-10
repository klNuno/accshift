import type { PlatformDef } from "$lib/shared/platform";
import { getPlatform, registerPlatform } from "$lib/shared/platform";
import type { PlatformAdapter } from "$lib/shared/platform";
import {
  CLIENT_STORE_ROBLOX_PROFILE_CACHE,
  CLIENT_STORE_STEAM_BAN_CHECK_STATE,
  CLIENT_STORE_STEAM_BAN_INFO_CACHE,
  CLIENT_STORE_STEAM_PROFILE_CACHE,
  STORAGE_TARGET_EPIC_SNAPSHOTS,
  STORAGE_TARGET_RIOT_SNAPSHOTS,
  STORAGE_TARGET_UBISOFT_SNAPSHOTS,
} from "$lib/storage/clientStorage";
import { steamSettingsSchema } from "./steam/settingsSchema";

export const PLATFORM_DEFS: PlatformDef[] = [
  {
    id: "steam",
    name: "Steam",
    accent: "#2563eb",
    implemented: true,
    supportedOs: ["windows", "linux", "macos"],
    settingsTabKey: "settings.steam",
    settingsComponent: () => import("./steam/SteamSettingsTab.svelte"),
    pathLabelKey: "settings.steamFolder",
    pathPlaceholder: {
      windows: "C:\\Program Files (x86)\\Steam",
      linux: "~/.local/share/Steam",
      macos: "~/Library/Application Support/Steam",
    },
    capabilities: {
      bulkEdit: { loadBar: () => import("./steam/BulkEditBar.svelte") },
      profileRefresh: { avatars: true, bans: true },
      accountUsernames: true,
      primeProfileAfterAdd: true,
      accountWarnings: true,
      externalDataStores: [
        CLIENT_STORE_STEAM_PROFILE_CACHE,
        CLIENT_STORE_STEAM_BAN_CHECK_STATE,
        CLIENT_STORE_STEAM_BAN_INFO_CACHE,
      ],
      settings: steamSettingsSchema,
    },
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
    capabilities: {
      lastLoginUnknownKey: "time.neverConnected",
      externalDataStores: [STORAGE_TARGET_RIOT_SNAPSHOTS],
    },
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
    capabilities: {
      externalDataStores: [STORAGE_TARGET_UBISOFT_SNAPSHOTS],
    },
  },
  {
    id: "roblox",
    name: "Roblox",
    accent: "#e1242a",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.roblox",
    settingsComponent: () => import("./roblox/RobloxSettingsTab.svelte"),
    capabilities: {
      accountWarnings: true,
      externalDataStores: [CLIENT_STORE_ROBLOX_PROFILE_CACHE],
    },
  },
  {
    id: "epic",
    name: "Epic Games",
    accent: "#0078f2",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.epic",
    settingsComponent: () => import("./epic/EpicSettingsTab.svelte"),
    pathLabelKey: "settings.epicPath",
    pathPlaceholder:
      "C:\\Program Files (x86)\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe",
    capabilities: {
      externalDataStores: [STORAGE_TARGET_EPIC_SNAPSHOTS],
    },
  },
  {
    id: "gog",
    name: "GOG Galaxy",
    accent: "#a02de3",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.gog",
    settingsComponent: () => import("./gog/GogSettingsTab.svelte"),
    pathLabelKey: "settings.gogPath",
    pathPlaceholder: "C:\\Program Files (x86)\\GOG Galaxy\\GalaxyClient.exe",
  },
  {
    id: "jagex",
    name: "Jagex Launcher",
    accent: "#eab308",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.jagex",
    settingsComponent: () => import("./jagex/JagexSettingsTab.svelte"),
    pathLabelKey: "settings.jagexPath",
    pathPlaceholder: "C:\\Program Files (x86)\\Jagex Launcher\\JagexLauncher.exe",
  },
  {
    id: "discord",
    name: "Discord",
    accent: "#5865f2",
    implemented: true,
    supportedOs: ["windows"],
    settingsTabKey: "settings.discord",
    settingsComponent: () => import("./discord/DiscordSettingsTab.svelte"),
    pathLabelKey: "settings.discordPath",
    pathPlaceholder: "%LOCALAPPDATA%\\Discord\\Update.exe",
  },
];

const PLATFORM_LOADERS: Record<string, () => Promise<PlatformAdapter>> = {
  steam: () => import("./steam/adapter").then((mod) => mod.steamAdapter),
  riot: () => import("./riot/adapter").then((mod) => mod.riotAdapter),
  "battle-net": () => import("./battle-net/adapter").then((mod) => mod.battleNetAdapter),
  ubisoft: () => import("./ubisoft/adapter").then((mod) => mod.ubisoftAdapter),
  roblox: () => import("./roblox/adapter").then((mod) => mod.robloxAdapter),
  epic: () => import("./epic/adapter").then((mod) => mod.epicAdapter),
  gog: () => import("./gog/adapter").then((mod) => mod.gogAdapter),
  jagex: () => import("./jagex/adapter").then((mod) => mod.jagexAdapter),
  discord: () => import("./discord/adapter").then((mod) => mod.discordAdapter),
};

const platformLoadTasks = new Map<string, Promise<PlatformAdapter>>();

export async function ensurePlatformLoaded(
  platformId: string,
): Promise<PlatformAdapter | undefined> {
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
