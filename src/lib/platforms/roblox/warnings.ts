import type { AccountWarningPresentation } from "$lib/shared/accountWarnings";
import type {
  PlatformAccount,
  PlatformUiCallbacks,
  PlatformWarningLoadOptions,
} from "$lib/shared/platform";
import { getSettings } from "$lib/features/settings/store";
import { checkSessions } from "./robloxApi";

// Dead sessions confirmed by the backend probe this session. Roblox cookies
// carry no readable expiry, so the only signal is a network check; we run it
// once per session (unless forced) and cache the result in memory.
let deadUserIds = new Set<string>();
let checkedThisSession = false;
let inFlight: Promise<void> | null = null;

function expiredPresentation(t: PlatformUiCallbacks["t"]): AccountWarningPresentation {
  return {
    tooltipText: t("roblox.sessionExpiredTooltip"),
    cardOutlineTone: "orange",
    listHasOrange: true,
    chips: [{ tone: "orange", text: t("roblox.sessionExpired") }],
  };
}

function toWarningMap(t: PlatformUiCallbacks["t"]): Record<string, AccountWarningPresentation> {
  const warnings: Record<string, AccountWarningPresentation> = {};
  for (const userId of deadUserIds) {
    warnings[userId] = expiredPresentation(t);
  }
  return warnings;
}

function healthCheckEnabled(): boolean {
  return getSettings().healthCheckPerPlatform["roblox"] !== false;
}

// A switch that failed with HTTP 401 is proof the stored cookie is dead, so
// flag the account immediately instead of waiting for the next probe. Not
// gated on the health-check setting: that setting controls the background
// network probe, and this signal comes from a user-initiated switch.
export function markRobloxSessionExpired(userId: string): void {
  deadUserIds.add(userId);
}

// A fresh cookie was stored for this account (re-add) or a switch succeeded;
// the expired flag no longer applies.
export function clearRobloxSessionExpired(userId: string): void {
  deadUserIds.delete(userId);
}

export function getCachedRobloxWarningStates(
  callbacks: PlatformUiCallbacks,
): Record<string, AccountWarningPresentation> {
  return toWarningMap(callbacks.t);
}

export async function loadRobloxWarningStates(
  accounts: PlatformAccount[],
  options: PlatformWarningLoadOptions,
): Promise<Record<string, AccountWarningPresentation>> {
  const { forceRefresh = false, t } = options;
  // The setting gates the network probe only; entries marked by failed
  // switches still render so the map must be returned either way.
  if (!healthCheckEnabled()) return toWarningMap(t);
  if (accounts.length === 0) return toWarningMap(t);

  if (checkedThisSession && !forceRefresh) return toWarningMap(t);

  if (!inFlight) {
    inFlight = (async () => {
      try {
        const dead = await checkSessions();
        deadUserIds = new Set(dead);
        checkedThisSession = true;
      } catch (e) {
        console.error("[roblox-health] session check failed:", e);
      } finally {
        inFlight = null;
      }
    })();
  }
  await inFlight;

  return toWarningMap(t);
}
