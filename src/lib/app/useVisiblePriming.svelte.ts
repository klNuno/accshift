const VISIBLE_PRIME_DEBOUNCE_MS = 120;
/**
 * How long an account primed while on screen stays primed. Losing focus, or
 * opening Settings, no longer forgets it: an alt-tab back within this window
 * sends no profile or ban request for the accounts already shown.
 */
export const VISIBLE_PRIME_TTL_MS = 5 * 60 * 1000;

type VisiblePrimingDeps = {
  prepareAccountIds: (accountIds: readonly string[], forceRefresh?: boolean) => number;
  primeAccountIds: (
    accountIds: readonly string[],
    checkBans?: boolean,
    forceRefresh?: boolean,
    silent?: boolean,
    deferBackground?: boolean,
  ) => Promise<unknown>;
  now?: () => number;
};

export function createVisiblePriming(loader: VisiblePrimingDeps) {
  const now = loader.now ?? Date.now;
  let visiblePrimeTimer: ReturnType<typeof setTimeout> | null = null;
  let lastPreparedVisibleKey = "";
  let primedAt = new Map<string, number>();

  function clearTimer() {
    if (visiblePrimeTimer) {
      clearTimeout(visiblePrimeTimer);
      visiblePrimeTimer = null;
    }
  }

  /** Forget everything: the accounts on screen now belong to another load or tab. */
  function reset() {
    clearTimer();
    lastPreparedVisibleKey = "";
    primedAt = new Map();
  }

  /**
   * Stop priming while the grid is out of sight (window in background,
   * Settings open) without forgetting what was primed. Coming back prepares
   * the visible ids again and primes only those past the TTL.
   */
  function pause() {
    clearTimer();
    lastPreparedVisibleKey = "";
  }

  function scheduleVisiblePrime(visibleIds: string[], dueIds: string[]) {
    clearTimer();
    visiblePrimeTimer = setTimeout(() => {
      visiblePrimeTimer = null;
      loader.prepareAccountIds(visibleIds);
      if (dueIds.length === 0) return;
      const primedNow = now();
      for (const accountId of dueIds) primedAt.set(accountId, primedNow);
      void loader.primeAccountIds(dueIds, true, false, true, true);
    }, VISIBLE_PRIME_DEBOUNCE_MS);
  }

  /**
   * Call from a reactive context (e.g. `$effect`). Handles deduplication
   * via the visible key and tracks which IDs were already primed.
   *
   * Returns `false` when the effect should bail out (guard conditions not met),
   * so the caller can call `reset()` in that branch.
   */
  function processVisible(visibleIds: string[], activeTab: string, isSearching: boolean) {
    const visibleKey = `${activeTab}:${isSearching ? "search" : "folder"}:${[...visibleIds].sort().join(",")}`;
    if (visibleKey === lastPreparedVisibleKey) return;
    lastPreparedVisibleKey = visibleKey;
    const cutoff = now() - VISIBLE_PRIME_TTL_MS;
    scheduleVisiblePrime(
      visibleIds,
      visibleIds.filter((accountId) => (primedAt.get(accountId) ?? -Infinity) < cutoff),
    );
  }

  return {
    processVisible,
    reset,
    pause,
    destroy: clearTimer,
  };
}
