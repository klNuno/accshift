import type { PathPlaceholder, PlatformDef, RuntimeOs } from "$lib/shared/platform";
import { getPlatform, registerPlatform } from "$lib/shared/platform";
import type { PlatformAdapter } from "$lib/shared/platform";
import { createGenericAdapter } from "$lib/platforms/genericAdapter";
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

/** The part of a descriptor this side reads. The rest of the file drives the
 * Rust engine and is none of the frontend's business. */
export interface PlatformDescriptor {
  id: string;
  name: string;
  os: Record<string, { executable?: { candidates?: { kind: string; template?: string }[] } }>;
}

/** The descriptors shipped with the app, read from the very files the engine
 * runs. A platform described as data is described once: adding one to
 * `crates/accshift-core/src/platforms/descriptor/descriptors/` puts it on
 * screen with no entry to write here. */
const SHIPPED_DESCRIPTORS = Object.values(
  import.meta.glob<PlatformDescriptor>(
    "../../../crates/accshift-core/src/platforms/descriptor/descriptors/*.json",
    { eager: true, import: "default" },
  ),
);

/** What a platform looks like, as opposed to what it does: the colour it is
 * drawn in, its settings tab, the features the app layer may offer for it.
 *
 * A descriptor owns the name, the systems and where the launcher lives, so
 * none of those appear here for a platform that has one. The four platforms
 * with no descriptor carry them, because nothing else describes those. */
type PlatformChrome = Partial<PlatformDef> & Pick<PlatformDef, "accent">;

/** Declaration order is display order: the tab bar, the settings list and the
 * startup picker all read [`PLATFORM_DEFS`] as it is built below. */
const PLATFORM_CHROME: Record<string, PlatformChrome> = {
  steam: {
    name: "Steam",
    accent: "#2563eb",
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
  riot: {
    name: "Riot Games",
    accent: "#ef4444",
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
  "battle-net": {
    name: "Battle.net",
    accent: "#38bdf8",
    supportedOs: ["windows", "macos"],
    settingsTabKey: "settings.battleNet",
    settingsComponent: () => import("./battle-net/BattleNetSettingsTab.svelte"),
    pathLabelKey: "settings.battleNetPath",
    pathPlaceholder: {
      windows: "C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe",
      macos: "/Applications/Battle.net.app",
    },
  },
  ubisoft: {
    accent: "#0070ff",
    settingsTabKey: "settings.ubisoft",
    settingsComponent: () => import("./ubisoft/UbisoftSettingsTab.svelte"),
    pathLabelKey: "settings.ubisoftPath",
    capabilities: {
      externalDataStores: [STORAGE_TARGET_UBISOFT_SNAPSHOTS],
    },
  },
  roblox: {
    name: "Roblox",
    accent: "#e1242a",
    supportedOs: ["windows"],
    settingsTabKey: "settings.roblox",
    settingsComponent: () => import("./roblox/RobloxSettingsTab.svelte"),
    capabilities: {
      accountWarnings: true,
      externalDataStores: [CLIENT_STORE_ROBLOX_PROFILE_CACHE],
    },
  },
  epic: {
    accent: "#0078f2",
    settingsTabKey: "settings.epic",
    settingsComponent: () => import("./epic/EpicSettingsTab.svelte"),
    pathLabelKey: "settings.epicPath",
    capabilities: {
      externalDataStores: [STORAGE_TARGET_EPIC_SNAPSHOTS],
    },
  },
  gog: {
    accent: "#a02de3",
    settingsTabKey: "settings.gog",
    settingsComponent: () => import("./gog/GogSettingsTab.svelte"),
    pathLabelKey: "settings.gogPath",
  },
  jagex: {
    accent: "#eab308",
    settingsTabKey: "settings.jagex",
    settingsComponent: () => import("./jagex/JagexSettingsTab.svelte"),
    pathLabelKey: "settings.jagexPath",
  },
  discord: {
    accent: "#5865f2",
    settingsTabKey: "settings.discord",
    settingsComponent: () => import("./discord/DiscordSettingsTab.svelte"),
    pathLabelKey: "settings.discordPath",
  },
};

/** Renders a descriptor path template the way the user reads a path, so the
 * placeholder in the settings field says what the engine actually looks at.
 * `${LOCALAPPDATA}` is `%LOCALAPPDATA%` on Windows and `$LOCALAPPDATA`
 * elsewhere, which is how each system spells its own variables. */
function renderTemplate(template: string, os: RuntimeOs): string {
  const expanded = template.replace(/\$\{(\w+(?:\([^)]*\))?)\}/g, (_match, name: string) =>
    os === "windows" ? `%${name}%` : `$${name}`,
  );
  return os === "windows" ? expanded.replace(/\//g, "\\") : expanded.replace(/\\/g, "/");
}

/** Where the launcher usually lives, per system, taken from the first place
 * the descriptor says to look. */
function placeholderFrom(descriptor: PlatformDescriptor): PathPlaceholder | undefined {
  const placeholder: Partial<Record<RuntimeOs, string>> = {};
  for (const [os, profile] of Object.entries(descriptor.os)) {
    const candidate = profile.executable?.candidates?.find(
      (entry) => entry.kind === "path" && entry.template,
    );
    if (candidate?.template) {
      placeholder[os as RuntimeOs] = renderTemplate(candidate.template, os as RuntimeOs);
    }
  }
  return Object.keys(placeholder).length > 0 ? placeholder : undefined;
}

/** A stable colour for a platform nobody picked one for. Same id, same hue,
 * every launch, so a user-added platform does not change colour on them. */
function accentFor(id: string): string {
  let hue = 0;
  for (const char of id) hue = (hue * 31 + char.charCodeAt(0)) % 360;
  return `hsl(${hue} 62% 52%)`;
}

function buildDef(
  id: string,
  chrome: PlatformChrome | undefined,
  descriptor: PlatformDescriptor | undefined,
  userProvided = false,
): PlatformDef {
  const accent = chrome?.accent ?? accentFor(id);
  return {
    ...chrome,
    id,
    accent,
    implemented: true,
    name: descriptor?.name ?? chrome?.name ?? id,
    supportedOs: descriptor
      ? (Object.keys(descriptor.os) as RuntimeOs[])
      : (chrome?.supportedOs ?? []),
    pathPlaceholder: descriptor ? placeholderFrom(descriptor) : chrome?.pathPlaceholder,
    ...(userProvided ? { userProvided: true } : {}),
  };
}

function buildShippedDefs(): PlatformDef[] {
  const byId = new Map(SHIPPED_DESCRIPTORS.map((descriptor) => [descriptor.id, descriptor]));
  const defs = Object.entries(PLATFORM_CHROME).map(([id, chrome]) =>
    buildDef(id, chrome, byId.get(id)),
  );
  // A descriptor added to the crate without an entry above still shows up,
  // rather than shipping in the binary and being invisible on screen.
  for (const descriptor of SHIPPED_DESCRIPTORS) {
    if (!(descriptor.id in PLATFORM_CHROME))
      defs.push(buildDef(descriptor.id, undefined, descriptor));
  }
  return defs;
}

export const PLATFORM_DEFS: PlatformDef[] = buildShippedDefs();

/**
 * Replaces the platforms built from the user's own descriptor folder with what
 * the backend just read there. The folder is the truth, so an id whose file is
 * gone stops being a platform.
 *
 * The array is edited in place because the settings and persona stores hold
 * this very object; handing back a new one would leave them on the old list.
 */
export function registerUserPlatforms(descriptors: PlatformDescriptor[]): void {
  for (let index = PLATFORM_DEFS.length - 1; index >= 0; index -= 1) {
    if (PLATFORM_DEFS[index].userProvided) PLATFORM_DEFS.splice(index, 1);
  }
  for (const descriptor of descriptors) {
    // A shipped id wins: the backend refuses the file too, so this only ever
    // guards against a payload from an older build.
    if (PLATFORM_DEFS.some((platform) => platform.id === descriptor.id)) continue;
    PLATFORM_DEFS.push(buildDef(descriptor.id, undefined, descriptor, true));
  }
}

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

/** A platform this build was never compiled for talks to the same generic
 * `platform_*` commands as any other, so it needs no module of its own. Its
 * wording comes from the shared `platform.*` keys, with the name the
 * descriptor gave it filled in. */
function userAdapterLoader(platformId: string): (() => Promise<PlatformAdapter>) | undefined {
  const definition = getPlatformDefinition(platformId);
  if (!definition?.userProvided) return undefined;
  return async () =>
    createGenericAdapter({
      id: platformId,
      i18nPrefix: "platform",
      reloadAfterAdd: true,
      noAccountsToastKey: "app.noAccountsFound",
      noAccountsHintKey: "app.noAccountsHint",
      messageParams: { platform: definition.name },
    });
}

const platformLoadTasks = new Map<string, Promise<PlatformAdapter>>();

export async function ensurePlatformLoaded(
  platformId: string,
): Promise<PlatformAdapter | undefined> {
  const existing = getPlatform(platformId);
  if (existing) return existing;

  const loadPlatform = PLATFORM_LOADERS[platformId] ?? userAdapterLoader(platformId);
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
