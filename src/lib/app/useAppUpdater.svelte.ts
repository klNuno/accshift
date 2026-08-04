import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { trackUpdate } from "$lib/app/telemetryClient";
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

    // Which half failed matters: a check that never reaches the manifest is a
    // release-infrastructure problem, a download that dies is a payload or a
    // network one. Both used to look identical from outside, and both look
    // like a user who simply stopped launching the app.
    let stage: "check" | "download" = "check";
    try {
      const update = await check();
      if (!update) {
        updateState = "idle";
        return;
      }

      pendingUpdate = update;
      updateVersion = update.version;
      updateState = "downloading";
      trackUpdate("available", updateVersion);

      stage = "download";
      await update.download();

      updateState = "ready";
      trackUpdate("downloaded", updateVersion);
      addToast(
        updateVersion
          ? t("update.readyToastVersion", { version: updateVersion })
          : t("update.readyToast"),
      );
    } catch (error) {
      console.error("Updater check/download failed:", error);
      trackUpdate(
        "failed",
        updateVersion || undefined,
        stage === "check" ? "check_failed" : "download_failed",
      );
      pendingUpdate = null;
      updateVersion = "";
      updateState = "idle";
      updateCheckStarted = false;
    }
  }

  async function applyReadyUpdate() {
    if (updateState !== "ready" || !pendingUpdate) return;

    let stage: "install" | "relaunch" = "install";
    try {
      updateState = "applying";
      await beforeRelaunch?.();
      await pendingUpdate.install();
      stage = "relaunch";
      // Emitted before the relaunch, not after: the process is about to be
      // replaced, so there is no "after" in which to report anything.
      trackUpdate("applied", updateVersion);
      await relaunch();
    } catch (error) {
      console.error("Failed to restart for update:", error);
      trackUpdate(
        "failed",
        updateVersion || undefined,
        stage === "install" ? "install_failed" : "relaunch_failed",
      );
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
