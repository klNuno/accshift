import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { createSafeContextAction } from "$lib/shared/contextMenu/actions";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { captureProfile, forgetProfile } from "./riotApi";
import { forgetCachedRiotProfile, getCachedRiotProfileMeta } from "./accountCache";

export function getRiotContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const profile = getCachedRiotProfileMeta(account.id);
  const captureLabel =
    profile?.snapshot_state === "ready"
      ? callbacks.t("riot.recaptureSession")
      : callbacks.t("riot.captureSession");

  const captureAction: ContextMenuAction = {
    id: `riot.capture.${account.id}`,
    group: "platform.primary",
    label: captureLabel,
    action: createSafeContextAction(callbacks, async () => {
      await captureProfile(account.id);
      callbacks.showToast(callbacks.t("riot.capturedSession", { profile: account.displayName }));
      callbacks.refreshAccounts();
    }),
  };

  return [
    captureAction,
    ...buildPlatformContextMenu("riot", account, callbacks, {
      copyItems: [],
      forget: {
        titleKey: "riot.forgetConfirmTitle",
        messageKey: "riot.forgetConfirmMessage",
        confirmLabelKey: "riot.forget",
        toastKey: "riot.forgotProfile",
        toastParams: { profile: account.displayName },
        action: async () => {
          await forgetProfile(account.id);
          forgetCachedRiotProfile(account.id);
        },
      },
      displayValue: (account.displayName || account.username).trim() || account.username,
    }),
  ];
}
