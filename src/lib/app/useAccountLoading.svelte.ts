import { invoke } from "@tauri-apps/api/core";
import { syncAccounts } from "$lib/features/folders/store";
import { DEFAULT_LOCALE, translate } from "$lib/i18n";
import { ensurePlatformLoaded } from "$lib/platforms/registry";
import {
  getPlatform,
  type PlatformAccount,
  type PlatformAddFlowStatus,
} from "$lib/shared/platform";
import { createAccountLoader } from "$lib/shared/useAccountLoader.svelte";
import { isPlatformUsable, type createPlatformShellState } from "./platformShell.svelte";
import { createVisiblePriming } from "./useVisiblePriming.svelte";

type PlatformShell = ReturnType<typeof createPlatformShellState>;

type AccountLoadingDeps = {
  shell: Pick<
    PlatformShell,
    | "adapter"
    | "activeTab"
    | "activeTabUsable"
    | "runtimeOs"
    | "settings"
    | "adapterRegistryChanged"
  >;
  navigation: {
    readonly isSearching: boolean;
    refreshCurrentItems: () => void;
  };
  grid: {
    queueCalculatePadding: () => void;
  };
  /** The ids the grid renders, sections and folder contents included. Read
   *  lazily: the display pipeline is built from this loader later on. */
  getVisibleRenderedAccountIds: () => string[];
  /** Read lazily: the add flow is built with `loadAccounts` from here. */
  getAddFlow: () => {
    readonly flow: unknown;
    start: (platformId: string, status: PlatformAddFlowStatus) => void;
  };
  /** Read lazily: the persona controller waits on this loader's switch. */
  isPersonaSwitching: () => boolean;
};

type VisiblePrimingTrackDeps = {
  /** Settings open, window in the background or render suspended: the grid
   *  is out of sight, so priming pauses. */
  isGridHidden: () => boolean;
};

/**
 * The account loader of the active tab and everything that drives it: lazy
 * adapter loading, the reload after a load, add and switch, and the priming
 * of the accounts on screen.
 */
export function createAccountLoading({
  shell,
  navigation,
  grid,
  getVisibleRenderedAccountIds,
  getAddFlow,
  isPersonaSwitching,
}: AccountLoadingDeps) {
  const loader = createAccountLoader(
    () => shell.adapter,
    // The ids the grid renders, sections and folder contents included, so the
    // background profile work starts with what is on screen.
    getVisibleRenderedAccountIds,
    (key, params) => translate(shell.settings.language ?? DEFAULT_LOCALE, key, params),
  );
  const visiblePriming = createVisiblePriming(loader);

  let loadingAdapterFor = $state<string | null>(null);
  let adapterLoading = $derived(loadingAdapterFor === shell.activeTab && !shell.adapter);

  async function ensureAdapterReady(platformId: string) {
    const existing = getPlatform(platformId);
    if (existing) return existing;
    const affectsVisibleUi = platformId === shell.activeTab;
    if (affectsVisibleUi) {
      loadingAdapterFor = platformId;
    }
    try {
      const loaded = await ensurePlatformLoaded(platformId);
      if (loaded) {
        shell.adapterRegistryChanged();
      }
      return loaded;
    } finally {
      if (loadingAdapterFor === platformId) {
        loadingAdapterFor = null;
      }
    }
  }

  async function loadAccounts(
    silent = false,
    showRefreshedToast = false,
    forceRefresh = false,
    checkBans = false,
    deferBackground = true,
  ) {
    if (!isPlatformUsable(shell.activeTab, shell.runtimeOs)) return;
    const adapterReady = await ensureAdapterReady(shell.activeTab);
    if (!adapterReady) return;
    return loader.load(
      (platformId) => {
        syncAccounts(
          loader.accounts.map((a) => a.id),
          platformId,
        );
        navigation.refreshCurrentItems();
        grid.queueCalculatePadding();
      },
      silent,
      showRefreshedToast,
      forceRefresh,
      checkBans,
      deferBackground,
    );
  }

  /** Accounts of any platform, not just the active tab (personas, deep links). */
  async function loadPlatformAccounts(platformId: string) {
    const adapter = await ensurePlatformLoaded(platformId);
    if (!adapter) return [];
    return adapter.loadAccounts();
  }

  async function handleAddAccount() {
    const addFlow = getAddFlow();
    if (loader.adding || addFlow.flow) return;
    const adapterReady = await ensureAdapterReady(shell.activeTab);
    if (!adapterReady) return;
    const platformId = adapterReady.id;
    const result = await loader.addNew();
    if (result?.setupStatus) {
      addFlow.start(platformId, result.setupStatus);
    }
  }

  function handleRefreshClick() {
    if (!shell.activeTabUsable) return;
    void loadAccounts(false, true, false, true);
  }

  function handleAddAccountClick() {
    if (!shell.activeTabUsable || loader.adding || getAddFlow().flow) return;
    void handleAddAccount();
  }

  async function handleAccountSwitch(account: PlatformAccount) {
    // A persona switch is relaunching clients one platform at a time; a
    // second switch would race it for the same client.
    if (isPersonaSwitching()) return false;
    // Minimize only after a successful switch: minimizing first hid the error
    // toast (and with suspendGraphicsWhenMinimized, unmounted it entirely).
    const switched = await loader.switchTo(account);
    if (switched && shell.settings.minimizeOnAccountSwitch) {
      try {
        await invoke("minimize_window");
      } catch (e) {
        console.error("Failed to minimize window after switching account:", e);
      }
    }
    return switched;
  }

  /**
   * Primes the profile data of the accounts on screen. Call once during
   * component init, where the effect is meant to sit among the others.
   */
  function trackVisiblePriming({ isGridHidden }: VisiblePrimingTrackDeps) {
    $effect(() => {
      if (!shell.adapter) {
        visiblePriming.reset();
        return;
      }
      // A load refreshes the accounts it shows itself, and a tab switch resets
      // explicitly, so these only pause: the primed accounts stay primed.
      if (loader.loading || isGridHidden()) {
        visiblePriming.pause();
        return;
      }
      const visibleIds = getVisibleRenderedAccountIds();
      if (visibleIds.length === 0) {
        visiblePriming.pause();
        return;
      }
      visiblePriming.processVisible(visibleIds, shell.activeTab, navigation.isSearching);
    });
  }

  return {
    loader,
    visiblePriming,
    get adapterLoading() {
      return adapterLoading;
    },
    ensureAdapterReady,
    loadAccounts,
    loadPlatformAccounts,
    handleRefreshClick,
    handleAddAccountClick,
    handleAccountSwitch,
    trackVisiblePriming,
  };
}
