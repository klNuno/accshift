export interface PlatformDef {
  id: string;
  name: string;
  accent: string;
}

export interface AppSettings {
  avatarCacheDays: number;
  banCheckDays: number;
  enabledPlatforms: string[];
  inactivityBlurSeconds: number;
}
