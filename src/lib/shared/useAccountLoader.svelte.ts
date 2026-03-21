import { untrack } from "svelte";
import type { PlatformAdapter, PlatformAccount } from "./platform";
import { addToast } from "../features/notifications/store.svelte";
import type { AccountWarningPresentation } from "./accountWarnings";
import { DEFAULT_LOCALE, translate, type MessageKey, type TranslationParams } from "$lib/i18n";

const BATCH_SIZE = 5;
const LOAD_TOAST_COOLDOWN_MS = 30000;
type AvatarState = { url: string | null; loading: boolean; refreshing: boolean };

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

export function createAccountLoader(
  getAdapter: () => PlatformAdapter | undefined,
  getVisibleAccountIds?: () => string[],
  translateMessage?: (key: MessageKey, params?: TranslationParams) => string,
) {
  // Centralized UI state for account loading, switching, avatars, and platform warnings.
  let accounts = $state<PlatformAccount[]>([]);
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
    const adapter = getAdapter();
    const matched = accounts.find(
      (account) =>
        adapter?.isCurrentAccount?.(account, raw) ||
        account.username.trim().toLowerCase() === needle ||
        (account.displayName || "").trim().toLowerCase() === needle,
    );
    return matched?.id ?? null;
  });
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);
  let avatarStates = $state<Record<string, AvatarState>>({});
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

  function updateAvatarState(accountId: string, next: Partial<AvatarState>) {
    const previous = untrack(() => avatarStates[accountId]);
    const nextState: AvatarState = {
      url: previous?.url ?? null,
      loading: previous?.loading ?? false,
      refreshing: previous?.refreshing ?? false,
      ...next,
    };
    if (
      previous &&
      previous.url === nextState.url &&
      previous.loading === nextState.loading &&
      previous.refreshing === nextState.refreshing
    ) {
      return;
    }
    untrack(() => {
      avatarStates = {
        ...avatarStates,
        [accountId]: nextState,
      };
    });
  }

  function mergeAvatarStates(updates: Record<string, AvatarState>) {
    const current = untrack(() => avatarStates);
    let changed = false;
    const nextStates: Record<string, AvatarState> = { ...current };
    for (const [accountId, nextState] of Object.entries(updates)) {
      const previous = current[accountId];
      if (
        previous &&
        previous.url === nextState.url &&
        previous.loading === nextState.loading &&
        previous.refreshing === nextState.refreshing
      ) {
        continue;
      }
      nextStates[accountId] = nextState;
      changed = true;
    }
    if (!changed) return;
    untrack(() => {
      avatarStates = nextStates;
    });
  }

  function updateAccountDisplayName(accountId: string, displayName: string) {
    const index = untrack(() => accounts.findIndex((account) => account.id === accountId));
    if (index === -1) return;
    const account = untrack(() => accounts[index]);
    if (!account || account.displayName === displayName) return;
    untrack(() => {
      const nextAccounts = accounts.slice();
      nextAccounts[index] = { ...account, displayName };
      accounts = nextAccounts;
    });
  }

  function seedAvatarStatesForAccounts(
    accts: PlatformAccount[],
    forceRefresh = false,
  ): PlatformAccount[] {
    const adapter = getAdapter();
    if (!adapter) return [];

    const forceAvatarRefresh = forceRefresh;
    const needsRefresh: PlatformAccount[] = [];
    const updates: Record<string, AvatarState> = {};

    for (const account of accts) {
      const existing = untrack(() => avatarStates[account.id]);
      const cached = adapter.getCachedProfile?.(account.id);
      if (cached) {
        const shouldRefresh = cached.expired || forceAvatarRefresh;
        updates[account.id] = {
          url: cached.url,
          loading: false,
          refreshing: shouldRefresh,
        };
        if (shouldRefresh) {
          needsRefresh.push(account);
        }
      } else if (adapter.getProfileInfo) {
        updates[account.id] = {
          url: existing?.url ?? null,
          loading: true,
          refreshing: false,
        };
        needsRefresh.push(account);
      }
    }

    mergeAvatarStates(updates);
    return needsRefresh;
  }

  function applyProfileUpdate(
    account: PlatformAccount,
    profile: Awaited<ReturnType<NonNullable<PlatformAdapter["getProfileInfo"]>>>,
  ) {
    if (profile) {
      updateAvatarState(account.id, {
        url: profile.avatarUrl,
        loading: profile.avatarLoading ?? false,
        refreshing: false,
      });
      if (profile.displayName && profile.displayName !== account.displayName) {
        updateAccountDisplayName(account.id, profile.displayName);
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

  async function loadProfilesForAccounts(accts: PlatformAccount[], forceRefresh = false) {
    const adapter = getAdapter();
    if (!adapter) return;
    const needsRefresh = seedAvatarStatesForAccounts(accts, forceRefresh);

    // Keep requests bounded to avoid API/UI spikes on large account lists.
    for (let i = 0; i < needsRefresh.length; i += BATCH_SIZE) {
      const batch = needsRefresh.slice(i, i + BATCH_SIZE);
      await Promise.all(
        batch.map(async (account) => {
          await refreshProfile(adapter, account);
        }),
      );
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
      seedAvatarStatesForAccounts(resolveVisibleAccounts(accounts), forceRefresh);
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
    if (loadId !== latestLoadId) return;
    loading = false;
  }

  async function switchTo(account: PlatformAccount) {
    const adapter = getAdapter();
    if (!adapter || switching) return;
    switching = true;
    error = null;
    try {
      await adapter.switchAccount(account);
      currentAccount = account.id;
      if (adapter.getProfileInfo) {
        updateAvatarState(account.id, { refreshing: true });
        void adapter
          .getProfileInfo(account.id)
          .then((profile) => {
            applyProfileUpdate(account, profile);
          })
          .catch(() => {
            updateAvatarState(account.id, { refreshing: false });
          });
      }
    } catch (e) {
      error = String(e);
      const adapter = getAdapter();
      const mapped = adapter?.getSwitchErrorToastMessage?.(error, { t });
      addToast(mapped ?? error);
    }
    switching = false;
  }

  async function addNew() {
    const adapter = getAdapter();
    if (!adapter) return;
    currentAccount = "";
    let result: Awaited<ReturnType<PlatformAdapter["addAccount"]>>;
    try {
      result = await adapter.addAccount();
    } catch (e) {
      error = String(e);
      addToast(error);
      return;
    }
    if (result.setupStatus) {
      return result;
    }
    if (adapter.reloadAfterAdd) {
      await load(undefined, true, false, false, false, false);
    }
    return result;
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
      const tasks: Promise<unknown>[] = [loadProfilesForAccounts(visibleAccounts, forceRefresh)];
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

  function prepareVisibleAccounts(forceRefresh = false): number {
    const visibleAccounts = resolveVisibleAccounts(accounts);
    seedAvatarStatesForAccounts(visibleAccounts, forceRefresh);
    return visibleAccounts.length;
  }

  function clearForPlatformChange() {
    latestLoadId += 1;
    accounts = [];
    currentAccount = "";
    loading = true;
    error = null;
    avatarStates = {};
    warningStates = {};
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
    get switching() {
      return switching;
    },
    get error() {
      return error;
    },
    get avatarStates() {
      return avatarStates;
    },
    get warningStates() {
      return warningStates;
    },
    load,
    switchTo,
    addNew,
    clearForPlatformChange,
    prepareVisibleAccounts,
    primeVisibleAccounts: refreshVisibleAccounts,
    refreshVisibleAccounts,
  };
}
