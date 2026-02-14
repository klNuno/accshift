import type { PlatformAdapter, PlatformAccount } from "./platform";
import type { BanInfo } from "../features/steam/types";
import { addNotification, getUnreadCount } from "../features/notifications/store";
import { getPlayerBans } from "../features/steam/steamApi";

const BATCH_SIZE = 5;

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
  let notifCount = $state(getUnreadCount());

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
          addNotification(`Display name changed: ${account.displayName} \u2192 ${profile.display_name}`);
          notifCount = getUnreadCount();
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

  async function loadProfilesForAccounts(accts: PlatformAccount[]) {
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

    if (needsRefresh.length > 0) {
      addNotification(`Refreshing profiles for ${needsRefresh.length} account${needsRefresh.length > 1 ? "s" : ""}...`);
      notifCount = getUnreadCount();
    }

    // Parallel loading in batches
    let refreshedCount = 0;
    for (let i = 0; i < needsRefresh.length; i += BATCH_SIZE) {
      const batch = needsRefresh.slice(i, i + BATCH_SIZE);
      await Promise.all(batch.map(async (account) => {
        await refreshProfile(adapter, account);
        refreshedCount++;
      }));
    }

    if (refreshedCount > 0) {
      addNotification(`Profiles refreshed for ${refreshedCount} account${refreshedCount > 1 ? "s" : ""}.`);
      notifCount = getUnreadCount();
    }
  }

  async function fetchBanStates(accts: PlatformAccount[]) {
    if (getActiveTab() !== "steam" || accts.length === 0) return;
    addNotification(`Checking ban status for ${accts.length} account${accts.length > 1 ? "s" : ""}...`);
    notifCount = getUnreadCount();
    try {
      const steamIds = accts.map(a => a.id);
      const bans = await getPlayerBans(steamIds);
      let bannedCount = 0;
      for (const ban of bans) {
        banStates[ban.steam_id] = ban;
        if (ban.vac_banned || ban.community_banned || ban.number_of_game_bans > 0) {
          bannedCount++;
        }
      }
      if (bannedCount > 0) {
        addNotification(`Ban check complete: ${bannedCount} account${bannedCount > 1 ? "s" : ""} with bans detected.`);
      } else {
        addNotification("Ban check complete: no bans detected.");
      }
      notifCount = getUnreadCount();
    } catch (e) {
      addNotification(`Ban check failed: ${String(e)}`);
      notifCount = getUnreadCount();
      console.error("Failed to fetch ban states:", e);
    }
  }

  async function load(onAfterLoad?: () => void) {
    const adapter = getAdapter();
    if (!adapter) return;
    loading = true;
    error = null;
    try {
      accounts = await adapter.loadAccounts();
      currentAccount = await adapter.getCurrentAccount();
      onAfterLoad?.();
      loadProfilesForAccounts(accounts);
      fetchBanStates(accounts);
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
    }
    switching = false;
  }

  async function addNew() {
    const adapter = getAdapter();
    if (!adapter) return;
    try { await adapter.addAccount(); } catch (e) { error = String(e); }
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
    get notifCount() { return notifCount; },
    set notifCount(v: number) { notifCount = v; },
    load,
    switchTo,
    addNew,
  };
}
