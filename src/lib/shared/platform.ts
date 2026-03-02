import type { ContextMenuAction } from "./contextMenu/types";
import type { AccountWarningPresentation } from "./accountWarnings";
import type { MessageKey, TranslationParams } from "$lib/i18n";

export interface PlatformAccount {
  id: string;
  displayName: string;
  username: string;
  lastLoginAt?: number | null;
}

export interface PlatformContextMenuConfirmConfig {
  title: string;
  message: string;
  confirmLabel?: string;
  onConfirm: () => void | Promise<void>;
}

export interface PlatformContextMenuCallbacks {
  copyToClipboard: (text: string, label: string) => void | Promise<void>;
  showToast: (msg: string) => void;
  getCurrentAccountId: () => string | null;
  refreshAccounts: () => void;
  confirmAction: (config: PlatformContextMenuConfirmConfig) => void;
  t: (key: MessageKey, params?: TranslationParams) => string;
}

export interface PlatformUiCallbacks {
  t: (key: MessageKey, params?: TranslationParams) => string;
}

export interface PlatformWarningLoadOptions extends PlatformUiCallbacks {
  forceRefresh?: boolean;
  silent?: boolean;
}

export interface PlatformProfileInfo {
  avatarUrl: string | null;
  displayName?: string | null;
}

export interface CachedPlatformProfile {
  url: string;
  displayName?: string;
  expired: boolean;
}

export interface PlatformAdapter {
  id: string;
  name: string;
  accent: string;
  reloadAfterAdd?: boolean;

  loadAccounts(): Promise<PlatformAccount[]>;
  getCurrentAccount(): Promise<string>;
  getStartupSnapshot?(): Promise<{
    accounts: PlatformAccount[];
    currentAccount: string;
  }>;
  isCurrentAccount?(account: PlatformAccount, currentAccount: string): boolean;
  switchAccount(account: PlatformAccount): Promise<void>;
  addAccount(): Promise<void>;

  getContextMenuActions(account: PlatformAccount, callbacks: PlatformContextMenuCallbacks): ContextMenuAction[];

  getProfileInfo?(accountId: string): Promise<PlatformProfileInfo | null>;
  getCachedProfile?(accountId: string): CachedPlatformProfile | null;
  getCachedWarningStates?(callbacks: PlatformUiCallbacks): Record<string, AccountWarningPresentation>;
  loadWarningStates?(
    accounts: PlatformAccount[],
    options: PlatformWarningLoadOptions
  ): Promise<Record<string, AccountWarningPresentation>>;
  getNoAccountsToastMessage?(callbacks: PlatformUiCallbacks): string | null;
  getLoadErrorToastMessage?(message: string, callbacks: PlatformUiCallbacks): string | null;
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
