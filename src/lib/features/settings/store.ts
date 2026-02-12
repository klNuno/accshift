import type { PlatformDef, AppSettings } from "./types";

const SETTINGS_KEY = "zazaswitcher_settings";

export const ALL_PLATFORMS: PlatformDef[] = [
  { id: "steam", name: "Steam", accent: "#3b82f6" },
  { id: "riot", name: "Riot Games", accent: "#ef4444" },
];

const DEFAULTS: AppSettings = {
  avatarCacheDays: 7,
  enabledPlatforms: ["steam"],
};

export function getSettings(): AppSettings {
  try {
    const data = localStorage.getItem(SETTINGS_KEY);
    if (!data) return { ...DEFAULTS };
    const parsed = JSON.parse(data);
    return {
      ...DEFAULTS,
      ...parsed,
      enabledPlatforms: parsed.enabledPlatforms || DEFAULTS.enabledPlatforms,
    };
  } catch {
    return { ...DEFAULTS };
  }
}

export function saveSettings(settings: AppSettings) {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
}

export function getCacheDuration(): number {
  const settings = getSettings();
  return settings.avatarCacheDays * 24 * 60 * 60 * 1000;
}
