import {
  checkCs2Bridge,
  fetchCs2BridgeAccounts,
  getCs2BridgeSettings,
  type Cs2BridgeAccount,
} from "./steamApi";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";

/** The source refreshes GC stats every ~3h; 5 min keeps hover data fresh
 * without hammering the bridge on every tab switch. */
const CACHE_TTL_MS = 5 * 60 * 1000;

let dataBySteamId = $state<Record<string, Cs2BridgeAccount>>({});
let version = $state(0);
let enabled = $state(false);
let enabledKnown = false;
let lastFetchAt = 0;
let inFlight: Promise<void> | null = null;
let operationQueue: Promise<void> = Promise.resolve();

/** The bridge exposes one shared server-side check queue. Keep every GET and
 * POST ordered locally too, so a fetch can never replace a freshly checked row
 * and rapid account switches cannot overlap checks. */
function enqueueBridgeOperation(operation: () => Promise<void>): Promise<void> {
  const run = operationQueue.catch(() => {}).then(operation);
  operationQueue = run.catch(() => {});
  return run;
}

export function getCs2BridgeData(steamId: string): Cs2BridgeAccount | null {
  return dataBySteamId[steamId] ?? null;
}

/** Bumps whenever bridge data changes; cheap memo key for extension content. */
export function getCs2BridgeVersion(): number {
  return version;
}

/** Called from settings after a config change so the next load refetches. */
export function invalidateCs2Bridge() {
  enabledKnown = false;
  lastFetchAt = 0;
  dataBySteamId = {};
  version += 1;
}

/** Fetch bridge data if the integration is enabled and the cache is stale.
 * Silent by design: the card extension simply shows nothing on failure. */
export async function loadCs2BridgeData(force = false): Promise<void> {
  if (inFlight) return inFlight;
  if (!force && lastFetchAt && Date.now() - lastFetchAt < CACHE_TTL_MS) return;

  const run = enqueueBridgeOperation(async () => {
    try {
      if (!enabledKnown) {
        enabled = (await getCs2BridgeSettings()).enabled;
        enabledKnown = true;
      }
      if (!enabled) return;
      const accounts = await fetchCs2BridgeAccounts();
      const map: Record<string, Cs2BridgeAccount> = {};
      for (const account of accounts) map[account.steamId] = account;
      dataBySteamId = map;
      version += 1;
      void logAppEvent("info", "frontend.cs2_bridge", "Bridge data refreshed", {
        accountCount: accounts.length,
      });
    } catch (error) {
      console.warn("[cs2-bridge] fetch failed:", error);
      void logAppEvent("warn", "frontend.cs2_bridge", "Bridge fetch failed", {
        error: serializeLogValue(error),
      });
    } finally {
      lastFetchAt = Date.now();
    }
  });
  inFlight = run;
  const clearInFlight = () => {
    if (inFlight === run) inFlight = null;
  };
  // Observe both outcomes directly. `finally()` would create a second rejected
  // promise when `run` fails, which becomes an unhandled rejection.
  void run.then(clearInFlight, clearInFlight);
  return run;
}

/** After switching away from a Steam account, ask the bridge to re-check that
 * previous account server-side and merge the fresh row into the cache so its
 * hover card updates live. Best-effort and silent: on failure the periodic read
 * stays the fallback. No-op when the bridge is disabled. */
export async function triggerCs2BridgeCheck(steamId: string): Promise<void> {
  return enqueueBridgeOperation(async () => {
    try {
      if (!enabledKnown) {
        enabled = (await getCs2BridgeSettings()).enabled;
        enabledKnown = true;
      }
      if (!enabled) return;
      const account = await checkCs2Bridge(steamId);
      if (account) {
        dataBySteamId = { ...dataBySteamId, [account.steamId]: account };
        version += 1;
        void logAppEvent("info", "frontend.cs2_bridge", "Bridge check applied", {
          steamId: account.steamId,
          level: account.level,
        });
      }
    } catch (error) {
      console.warn("[cs2-bridge] check failed:", error);
      void logAppEvent("warn", "frontend.cs2_bridge", "Bridge check failed", {
        error: serializeLogValue(error),
      });
    }
  });
}
