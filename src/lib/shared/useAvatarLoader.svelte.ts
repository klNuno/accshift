import { untrack } from "svelte";
import type { PlatformAdapter, PlatformAccount } from "./platform";

const BATCH_SIZE = 15;

export type AvatarState = { url: string | null; loading: boolean; refreshing: boolean };

export function createAvatarLoader(getAdapter: () => PlatformAdapter | undefined) {
  let avatarStates = $state.raw<Record<string, AvatarState>>({});

  function updateState(accountId: string, next: Partial<AvatarState>) {
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
      avatarStates = { ...avatarStates, [accountId]: nextState };
    });
  }

  function mergeStates(updates: Record<string, AvatarState>) {
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

  function updateAccountDisplayName(
    getAccounts: () => PlatformAccount[],
    setAccounts: (v: PlatformAccount[]) => void,
    accountId: string,
    displayName: string,
  ) {
    // Read the current list at update time so concurrent updates do not
    // overwrite each other through a stale snapshot.
    untrack(() => {
      const accounts = getAccounts();
      const index = accounts.findIndex((a) => a.id === accountId);
      if (index === -1) return;
      const account = accounts[index];
      if (!account || account.displayName === displayName) return;
      const next = accounts.slice();
      next[index] = { ...account, displayName };
      setAccounts(next);
    });
  }

  function seedForAccounts(accts: PlatformAccount[], forceRefresh = false): PlatformAccount[] {
    const adapter = getAdapter();
    if (!adapter) return [];

    const needsRefresh: PlatformAccount[] = [];
    const updates: Record<string, AvatarState> = {};

    for (const account of accts) {
      const existing = untrack(() => avatarStates[account.id]);
      const cached = adapter.getCachedProfile?.(account.id);
      if (cached) {
        const shouldRefresh = cached.expired || forceRefresh;
        updates[account.id] = {
          url: cached.url,
          loading: false,
          refreshing: shouldRefresh,
        };
        if (shouldRefresh) needsRefresh.push(account);
      } else if (adapter.getProfileInfo) {
        if (!existing) {
          updates[account.id] = { url: null, loading: true, refreshing: false };
        }
        needsRefresh.push(account);
      }
    }

    mergeStates(updates);
    return needsRefresh;
  }

  function applyProfileUpdate(
    account: PlatformAccount,
    profile: Awaited<ReturnType<NonNullable<PlatformAdapter["getProfileInfo"]>>>,
    getAccounts: () => PlatformAccount[],
    setAccounts: (v: PlatformAccount[]) => void,
  ) {
    if (profile) {
      updateState(account.id, {
        url: profile.avatarUrl,
        loading: profile.avatarLoading ?? false,
        refreshing: false,
      });
      if (profile.displayName && profile.displayName !== account.displayName) {
        updateAccountDisplayName(getAccounts, setAccounts, account.id, profile.displayName);
      }
      return;
    }
    updateState(account.id, {
      url: avatarStates[account.id]?.url || null,
      loading: false,
      refreshing: false,
    });
  }

  async function refreshProfile(
    adapter: PlatformAdapter,
    account: PlatformAccount,
    getAccounts: () => PlatformAccount[],
    setAccounts: (v: PlatformAccount[]) => void,
    shouldContinue: () => boolean = () => true,
  ) {
    if (!adapter.getProfileInfo) return;
    if (!shouldContinue()) return;
    const profile = await adapter.getProfileInfo(account.id);
    if (!shouldContinue()) return;
    applyProfileUpdate(account, profile, getAccounts, setAccounts);
  }

  async function loadProfilesForAccounts(
    accts: PlatformAccount[],
    getAccounts: () => PlatformAccount[],
    setAccounts: (v: PlatformAccount[]) => void,
    forceRefresh = false,
    shouldContinue: () => boolean = () => true,
  ) {
    const adapter = getAdapter();
    if (!adapter) return;
    const needsRefresh = seedForAccounts(accts, forceRefresh);

    for (let i = 0; i < needsRefresh.length; i += BATCH_SIZE) {
      if (!shouldContinue()) return;
      const batch = needsRefresh.slice(i, i + BATCH_SIZE);
      await Promise.all(
        batch.map((account) =>
          refreshProfile(adapter, account, getAccounts, setAccounts, shouldContinue),
        ),
      );
    }
  }

  function removeAvatar(accountId: string) {
    const { [accountId]: _, ...rest } = avatarStates;
    avatarStates = rest;
  }

  function clear() {
    avatarStates = {};
  }

  return {
    get states() {
      return avatarStates;
    },
    updateState,
    seedForAccounts,
    applyProfileUpdate,
    loadProfilesForAccounts,
    removeAvatar,
    clear,
  };
}
