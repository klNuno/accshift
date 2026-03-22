import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { confirmSafeContextAction, createSafeContextAction } from "$lib/shared/contextMenu/actions";
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
  return [
    {
      id: `riot.capture.${account.id}`,
      group: "platform.primary",
      label: captureLabel,
      action: createSafeContextAction(callbacks, async () => {
        await captureProfile(account.id);
        callbacks.showToast(callbacks.t("riot.capturedSession", { profile: account.displayName }));
        callbacks.refreshAccounts();
      }),
    },
    {
      id: `riot.forget.${account.id}`,
      group: "platform.danger",
      label: callbacks.t("riot.forget"),
      action: () => {
        const display = (account.displayName || account.username).trim() || account.username;
        confirmSafeContextAction(
          callbacks,
          {
            title: callbacks.t("riot.forgetConfirmTitle", { display }),
            message: callbacks.t("riot.forgetConfirmMessage"),
            confirmLabel: callbacks.t("riot.forget"),
          },
          async () => {
            await forgetProfile(account.id);
            forgetCachedRiotProfile(account.id);
            callbacks.showToast(
              callbacks.t("riot.forgotProfile", { profile: account.displayName }),
            );
            callbacks.removeAccount(account.id);
          },
        );
      },
    },
  ];
}
