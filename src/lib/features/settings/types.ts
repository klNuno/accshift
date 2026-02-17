export interface PlatformDef {
  id: string;
  name: string;
  accent: string;
}

export interface AppSettings {
  theme: "dark" | "light";
  avatarCacheDays: number;
  banCheckDays: number;
  enabledPlatforms: string[];
  defaultPlatformId: string;
  inactivityBlurSeconds: number;
  steamRunAsAdmin: boolean;
  steamLaunchOptions: string;
  showUsernames: boolean;
  showLastLogin: boolean;
  pinEnabled: boolean;
  pinCode: string;
}
