import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { trackUpdate } from "$lib/app/telemetryClient";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type PendingUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;
type UpdateState = "idle" | "checking" | "downloading" | "ready" | "applying";

/** The three ways a manifest check can end badly, as telemetry error codes. */
export type UpdateCheckErrorCode =
  | "update_target_missing"
  | "update_manifest_invalid"
  | "check_failed";

// The plugin serializes its error enum through Display, so a rejection is a
// string and there is no variant to switch on. These patterns come from
// tauri-plugin-updater 2.10.1 src/error.rs.

// Error::TargetNotFound and Error::TargetsNotFound. Both end on the same
// phrase, and both mean the same thing: this build's target key is absent from
// latest.json, so the release simply does not ship an update for this OS.
const TARGET_MISSING_RE = /found in the response `platforms` object/;

// The endpoint answered, but the answer is not a release manifest we can use:
// Error::ReleaseNotFound, Error::SignatureUtf8, Error::InvalidUpdaterFormat,
// and the serde_json and semver errors the plugin forwards verbatim.
const MANIFEST_INVALID_PATTERNS = [
  /Could not fetch a valid release JSON/i,
  /could not be decoded/i,
  /invalid updater binary format/i,
  /missing field/i,
  /expected value at line/i,
  /invalid type:/i,
  /trailing characters at line/i,
  /while parsing (major|minor|patch) version number/i,
];

function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/**
 * Sorts a failed `check()` into a telemetry code.
 *
 * A missing target is not the same incident as a dead endpoint: one is a
 * release that never published an artifact for this platform, the other is a
 * network or infrastructure fault. Collapsing both into `check_failed` made
 * every Linux and macOS launch look like an outage.
 */
export function classifyUpdateCheckError(error: unknown): UpdateCheckErrorCode {
  const text = errorText(error);
  if (TARGET_MISSING_RE.test(text)) return "update_target_missing";
  if (MANIFEST_INVALID_PATTERNS.some((pattern) => pattern.test(text))) {
    return "update_manifest_invalid";
  }
  return "check_failed";
}

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
      const errorCode = stage === "check" ? classifyUpdateCheckError(error) : "download_failed";
      if (errorCode === "update_target_missing") {
        // Nothing broke and nothing is wrong with this install: the release
        // just carries no artifact for this platform. Logging it as an error
        // buries the real ones.
        console.info("Updater: this platform is not in the release manifest:", error);
      } else {
        console.error("Updater check/download failed:", error);
      }
      trackUpdate("failed", updateVersion || undefined, errorCode);
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
