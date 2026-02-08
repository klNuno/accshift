const SETTINGS_KEY = "zazaswitcher_settings";

export interface AppSettings {
  avatarCacheDays: number;
}

const DEFAULTS: AppSettings = {
  avatarCacheDays: 7,
};

export function getSettings(): AppSettings {
  try {
    const data = localStorage.getItem(SETTINGS_KEY);
    if (!data) return { ...DEFAULTS };
    return { ...DEFAULTS, ...JSON.parse(data) };
  } catch {
    return { ...DEFAULTS };
  }
}

export function saveSettings(settings: AppSettings) {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
}

// Returns cache duration in milliseconds
export function getCacheDuration(): number {
  const settings = getSettings();
  return settings.avatarCacheDays * 24 * 60 * 60 * 1000;
}
