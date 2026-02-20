import type { PlatformAdapter, PlatformAccount } from "./platform";
import type { BanInfo } from "../platforms/steam/types";
import { addToast, removeToast } from "../features/notifications/store.svelte";
import { getApiKey, getPlayerBans } from "../platforms/steam/steamApi";

import { getSettings } from "../features/settings/store";

const BATCH_SIZE = 5;
const BAN_CHECK_STATE_KEY = "accshift_ban_check_state_v2";
const BAN_INFO_CACHE_KEY = "accshift_ban_info_cache_v1";
const BAN_ERROR_TOAST_COOLDOWN_MS = 30000;
const LOAD_TOAST_COOLDOWN_MS = 30000;

interface BanCheckState {
  lastSuccessAt: number;
  checkedSteamIds: string[];
}

function readBanCheckState(): BanCheckState | null {
  try {
    const raw = localStorage.getItem(BAN_CHECK_STATE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<BanCheckState>;
    const lastSuccessAt = Number(parsed.lastSuccessAt);
    const checkedSteamIds = Array.isArray(parsed.checkedSteamIds)
      ? parsed.checkedSteamIds.filter((id): id is string => typeof id === "string")
      : [];
    if (!Number.isFinite(lastSuccessAt) || lastSuccessAt < 0) return null;
    return {
      lastSuccessAt,
      checkedSteamIds: Array.from(new Set(checkedSteamIds)),
    };
  } catch {
    return null;
  }
}

function writeBanCheckState(state: BanCheckState) {
  localStorage.setItem(BAN_CHECK_STATE_KEY, JSON.stringify(state));
}

function isBanInfo(value: unknown): value is BanInfo {
  if (!value || typeof value !== "object") return false;
  const v = value as Partial<BanInfo>;
  return typeof v.steam_id === "string"
    && typeof v.community_banned === "boolean"
    && typeof v.vac_banned === "boolean"
    && typeof v.number_of_vac_bans === "number"
    && typeof v.days_since_last_ban === "number"
    && typeof v.number_of_game_bans === "number"
    && typeof v.economy_ban === "string";
}

function readBanInfoCache(): Record<string, BanInfo> {
  try {
    const raw = localStorage.getItem(BAN_INFO_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") return {};

    const entries = Object.entries(parsed).filter(([, value]) => isBanInfo(value));
    return Object.fromEntries(entries.map(([key, value]) => [key, value as BanInfo]));
  } catch {
    return {};
  }
}

function writeBanInfoCache(bans: Record<string, BanInfo>) {
  localStorage.setItem(BAN_INFO_CACHE_KEY, JSON.stringify(bans));
}

function isSteamPathMissingError(message: string): boolean {
  const normalized = message.trim().toLowerCase();
  return normalized.includes("could not locate steam installation")
    || normalized.includes("could not read steam configuration");
}

function deferBackgroundTask(task: () => void) {
  if (typeof window !== "undefined" && "requestIdleCallback" in window) {
    const requestIdle = (
      window as Window & {
        requestIdleCallback: (callback: IdleRequestCallback, options?: IdleRequestOptions) => number;
      }
    ).requestIdleCallback;
    requestIdle(() => task(), { timeout: 600 });
    return;
  }
  setTimeout(task, 0);
}

export function createAccountLoader(getAdapter: () => PlatformAdapter | undefined, getActiveTab: () => string) {
  // Centralized UI state for account loading, switching, avatars, and Steam ban checks.
  let accounts = $state<PlatformAccount[]>([]);
  let accountMap = $derived<Record<string, PlatformAccount>>(
    Object.fromEntries(accounts.map(a => [a.id, a]))
  );
  let currentAccount = $state("");
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);
  let avatarStates = $state<Record<string, { url: string | null; loading: boolean; refreshing: boolean }>>({});
  let banStates = $state<Record<string, BanInfo>>(readBanInfoCache());
  let sessionBanCheckedIds = new Set<string>();
  let lastBanErrorToastAt = 0;
  let activeBanCheckToastId: string | null = null;
  let lastSteamPathToastAt = 0;
  let lastNoAccountsToastAt = 0;
  let latestLoadId = 0;

  async function refreshProfile(adapter: PlatformAdapter, account: PlatformAccount) {
    if (!adapter.getProfileInfo) return;
    const profile = await adapter.getProfileInfo(account.id);
    if (profile) {
      avatarStates[account.id] = {
        url: profile.avatar_url || avatarStates[account.id]?.url || null,
        loading: false,
        refreshing: false,
      };
      if (profile.display_name && profile.display_name !== account.displayName) {
        const idx = accounts.findIndex(a => a.id === account.id);
        if (idx !== -1) {
          accounts[idx] = { ...accounts[idx], displayName: profile.display_name };
        }
      }
    } else {
      avatarStates[account.id] = {
        url: avatarStates[account.id]?.url || null,
        loading: false,
        refreshing: false,
      };
    }
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
        avatarStates[account.id] = { url: cached.url, loading: false, refreshing: shouldRefresh };
        if (shouldRefresh) {
          needsRefresh.push(account);
        }
      } else if (adapter.getProfileInfo) {
        avatarStates[account.id] = { url: null, loading: true, refreshing: false };
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

  async function fetchBanStates(accts: PlatformAccount[], silent = false, forceRefresh = false) {
    if (getActiveTab() !== "steam" || accts.length === 0) return;

    const delayDays = getSettings().banCheckDays;
    const forceBanRefresh = forceRefresh;
    const steamIds = Array.from(new Set(accts.map(a => a.id)));
    if (steamIds.length === 0) return;

    const apiKey = (await getApiKey().catch((e) => {
      console.error("[ban-check] failed to read API key:", e);
      return "";
    })).trim();
    if (!apiKey) {
      console.info("[ban-check] skipped: missing Steam API key");
      return;
    }

    const now = Date.now();
    const cachedState = readBanCheckState();
    const delayMs = delayDays * 24 * 60 * 60 * 1000;
    const withinDelayWindow = delayDays > 0
      && !!cachedState
      && now - cachedState.lastSuccessAt < delayMs;
    const cachedCheckedIds = new Set(cachedState?.checkedSteamIds ?? []);

    // Refresh policy:
    // - forceRefresh: always fetch all accounts
    // - delayDays = 0: fetch once per session
    // - delayDays > 0: fetch only unchecked IDs inside the delay window
    let idsToFetch: string[] = [];
    if (forceBanRefresh) {
      idsToFetch = steamIds;
    } else if (delayDays === 0) {
      idsToFetch = steamIds.filter(id => !sessionBanCheckedIds.has(id));
    } else if (withinDelayWindow) {
      idsToFetch = steamIds.filter(id => !cachedCheckedIds.has(id));
    } else {
      idsToFetch = steamIds;
    }
    if (idsToFetch.length === 0) {
      console.info("[ban-check] skipped: no accounts to check", {
        totalAccounts: steamIds.length,
        delayDays,
        forceBanRefresh,
      });
      return;
    }

    let checkingToastId: string | null = null;
    if (!silent) {
      if (activeBanCheckToastId) {
        removeToast(activeBanCheckToastId);
      }
      activeBanCheckToastId = addToast("Checking bans...", { durationMs: null });
      checkingToastId = activeBanCheckToastId;
    }

    console.info("[ban-check] started", {
      totalAccounts: steamIds.length,
      idsToFetch: idsToFetch.length,
      delayDays,
      forceBanRefresh,
      withinDelayWindow,
    });

    try {
      const bans = await getPlayerBans(idsToFetch);
      let bannedCount = 0;
      const returnedIds = new Set<string>();
      let malformedRows = 0;
      for (const ban of bans) {
        if (typeof ban.steam_id !== "string" || ban.steam_id.length === 0) {
          malformedRows++;
          continue;
        }
        banStates[ban.steam_id] = ban;
        returnedIds.add(ban.steam_id);
        if (ban.vac_banned || ban.community_banned || ban.number_of_game_bans > 0) {
          bannedCount++;
        }
      }
      if (malformedRows > 0) {
        console.error("[ban-check] malformed ban rows without steam_id", {
          malformedRows,
          totalRows: bans.length,
          sampleRow: bans[0] ?? null,
        });
      }
      for (const steamId of idsToFetch) {
        sessionBanCheckedIds.add(steamId);
      }
      if (returnedIds.size !== idsToFetch.length) {
        const missingIds = idsToFetch.filter(steamId => !returnedIds.has(steamId));
        console.warn("[ban-check] missing results for some Steam IDs", {
          expected: idsToFetch.length,
          received: returnedIds.size,
          missingIds,
        });
      }

      if (delayDays > 0) {
        const mergedCheckedIds = forceBanRefresh || !withinDelayWindow
          ? steamIds
          : Array.from(new Set([...(cachedState?.checkedSteamIds ?? []), ...idsToFetch]));
        writeBanCheckState({
          lastSuccessAt: now,
          checkedSteamIds: mergedCheckedIds,
        });
      } else {
        localStorage.removeItem(BAN_CHECK_STATE_KEY);
      }
      writeBanInfoCache(Object.fromEntries(Object.entries(banStates)));

      if (!silent && bannedCount > 0) {
        addToast(`Ban check: ${bannedCount} accounts with bans`);
      }

      console.info("[ban-check] completed", {
        checkedAccounts: idsToFetch.length,
        returnedRows: bans.length,
        bannedCount,
      });
    } catch (e) {
      if (!silent && now - lastBanErrorToastAt >= BAN_ERROR_TOAST_COOLDOWN_MS) {
        addToast(`Ban check failed: ${String(e)}`);
        lastBanErrorToastAt = now;
      }
      console.error("[ban-check] failed to fetch ban states:", e);
    } finally {
      if (checkingToastId && activeBanCheckToastId === checkingToastId) {
        removeToast(checkingToastId);
        activeBanCheckToastId = null;
      }
    }
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
      if (adapter.getStartupSnapshot) {
        const snapshot = await adapter.getStartupSnapshot();
        accounts = snapshot.accounts;
        currentAccount = snapshot.currentAccount;
      } else {
        accounts = await adapter.loadAccounts();
        currentAccount = await adapter.getCurrentAccount();
      }
      if (getActiveTab() === "steam" && accounts.length === 0) {
        const now = Date.now();
        if (now - lastNoAccountsToastAt >= LOAD_TOAST_COOLDOWN_MS) {
          addToast("No Steam accounts found. Sign in to Steam at least once, then refresh.");
          lastNoAccountsToastAt = now;
        }
      } else if (showRefreshedToast && !silent) {
        const count = accounts.length;
        addToast(`${count} ${count === 1 ? "account" : "accounts"} refreshed`);
      }
      onAfterLoad?.();
      const loadedAccounts = [...accounts];
      const runBackgroundTasks = () => {
        if (loadId !== latestLoadId) return;
        void loadProfilesForAccounts(loadedAccounts, forceRefresh);
        if (checkBans) {
          void fetchBanStates(loadedAccounts, silent, forceRefresh);
        }
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
      if (getActiveTab() === "steam" && isSteamPathMissingError(message)) {
        const now = Date.now();
        if (now - lastSteamPathToastAt >= LOAD_TOAST_COOLDOWN_MS) {
          addToast("Steam folder was not found. Set it manually in Settings > Steam folder.");
          lastSteamPathToastAt = now;
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
        avatarStates[account.id] = { ...avatarStates[account.id], refreshing: true };
        void adapter.getProfileInfo(account.id)
          .then((profile) => {
            if (profile) {
              avatarStates[account.id] = {
                url: profile.avatar_url || avatarStates[account.id]?.url,
                loading: false,
                refreshing: false,
              };
            } else {
              avatarStates[account.id] = { ...avatarStates[account.id], refreshing: false };
            }
          })
          .catch(() => {
            avatarStates[account.id] = { ...avatarStates[account.id], refreshing: false };
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
    try { await adapter.addAccount(); } catch (e) { 
      error = String(e);
      addToast(error);
    }
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
    get banStates() { return banStates; },
    load,
    switchTo,
    addNew,
  };
}
