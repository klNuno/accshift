import type { Locale } from "$lib/i18n";

export interface PlatformDef {
  id: string;
  name: string;
  accent: string;
}

export interface AppSettings {
  language: Locale;
  theme: "dark" | "light";
  uiScalePercent: number;
  avatarCacheDays: number;
  banCheckDays: number;
  enabledPlatforms: string[];
  defaultPlatformId: string;
  inactivityBlurSeconds: number;
  steamRunAsAdmin: boolean;
  steamLaunchOptions: string;
  showUsernames: boolean;
  showLastLogin: boolean;
  showCardNotesInline: boolean;
  pinEnabled: boolean;
  pinCode: string;
}
