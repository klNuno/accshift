import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import {
  buildPlatformContextMenu,
  type CopyItemConfig,
} from "$lib/shared/contextMenu/platformMenuBuilder";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import { createPlatformApi } from "$lib/platforms/platformApi";

/** Raw account shape shared by the simple snapshot-based platforms
 * (GOG, Jagex, Discord, Epic). Custom shapes supply their own `toAccount`. */
export interface GenericRawAccount {
  accountId: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved?: boolean;
}

interface GenericStartupSnapshot<TRaw> {
  accounts: TRaw[];
  currentAccount: string;
}

export interface GenericAdapterConfig<TRaw> {
  id: string;
  /** i18n key prefix; defaults to `id`. Battle.net keys use "battlenet". */
  i18nPrefix?: string;
  reloadAfterAdd?: boolean;
  noAccountsToastKey: MessageKey;
  noAccountsHintKey?: MessageKey;
  /** Maps the backend account payload to the UI shape. Defaults to the
   * `GenericRawAccount` mapping (id/label/lastUsedAt). */
  toAccount?: (raw: TRaw) => PlatformAccount;
  /** Copy entries of the context menu. Defaults to a single account-id item
   * labelled `${i18nPrefix}.copyLabelAccountId`. */
  copyItems?: (account: PlatformAccount) => CopyItemConfig[];
  /** Extra params for the `${i18nPrefix}.forgotAccount` toast. */
  forgetToastParams?: (display: string) => TranslationParams;
  /** false = backend has no set_account_label command (Battle.net). */
  supportsAccountLabels?: boolean;
  /** Redacts the account id in switch logs (Battle.net masks emails). */
  maskSwitchLogId?: (accountId: string) => string;
}

function defaultToAccount(raw: GenericRawAccount): PlatformAccount {
  return {
    id: raw.accountId,
    displayName: raw.label || raw.accountId,
    username: raw.accountId,
    lastLoginAt: raw.lastUsedAt ?? null,
  };
}

/** Builds the full adapter (account list, switch, add flow, context menu,
 * labels) for platforms whose frontend is entirely driven by the shared
 * `platform_*` Tauri commands. */
export function createGenericAdapter<TRaw = GenericRawAccount>(
  config: GenericAdapterConfig<TRaw>,
): PlatformAdapter {
  const api = createPlatformApi(config.id);
  const prefix = config.i18nPrefix ?? config.id;
  const key = (suffix: string) => `${prefix}.${suffix}` as MessageKey;
  const toAccount =
    config.toAccount ?? (defaultToAccount as unknown as (raw: TRaw) => PlatformAccount);

  function getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    const display = (account.displayName || account.id).trim() || account.id;
    return buildPlatformContextMenu(config.id, account, callbacks, {
      copyItems: config.copyItems?.(account) ?? [
        {
          field: "accountId",
          value: account.id,
          labelKey: key("copyLabelAccountId"),
          clipboardLabelKey: key("copyLabelAccountId"),
        },
      ],
      forget: {
        titleKey: key("forgetConfirmTitle"),
        messageKey: key("forgetConfirmMessage"),
        confirmLabelKey: key("forget"),
        toastKey: key("forgotAccount"),
        toastParams: config.forgetToastParams?.(display),
        action: () => api.forgetAccount(account.id),
      },
      displayValue: display,
    });
  }

  const adapter: PlatformAdapter = {
    id: config.id,
    ...(config.reloadAfterAdd ? { reloadAfterAdd: true } : {}),

    ...createPlatformAddFlowHandlers({
      beginSetup: api.beginSetup,
      getSetupStatus: api.getSetupStatus,
      cancelSetup: api.cancelSetup,
    }),

    async loadAccounts(): Promise<PlatformAccount[]> {
      const accounts = await api.getAccounts<TRaw>();
      return accounts.map(toAccount);
    },

    async getCurrentAccount(): Promise<string> {
      return api.getCurrentAccount();
    },

    async getStartupSnapshot() {
      const snapshot = await api.getStartupSnapshot<GenericStartupSnapshot<TRaw>>();
      return {
        accounts: snapshot.accounts.map(toAccount),
        currentAccount: snapshot.currentAccount,
      };
    },

    async switchAccount(account: PlatformAccount): Promise<void> {
      const logDetails = config.maskSwitchLogId
        ? { accountId: config.maskSwitchLogId(account.id) }
        : undefined;
      await api.switchAccount(account.id, {}, logDetails);
    },

    getContextMenuActions,

    getNoAccountsToastMessage(callbacks) {
      return callbacks.t(config.noAccountsToastKey);
    },
  };

  if (config.supportsAccountLabels !== false) {
    adapter.setAccountLabel = (accountId: string, label: string) =>
      api.setAccountLabel(accountId, label);
  }

  if (config.noAccountsHintKey) {
    const hintKey = config.noAccountsHintKey;
    adapter.getNoAccountsHintMessage = (callbacks) => callbacks.t(hintKey);
  }

  return adapter;
}
