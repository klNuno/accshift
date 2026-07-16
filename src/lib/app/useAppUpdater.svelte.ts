import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type PendingUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;
type UpdateState = "idle" | "checking" | "downloading" | "ready" | "applying";

type AppUpdaterOptions = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  addToast: (message: string) => void;
  beforeRelaunch?: () => Promise<void>;
};

export function createAppUpdater({ t, addToast, beforeRelaunch }: AppUpdaterOptions) {
  let updateState = $state<UpdateState>("idle");
  let updateVersion = $state("");
  let pendingUpdate = $state<PendingUpdate | null>(null);
  let updateCheckStarted = false;

  let ctaLabel = $derived(
    updateState === "ready"
      ? t("update.ctaAvailable")
      : updateState === "applying"
        ? t("update.ctaInstalling")
        : null,
  );

  let ctaTitle = $derived(
    updateVersion
      ? t("update.restartToApplyVersion", { version: updateVersion })
      : t("update.restartToApply"),
  );

  let ctaDisabled = $derived(updateState === "applying");

  async function startBackgroundUpdateFlow() {
    if (import.meta.env.DEV) return;
    if (updateCheckStarted) return;
    updateCheckStarted = true;
    updateState = "checking";

    try {
      const update = await check();
      if (!update) {
        updateState = "idle";
        return;
      }

      pendingUpdate = update;
      updateVersion = update.version;
      updateState = "downloading";

      await update.download();

      updateState = "ready";
      addToast(
        updateVersion
          ? t("update.readyToastVersion", { version: updateVersion })
          : t("update.readyToast"),
      );
    } catch (error) {
      console.error("Updater check/download failed:", error);
      pendingUpdate = null;
      updateVersion = "";
      updateState = "idle";
      updateCheckStarted = false;
    }
  }

  async function applyReadyUpdate() {
    if (updateState !== "ready" || !pendingUpdate) return;

    try {
      updateState = "applying";
      await beforeRelaunch?.();
      await pendingUpdate.install();
      await relaunch();
    } catch (error) {
      console.error("Failed to restart for update:", error);
      pendingUpdate = null;
      updateVersion = "";
      updateState = "idle";
      updateCheckStarted = false;
      addToast(t("update.restartFailed"));
      void startBackgroundUpdateFlow();
    }
  }

  return {
    get ctaLabel() {
      return ctaLabel;
    },
    get ctaTitle() {
      return ctaTitle;
    },
    get ctaDisabled() {
      return ctaDisabled;
    },
    startBackgroundUpdateFlow,
    applyReadyUpdate,
  };
}
