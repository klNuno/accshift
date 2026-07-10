import type { Component } from "svelte";
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
  confirmColor?: string;
  onConfirm: () => void | Promise<void>;
}

export interface PlatformContextMenuInputConfig {
  title: string;
  placeholder: string;
  initialValue: string;
  allowEmpty?: boolean;
  onConfirm: (value: string) => void | Promise<void>;
}

export interface PlatformContextMenuCallbacks {
  copyToClipboard: (text: string, label: string) => void | Promise<void>;
  showToast: (msg: string) => void;
  getCurrentAccountId: () => string | null;
  refreshAccounts: () => void;
  removeAccount: (accountId: string) => void;
  confirmAction: (config: PlatformContextMenuConfirmConfig) => void;
  openInputDialog?: (config: PlatformContextMenuInputConfig) => void;
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
  avatarLoading?: boolean;
}

export interface CachedPlatformProfile {
  // null = cached "no avatar" result; the entry still suppresses refetches.
  url: string | null;
  displayName?: string;
  expired: boolean;
}

export type PlatformAddFlowState =
  | "waiting_for_client"
  | "waiting_for_login"
  | "capturing"
  | "ready"
  | "failed";

export interface PlatformAddFlowStatus {
  setupId: string;
  state: PlatformAddFlowState | string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export interface PlatformAddAccountResult {
  setupStatus?: PlatformAddFlowStatus | null;
}

/** Platform-agnostic shape of a bulk edit outcome. Platform results may carry
 * extra data per failure; the app layer only reads the counts. */
export interface PlatformBulkEditResult {
  succeeded: number;
  failed: { error: string }[];
}

/** Props contract every platform bulk edit bar component must accept. */
export interface PlatformBulkEditBarProps {
  selectedIds: Set<string>;
  activeAccountSelected?: boolean;
  onSelectAll: () => void;
  onDeselectAll: () => void;
  onCopyUrls: (urls: string[]) => void;
  onClose: () => void;
  onResult: (result: PlatformBulkEditResult) => void;
  t: (key: MessageKey, params?: TranslationParams) => string;
}

export type PlatformBulkEditBarComponent = Component<PlatformBulkEditBarProps>;

export interface PlatformBulkEditCapability {
  /** Lazy import of the platform's bulk edit bar component. */
  loadBar: () => Promise<{ default: PlatformBulkEditBarComponent }>;
}

/** Per-platform settings schema: defaults for the settings store plus a
 * sanitizer applied on every read of persisted settings. `legacyRoot` is the
 * raw top-level settings object, for platforms that migrated flat keys. */
export interface PlatformSettingsSchema<T = unknown> {
  defaults: () => T;
  sanitize: (raw: unknown, legacyRoot: Record<string, unknown>) => T;
}

/** Declarative capabilities consulted by the app layer without loading the
 * (lazy) adapter. Lives on the static platform definition in the registry. */
export interface PlatformCapabilities {
  /** Platform offers a multi-select bulk edit bar (titlebar button + bar). */
  bulkEdit?: PlatformBulkEditCapability;
  /** Platform participates in the settings "refresh now" actions. */
  profileRefresh?: { avatars?: boolean; bans?: boolean };
  /** Account cards can show a distinct username line under the display name. */
  accountUsernames?: boolean;
  /** Message shown when an account has no known last login. */
  lastLoginUnknownKey?: MessageKey;
  /** Prefetch the profile right after a completed add flow. */
  primeProfileAfterAdd?: boolean;
  /** Platform has account warning states worth rechecking on reloads. */
  accountWarnings?: boolean;
  /** Client-storage targets holding this platform's account data; an external
   * change to one of them triggers an account reload while on this tab. */
  externalDataStores?: string[];
  /** Per-platform settings blob stored under `platformSettings[id]`. */
  settings?: PlatformSettingsSchema;
}

export type RuntimeOs = "windows" | "linux" | "macos" | "unknown";

export type PathPlaceholder = string | Partial<Record<RuntimeOs, string>>;

/** Static, eagerly-loaded description of a platform. The (lazy) adapter is
 * only fetched when the platform is actually used. */
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
  capabilities?: PlatformCapabilities;
}

export function resolvePathPlaceholder(
  placeholder: PathPlaceholder | undefined,
  os: RuntimeOs,
): string {
  if (!placeholder) return "";
  if (typeof placeholder === "string") return placeholder;
  return placeholder[os] ?? placeholder.windows ?? placeholder.linux ?? placeholder.macos ?? "";
}

/** Augmented by each platform (via `declare module`) to type its own entry.
 * The stored shape is `platformSettings[platformId]`; defaults and
 * sanitization come from the `settings` capability in the platform's
 * registry definition. */
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface PlatformSettingsRegistry {}

export type PlatformSettings = PlatformSettingsRegistry & Record<string, unknown>;

export interface PlatformAdapter {
  id: string;
  reloadAfterAdd?: boolean;

  loadAccounts(): Promise<PlatformAccount[]>;
  getCurrentAccount(): Promise<string>;
  getStartupSnapshot?(): Promise<{
    accounts: PlatformAccount[];
    currentAccount: string;
  }>;
  switchAccount(account: PlatformAccount): Promise<void>;
  addAccount(): Promise<PlatformAddAccountResult>;
  pollAddFlow?(setupId: string): Promise<PlatformAddFlowStatus>;
  cancelAddFlow?(setupId: string): Promise<void>;

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[];

  setAccountLabel?(accountId: string, label: string): Promise<void>;

  getProfileInfo?(accountId: string): Promise<PlatformProfileInfo | null>;
  /** Batch variant of `getProfileInfo`: one backend call for many accounts.
   * A `null` entry means "no data" (keep whatever avatar is on screen). */
  getProfileInfos?(accountIds: string[]): Promise<Record<string, PlatformProfileInfo | null>>;
  getCachedProfile?(accountId: string): CachedPlatformProfile | null;
  getCachedWarningStates?(
    callbacks: PlatformUiCallbacks,
  ): Record<string, AccountWarningPresentation>;
  loadWarningStates?(
    accounts: PlatformAccount[],
    options: PlatformWarningLoadOptions,
  ): Promise<Record<string, AccountWarningPresentation>>;
  getNoAccountsToastMessage?(callbacks: PlatformUiCallbacks): string | null;
  getNoAccountsHintMessage?(callbacks: PlatformUiCallbacks): string | null;
  getSwitchErrorToastMessage?(message: string, callbacks: PlatformUiCallbacks): string | null;
  getLoadErrorToastMessage?(message: string, callbacks: PlatformUiCallbacks): string | null;
}

const adapters = new Map<string, PlatformAdapter>();

export function registerPlatform(adapter: PlatformAdapter) {
  adapters.set(adapter.id, adapter);
}

export function getPlatform(id: string): PlatformAdapter | undefined {
  return adapters.get(id);
}
