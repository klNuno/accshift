const VISIBLE_PRIME_DEBOUNCE_MS = 120;

type VisiblePrimingDeps = {
  prepareAccountIds: (accountIds: readonly string[], forceRefresh?: boolean) => number;
  primeAccountIds: (
    accountIds: readonly string[],
    checkBans?: boolean,
    forceRefresh?: boolean,
    silent?: boolean,
    deferBackground?: boolean,
  ) => Promise<unknown>;
};

export function createVisiblePriming(loader: VisiblePrimingDeps) {
  let visiblePrimeTimer: ReturnType<typeof setTimeout> | null = null;
  let lastPreparedVisibleKey = "";
  let lastPrimedVisibleIds = new Set<string>();

  function clearTimer() {
    if (visiblePrimeTimer) {
      clearTimeout(visiblePrimeTimer);
      visiblePrimeTimer = null;
    }
  }

  function reset() {
    clearTimer();
    lastPreparedVisibleKey = "";
    lastPrimedVisibleIds = new Set();
  }

  function scheduleVisiblePrime(visibleIds: string[], newlyVisibleIds: string[]) {
    clearTimer();
    visiblePrimeTimer = setTimeout(() => {
      visiblePrimeTimer = null;
      loader.prepareAccountIds(visibleIds);
      void loader.primeAccountIds(
        newlyVisibleIds.length > 0 ? newlyVisibleIds : visibleIds,
        true,
        false,
        true,
        true,
      );
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
    const previouslyPrimedIds = lastPrimedVisibleIds;
    lastPreparedVisibleKey = visibleKey;
    lastPrimedVisibleIds = new Set(visibleIds);
    scheduleVisiblePrime(
      visibleIds,
      visibleIds.filter((accountId) => !previouslyPrimedIds.has(accountId)),
    );
  }

  return {
    scheduleVisiblePrime,
    processVisible,
    reset,
    destroy: clearTimer,
  };
}
