import type { ContextMenuItem } from "./types";

export interface PlatformAccount {
  id: string;
  displayName: string;
  username: string;
  lastLoginAt?: number | null;
}

export interface PlatformAdapter {
  id: string;
  name: string;
  accent: string;

  loadAccounts(): Promise<PlatformAccount[]>;
  getCurrentAccount(): Promise<string>;
  getStartupSnapshot?(): Promise<{
    accounts: PlatformAccount[];
    currentAccount: string;
  }>;
  switchAccount(account: PlatformAccount): Promise<void>;
  addAccount(): Promise<void>;

  getContextMenuItems(account: PlatformAccount, callbacks: {
    copyToClipboard: (text: string, label: string) => void;
    showToast: (msg: string) => void;
  }): ContextMenuItem[];

  getProfileInfo?(accountId: string): Promise<{
    avatar_url: string | null;
    display_name: string | null;
    vac_banned: boolean;
    trade_ban_state: string;
  } | null>;
  getCachedProfile?(accountId: string): {
    url: string;
    displayName?: string;
    vacBanned?: boolean;
    tradeBanState?: string;
    expired: boolean;
  } | null;
}

const adapters = new Map<string, PlatformAdapter>();

export function registerPlatform(adapter: PlatformAdapter) {
  adapters.set(adapter.id, adapter);
}

export function getPlatform(id: string): PlatformAdapter | undefined {
  return adapters.get(id);
}

export function getAllPlatforms(): PlatformAdapter[] {
  return Array.from(adapters.values());
}
