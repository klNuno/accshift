import type { ContextMenuItem } from "./types";

export interface PlatformAccount {
  id: string;
  displayName: string;
  username: string;
}

export interface PlatformAdapter {
  id: string;
  name: string;
  accent: string;

  loadAccounts(): Promise<PlatformAccount[]>;
  getCurrentAccount(): Promise<string>;
  switchAccount(account: PlatformAccount): Promise<void>;
  addAccount(): Promise<void>;

  getContextMenuItems(account: PlatformAccount, callbacks: {
    copyToClipboard: (text: string, label: string) => void;
    showToast: (msg: string) => void;
  }): ContextMenuItem[];

  getAvatarUrl?(accountId: string): Promise<string | null>;
  getCachedAvatar?(accountId: string): { url: string; expired: boolean } | null;
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
