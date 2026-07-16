import type { PlatformAdapter, PlatformAccount } from "./platform";
import { addToast } from "../features/notifications/store.svelte";
import type { AccountWarningChip, AccountWarningPresentation } from "./accountWarnings";
import { DEFAULT_LOCALE, translate, type MessageKey, type TranslationParams } from "$lib/i18n";
import { createAvatarLoader } from "./useAvatarLoader.svelte";

const LOAD_TOAST_COOLDOWN_MS = 30000;

function deferBackgroundTask(task: () => void | Promise<void>) {
  if (typeof window !== "undefined" && "requestIdleCallback" in window) {
    const requestIdle = (
      window as Window & {
        requestIdleCallback: (
          callback: IdleRequestCallback,
          options?: IdleRequestOptions,
        ) => number;
      }
    ).requestIdleCallback;
    requestIdle(
      () => {
        void task();
      },
      { timeout: 600 },
    );
    return;
  }
  setTimeout(() => {
    void task();
  }, 0);
}

function warningChipsEqual(a?: AccountWarningChip[], b?: AccountWarningChip[]): boolean {
  const left = a ?? [];
  const right = b ?? [];
  if (left.length !== right.length) return false;
  return left.every((chip, i) => chip.tone === right[i].tone && chip.text === right[i].text);
}

function warningsEqual(a: AccountWarningPresentation, b: AccountWarningPresentation): boolean {
  if (a === b) return true;
  return (
    a.tooltipText === b.tooltipText &&
    (a.cardOutlineTone ?? null) === (b.cardOutlineTone ?? null) &&
    !!a.listHasRed === !!b.listHasRed &&
    !!a.listHasOrange === !!b.listHasOrange &&
    warningChipsEqual(a.chips, b.chips)
  );
}

export function createAccountLoader(
  getAdapter: () => PlatformAdapter | undefined,
  getVisibleAccountIds?: () => string[],
  translateMessage?: (key: MessageKey, params?: TranslationParams) => string,
) {
  // Centralized UI state for account loading, switching, avatars, and platform warnings.
  let accounts = $state.raw<PlatformAccount[]>([]);
  let accountMap = $derived<Record<string, PlatformAccount>>(
    Object.fromEntries(accounts.map((a) => [a.id, a])),
  );
  let currentAccount = $state("");
  let currentAccountId = $derived.by(() => {
    const raw = currentAccount.trim();
    if (!raw) return null;
    const needle = raw.toLowerCase();
    const direct = accounts.find((account) => account.id.trim().toLowerCase() === needle);
    if (direct) return direct.id;
    const matched = accounts.find(
      (account) =>
        account.username.trim().toLowerCase() === needle ||
        (account.displayName || "").trim().toLowerCase() === needle,
    );
    return matched?.id ?? null;
  });
  let loading = $state(true);
  let adding = $state(false);
  let switching = $state(false);
  let switchingAccountId = $state<string | null>(null);
  let error = $state<string | null>(null);
  const avatars = createAvatarLoader(getAdapter);
  let warningStates = $state.raw<Record<string, AccountWarningPresentation>>({});
  let pendingWarningStates: Record<string, AccountWarningPresentation> | null = null;
  let warningFlushQueued = false;

  // Replacing the $state.raw record invalidates every card, so reuse unchanged entry
  // objects and skip the assignment entirely when nothing changed.
  function commitWarningStates(next: Record<string, AccountWarningPresentation>) {
    const prev = warningStates;
    const merged: Record<string, AccountWarningPresentation> = {};
    let changed = false;
    for (const [id, warning] of Object.entries(next)) {
      const previous = prev[id];
      if (previous && warningsEqual(previous, warning)) {
        merged[id] = previous;
      } else {
        merged[id] = warning;
        changed = true;
      }
    }
    if (!changed && Object.keys(prev).length === Object.keys(merged).length) return;
    warningStates = merged;
  }

  // Batch results landing in the same tick coalesce into one replacement. Each
  // adapter returns a full map, so last write wins is safe.
  function queueWarningStates(next: Record<string, AccountWarningPresentation>) {
    pendingWarningStates = next;
    if (warningFlushQueued) return;
    warningFlushQueued = true;
    queueMicrotask(() => {
      warningFlushQueued = false;
      const queued = pendingWarningStates;
      pendingWarningStates = null;
      if (queued) commitWarningStates(queued);
    });
  }

  function replaceWarningStates(next: Record<string, AccountWarningPresentation>) {
    pendingWarningStates = null;
    commitWarningStates(next);
  }

  let lastLoadErrorToastAt = 0;
  let lastNoAccountsToastAt = 0;
  let latestLoadId = 0;
  let latestPrimeRunId = 0;
  let latestSwitchId = 0;
  const t = (key: MessageKey, params?: TranslationParams) =>
    translateMessage?.(key, params) ?? translate(DEFAULT_LOCALE, key, params);

  function resolveAccountsByIds(
    source: PlatformAccount[],
    requestedIds: readonly string[],
  ): PlatformAccount[] {
    if (!requestedIds.length) return [];
    const byId = new Map(source.map((account) => [account.id, account]));
    const out: PlatformAccount[] = [];
    const seen = new Set<string>();
    for (const id of requestedIds) {
      if (seen.has(id)) continue;
      seen.add(id);
      const account = byId.get(id);
      if (account) out.push(account);
    }
    return out;
  }

  function resolveVisibleAccounts(source: PlatformAccount[]): PlatformAccount[] {
    if (!getVisibleAccountIds) return source;
    return resolveAccountsByIds(source, getVisibleAccountIds());
  }

  // A profile fetch started before removeAccount() can still resolve afterward and
  // repopulate avatar state for an account that is no longer in the list. Sweep it
  // back out so a stray in-flight response cannot leave an orphaned entry behind.
  function pruneOrphanedAvatarStates() {
    for (const accountId of Object.keys(avatars.states)) {
      if (!accountMap[accountId]) {
        avatars.removeAvatar(accountId);
      }
    }
  }

  async function loadWarningStatesForAccounts(
    adapter: PlatformAdapter,
    accts: PlatformAccount[],
    silent = true,
    forceRefresh = false,
    shouldContinue: () => boolean = () => true,
  ) {
    if (!adapter.loadWarningStates || accts.length === 0) return;
    if (!shouldContinue()) return;
    const nextWarningStates = await adapter.loadWarningStates(accts, { forceRefresh, silent, t });
    if (!shouldContinue()) return;
    queueWarningStates(nextWarningStates);
  }

  function createPrimeGuard() {
    const runId = ++latestPrimeRunId;
    const adapter = getAdapter();
    return () => runId === latestPrimeRunId && adapter === getAdapter();
  }

  async function load(
    onAfterLoad?: () => void,
    silent = false,
    showRefreshedToast = false,
    forceRefresh = false,
    checkBans = false,
    deferBackground = false,
  ) {
    const adapter = getAdapter();
    if (!adapter) return;
    const loadId = ++latestLoadId;
    latestPrimeRunId += 1;
    loading = true;
    error = null;
    try {
      replaceWarningStates(adapter.getCachedWarningStates?.({ t }) ?? {});
      let nextAccounts: PlatformAccount[];
      let nextCurrentAccount: string;
      if (adapter.getStartupSnapshot) {
        const snapshot = await adapter.getStartupSnapshot();
        if (loadId !== latestLoadId) return;
        nextAccounts = snapshot.accounts;
        nextCurrentAccount = snapshot.currentAccount;
      } else {
        nextAccounts = await adapter.loadAccounts();
        if (loadId !== latestLoadId) return;
        nextCurrentAccount = await adapter.getCurrentAccount();
        if (loadId !== latestLoadId) return;
      }
      accounts = nextAccounts;
      currentAccount = nextCurrentAccount;
      avatars.seedForAccounts(resolveVisibleAccounts(accounts), forceRefresh);
      if (accounts.length === 0) {
        const now = Date.now();
        const emptyToast = adapter.getNoAccountsToastMessage?.({ t });
        if (emptyToast && now - lastNoAccountsToastAt >= LOAD_TOAST_COOLDOWN_MS) {
          addToast(emptyToast);
          lastNoAccountsToastAt = now;
        }
      } else if (showRefreshedToast && !silent) {
        const count = accounts.length;
        addToast(
          t(count === 1 ? "toast.accountsRefreshed.single" : "toast.accountsRefreshed.multiple", {
            count,
          }),
          { type: "success" },
        );
      }
      onAfterLoad?.();
      const runBackgroundTasks = () => {
        if (loadId !== latestLoadId) return;
        void refreshVisibleAccounts(checkBans, forceRefresh, silent, false);
      };
      if (deferBackground) {
        deferBackgroundTask(runBackgroundTasks);
      } else {
        runBackgroundTasks();
      }
    } catch (e) {
      if (loadId !== latestLoadId) return;
      const message = String(e);
      error = message;
      accounts = [];
      currentAccount = "";
      replaceWarningStates({});
      console.error("[accounts] load failed:", e);
      const errorToast = adapter.getLoadErrorToastMessage?.(message, { t });
      if (errorToast) {
        const now = Date.now();
        if (now - lastLoadErrorToastAt >= LOAD_TOAST_COOLDOWN_MS) {
          addToast(errorToast, { type: "error" });
          lastLoadErrorToastAt = now;
        }
      }
    }
    if (loadId !== latestLoadId) return;
    loading = false;
  }

  async function switchTo(account: PlatformAccount): Promise<boolean> {
    const adapter = getAdapter();
    if (!adapter || switching) return false;
    // Invalidate in-flight loads so a pre-switch result cannot clobber currentAccount.
    latestLoadId += 1;
    // Our own generation token: if a platform/tab change (clearForPlatformChange) or
    // another switchTo() happens while we await below, switchId stops matching and we
    // stop applying currentAccount/switching updates to state that no longer belongs to us.
    const switchId = ++latestSwitchId;
    // The account we're leaving is the one we just played on, so its GC stats
    // (XP/level/weekly case) are what changed. Capture it before we overwrite
    // currentAccount so we can re-check it, not the destination.
    const previousAccountId = currentAccountId;
    switching = true;
    switchingAccountId = account.id;
    error = null;
    let succeeded = false;
    try {
      await adapter.switchAccount(account);
      if (switchId !== latestSwitchId) return false;
      succeeded = true;
      currentAccount = account.id;
      // CS2 bridge: re-check the account we just left (Steam only, SteamID64),
      // then refresh its hover card. Fire-and-forget, never impacts the switch.
      if (adapter.id === "steam" && previousAccountId && previousAccountId !== account.id) {
        const sourceId = previousAccountId;
        void import("$lib/platforms/steam/cs2Bridge.svelte").then((m) =>
          m.triggerCs2BridgeCheck(sourceId),
        );
      }
      if (adapter.getProfileInfo) {
        avatars.updateState(account.id, { refreshing: true });
        void adapter
          .getProfileInfo(account.id)
          .then((profile) => {
            // The account may have been removed while this fetch was in flight.
            if (!accountMap[account.id]) return;
            avatars.applyProfileUpdate(
              account,
              profile,
              () => accounts,
              (v) => {
                accounts = v;
              },
            );
          })
          .catch(() => {
            if (!accountMap[account.id]) return;
            avatars.updateState(account.id, { refreshing: false });
          });
      }
    } catch (e) {
      if (switchId !== latestSwitchId) return false;
      error = String(e);
      console.error("[accounts] switch failed:", e);
      const adapter = getAdapter();
      const mapped = adapter?.getSwitchErrorToastMessage?.(error, { t });
      addToast(mapped ?? t("toast.switchFailed"), { type: "error" });
    }
    if (switchId !== latestSwitchId) return succeeded;
    switching = false;
    switchingAccountId = null;
    return succeeded;
  }

  async function addNew() {
    if (adding) return;
    const adapter = getAdapter();
    if (!adapter) return;
    adding = true;
    try {
      currentAccount = "";
      let result: Awaited<ReturnType<PlatformAdapter["addAccount"]>>;
      try {
        result = await adapter.addAccount();
      } catch (e) {
        error = String(e);
        console.error("[accounts] add account failed:", e);
        // "Could not locate <client> executable" is the backend's stable wording
        // for a missing launcher; surface it as a human answer instead of the
        // generic failure line.
        if (/could not locate .* executable/i.test(error)) {
          const { getPlatformDefinition } = await import("$lib/platforms/registry");
          const platformName = getPlatformDefinition(adapter.id)?.name ?? adapter.id;
          addToast(t("toast.addAccountClientMissing", { platform: platformName }), {
            type: "error",
          });
        } else {
          addToast(t("toast.addAccountFailed"), { type: "error" });
        }
        return;
      }
      if (result.setupStatus) {
        return result;
      }
      if (adapter.reloadAfterAdd) {
        await load(undefined, true, false, false, false, false);
      }
      return result;
    } finally {
      adding = false;
    }
  }

  async function refreshVisibleAccounts(
    checkBans = false,
    forceRefresh = false,
    silent = true,
    deferBackground = true,
  ): Promise<number> {
    const visibleAccounts = resolveVisibleAccounts(accounts);
    return refreshAccounts(visibleAccounts, checkBans, forceRefresh, silent, deferBackground);
  }

  async function refreshAccountIds(
    accountIds: readonly string[],
    checkBans = false,
    forceRefresh = false,
    silent = true,
    deferBackground = true,
  ): Promise<number> {
    const targetAccounts = resolveAccountsByIds(accounts, accountIds);
    return refreshAccounts(targetAccounts, checkBans, forceRefresh, silent, deferBackground);
  }

  async function refreshAccounts(
    targetAccounts: PlatformAccount[],
    checkBans = false,
    forceRefresh = false,
    silent = true,
    deferBackground = true,
  ): Promise<number> {
    const adapter = getAdapter();
    if (targetAccounts.length === 0) return 0;
    const shouldContinue = createPrimeGuard();
    const run = async () => {
      if (!shouldContinue()) return 0;
      const tasks: Promise<unknown>[] = [
        avatars.loadProfilesForAccounts(
          targetAccounts,
          () => accounts,
          (v) => {
            accounts = v;
          },
          forceRefresh,
          shouldContinue,
        ),
      ];
      if (checkBans && adapter?.loadWarningStates) {
        tasks.push(
          loadWarningStatesForAccounts(
            adapter,
            targetAccounts,
            silent,
            forceRefresh,
            shouldContinue,
          ),
        );
      }
      await Promise.all(tasks);
      // A removeAccount() call while these fetches were in flight can have
      // resurrected avatar state for an account that is gone; sweep it back out.
      pruneOrphanedAvatarStates();
      return shouldContinue() ? targetAccounts.length : 0;
    };
    if (deferBackground) {
      return new Promise((resolve) => {
        deferBackgroundTask(async () => {
          resolve(await run());
        });
      });
    }
    return run();
  }

  function prepareVisibleAccounts(forceRefresh = false): number {
    const visibleAccounts = resolveVisibleAccounts(accounts);
    return prepareAccounts(visibleAccounts, forceRefresh);
  }

  function prepareAccountIds(accountIds: readonly string[], forceRefresh = false): number {
    return prepareAccounts(resolveAccountsByIds(accounts, accountIds), forceRefresh);
  }

  function prepareAccounts(targetAccounts: PlatformAccount[], forceRefresh = false): number {
    avatars.seedForAccounts(targetAccounts, forceRefresh);
    return targetAccounts.length;
  }

  function removeAccount(accountId: string) {
    accounts = accounts.filter((a) => a.id !== accountId);
    if (currentAccount === accountId) {
      currentAccount = "";
    }
    avatars.removeAvatar(accountId);
    pruneOrphanedAvatarStates();
    const { [accountId]: _warning, ...restWarnings } = warningStates;
    replaceWarningStates(restWarnings);
  }

  function clearForPlatformChange() {
    latestLoadId += 1;
    latestPrimeRunId += 1;
    latestSwitchId += 1;
    accounts = [];
    currentAccount = "";
    loading = true;
    error = null;
    switching = false;
    switchingAccountId = null;
    avatars.clear();
    replaceWarningStates({});
  }

  return {
    get accounts() {
      return accounts;
    },
    set accounts(v: PlatformAccount[]) {
      accounts = v;
    },
    get accountMap() {
      return accountMap;
    },
    get currentAccount() {
      return currentAccount;
    },
    get currentAccountId() {
      return currentAccountId;
    },
    get loading() {
      return loading;
    },
    get adding() {
      return adding;
    },
    get switchingAccountId() {
      return switchingAccountId;
    },
    get error() {
      return error;
    },
    get avatarStates() {
      return avatars.states;
    },
    get warningStates() {
      return warningStates;
    },
    load,
    switchTo,
    addNew,
    removeAccount,
    clearForPlatformChange,
    prepareAccountIds,
    prepareVisibleAccounts,
    primeAccountIds: refreshAccountIds,
    refreshVisibleAccounts,
  };
}
