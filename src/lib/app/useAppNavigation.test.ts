import { describe, it, expect, vi, beforeEach } from "vitest";
import { createAppNavigationController } from "./useAppNavigation.svelte";

function makeDeps(overrides: { activeTab?: string; showSettings?: boolean } = {}) {
  let activeTab = overrides.activeTab ?? "steam";
  let showSettings = overrides.showSettings ?? false;
  const spies = {
    pushState: vi.fn(),
    back: vi.fn(),
    replaceState: vi.fn(),
    clearForPlatformChange: vi.fn(),
    loadAccounts: vi.fn(),
    setActiveTab: vi.fn((tab: string) => {
      activeTab = tab;
    }),
    setShowSettings: vi.fn((value: boolean) => {
      showSettings = value;
    }),
  };
  vi.stubGlobal("history", {
    get state() {
      return null;
    },
    pushState: spies.pushState,
    back: spies.back,
    replaceState: spies.replaceState,
  });
  const controller = createAppNavigationController({
    shell: {
      get activeTab() {
        return activeTab;
      },
      get runtimeOs() {
        return "windows" as const;
      },
      get settings() {
        return { enabledPlatforms: ["steam"] } as never;
      },
      setActiveTab: spies.setActiveTab,
      refreshSettings: () => {},
    },
    navigation: {
      currentFolderId: null,
      searchQuery: "",
      refreshCurrentItems: () => {},
    },
    loader: {
      clearForPlatformChange: spies.clearForPlatformChange,
      prepareVisibleAccounts: () => {},
    },
    addFlow: {
      get flow() {
        return null;
      },
      cancel: async () => {},
      cancelIfConflicting: async () => {},
    },
    getShowSettings: () => showSettings,
    setShowSettings: spies.setShowSettings,
    loadSettingsComponent: async () => {},
    loadAccounts: spies.loadAccounts,
    closeBulkEdit: () => {},
    queueGridPadding: () => {},
    onSettingsClosed: () => {},
    getParentFolderId: () => null,
    resetVisiblePrimeState: () => {},
  });
  return { controller, spies, getShowSettings: () => showSettings };
}

beforeEach(() => {
  vi.unstubAllGlobals();
});

describe("handleTabChange same-tab guard", () => {
  it("no-ops the re-click without wiping or reloading the grid", async () => {
    const { controller, spies } = makeDeps({ activeTab: "steam" });
    await controller.handleTabChange("steam");
    expect(spies.clearForPlatformChange).not.toHaveBeenCalled();
    expect(spies.loadAccounts).not.toHaveBeenCalled();
    expect(spies.pushState).not.toHaveBeenCalled();
    expect(spies.setActiveTab).not.toHaveBeenCalled();
  });

  it("still exits settings when clicking the current tab", async () => {
    const { controller, spies, getShowSettings } = makeDeps({
      activeTab: "steam",
      showSettings: true,
    });
    await controller.handleTabChange("steam");
    expect(getShowSettings()).toBe(false);
    expect(spies.setShowSettings).toHaveBeenCalledWith(false);
    expect(spies.loadAccounts).not.toHaveBeenCalled();
    expect(spies.clearForPlatformChange).not.toHaveBeenCalled();
  });

  it("still switches normally on a different tab", async () => {
    const { controller, spies } = makeDeps({ activeTab: "steam" });
    await controller.handleTabChange("riot");
    expect(spies.setActiveTab).toHaveBeenCalledWith("riot");
    expect(spies.clearForPlatformChange).toHaveBeenCalled();
    expect(spies.loadAccounts).toHaveBeenCalled();
  });
});
