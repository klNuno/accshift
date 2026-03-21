import type { PlatformAdapter } from "$lib/shared/platform";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type EnsureAdapterReady = (platformId: string) => Promise<PlatformAdapter | undefined>;

type SteamMaintenanceControllerOptions = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  ensureAdapterReady: EnsureAdapterReady;
  getActiveTab: () => string;
  loadAccounts: (
    silent?: boolean,
    showRefreshedToast?: boolean,
    forceRefresh?: boolean,
    checkBans?: boolean,
    deferBackground?: boolean,
  ) => void | Promise<unknown>;
  showToast: (message: string) => void;
};

export function createSteamMaintenanceController({
  t,
  ensureAdapterReady,
  getActiveTab,
  loadAccounts,
  showToast,
}: SteamMaintenanceControllerOptions) {
  async function refreshAvatarsNow() {
    const steamAdapter = await ensureAdapterReady("steam");
    if (!steamAdapter?.getProfileInfo) return;

    try {
      const steamAccounts = await steamAdapter.loadAccounts();
      if (steamAccounts.length === 0) {
        showToast(t("toast.noSteamAccountsFound"));
        return;
      }

      await Promise.all(
        steamAccounts.map((account) => steamAdapter.getProfileInfo!(account.id).catch(() => null)),
      );

      const count = steamAccounts.length;
      if (getActiveTab() === "steam") {
        void loadAccounts(true, false, true, false, false);
      }

      showToast(t("toast.avatarRefreshComplete", { count }));
    } catch (error) {
      showToast(String(error));
    }
  }

  async function refreshBansNow() {
    const steamAdapter = await ensureAdapterReady("steam");
    if (!steamAdapter?.loadWarningStates) return;

    try {
      const steamAccounts = await steamAdapter.loadAccounts();
      if (steamAccounts.length === 0) {
        showToast(t("toast.noSteamAccountsFound"));
        return;
      }

      await steamAdapter.loadWarningStates(steamAccounts, {
        forceRefresh: true,
        silent: false,
        t,
      });

      const count = steamAccounts.length;
      if (getActiveTab() === "steam") {
        void loadAccounts(true, false, false, true, false);
      }

      showToast(t("toast.banRefreshComplete", { count }));
    } catch (error) {
      showToast(String(error));
    }
  }

  return {
    refreshAvatarsNow,
    refreshBansNow,
  };
}
