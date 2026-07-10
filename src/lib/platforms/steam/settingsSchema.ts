import type { PlatformSettingsSchema } from "$lib/shared/platform";
import type { SteamPlatformSettings } from "./types";

function asRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  return value as Record<string, unknown>;
}

/** Defaults and sanitization for the `steam` entry of `platformSettings`.
 * Kept adapter-free so the settings store can run it before any lazy
 * platform code loads. Legacy flat keys (`steamRunAsAdmin`,
 * `steamLaunchOptions`) still resolve for configs written before the
 * per-platform settings object existed. */
export const steamSettingsSchema: PlatformSettingsSchema<SteamPlatformSettings> = {
  defaults: () => ({
    runAsAdmin: false,
    launchOptions: "",
    shutdownMode: "graceful",
  }),
  sanitize: (raw, legacyRoot) => {
    const record = asRecord(raw);
    const launchOptionsRaw = record.launchOptions ?? legacyRoot.steamLaunchOptions;
    return {
      runAsAdmin: Boolean(record.runAsAdmin ?? legacyRoot.steamRunAsAdmin),
      launchOptions:
        typeof launchOptionsRaw === "string" ? launchOptionsRaw.trim().slice(0, 256) : "",
      shutdownMode: record.shutdownMode === "force" ? "force" : "graceful",
    };
  },
};
