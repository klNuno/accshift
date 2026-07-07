import type { PlatformDef, AppSettings } from "./types";
import { DEFAULT_LOCALE, detectPreferredLocale, normalizeLocale } from "$lib/i18n";
import { isValidPinHash } from "$lib/shared/pin";
import { PLATFORM_DEFS } from "$lib/platforms/registry";
import { getThemeDefinition } from "$lib/theme/themes";
import {
  CLIENT_STORE_SETTINGS,
  getClientStoreRevision,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";

export const ALL_PLATFORMS: PlatformDef[] = PLATFORM_DEFS;

const DEFAULTS: AppSettings = {
  language: DEFAULT_LOCALE,
  themeId: "dark",
  backgroundOpacity: 100,
  uiScalePercent: 100,
  animations: "system",
  streamerMode: "auto",
  suspendGraphicsWhenMinimized: true,
  minimizeOnAccountSwitch: false,
  dataRefresh: {
    avatarCacheDays: 7,
    banCheckDays: 7,
  },
  enabledPlatforms: ["steam"],
  defaultPlatformId: "steam",
  inactivityBlurSeconds: 60,
  deepLinksEnabled: true,
  platformSettings: {
    steam: {
      runAsAdmin: false,
      launchOptions: "",
      shutdownMode: "force",
    },
  },
  accountDisplay: {
    showUsernames: true,
    showLastLoginPerPlatform: {
      steam: false,
      riot: false,
      "battle-net": false,
      roblox: false,
      epic: false,
      ubisoft: false,
    },
    showCardNotesInline: false,
    expandedFolders: false,
  },
  pinEnabled: false,
  pinHash: "",
};
const PLATFORM_IDS = new Set(ALL_PLATFORMS.map((platform) => platform.id));
let cachedSettings: AppSettings | null = null;
let cachedSettingsRevision = -1;

function asRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return value as Record<string, unknown>;
}

function clampInt(value: unknown, min: number, max: number, fallback: number): number {
  const numeric = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(numeric)) return fallback;
  return Math.min(max, Math.max(min, Math.round(numeric)));
}

function sanitizePinHash(value: unknown): string {
  if (typeof value !== "string") return "";
  const normalized = value.trim().toLowerCase();
  return isValidPinHash(normalized) ? normalized : "";
}

const LEGACY_LAST_LOGIN_KEYS: Record<string, string> = {
  steam: "showLastLogin",
  riot: "showRiotLastLogin",
  "battle-net": "showBattleNetLastLogin",
};

function sanitizeShowLastLoginPerPlatform(
  rawAccountDisplay: Record<string, unknown>,
): Record<string, boolean> {
  const rawMap = asRecord(rawAccountDisplay.showLastLoginPerPlatform);
  const defaults = DEFAULTS.accountDisplay.showLastLoginPerPlatform;
  const result: Record<string, boolean> = {};
  for (const id of PLATFORM_IDS) {
    const legacyKey = LEGACY_LAST_LOGIN_KEYS[id];
    if (id in rawMap) {
      result[id] = Boolean(rawMap[id]);
    } else if (legacyKey && legacyKey in rawAccountDisplay) {
      result[id] = Boolean(rawAccountDisplay[legacyKey]);
    } else {
      result[id] = defaults[id] ?? false;
    }
  }
  return result;
}

function sanitizeSettings(value: unknown): AppSettings {
  const raw = asRecord(value);
  const rawDataRefresh = asRecord(raw.dataRefresh);
  const rawPlatformSettings = asRecord(raw.platformSettings);
  const rawSteamSettings = asRecord(rawPlatformSettings.steam);
  const rawAccountDisplay = asRecord(raw.accountDisplay);
  const hasLanguage = Object.prototype.hasOwnProperty.call(raw, "language");
  const enabledPlatformsRaw = Array.isArray(raw.enabledPlatforms) ? raw.enabledPlatforms : [];
  const enabledPlatforms = Array.from(
    new Set(
      enabledPlatformsRaw
        .filter((platformId): platformId is string => typeof platformId === "string")
        .filter((platformId) => PLATFORM_IDS.has(platformId)),
    ),
  );
  const normalizedEnabledPlatforms =
    enabledPlatforms.length > 0 ? enabledPlatforms : [...DEFAULTS.enabledPlatforms];

  const defaultPlatformIdRaw =
    typeof raw.defaultPlatformId === "string" ? raw.defaultPlatformId : DEFAULTS.defaultPlatformId;
  const defaultPlatformId = normalizedEnabledPlatforms.includes(defaultPlatformIdRaw)
    ? defaultPlatformIdRaw
    : normalizedEnabledPlatforms[0];
  const pinEnabled = Boolean(raw.pinEnabled);
  const pinHash = pinEnabled ? sanitizePinHash(raw.pinHash) : "";

  return {
    language: hasLanguage ? normalizeLocale(raw.language) : detectPreferredLocale(),
    themeId: getThemeDefinition(
      typeof raw.themeId === "string"
        ? raw.themeId
        : raw.theme === "light"
          ? "light"
          : DEFAULTS.themeId,
    ).id,
    backgroundOpacity: clampInt(raw.backgroundOpacity, 0, 100, DEFAULTS.backgroundOpacity),
    uiScalePercent: clampInt(raw.uiScalePercent, 75, 150, DEFAULTS.uiScalePercent),
    animations: raw.animations === "on" || raw.animations === "off" ? raw.animations : "system",
    streamerMode: raw.streamerMode === "off" ? "off" : "auto",
    suspendGraphicsWhenMinimized: raw.suspendGraphicsWhenMinimized !== false,
    minimizeOnAccountSwitch: Boolean(raw.minimizeOnAccountSwitch),
    dataRefresh: {
      avatarCacheDays: clampInt(
        rawDataRefresh.avatarCacheDays ?? raw.avatarCacheDays,
        0,
        90,
        DEFAULTS.dataRefresh.avatarCacheDays,
      ),
      banCheckDays: clampInt(
        rawDataRefresh.banCheckDays ?? raw.banCheckDays,
        0,
        90,
        DEFAULTS.dataRefresh.banCheckDays,
      ),
    },
    enabledPlatforms: normalizedEnabledPlatforms,
    defaultPlatformId,
    inactivityBlurSeconds: clampInt(
      raw.inactivityBlurSeconds,
      0,
      3600,
      DEFAULTS.inactivityBlurSeconds,
    ),
    deepLinksEnabled: raw.deepLinksEnabled !== false,
    platformSettings: {
      steam: {
        runAsAdmin: Boolean(rawSteamSettings.runAsAdmin ?? raw.steamRunAsAdmin),
        launchOptions:
          typeof (rawSteamSettings.launchOptions ?? raw.steamLaunchOptions) === "string"
            ? String(rawSteamSettings.launchOptions ?? raw.steamLaunchOptions)
                .trim()
                .slice(0, 256)
            : "",
        shutdownMode: rawSteamSettings.shutdownMode === "force" ? "force" : "graceful",
      },
    },
    accountDisplay: {
      showUsernames: rawAccountDisplay.showUsernames !== false && raw.showUsernames !== false,
      showLastLoginPerPlatform: sanitizeShowLastLoginPerPlatform(rawAccountDisplay),
      showCardNotesInline: Boolean(
        rawAccountDisplay.showCardNotesInline ?? raw.showCardNotesInline,
      ),
      expandedFolders: Boolean(rawAccountDisplay.expandedFolders ?? raw.expandedFolders),
    },
    pinEnabled,
    pinHash,
  };
}

function cloneSettings(settings: AppSettings): AppSettings {
  return structuredClone(settings);
}

function loadSettingsFromStorage(): AppSettings {
  return sanitizeSettings(getClientStoreValue(CLIENT_STORE_SETTINGS) ?? {});
}

export function getSettings(): AppSettings {
  const revision = getClientStoreRevision(CLIENT_STORE_SETTINGS);
  if (!cachedSettings || cachedSettingsRevision !== revision) {
    cachedSettings = loadSettingsFromStorage();
    cachedSettingsRevision = revision;
  }
  return cloneSettings(cachedSettings);
}

export function saveSettings(settings: AppSettings) {
  const sanitized = sanitizeSettings(settings);
  cachedSettings = sanitized;
  setClientStoreValue(CLIENT_STORE_SETTINGS, sanitized);
  cachedSettingsRevision = getClientStoreRevision(CLIENT_STORE_SETTINGS);
  try {
    const theme = getThemeDefinition(sanitized.themeId);
    localStorage.setItem(
      "accshift_boot_theme",
      JSON.stringify({
        colorScheme: theme.colorScheme,
        bg: `rgb(${theme.tokens.bgRgb})`,
        fg: theme.tokens.fg,
      }),
    );
  } catch {
    // non-critical
  }
}

export function getCacheDuration(): number {
  const settings = getSettings();
  if (settings.dataRefresh.avatarCacheDays === 0) return 0;
  return settings.dataRefresh.avatarCacheDays * 24 * 60 * 60 * 1000;
}
