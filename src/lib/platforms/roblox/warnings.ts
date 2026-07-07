import type { AccountWarningPresentation } from "$lib/shared/accountWarnings";
import type {
  PlatformAccount,
  PlatformUiCallbacks,
  PlatformWarningLoadOptions,
} from "$lib/shared/platform";
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
