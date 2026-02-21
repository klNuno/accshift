import type { PlatformDef, AppSettings } from "./types";
import { DEFAULT_LOCALE, detectPreferredLocale, normalizeLocale } from "$lib/i18n";

const SETTINGS_KEY = "accshift_settings";

export const ALL_PLATFORMS: PlatformDef[] = [
  { id: "steam", name: "Steam", accent: "#3b82f6" },
  { id: "riot", name: "Riot Games", accent: "#ef4444" },
];

const DEFAULTS: AppSettings = {
  language: DEFAULT_LOCALE,
  theme: "dark",
  uiScalePercent: 100,
  avatarCacheDays: 7,
  banCheckDays: 7,
  enabledPlatforms: ["steam"],
  defaultPlatformId: "steam",
  inactivityBlurSeconds: 60,
  steamRunAsAdmin: false,
  steamLaunchOptions: "",
  showUsernames: true,
  showLastLogin: false,
  showCardNotesInline: false,
  pinEnabled: false,
  pinCode: "",
};
const PLATFORM_IDS = new Set(ALL_PLATFORMS.map((platform) => platform.id));
let cachedSettings: AppSettings | null = null;

function asRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return value as Record<string, unknown>;
}

function clampInt(value: unknown, min: number, max: number, fallback: number): number {
  const numeric = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(numeric)) return fallback;
  return Math.min(max, Math.max(min, Math.round(numeric)));
}

function sanitizeSettings(value: unknown): AppSettings {
  const raw = asRecord(value);
  const hasLanguage = Object.prototype.hasOwnProperty.call(raw, "language");
  const enabledPlatformsRaw = Array.isArray(raw.enabledPlatforms) ? raw.enabledPlatforms : [];
  const enabledPlatforms = Array.from(new Set(
    enabledPlatformsRaw
      .filter((platformId): platformId is string => typeof platformId === "string")
      .filter((platformId) => PLATFORM_IDS.has(platformId))
  ));
  const normalizedEnabledPlatforms = enabledPlatforms.length > 0
    ? enabledPlatforms
    : [...DEFAULTS.enabledPlatforms];

  const defaultPlatformIdRaw = typeof raw.defaultPlatformId === "string" ? raw.defaultPlatformId : DEFAULTS.defaultPlatformId;
  const defaultPlatformId = normalizedEnabledPlatforms.includes(defaultPlatformIdRaw)
    ? defaultPlatformIdRaw
    : normalizedEnabledPlatforms[0];
  const pinEnabled = Boolean(raw.pinEnabled);
  const pinCode = pinEnabled && typeof raw.pinCode === "string"
    ? raw.pinCode.trim().slice(0, 32)
    : "";

  return {
    language: hasLanguage ? normalizeLocale(raw.language) : detectPreferredLocale(),
    theme: raw.theme === "light" ? "light" : "dark",
    uiScalePercent: clampInt(raw.uiScalePercent, 75, 150, DEFAULTS.uiScalePercent),
    avatarCacheDays: clampInt(raw.avatarCacheDays, 0, 90, DEFAULTS.avatarCacheDays),
    banCheckDays: clampInt(raw.banCheckDays, 0, 90, DEFAULTS.banCheckDays),
    enabledPlatforms: normalizedEnabledPlatforms,
    defaultPlatformId,
    inactivityBlurSeconds: clampInt(raw.inactivityBlurSeconds, 0, 3600, DEFAULTS.inactivityBlurSeconds),
    steamRunAsAdmin: Boolean(raw.steamRunAsAdmin),
    steamLaunchOptions: typeof raw.steamLaunchOptions === "string" ? raw.steamLaunchOptions.trim().slice(0, 256) : "",
    showUsernames: raw.showUsernames !== false,
    showLastLogin: Boolean(raw.showLastLogin),
    showCardNotesInline: Boolean(raw.showCardNotesInline),
    pinEnabled,
    pinCode,
  };
}

function cloneSettings(settings: AppSettings): AppSettings {
  return {
    ...settings,
    enabledPlatforms: [...settings.enabledPlatforms],
  };
}

function loadSettingsFromStorage(): AppSettings {
  try {
    const data = localStorage.getItem(SETTINGS_KEY);
    if (!data) return sanitizeSettings({});
    return sanitizeSettings(JSON.parse(data));
  } catch {
    return sanitizeSettings({});
  }
}

export function getSettings(): AppSettings {
  if (!cachedSettings) {
    cachedSettings = loadSettingsFromStorage();
  }
  return cloneSettings(cachedSettings);
}

export function saveSettings(settings: AppSettings) {
  const sanitized = sanitizeSettings(settings);
  cachedSettings = sanitized;
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(sanitized));
}

export function getCacheDuration(): number {
  const settings = getSettings();
  if (settings.avatarCacheDays === 0) return 0;
  return settings.avatarCacheDays * 24 * 60 * 60 * 1000;
}
