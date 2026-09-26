import type { AddToastOptions } from "$lib/features/notifications/store.svelte";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import type { PlatformAdapter, PlatformDef } from "$lib/shared/platform";

type ProfileRefreshDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  showToast: (message: string, options?: AddToastOptions) => unknown;
  /** Every platform definition; only those declaring the matching
   *  profileRefresh capability are refreshed. */
  platforms: readonly PlatformDef[];
  ensureAdapterReady: (platformId: string) => Promise<PlatformAdapter | null | undefined>;
  getActiveTab: () => string;
  loadAccounts: (
    silent?: boolean,
    showRefreshedToast?: boolean,
    forceRefresh?: boolean,
    checkBans?: boolean,
    deferBackground?: boolean,
  ) => Promise<unknown>;
};

/** The settings "refresh now" actions for avatars and bans (currently Steam only). */
export function createProfileRefresh({
  t,
  showToast,
  platforms,
  ensureAdapterReady,
  getActiveTab,
  loadAccounts,
}: ProfileRefreshDeps) {
  async function refreshAvatarsNow() {
    for (const def of platforms) {
      if (!def.capabilities?.profileRefresh?.avatars) continue;
      const adapter = await ensureAdapterReady(def.id);
      if (!adapter?.getProfileInfo) continue;
      try {
        const accounts = await adapter.loadAccounts();
        if (accounts.length === 0) {
          const noAccountsMsg = adapter.getNoAccountsToastMessage?.({ t });
          if (noAccountsMsg) showToast(noAccountsMsg);
          continue;
        }
        await Promise.all(accounts.map((a) => adapter.getProfileInfo!(a.id).catch(() => null)));
        if (getActiveTab() === def.id) void loadAccounts(true, false, true, false, false);
        showToast(t("toast.avatarRefreshComplete", { count: accounts.length }), {
          type: "success",
        });
      } catch (error) {
        console.error("[avatars] refresh failed:", error);
        showToast(t("toast.refreshFailed"), { type: "error" });
      }
    }
  }

  async function refreshBansNow() {
    for (const def of platforms) {
      if (!def.capabilities?.profileRefresh?.bans) continue;
      const adapter = await ensureAdapterReady(def.id);
      if (!adapter?.loadWarningStates) continue;
      try {
        const accounts = await adapter.loadAccounts();
        if (accounts.length === 0) {
          const noAccountsMsg = adapter.getNoAccountsToastMessage?.({ t });
          if (noAccountsMsg) showToast(noAccountsMsg);
          continue;
        }
        await adapter.loadWarningStates(accounts, { forceRefresh: true, silent: false, t });
        if (getActiveTab() === def.id) void loadAccounts(true, false, false, true, false);
        showToast(t("toast.banRefreshComplete", { count: accounts.length }), { type: "success" });
      } catch (error) {
        console.error("[bans] refresh failed:", error);
        showToast(t("toast.refreshFailed"), { type: "error" });
      }
    }
  }

  return { refreshAvatarsNow, refreshBansNow };
}
