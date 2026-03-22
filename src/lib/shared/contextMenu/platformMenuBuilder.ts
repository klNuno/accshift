import type { ContextMenuAction } from "./types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "../platform";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import { confirmSafeContextAction } from "./actions";

export interface CopyItemConfig {
  field: string;
  value: string;
  labelKey: MessageKey;
  clipboardLabelKey: MessageKey;
}

export interface ForgetConfig {
  titleKey: MessageKey;
  titleParams?: TranslationParams;
  messageKey: MessageKey;
  confirmLabelKey: MessageKey;
  toastKey: MessageKey;
  toastParams?: TranslationParams;
  action: () => Promise<void>;
}

export function buildPlatformContextMenu(
  platformId: string,
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
  options: {
    copyItems: CopyItemConfig[];
    forget: ForgetConfig;
    displayValue?: string;
  },
): ContextMenuAction[] {
  const { copyItems, forget } = options;
  const display =
    options.displayValue ??
    ((account.displayName || account.username || account.id).trim() || account.id);

  const items: ContextMenuAction[] = copyItems.map((item) => ({
    id: `${platformId}.copy.${item.field}.${account.id}`,
    group: "platform.copy",
    label: callbacks.t(item.labelKey),
    action: () => callbacks.copyToClipboard(item.value, callbacks.t(item.clipboardLabelKey)),
  }));

  items.push({
    id: `${platformId}.forget.${account.id}`,
    group: "platform.danger",
    label: callbacks.t(forget.confirmLabelKey),
    action: () => {
      confirmSafeContextAction(
        callbacks,
        {
          title: callbacks.t(forget.titleKey, { display, ...forget.titleParams }),
          message: callbacks.t(forget.messageKey),
          confirmLabel: callbacks.t(forget.confirmLabelKey),
        },
        async () => {
          await forget.action();
          callbacks.showToast(callbacks.t(forget.toastKey, { display, ...forget.toastParams }));
          callbacks.removeAccount(account.id);
        },
      );
    },
  });

  return items;
}
