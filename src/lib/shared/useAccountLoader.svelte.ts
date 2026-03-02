import type { PlatformAdapter, PlatformAccount } from "./platform";
import { addToast } from "../features/notifications/store.svelte";
import type { AccountWarningPresentation } from "./accountWarnings";
import { DEFAULT_LOCALE, translate, type MessageKey, type TranslationParams } from "$lib/i18n";

const BATCH_SIZE = 5;
const LOAD_TOAST_COOLDOWN_MS = 30000;

function deferBackgroundTask(task: () => void | Promise<void>) {
  if (typeof window !== "undefined" && "requestIdleCallback" in window) {
    const requestIdle = (
      window as Window & {
        requestIdleCallback: (callback: IdleRequestCallback, options?: IdleRequestOptions) => number;
      }
    ).requestIdleCallback;
    requestIdle(() => { void task(); }, { timeout: 600 });
    return;
  }
  setTimeout(() => { void task(); }, 0);
}

export function createAccountLoader(
  getAdapter: () => PlatformAdapter | undefined,
  getVisibleAccountIds?: () => string[],
  translateMessage?: (key: MessageKey, params?: TranslationParams) => string,
) {
  // Centralized UI state for account loading, switching, avatars, and platform warnings.
  let accounts = $state<PlatformAccount[]>([]);
  let accountMap = $derived<Record<string, PlatformAccount>>(
    Object.fromEntries(accounts.map(a => [a.id, a]))
  );
  let currentAccount = $state("");
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);
  let avatarStates = $state<Record<string, { url: string | null; loading: boolean; refreshing: boolean }>>({});
  let warningStates = $state<Record<string, AccountWarningPresentation>>({});
  let lastLoadErrorToastAt = 0;
  let lastNoAccountsToastAt = 0;
  let latestLoadId = 0;
  const t = (key: MessageKey, params?: TranslationParams) =>
    translateMessage?.(key, params) ?? translate(DEFAULT_LOCALE, key, params);

  function resolveVisibleAccounts(source: PlatformAccount[]): PlatformAccount[] {
    if (!getVisibleAccountIds) return source;
    const requestedIds = getVisibleAccountIds();
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

  function updateAvatarState(accountId: string, next: Partial<{ url: string | null; loading: boolean; refreshing: boolean }>) {
    avatarStates[accountId] = {
      url: avatarStates[accountId]?.url ?? null,
      loading: avatarStates[accountId]?.loading ?? false,
      refreshing: avatarStates[accountId]?.refreshing ?? false,
      ...next,
    };
  }

  function applyProfileUpdate(account: PlatformAccount, profile: Awaited<ReturnType<NonNullable<PlatformAdapter["getProfileInfo"]>>>) {
    if (profile) {
      updateAvatarState(account.id, {
        url: profile.avatarUrl || avatarStates[account.id]?.url || null,
        loading: false,
        refreshing: false,
      });
      if (profile.displayName && profile.displayName !== account.displayName) {
        const idx = accounts.findIndex(a => a.id === account.id);
        if (idx !== -1) {
          accounts[idx] = { ...accounts[idx], displayName: profile.displayName };
        }
      }
      return;
    }
    updateAvatarState(account.id, {
      url: avatarStates[account.id]?.url || null,
      loading: false,
      refreshing: false,
    });
  }

  async function refreshProfile(adapter: PlatformAdapter, account: PlatformAccount) {
    if (!adapter.getProfileInfo) return;
    const profile = await adapter.getProfileInfo(account.id);
    applyProfileUpdate(account, profile);
  }

  async function loadProfilesForAccounts(
    accts: PlatformAccount[],
    forceRefresh = false
  ) {
    const adapter = getAdapter();
    if (!adapter) return;
    const forceAvatarRefresh = forceRefresh;

    // Use cached avatars immediately, then refresh only stale/missing entries.
    const needsRefresh: PlatformAccount[] = [];
    for (const account of accts) {
      const cached = adapter.getCachedProfile?.(account.id);
      if (cached) {
        const shouldRefresh = cached.expired || forceAvatarRefresh;
        updateAvatarState(account.id, { url: cached.url, loading: false, refreshing: shouldRefresh });
        if (shouldRefresh) {
          needsRefresh.push(account);
        }
      } else if (adapter.getProfileInfo) {
        updateAvatarState(account.id, { url: null, loading: true, refreshing: false });
        needsRefresh.push(account);
      }
    }

    // Keep requests bounded to avoid API/UI spikes on large account lists.
    for (let i = 0; i < needsRefresh.length; i += BATCH_SIZE) {
      const batch = needsRefresh.slice(i, i + BATCH_SIZE);
      await Promise.all(batch.map(async (account) => {
        await refreshProfile(adapter, account);
      }));
    }
  }

  async function loadWarningStatesForAccounts(
    adapter: PlatformAdapter,
    accts: PlatformAccount[],
    silent = true,
    forceRefresh = false,
  ) {
    if (!adapter.loadWarningStates || accts.length === 0) return;
    warningStates = await adapter.loadWarningStates(accts, { forceRefresh, silent, t });
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
    loading = true;
    error = null;
    try {
      warningStates = adapter.getCachedWarningStates?.({ t }) ?? {};
      if (adapter.getStartupSnapshot) {
        const snapshot = await adapter.getStartupSnapshot();
        accounts = snapshot.accounts;
        currentAccount = snapshot.currentAccount;
      } else {
        accounts = await adapter.loadAccounts();
        currentAccount = await adapter.getCurrentAccount();
      }
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
          })
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
      const message = String(e);
      error = message;
      accounts = [];
      currentAccount = "";
      warningStates = {};
      const errorToast = adapter.getLoadErrorToastMessage?.(message, { t });
      if (errorToast) {
        const now = Date.now();
        if (now - lastLoadErrorToastAt >= LOAD_TOAST_COOLDOWN_MS) {
          addToast(errorToast);
          lastLoadErrorToastAt = now;
        }
      }
    }
    loading = false;
  }

  async function switchTo(account: PlatformAccount) {
    const adapter = getAdapter();
    if (!adapter || switching) return;
    switching = true;
    error = null;
    try {
      await adapter.switchAccount(account);
      currentAccount = account.username;
      if (adapter.getProfileInfo) {
        updateAvatarState(account.id, { refreshing: true });
        void adapter.getProfileInfo(account.id)
          .then((profile) => {
            applyProfileUpdate(account, profile);
          })
          .catch(() => {
            updateAvatarState(account.id, { refreshing: false });
          });
      }
    } catch (e) {
      error = String(e);
      addToast(error);
    }
    switching = false;
  }

  async function addNew() {
    const adapter = getAdapter();
    if (!adapter) return;
    currentAccount = "";
    try { await adapter.addAccount(); } catch (e) {
      error = String(e);
      addToast(error);
    }
  }

  async function refreshVisibleAccounts(
    checkBans = false,
    forceRefresh = false,
    silent = true,
    deferBackground = true,
  ): Promise<number> {
    const adapter = getAdapter();
    const visibleAccounts = resolveVisibleAccounts(accounts);
    if (visibleAccounts.length === 0) return 0;
    const run = async () => {
      const tasks: Promise<unknown>[] = [
        loadProfilesForAccounts(visibleAccounts, forceRefresh),
      ];
      if (checkBans && adapter?.loadWarningStates) {
        tasks.push(loadWarningStatesForAccounts(adapter, visibleAccounts, silent, forceRefresh));
      }
      await Promise.all(tasks);
      return visibleAccounts.length;
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

  return {
    get accounts() { return accounts; },
    set accounts(v: PlatformAccount[]) { accounts = v; },
    get accountMap() { return accountMap; },
    get currentAccount() { return currentAccount; },
    get loading() { return loading; },
    get switching() { return switching; },
    get error() { return error; },
    get avatarStates() { return avatarStates; },
    get warningStates() { return warningStates; },
    load,
    switchTo,
    addNew,
    primeVisibleAccounts: refreshVisibleAccounts,
    refreshVisibleAccounts,
  };
}
