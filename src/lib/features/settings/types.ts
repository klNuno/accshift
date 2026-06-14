import type { Locale } from "$lib/i18n";

export type RuntimeOs = "windows" | "linux" | "macos" | "unknown";

export type PathPlaceholder = string | Partial<Record<RuntimeOs, string>>;

export interface PlatformDef {
  id: string;
  name: string;
  accent: string;
  implemented: boolean;
  supportedOs: RuntimeOs[];
  settingsTabKey?: string;
  settingsComponent?: () => Promise<{ default: any }>;
  pathLabelKey?: string;
  pathPlaceholder?: PathPlaceholder;
}

export function resolvePathPlaceholder(
  placeholder: PathPlaceholder | undefined,
  os: RuntimeOs,
): string {
  if (!placeholder) return "";
  if (typeof placeholder === "string") return placeholder;
  return placeholder[os] ?? placeholder.windows ?? placeholder.linux ?? placeholder.macos ?? "";
}

export interface DataRefreshSettings {
  avatarCacheDays: number;
  banCheckDays: number;
}

export interface AccountDisplaySettings {
  showUsernames: boolean;
  showLastLoginPerPlatform: Record<string, boolean>;
  showCardNotesInline: boolean;
  expandedFolders: boolean;
}

/** "system" follows the OS reduced-motion preference; "on"/"off" force it. */
export type AnimationsMode = "system" | "on" | "off";

export type SteamShutdownMode = "graceful" | "force";

export interface SteamPlatformSettings {
  runAsAdmin: boolean;
  launchOptions: string;
  shutdownMode: SteamShutdownMode;
}

export interface PlatformSettings {
  steam: SteamPlatformSettings;
}

export interface AppSettings {
  language: Locale;
  themeId: string;
  backgroundOpacity: number;
  uiScalePercent: number;
  animations: AnimationsMode;
  suspendGraphicsWhenMinimized: boolean;
  minimizeOnAccountSwitch: boolean;
  dataRefresh: DataRefreshSettings;
  enabledPlatforms: string[];
  defaultPlatformId: string;
  inactivityBlurSeconds: number;
  deepLinksEnabled: boolean;
  platformSettings: PlatformSettings;
  accountDisplay: AccountDisplaySettings;
  pinEnabled: boolean;
  pinHash: string;
}
