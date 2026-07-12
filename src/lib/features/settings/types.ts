import type { Locale } from "$lib/i18n";
import type { PlatformSettings } from "$lib/shared/platform";

export type { PlatformSettings };

export interface DataRefreshSettings {
  avatarCacheDays: number;
  banCheckDays: number;
}

export interface AccountDisplaySettings {
  showUsernames: boolean;
  showLastLoginPerPlatform: Record<string, boolean>;
  showCardNotesInline: boolean;
  expandedFolders: boolean;
  /** Colored outline around cards that carry a custom color. */
  cardColorOutlines: boolean;
}

/** "system" follows the OS reduced-motion preference; "on"/"off" force it. */
export type AnimationsMode = "system" | "on" | "off";

/** "auto" blurs the UI when streaming software is detected; "off" never does. */
export type StreamerMode = "auto" | "off";

export interface AppSettings {
  language: Locale;
  themeId: string;
  backgroundOpacity: number;
  uiScalePercent: number;
  animations: AnimationsMode;
  streamerMode: StreamerMode;
  suspendGraphicsWhenMinimized: boolean;
  minimizeOnAccountSwitch: boolean;
  dataRefresh: DataRefreshSettings;
  enabledPlatforms: string[];
  /** Per-platform opt-out for network account health checks (e.g. Roblox session probe). */
  healthCheckPerPlatform: Record<string, boolean>;
  personasEnabled: boolean;
  defaultPlatformId: string;
  inactivityBlurSeconds: number;
  deepLinksEnabled: boolean;
  /** Mirrored by the CLI binary, which refuses list/switch when false. */
  cliEnabled: boolean;
  platformSettings: PlatformSettings;
  accountDisplay: AccountDisplaySettings;
  pinEnabled: boolean;
  pinHash: string;
}
