import type { PlatformAdapter, PlatformAccount } from "./platform";
import type { BanInfo } from "../features/steam/types";
import { addToast } from "../features/notifications/store.svelte";
import { getApiKey, getPlayerBans } from "../features/steam/steamApi";

import { getSettings } from "../features/settings/store";

const BATCH_SIZE = 5;
const BAN_CHECK_KEY = "accshift_last_ban_check";

export function createAccountLoader(getAdapter: () => PlatformAdapter | undefined, getActiveTab: () => string) {
  let accounts = $state<PlatformAccount[]>([]);
  let accountMap = $derived<Record<string, PlatformAccount>>(
    Object.fromEntries(accounts.map(a => [a.id, a]))
  );
  let currentAccount = $state("");
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);
  let avatarStates = $state<Record<string, { url: string | null; loading: boolean; refreshing: boolean }>>({});
  let banStates = $state<Record<string, BanInfo>>({});

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
          // Optional: Toast for name change? User didn't ask for it specifically, but it's useful.
          // addToast(`Name changed: ${account.displayName} -> ${profile.display_name}`);
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
    silent = false,
    showRefreshedToast = false
  ) {
    const adapter = getAdapter();
    if (!adapter) return;

    // Separate into cached (just need display) and needs-refresh
    const needsRefresh: PlatformAccount[] = [];
    for (const account of accts) {
      const cached = adapter.getCachedProfile?.(account.id);
      if (cached) {
        avatarStates[account.id] = { url: cached.url, loading: false, refreshing: cached.expired };
        if (cached.expired) {
          needsRefresh.push(account);
        }
      } else if (adapter.getProfileInfo) {
        avatarStates[account.id] = { url: null, loading: true, refreshing: false };
        needsRefresh.push(account);
      }
    }

    // Don't toast "Refreshing...", just do it.

    // Parallel loading in batches
    let refreshedCount = 0;
    for (let i = 0; i < needsRefresh.length; i += BATCH_SIZE) {
      const batch = needsRefresh.slice(i, i + BATCH_SIZE);
      await Promise.all(batch.map(async (account) => {
        await refreshProfile(adapter, account);
        refreshedCount++;
      }));
    }

    if (showRefreshedToast && !silent && refreshedCount > 0) {
      addToast(`${refreshedCount} account${refreshedCount > 1 ? 's' : ''} refreshed`);
    }
  }

  async function fetchBanStates(accts: PlatformAccount[], silent = false) {
    if (getActiveTab() !== "steam" || accts.length === 0) return;

    const lastCheck = localStorage.getItem(BAN_CHECK_KEY);
    const now = Date.now();
    const delayMs = getSettings().banCheckDays * 24 * 60 * 60 * 1000;

    const apiKey = (await getApiKey().catch(() => "")).trim();
    if (!apiKey) {
      return;
    }
    
    if (lastCheck && now - parseInt(lastCheck) < delayMs) {
      // Skip check
      return;
    }

    try {
      const steamIds = accts.map(a => a.id);
      const bans = await getPlayerBans(steamIds);
      if (bans.length === 0) return;
      let bannedCount = 0;
      for (const ban of bans) {
        banStates[ban.steam_id] = ban;
        if (ban.vac_banned || ban.community_banned || ban.number_of_game_bans > 0) {
          bannedCount++;
        }
      }
      
      // Update timestamp only on success
      localStorage.setItem(BAN_CHECK_KEY, now.toString());

      if (bannedCount > 0) {
        addToast(`Ban check: ${bannedCount} accounts with bans`);
      }
    } catch (e) {
      addToast(`Ban check failed: ${String(e)}`);
      console.error("Failed to fetch ban states:", e);
    }
  }

  async function load(
    onAfterLoad?: () => void,
    silent = false,
    showRefreshedToast = false
  ) {
    const adapter = getAdapter();
    if (!adapter) return;
    loading = true;
    error = null;
    try {
      accounts = await adapter.loadAccounts();
      currentAccount = await adapter.getCurrentAccount();
      onAfterLoad?.();
      loadProfilesForAccounts(accounts, silent, showRefreshedToast);
      fetchBanStates(accounts, silent);
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  async function switchTo(account: PlatformAccount) {
    const adapter = getAdapter();
    if (!adapter || switching || account.username === currentAccount) return;
    switching = true;
    error = null;
    try {
      await adapter.switchAccount(account);
      currentAccount = account.username;
      if (adapter.getProfileInfo) {
        avatarStates[account.id] = { ...avatarStates[account.id], refreshing: true };
        const profile = await adapter.getProfileInfo(account.id);
        if (profile) {
          avatarStates[account.id] = { url: profile.avatar_url || avatarStates[account.id]?.url, loading: false, refreshing: false };
        } else {
          avatarStates[account.id] = { ...avatarStates[account.id], refreshing: false };
        }
      }
    } catch (e) {
      error = String(e);
      addToast(error); // Toast errors on switch
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
    // notifCount removed
    load,
    switchTo,
    addNew,
  };
}
