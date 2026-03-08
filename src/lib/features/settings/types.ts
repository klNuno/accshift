import type { Locale } from "$lib/i18n";

export type RuntimeOs = "windows" | "linux" | "macos" | "unknown";

export interface PlatformDef {
  id: string;
  name: string;
  accent: string;
  implemented: boolean;
  supportedOs: RuntimeOs[];
}

export interface DataRefreshSettings {
  avatarCacheDays: number;
  banCheckDays: number;
}

export interface AccountDisplaySettings {
  showUsernames: boolean;
  showLastLogin: boolean;
  showRiotLastLogin: boolean;
  showBattleNetLastLogin: boolean;
  showCardNotesInline: boolean;
}

export interface SteamPlatformSettings {
  runAsAdmin: boolean;
  launchOptions: string;
}

export interface PlatformSettings {
  steam: SteamPlatformSettings;
}

export interface AppSettings {
  language: Locale;
  theme: "dark" | "light";
  uiScalePercent: number;
  suspendGraphicsWhenMinimized: boolean;
  minimizeOnAccountSwitch: boolean;
  dataRefresh: DataRefreshSettings;
  enabledPlatforms: string[];
  defaultPlatformId: string;
  inactivityBlurSeconds: number;
  platformSettings: PlatformSettings;
  accountDisplay: AccountDisplaySettings;
  pinEnabled: boolean;
  pinHash: string;
}
