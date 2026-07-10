import { untrack } from "svelte";
import type { PlatformAdapter, PlatformAccount, PlatformProfileInfo } from "./platform";

const BATCH_SIZE = 15;

export type AvatarState = { url: string | null; loading: boolean; refreshing: boolean };

const scheduleFlush =
  typeof requestAnimationFrame === "function"
    ? (fn: () => void) => requestAnimationFrame(fn)
    : (fn: () => void) => queueMicrotask(fn);

export function createAvatarLoader(getAdapter: () => PlatformAdapter | undefined) {
  let avatarStates = $state.raw<Record<string, AvatarState>>({});

  // Coalesce individual resolutions into one record replacement per frame.
  // Replacing the record per avatar invalidates avatarStates[id] on every
  // rendered card: N avatars x M cards re-evaluations at boot.
  let pendingUpdates: Record<string, AvatarState> | null = null;

  function readState(accountId: string): AvatarState | undefined {
    return pendingUpdates?.[accountId] ?? untrack(() => avatarStates[accountId]);
  }

  function flushPending() {
    const updates = pendingUpdates;
    pendingUpdates = null;
    if (!updates) return;
    untrack(() => {
      avatarStates = { ...avatarStates, ...updates };
    });
  }

  function queueUpdate(accountId: string, nextState: AvatarState) {
    if (!pendingUpdates) {
      pendingUpdates = {};
      scheduleFlush(flushPending);
    }
    pendingUpdates[accountId] = nextState;
  }

  function sameState(previous: AvatarState | undefined, next: AvatarState): boolean {
    return (
      !!previous &&
      previous.url === next.url &&
      previous.loading === next.loading &&
      previous.refreshing === next.refreshing
    );
  }

  function updateState(accountId: string, next: Partial<AvatarState>) {
    const previous = readState(accountId);
    const nextState: AvatarState = {
      url: previous?.url ?? null,
      loading: previous?.loading ?? false,
      refreshing: previous?.refreshing ?? false,
      ...next,
    };
    if (sameState(previous, nextState)) return;
    queueUpdate(accountId, nextState);
  }

  function mergeStates(updates: Record<string, AvatarState>) {
    for (const [accountId, nextState] of Object.entries(updates)) {
      if (sameState(readState(accountId), nextState)) continue;
      queueUpdate(accountId, nextState);
    }
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
      const existing = readState(account.id);
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
      url: readState(account.id)?.url || null,
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

  /** Applies a batch of resolved profiles in a single reactive commit:
   * one `mergeStates` call instead of one record replacement per avatar. */
  function applyBatchProfileUpdates(
    accountsToUpdate: PlatformAccount[],
    profiles: Record<string, PlatformProfileInfo | null>,
    getAccounts: () => PlatformAccount[],
    setAccounts: (v: PlatformAccount[]) => void,
  ) {
    const updates: Record<string, AvatarState> = {};
    const displayNameUpdates: Array<readonly [string, string]> = [];

    for (const account of accountsToUpdate) {
      const profile = profiles[account.id] ?? null;
      if (profile) {
        updates[account.id] = {
          url: profile.avatarUrl,
          loading: profile.avatarLoading ?? false,
          refreshing: false,
        };
        if (profile.displayName && profile.displayName !== account.displayName) {
          displayNameUpdates.push([account.id, profile.displayName]);
        }
      } else {
        // No data: keep whatever avatar is on screen, just stop the spinners.
        updates[account.id] = {
          url: readState(account.id)?.url || null,
          loading: false,
          refreshing: false,
        };
      }
    }

    mergeStates(updates);
    for (const [accountId, displayName] of displayNameUpdates) {
      updateAccountDisplayName(getAccounts, setAccounts, accountId, displayName);
    }
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
    if (needsRefresh.length === 0) return;

    // Batch path (e.g. Steam): one backend call for every account, then one
    // reactive commit, instead of N invokes applied one by one.
    if (adapter.getProfileInfos) {
      if (!shouldContinue()) return;
      let profiles: Record<string, PlatformProfileInfo | null> | null = null;
      try {
        profiles = await adapter.getProfileInfos(needsRefresh.map((account) => account.id));
      } catch {
        // Batch failure: fall through to the per-account path below.
      }
      if (!shouldContinue()) return;
      if (profiles) {
        applyBatchProfileUpdates(needsRefresh, profiles, getAccounts, setAccounts);
        return;
      }
    }

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
    if (pendingUpdates) delete pendingUpdates[accountId];
    const { [accountId]: _, ...rest } = avatarStates;
    avatarStates = rest;
  }

  function clear() {
    pendingUpdates = null;
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
