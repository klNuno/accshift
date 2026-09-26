import { describe, it, expect, vi, beforeEach } from "vitest";
import { createAppNavigationController } from "./useAppNavigation.svelte";

function makeDeps(
  overrides: { activeTab?: string; showSettings?: boolean; enabledPlatforms?: string[] } = {},
) {
  let activeTab = overrides.activeTab ?? "steam";
  const enabledPlatforms = overrides.enabledPlatforms ?? ["steam"];
  const calls: string[] = [];
  let showSettings = overrides.showSettings ?? false;
  const spies = {
    pushState: vi.fn(),
    back: vi.fn(),
    replaceState: vi.fn(),
    clearForPlatformChange: vi.fn(() => {
      calls.push("clear");
    }),
    loadAccounts: vi.fn(() => {
      calls.push("load");
    }),
    resetVisiblePrimeState: vi.fn(),
    setActiveTab: vi.fn((tab: string) => {
      calls.push(`tab:${tab}`);
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
        return { enabledPlatforms, defaultPlatformId: enabledPlatforms[0] } as never;
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
    resetVisiblePrimeState: spies.resetVisiblePrimeState,
  });
  return { controller, spies, calls, getShowSettings: () => showSettings };
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

describe("handlePlatformsChanged", () => {
  it("clears the old platform's load before moving off a disabled tab", () => {
    // A Steam load still in flight must not land under the new tab and
    // rewrite its folders with Steam ids.
    const { controller, spies, calls } = makeDeps({
      activeTab: "steam",
      enabledPlatforms: ["riot"],
    });
    controller.handlePlatformsChanged();
    expect(calls).toEqual(["clear", "tab:riot", "load"]);
    expect(spies.resetVisiblePrimeState).toHaveBeenCalled();
  });

  it("keeps the grid when the active tab stays enabled", () => {
    const { controller, spies } = makeDeps({
      activeTab: "steam",
      enabledPlatforms: ["steam", "riot"],
    });
    controller.handlePlatformsChanged();
    expect(spies.clearForPlatformChange).not.toHaveBeenCalled();
    expect(spies.setActiveTab).not.toHaveBeenCalled();
    expect(spies.loadAccounts).toHaveBeenCalled();
  });
});
